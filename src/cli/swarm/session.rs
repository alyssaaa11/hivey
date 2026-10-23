//! `hivey swarm session <pane>`: a second session of the agent running in a pane, in a new
//! pane beside it (the sidebar's right-click "New session" items run it).
//!
//! Starts the same program with the same arguments in the same folder, minus the ones that
//! pick a conversation (`--continue`, `--resume ID`, `resume …`), so the new session starts
//! fresh while the original keeps running. The new session has no hivey name: it is the
//! user's, not a swarm member, so messages and relaunches never mix the two up.
//!
//! With `--worktree` the new session works in a git worktree of the agent's repository, on a
//! branch of its own. When the agent exits, the pane runs `--finish`: it offers to commit what
//! is left, merges the branch into the agent's folder and removes the worktree. A merge that
//! conflicts is aborted, so the agent's folder is never left half-merged.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::{json, Value};

use super::api;
use crate::swarm::adapter::strip_session_args;

pub(super) const HELP: &str = "\
usage: hivey swarm session <pane> [--direction right|down] [--worktree]
       hivey swarm session --finish <worktree> [--into <folder>]
  Opens a new session of the agent running in <pane> (same CLI, arguments and folder, a new
  conversation) in a pane split off it (default: right) and focuses it.
  --worktree  the session works in a new git worktree (branch hivey-session-N) of the agent's
              repository; when the agent exits, its work is merged back into the agent's folder
  --finish    merge a worktree session's branch into <folder> (default: the repository's main
              checkout) and remove the worktree; runs by itself when a worktree session ends";

/// Branches (and worktree folders) of worktree sessions: `hivey-session-1`, `-2`, …
const BRANCH_PREFIX: &str = "hivey-session-";

/// Script runners whose first argument is the agent's own script (`node …/codex.js`).
const RUNNERS: &[&str] = &["node", "bun", "deno", "python", "python3"];

pub(super) fn run(args: &[String]) -> std::io::Result<i32> {
    let mut pane = None;
    let mut direction = "right".to_string();
    let mut worktree = false;
    let mut finish = None;
    let mut into = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--worktree" => worktree = true,
            "--finish" | "--into" => match iter.next() {
                Some(path) if arg == "--finish" => finish = Some(PathBuf::from(path)),
                Some(path) => into = Some(PathBuf::from(path)),
                None => {
                    eprintln!("error: {arg} takes a folder\n{HELP}");
                    return Ok(2);
                }
            },
            "--direction" => match iter.next().map(String::as_str) {
                Some(value @ ("right" | "down")) => direction = value.to_string(),
                _ => {
                    eprintln!("error: --direction takes right or down\n{HELP}");
                    return Ok(2);
                }
            },
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(0);
            }
            flag if flag.starts_with("--") => {
                eprintln!("error: unknown flag {flag}\n{HELP}");
                return Ok(2);
            }
            _ => pane = Some(arg.clone()),
        }
    }
    if let Some(worktree) = finish {
        return match finish_worktree(&worktree, into, &mut ask) {
            // 0 only once the worktree is gone, so the pane's `&& exit` closes it.
            Ok(true) => Ok(0),
            Ok(false) => Ok(1),
            Err(err) => {
                eprintln!("error: {err}");
                Ok(1)
            }
        };
    }
    let Some(pane) = pane else {
        eprintln!("error: missing <pane>\n{HELP}");
        return Ok(2);
    };
    match open_session(&pane, &direction, worktree) {
        Ok(new_pane) => {
            println!("{new_pane}");
            Ok(0)
        }
        Err(err) => {
            eprintln!("error: {err}");
            Ok(1)
        }
    }
}

fn open_session(pane: &str, direction: &str, worktree: bool) -> Result<String, String> {
    let info = api("pane.process_info", json!({ "pane_id": pane }))?;
    let process = agent_process(&info["process_info"])
        .ok_or_else(|| format!("no agent is running in {pane}"))?;
    let argv: Vec<String> = process["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|arg| arg.as_str().map(str::to_string))
        .collect();
    let mut command = fresh_command(&argv).ok_or_else(|| format!("no command line for {pane}"))?;
    let mut cwd = match process["cwd"].as_str() {
        Some(cwd) => Value::from(cwd),
        None => api("pane.get", json!({ "pane_id": pane }))?["pane"]["cwd"].clone(),
    };
    if worktree {
        let folder = cwd
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| format!("no folder for {pane}"))?;
        let root = crate::worktree::expand_tilde_absolute_path(
            &crate::config::Config::load().config.worktrees.directory,
        );
        let session = create_worktree(&folder, &root)?;
        let hivey = std::env::current_exe().map_err(|err| err.to_string())?;
        command = worktree_command(&command, &session, &hivey);
        cwd = Value::from(session.start.display().to_string());
    }
    let split = api(
        "pane.split",
        json!({
            "target_pane_id": pane,
            "direction": direction,
            "cwd": cwd,
            "focus": true,
        }),
    )?;
    let new_pane = split["pane"]["pane_id"]
        .as_str()
        .map(str::to_string)
        .ok_or("pane.split returned no pane")?;
    // Typed into the new shell; it runs once the shell's startup is done.
    api(
        "pane.send_input",
        json!({ "pane_id": new_pane, "text": command, "keys": ["Enter"] }),
    )?;
    Ok(new_pane)
}

/// A new worktree session: where it lives and what it merges back into.
struct WorktreeSession {
    /// The agent's checkout (the top of its repository).
    repo: PathBuf,
    path: PathBuf,
    branch: String,
    /// The agent's folder inside the worktree (the same subfolder of the repository).
    start: PathBuf,
}

/// Creates a worktree of the repository holding `folder`, on a new branch from its HEAD,
/// under `root` (herdr's `[worktrees] directory`) as `<repo>/<branch>`.
fn create_worktree(folder: &Path, root: &Path) -> Result<WorktreeSession, String> {
    let repo = git(folder, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .map_err(|_| {
            format!(
                "{} is not in a git repository, so it can't have a worktree",
                folder.display()
            )
        })?;
    git(&repo, &["rev-parse", "--verify", "HEAD"]).map_err(|_| {
        format!(
            "{} has no commits yet to start a worktree from",
            repo.display()
        )
    })?;
    let repo_name = repo
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo")
        .to_string();
    let (branch, path) = (1..1000)
        .map(|n| {
            let branch = format!("{BRANCH_PREFIX}{n}");
            let path = crate::worktree::default_checkout_path(root, &repo_name, &branch);
            (branch, path)
        })
        .find(|(branch, path)| {
            !path.exists()
                && git(
                    &repo,
                    &[
                        "rev-parse",
                        "--verify",
                        "--quiet",
                        &format!("refs/heads/{branch}"),
                    ],
                )
                .is_err()
        })
        .ok_or("no free hivey-session branch name")?;
    let path_arg = path.display().to_string();
    git(
        &repo,
        &["worktree", "add", "-b", &branch, &path_arg, "HEAD"],
    )?;
    let relative = crate::worktree::canonical_or_original(folder)
        .strip_prefix(crate::worktree::canonical_or_original(&repo))
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let start = path.join(relative);
    let start = if start.is_dir() { start } else { path.clone() };
    Ok(WorktreeSession {
        repo,
        path,
        branch,
        start,
    })
}

/// The shell line for a worktree session: a note on where it works, the agent, then the merge
/// back once it exits (and the pane closes when that went through).
fn worktree_command(agent: &str, session: &WorktreeSession, hivey: &Path) -> String {
    let note = format!(
        "hivey: worktree session on branch {} in {} (from the last commit of {}). Exit the agent to merge it back.",
        session.branch,
        session.path.display(),
        session.repo.display()
    );
    format!(
        "echo {}; {agent}; {} swarm session --finish {} --into {} && exit",
        shell_quote(&note),
        shell_quote(&hivey.display().to_string()),
        shell_quote(&session.path.display().to_string()),
        shell_quote(&session.repo.display().to_string()),
    )
}

/// Merges a worktree session back: commits what is left (if the user agrees), merges its
/// branch into `into` and removes the worktree and branch. `Ok(true)` once the worktree is
/// gone; `Ok(false)` when it is kept (the user said no, or the merge conflicted).
fn finish_worktree(
    worktree: &Path,
    into: Option<PathBuf>,
    ask: &mut dyn FnMut(&str, bool) -> bool,
) -> Result<bool, String> {
    let branch = git(worktree, &["symbolic-ref", "--short", "HEAD"])
        .map_err(|_| format!("{} is not a worktree on a branch", worktree.display()))?;
    let into = match into {
        Some(into) => into,
        None => main_checkout(worktree)?,
    };
    let wt = worktree.display().to_string();
    println!("\nhivey: finishing the worktree session {branch} ({wt})");

    let status = git(worktree, &["status", "--short"])?;
    if !status.is_empty() {
        println!("Uncommitted changes:\n{status}");
        if !ask(&format!("Commit them to {branch}? [Y/n] "), true) {
            println!("Kept the worktree as it is: {wt}");
            return Ok(false);
        }
        git(worktree, &["add", "-A"])?;
        git(
            worktree,
            &[
                "commit",
                "-m",
                &format!("work from the hivey session {branch}"),
            ],
        )?;
    }

    let target = git(&into, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let new = git(&into, &["log", "--oneline", &format!("HEAD..{branch}")])?;
    if new.is_empty() {
        println!("Nothing new to merge into {}.", into.display());
        return remove_worktree(&into, worktree, &branch, false).map(|()| true);
    }
    println!(
        "Commits to merge into {} ({target}):\n{new}",
        into.display()
    );
    if !ask(&format!("Merge them into {target}? [Y/n] "), true) {
        if ask(
            &format!("Delete the worktree and branch {branch}, dropping this work? [y/N] "),
            false,
        ) {
            return remove_worktree(&into, worktree, &branch, true).map(|()| true);
        }
        println!("Kept the worktree and branch {branch}: {wt}");
        return Ok(false);
    }

    if let Err(err) = git(&into, &["merge", "--no-edit", &branch]) {
        let conflicts = git(&into, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
        if conflicts.is_empty() {
            println!(
                "The merge didn't start, nothing changed in {}:\n{err}",
                into.display()
            );
        } else {
            let _ = git(&into, &["merge", "--abort"]);
            println!(
                "These files changed both here and in {}, so the merge was stopped and nothing changed there:\n{conflicts}",
                into.display()
            );
        }
        println!(
            "The work is kept on branch {branch} in {wt}. To resolve it there: git merge {target}, fix the conflicts, commit, then run:\n  hivey swarm session --finish {} --into {}",
            shell_quote(&wt),
            shell_quote(&into.display().to_string())
        );
        return Ok(false);
    }
    println!("Merged {branch} into {} ({target}).", into.display());
    remove_worktree(&into, worktree, &branch, false).map(|()| true)
}

/// The repository's main checkout: the first entry of `git worktree list`.
fn main_checkout(worktree: &Path) -> Result<PathBuf, String> {
    git(worktree, &["worktree", "list", "--porcelain"])?
        .lines()
        .find_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .ok_or_else(|| format!("no main checkout for {}", worktree.display()))
}

/// Removes the worktree and its branch (`discard`: even with unmerged work in them).
fn remove_worktree(
    repo: &Path,
    worktree: &Path,
    branch: &str,
    discard: bool,
) -> Result<(), String> {
    let wt = worktree.display().to_string();
    let mut remove = vec!["worktree", "remove"];
    if discard {
        remove.push("--force");
    }
    remove.push(&wt);
    git(repo, &remove)?;
    git(repo, &["branch", if discard { "-D" } else { "-d" }, branch])?;
    println!("Removed the worktree {wt} and branch {branch}.");
    Ok(())
}

/// Asks a yes/no question; an empty answer (or the end of input) is `default`.
fn ask(question: &str, default: bool) -> bool {
    match super::prompt(question) {
        Some(answer) if !answer.is_empty() => matches!(answer.to_lowercase().as_str(), "y" | "yes"),
        _ => default,
    }
}

/// Runs git in `dir`; its trimmed output, or its error message.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::noninteractive_process::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("git: {err}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Err(format!(
        "git {}: {}",
        args.first().copied().unwrap_or_default(),
        if stderr.is_empty() { stdout } else { stderr }
    ))
}

/// The agent among the pane's foreground processes: the process group leader (the CLI the
/// shell started; its helpers, e.g. MCP servers, share the group).
fn agent_process(info: &Value) -> Option<&Value> {
    let processes = info["foreground_processes"].as_array()?;
    let leader = info["foreground_process_group_id"].as_u64();
    processes
        .iter()
        .find(|process| leader.is_some() && process["pid"].as_u64() == leader)
        .or_else(|| processes.first())
}

/// The shell command for a new session of `argv`: the same program and arguments, minus
/// the ones that continue or resume a conversation.
fn fresh_command(argv: &[String]) -> Option<String> {
    let program = argv.first()?;
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    let lead = if RUNNERS.contains(&name) && argv.len() > 1 {
        2
    } else {
        1
    };
    let mut words: Vec<String> = argv[..lead].to_vec();
    words.extend(strip_session_args(&argv[lead..]));
    Some(
        words
            .iter()
            .map(|word| shell_quote(word))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b'='
                )
        })
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn new_session_drops_the_conversation_arguments() {
        let command = fresh_command(&argv(&[
            "claude",
            "--dangerously-skip-permissions",
            "--model",
            "opus",
            "--add-dir",
            "/Users/me/my agent",
            "--continue",
        ]));
        assert_eq!(
            command.as_deref(),
            Some(
                "claude --dangerously-skip-permissions --model opus --add-dir '/Users/me/my agent'"
            )
        );
        assert_eq!(
            fresh_command(&argv(&["claude", "--resume", "abc", "--model", "opus"])).as_deref(),
            Some("claude --model opus")
        );
    }

    #[test]
    fn script_runners_keep_their_script() {
        assert_eq!(
            fresh_command(&argv(&[
                "node",
                "/opt/codex/bin/codex.js",
                "resume",
                "--last"
            ]))
            .as_deref(),
            Some("node /opt/codex/bin/codex.js")
        );
        assert_eq!(fresh_command(&[]), None);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hivey-session-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        crate::worktree::canonical_or_original(&dir)
    }

    fn commit(dir: &Path, file: &str, text: &str) {
        std::fs::write(dir.join(file), text).expect("write file");
        git(dir, &["add", "-A"]).expect("git add");
        git(dir, &["commit", "-m", file]).expect("git commit");
    }

    /// A repository with one commit and an `agent/` subfolder, plus a worktree root.
    fn repo(name: &str) -> (PathBuf, PathBuf) {
        let base = temp_dir(name);
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join("agent")).expect("agent dir");
        git(&repo, &["init", "-q"]).expect("git init");
        git(&repo, &["config", "user.email", "test@example.com"]).expect("git config");
        git(&repo, &["config", "user.name", "test"]).expect("git config");
        commit(&repo, "agent/notes.md", "one\n");
        (repo, base.join("worktrees"))
    }

    #[test]
    fn worktree_session_merges_back_and_cleans_up() {
        let (repo, root) = repo("merge");
        let session = create_worktree(&repo.join("agent"), &root).expect("worktree");
        assert_eq!(session.branch, "hivey-session-1");
        assert_eq!(session.start, session.path.join("agent"));
        assert_eq!(session.path, root.join("repo").join("hivey-session-1"));
        // committed work, and work left uncommitted when the agent exits
        commit(&session.path, "agent/a.md", "a\n");
        std::fs::write(session.start.join("b.md"), "b\n").expect("write b");

        let mut questions = Vec::new();
        let mut yes = |question: &str, _default: bool| {
            questions.push(question.to_string());
            true
        };
        assert_eq!(finish_worktree(&session.path, None, &mut yes), Ok(true));
        assert_eq!(questions.len(), 2, "commit, then merge: {questions:?}");
        assert!(repo.join("agent/a.md").exists() && repo.join("agent/b.md").exists());
        assert!(!session.path.exists());
        assert!(git(
            &repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "refs/heads/hivey-session-1"
            ]
        )
        .is_err());
        let _ = std::fs::remove_dir_all(repo.parent().expect("base"));
    }

    #[test]
    fn conflicting_worktree_session_leaves_the_agent_folder_untouched() {
        let (repo, root) = repo("conflict");
        let session = create_worktree(&repo, &root).expect("worktree");
        commit(&session.path, "agent/notes.md", "from the session\n");
        commit(&repo, "agent/notes.md", "from the agent\n");
        let head = git(&repo, &["rev-parse", "HEAD"]).expect("head");

        let mut yes = |_: &str, _: bool| true;
        assert_eq!(
            finish_worktree(&session.path, Some(repo.clone()), &mut yes),
            Ok(false)
        );
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]).expect("head"), head);
        assert_eq!(git(&repo, &["status", "--short"]).expect("status"), "");
        assert!(session.path.exists(), "the session's work is kept");
        let _ = std::fs::remove_dir_all(repo.parent().expect("base"));
    }

    #[test]
    fn the_agent_is_the_process_group_leader() {
        let info = json!({
            "foreground_process_group_id": 7,
            "foreground_processes": [
                { "pid": 9, "argv": ["node", "mcp-server.js"] },
                { "pid": 7, "argv": ["claude"] },
            ],
        });
        assert_eq!(
            agent_process(&info).map(|p| p["pid"].clone()),
            Some(json!(7))
        );
    }
}

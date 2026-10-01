//! Agent adapters: what differs between the agent CLIs a swarm can run.
//!
//! Supported: Claude Code and Codex. Each adapter knows the CLI's brief file, default
//! arguments, folder-trust answer, how to continue the last conversation in a folder, and
//! the full command that resumes a session after a restart.

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentKind {
    #[default]
    Claude,
    Codex,
}

impl AgentKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" | "claude-code" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// The instructions file the CLI reads from its working directory.
    pub(crate) fn brief_file(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE.md",
            Self::Codex => "AGENTS.md",
        }
    }

    /// Unattended defaults: no approval prompts (the user's damage-control hooks still apply
    /// to Claude; Codex runs without its sandbox, like the swarm's Claude agents).
    pub(crate) fn default_args(self) -> &'static str {
        match self {
            Self::Claude => "--chrome --dangerously-skip-permissions --model opus",
            // --no-daemon: each agent's Codex runs in its own pane process. With the shared
            // daemon, hooks and shell commands see the daemon's environment, not the pane's,
            // so herdr never learns the session (no resume) and the agent's identity is lost.
            Self::Codex => "--dangerously-bypass-approvals-and-sandbox --no-daemon",
        }
    }

    /// Keys that accept the CLI's "trust this folder" dialog (the swarm created the folder).
    pub(crate) fn trust_keys(self) -> &'static [&'static str] {
        match self {
            // Claude Code: move to "Yes, I trust this folder", then confirm.
            Self::Claude => &["down", "enter"],
            // Codex: "1. Trust and continue" is preselected.
            Self::Codex => &["enter"],
        }
    }

    /// The session source herdr's integration reports for this CLI.
    pub(crate) fn herdr_source(self) -> &'static str {
        match self {
            Self::Claude => "herdr:claude",
            Self::Codex => "herdr:codex",
        }
    }

    /// Arguments that start the CLI continuing its latest conversation in this folder.
    pub(crate) fn continue_args(self, launch_args: &[String]) -> Vec<String> {
        let args = strip_session_args(launch_args);
        match self {
            Self::Claude => args.into_iter().chain(["--continue".to_string()]).collect(),
            Self::Codex => ["resume".to_string(), "--last".to_string()]
                .into_iter()
                .chain(args)
                .collect(),
        }
    }

    /// Full command (program first) that resumes `session_id` with the launch flags, within
    /// herdr's limits for reported resume commands; `None` when it can't be expressed.
    pub(crate) fn resume_argv(
        self,
        session_id: &str,
        launch_args: &[String],
    ) -> Option<Vec<String>> {
        let mut argv = match self {
            Self::Claude => vec![
                "claude".to_string(),
                "--resume".to_string(),
                session_id.to_string(),
            ],
            Self::Codex => vec![
                "codex".to_string(),
                "resume".to_string(),
                session_id.to_string(),
            ],
        };
        argv.extend(strip_session_args(launch_args));
        let fits = argv.len() <= 64
            && argv.iter().map(String::len).sum::<usize>() <= 8 * 1024
            && !argv
                .iter()
                .any(|a| a.contains('\'') || a.chars().any(char::is_control));
        fits.then_some(argv)
    }
}

/// Launch arguments without anything that picks a session (`--continue`, `--resume <id>`,
/// Codex's `resume [--last|<id>]` subcommand), so they can be re-applied to another one.
pub(crate) fn strip_session_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = args.iter().peekable();
    // Codex: a leading `resume` subcommand and its target.
    if iter.peek().is_some_and(|arg| *arg == "resume") {
        iter.next();
        if iter
            .peek()
            .is_some_and(|arg| !arg.starts_with('-') || *arg == "--last")
        {
            iter.next();
        }
    }
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--continue" | "-c" | "--last" => {}
            "--resume" | "-r" | "--session-id" => {
                iter.next();
            }
            _ if arg.starts_with("--resume=") || arg.starts_with("--session-id=") => {}
            _ => out.push(arg.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn claude_resume_keeps_flags_and_drops_session_flags() {
        let launch = args(&[
            "--dangerously-skip-permissions",
            "--model",
            "sonnet",
            "--add-dir",
            "/r",
            "--continue",
            "--resume",
            "old",
            "--session-id=x",
        ]);
        assert_eq!(
            AgentKind::Claude.resume_argv("abc", &launch).unwrap(),
            args(&[
                "claude",
                "--resume",
                "abc",
                "--dangerously-skip-permissions",
                "--model",
                "sonnet",
                "--add-dir",
                "/r"
            ])
        );
        assert!(AgentKind::Claude
            .resume_argv("abc", &args(&["--add-dir", "/it's"]))
            .is_none());
    }

    #[test]
    fn codex_resume_is_a_subcommand() {
        let launch = args(&[
            "resume",
            "--last",
            "--dangerously-bypass-approvals-and-sandbox",
            "--model",
            "gpt-5",
        ]);
        assert_eq!(
            AgentKind::Codex.resume_argv("s1", &launch).unwrap(),
            args(&[
                "codex",
                "resume",
                "s1",
                "--dangerously-bypass-approvals-and-sandbox",
                "--model",
                "gpt-5"
            ])
        );
        assert_eq!(
            AgentKind::Codex.continue_args(&args(&["--model", "gpt-5", "resume"])),
            args(&["resume", "--last", "--model", "gpt-5", "resume"]),
            "only a leading resume is a subcommand"
        );
        assert_eq!(
            AgentKind::Claude.continue_args(&args(&["--model", "opus", "--continue"])),
            args(&["--model", "opus", "--continue"])
        );
    }

    #[test]
    fn codex_runs_without_the_shared_daemon() {
        assert!(AgentKind::Codex.default_args().contains("--no-daemon"));
    }

    #[test]
    fn kinds_parse_and_name_their_brief() {
        assert_eq!(AgentKind::parse("codex"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::parse("claude-code"), Some(AgentKind::Claude));
        assert_eq!(AgentKind::parse("gemini"), None);
        assert_eq!(AgentKind::Codex.brief_file(), "AGENTS.md");
        assert_eq!(AgentKind::default(), AgentKind::Claude);
    }
}

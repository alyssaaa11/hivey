//! `hiver skills`: the skills library swarm and agent creators pick each agent's skills from,
//! and skills.sh (`npx skills`) for skills the library doesn't have. Skills always go into the
//! agent's own folder (`<agent>/.claude/skills`), never the global Claude Code skills; only
//! `find-skills` is global (install.sh adds it).

use crate::swarm::skills_library as library;
use std::path::Path;

const HELP: &str = "\
usage: hiver skills                          the library folder, its skills count, online search
       hiver skills list [--grep WORD] [--json]
                                             the library's skills (name and description)
       hiver skills dir <folder>             use another library folder (default ~/SKILLS)
       hiver skills online on|off            may creators search skills.sh for missing skills
       hiver skills copy <agent folder> <skill>...
                                             copy library skills into <agent folder>/.claude/skills
       hiver skills guide                    the instructions creators follow to pick and install
                                             each agent's skills (from the chosen skills plugin)
       hiver skills providers [--default ID] installed skills plugins (* = used)
       hiver skills find <query>             search skills.sh (npx skills find; how to judge the
                                             results: https://www.skills.sh/vercel-labs/skills/find-skills)
       hiver skills add <agent folder> <package> [--skill NAME]
                                             install a skills.sh skill into that agent only
  Also in hiver settings → skills. Swarm and agent creators use these to give every agent the
  skills its task needs; only find-skills is installed globally.";

/// The command line for the skills.sh CLI.
fn npx_skills() -> std::process::Command {
    let mut command = std::process::Command::new("npx");
    command.args(["-y", "skills"]);
    command
}

fn status(dir: &Path) -> i32 {
    let count = library::list(dir).len();
    let exists = if dir.is_dir() { "" } else { " (missing)" };
    println!("skills library: {}{exists}, {count} skills", dir.display());
    println!(
        "online search (skills.sh, asks before installing): {}",
        if library::online() { "on" } else { "off" }
    );
    let global = std::env::var_os("HOME")
        .map(|home| Path::new(&home).join(".claude/skills/find-skills/SKILL.md"))
        .is_some_and(|path| path.is_file());
    if !global {
        println!(
            "find-skills is not installed globally: npx -y skills add vercel-labs/skills \
             --skill find-skills -g -a claude-code -y"
        );
    }
    0
}

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    let dir = library::dir();
    let code = match args.first().map(String::as_str) {
        None | Some("status") => status(&dir),
        Some("help" | "--help" | "-h") => {
            println!("{HELP}");
            0
        }
        Some("list") => {
            let json = args.iter().any(|arg| arg == "--json");
            let grep = args
                .iter()
                .position(|arg| arg == "--grep")
                .and_then(|index| args.get(index + 1))
                .map(|word| word.to_lowercase());
            let skills: Vec<_> = library::list(&dir)
                .into_iter()
                .filter(|skill| {
                    grep.as_ref().is_none_or(|word| {
                        skill.name.to_lowercase().contains(word)
                            || skill.description.to_lowercase().contains(word)
                    })
                })
                .collect();
            if json {
                let items: Vec<_> = skills
                    .iter()
                    .map(|skill| {
                        serde_json::json!({ "name": skill.name, "description": skill.description })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&items).unwrap_or_default()
                );
            } else {
                for skill in &skills {
                    let short: String = skill.description.chars().take(110).collect();
                    println!("{:<28} {short}", skill.name);
                }
                if skills.is_empty() {
                    println!("no skills found in {}", dir.display());
                }
            }
            0
        }
        Some("guide") => {
            let (provider, text) = library::guide();
            println!("<!-- skills plugin: {provider} -->\n{text}");
            0
        }
        Some("providers") => {
            let providers = library::providers();
            if let Some(index) = args.iter().position(|arg| arg == "--default") {
                let Some(id) = args.get(index + 1) else {
                    eprintln!("usage: hiver skills providers --default ID");
                    return Ok(2);
                };
                if !providers.iter().any(|provider| &provider.id == id) {
                    eprintln!("hiver skills: {id:?} is not an installed skills plugin");
                    return Ok(1);
                }
                library::set_provider(id)?;
                println!("skills plugin: {id}");
                return Ok(0);
            }
            let current = library::pick_provider(&providers, library::chosen_provider().as_deref())
                .map(|provider| provider.id.clone());
            for provider in &providers {
                let mark = if current.as_deref() == Some(provider.id.as_str()) { "*" } else { " " };
                println!("{mark} {:<24} {}", provider.id, provider.name);
            }
            if providers.is_empty() {
                println!(
                    "no skills plugins installed: the built-in instructions are used \
                     (hiver plugin link <hiver repo>/plugins/skills)"
                );
            }
            0
        }
        Some("dir") => match args.get(1) {
            Some(folder) => {
                let path = library::expand(folder);
                if !path.is_dir() {
                    eprintln!("hiver skills: {} is not a folder", path.display());
                    1
                } else {
                    library::set_dir(&path)?;
                    println!(
                        "skills library: {} ({} skills)",
                        path.display(),
                        library::list(&path).len()
                    );
                    0
                }
            }
            None => {
                println!("{}", dir.display());
                0
            }
        },
        Some("online") => match args.get(1).map(String::as_str) {
            Some(value @ ("on" | "off")) => {
                library::set_online(value == "on")?;
                println!("online search: {value}");
                0
            }
            _ => {
                eprintln!("usage: hiver skills online on|off");
                2
            }
        },
        Some("copy") if args.len() >= 3 => {
            match library::copy_into(&dir, &library::expand(&args[1]), &args[2..]) {
                Ok(copied) => {
                    println!(
                        "copied into {}/.claude/skills: {}",
                        args[1],
                        copied.join(", ")
                    );
                    0
                }
                Err(err) => {
                    eprintln!("hiver skills: {err}");
                    1
                }
            }
        }
        Some("find") if args.len() >= 2 => npx_skills()
            .arg("find")
            .args(&args[1..])
            .status()
            .map_or(1, |status| status.code().unwrap_or(1)),
        Some("add") if args.len() >= 3 => {
            // Project-level install run from the agent's folder: lands in its .claude/skills.
            let agent = library::expand(&args[1]);
            if !agent.is_dir() {
                eprintln!("hiver skills: {} is not a folder", agent.display());
                return Ok(1);
            }
            npx_skills()
                .arg("add")
                .args(&args[2..])
                .args(["-a", "claude-code", "-y", "--copy"])
                .current_dir(&agent)
                .status()
                .map_or(1, |status| status.code().unwrap_or(1))
        }
        _ => {
            eprintln!("{HELP}");
            2
        }
    };
    Ok(code)
}

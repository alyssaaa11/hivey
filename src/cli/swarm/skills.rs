//! `hivey skills`: how swarm and agent creators give each agent its skills: from skylls (the
//! user's published skills and the ones friends shared), and skills.sh (`npx skills`) for
//! skills skylls doesn't have. Skills always go into the
//! agent's own folder (`<agent>/.claude/skills`), never the global Claude Code skills; only
//! `find-skills` is global (install.sh adds it).

use crate::swarm::skills_library as library;
use std::path::Path;

const HELP: &str = "\
usage: hivey skills                          is skylls installed, online search
       hivey skills online on|off            may creators search skills.sh for missing skills
       hivey skills guide                    the instructions creators follow to pick and install
                                             each agent's skills (from the chosen skills plugin)
       hivey skills providers [--default ID] installed skills plugins (* = used)
       hivey skills find <query>             search skills.sh (npx skills find; how to judge the
                                             results: https://www.skills.sh/vercel-labs/skills/find-skills)
       hivey skills add <agent folder> <package> [--skill NAME]
                                             install a skills.sh skill into that agent only
  The library is skylls: skylls --json find <words>, then in the agent's folder
  skylls add <name> -a claude. Also in hivey settings → skills. Only find-skills is
  installed globally.";

/// What `hivey skills` says when skylls is missing.
const SKYLLS_INSTALL: &str =
    "bash -c \"$(curl -fsSL https://raw.githubusercontent.com/jcsancho/skylls/main/install.sh)\"";

/// The command line for the skills.sh CLI.
fn npx_skills() -> std::process::Command {
    let mut command = std::process::Command::new("npx");
    command.args(["-y", "skills"]);
    command
}

fn status() -> i32 {
    if library::skylls_installed() {
        println!("skills library: skylls (skylls --json find <words>)");
    } else {
        println!("skills library: skylls, not installed: {SKYLLS_INSTALL}");
    }
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
    let code = match args.first().map(String::as_str) {
        None | Some("status") => status(),
        Some("help" | "--help" | "-h") => {
            println!("{HELP}");
            0
        }
        Some(old @ ("list" | "dir" | "copy")) => {
            eprintln!(
                "hivey skills {old}: the skills folder is gone, skills come from skylls: \
                 skylls --json find <words>, then in the agent's folder skylls add <name> -a claude"
            );
            2
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
                    eprintln!("usage: hivey skills providers --default ID");
                    return Ok(2);
                };
                if !providers.iter().any(|provider| &provider.id == id) {
                    eprintln!("hivey skills: {id:?} is not an installed skills plugin");
                    return Ok(1);
                }
                library::set_provider(id)?;
                println!("skills plugin: {id}");
                return Ok(0);
            }
            let current = library::pick_provider(&providers, library::chosen_provider().as_deref())
                .map(|provider| provider.id.clone());
            for provider in &providers {
                let mark = if current.as_deref() == Some(provider.id.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {:<24} {}", provider.id, provider.name);
            }
            if providers.is_empty() {
                println!(
                    "no skills plugins installed: the built-in instructions are used \
                     (hivey plugin link <hivey repo>/plugins/skills)"
                );
            }
            0
        }
        Some("online") => match args.get(1).map(String::as_str) {
            Some(value @ ("on" | "off")) => {
                library::set_online(value == "on")?;
                println!("online search: {value}");
                0
            }
            _ => {
                eprintln!("usage: hivey skills online on|off");
                2
            }
        },
        Some("find") if args.len() >= 2 => npx_skills()
            .arg("find")
            .args(&args[1..])
            .status()
            .map_or(1, |status| status.code().unwrap_or(1)),
        Some("add") if args.len() >= 3 => {
            // Project-level install run from the agent's folder: lands in its .claude/skills.
            let agent = library::expand(&args[1]);
            if !agent.is_dir() {
                eprintln!("hivey skills: {} is not a folder", agent.display());
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

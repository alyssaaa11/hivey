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
    /// xAI's Grok Build CLI (`grok`).
    Grok,
    /// Google's Antigravity CLI (`agy`, Gemini).
    Agy,
}

impl AgentKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" | "claude-code" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "grok" | "grok-build" => Some(Self::Grok),
            "agy" | "antigravity" | "antigravity-cli" | "gemini" => Some(Self::Agy),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Grok => "grok",
            Self::Agy => "agy",
        }
    }

    /// Every kind, for help texts and validation messages.
    pub(crate) const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::Grok, Self::Agy];

    /// "claude, codex, grok or agy".
    pub(crate) fn names() -> String {
        let names: Vec<&str> = Self::ALL.iter().map(|kind| kind.as_str()).collect();
        format!(
            "{} or {}",
            names[..names.len() - 1].join(", "),
            names[names.len() - 1]
        )
    }

    /// The manifest field that keeps this kind's launch flags (relaunch reads them back).
    pub(crate) fn args_field(self) -> &'static str {
        match self {
            Self::Claude => "claude_args",
            Self::Codex => "codex_args",
            Self::Grok => "grok_args",
            Self::Agy => "agy_args",
        }
    }

    /// The instructions file the CLI reads from its working directory.
    pub(crate) fn brief_file(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE.md",
            // Grok also reads CLAUDE.md, but AGENTS.md is its own; agy reads AGENTS.md and
            // GEMINI.md (see `extra_brief_files`).
            Self::Codex | Self::Grok | Self::Agy => "AGENTS.md",
        }
    }

    /// More names the CLI reads its instructions from, linked to the brief like `brief_file`.
    pub(crate) fn extra_brief_files(self) -> &'static [&'static str] {
        match self {
            Self::Agy => &["GEMINI.md"],
            _ => &[],
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
            Self::Grok => "--always-approve",
            Self::Agy => "--dangerously-skip-permissions",
        }
    }

    /// Keys that accept the CLI's "trust this folder" dialog (the swarm created the folder).
    pub(crate) fn trust_keys(self) -> &'static [&'static str] {
        match self {
            // Claude Code: move to "Yes, I trust this folder", then confirm.
            Self::Claude => &["down", "enter"],
            // Codex: "1. Trust and continue" is preselected.
            Self::Codex => &["enter"],
            // Grok: "Yes, proceed  y".
            Self::Grok => &["y"],
            // agy: "> Yes, I trust this folder" is preselected.
            Self::Agy => &["enter"],
        }
    }

    /// Whether the CLI accepts `--add-dir <dir>` (more folders it may work in).
    pub(crate) fn takes_add_dir(self) -> bool {
        !matches!(self, Self::Grok)
    }

    /// Whether the screen shows this CLI's folder-trust question.
    pub(crate) fn asks_for_trust(self, screen: &str) -> bool {
        let screen = screen.to_lowercase();
        // Claude Code, Codex, agy: "…trust this folder"; Grok: "Do you trust the contents of
        // this directory?"; agy also: "Do you trust the contents of this project?".
        ["trust this folder", "trust the contents of this"]
            .iter()
            .any(|question| screen.contains(question))
    }

    /// The session source herdr's integration reports for this CLI.
    pub(crate) fn herdr_source(self) -> &'static str {
        match self {
            Self::Claude => "herdr:claude",
            Self::Codex => "herdr:codex",
            Self::Grok => "herdr:grok",
            Self::Agy => "herdr:antigravity_cli",
        }
    }

    /// Arguments that start the CLI continuing its latest conversation in this folder.
    pub(crate) fn continue_args(self, launch_args: &[String]) -> Vec<String> {
        let args = strip_session_args(launch_args);
        match self {
            Self::Claude | Self::Grok | Self::Agy => {
                args.into_iter().chain(["--continue".to_string()]).collect()
            }
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
            Self::Grok => vec![
                "grok".to_string(),
                "--resume".to_string(),
                session_id.to_string(),
            ],
            Self::Agy => vec![
                "agy".to_string(),
                "--conversation".to_string(),
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
/// agy's `--conversation <id>`, Codex's `resume [--last|<id>]` subcommand), so they can be
/// re-applied to another one.
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
            "--resume" | "-r" | "--session-id" | "--conversation" => {
                iter.next();
            }
            _ if arg.starts_with("--resume=")
                || arg.starts_with("--session-id=")
                || arg.starts_with("--conversation=") => {}
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
    fn grok_and_agy_start_unattended_and_resume() {
        assert_eq!(AgentKind::parse("grok"), Some(AgentKind::Grok));
        assert_eq!(AgentKind::parse("gemini"), Some(AgentKind::Agy));
        assert_eq!(AgentKind::names(), "claude, codex, grok or agy");
        let launch = args(&["--always-approve", "--continue"]);
        assert_eq!(
            AgentKind::Grok.resume_argv("g1", &launch).unwrap(),
            args(&["grok", "--resume", "g1", "--always-approve"])
        );
        let launch = args(&["--dangerously-skip-permissions", "--conversation", "old"]);
        assert_eq!(
            AgentKind::Agy.resume_argv("a1", &launch).unwrap(),
            args(&[
                "agy",
                "--conversation",
                "a1",
                "--dangerously-skip-permissions"
            ])
        );
        assert_eq!(
            AgentKind::Agy.continue_args(&args(&["--model", "m"])),
            args(&["--model", "m", "--continue"])
        );
        assert!(AgentKind::Grok.asks_for_trust("Do you trust the contents of this directory?"));
        assert!(AgentKind::Agy.asks_for_trust("> Yes, I trust this folder"));
        assert!(!AgentKind::Grok.asks_for_trust("ready"));
        assert_eq!(AgentKind::Agy.extra_brief_files(), &["GEMINI.md"]);
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
        assert_eq!(AgentKind::parse("gemini"), Some(AgentKind::Agy));
        assert_eq!(AgentKind::parse("cursor"), None);
        assert_eq!(AgentKind::Codex.brief_file(), "AGENTS.md");
        assert_eq!(AgentKind::default(), AgentKind::Claude);
    }
}

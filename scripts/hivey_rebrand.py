#!/usr/bin/env python3
"""Rebrand herdr's user-facing text to hivey in src/ and tests/ (idempotent; `hivey update --check` runs it
after every herdr merge, after scripts/hivey_hooks.py).

What changes: CLI help, usage and messages (`herdr pane …` → `hivey pane …`, "restart the Herdr
server" → "restart the hivey server"). What stays herdr, on purpose: HERDR_* env vars,
herdr-plugin.toml, socket/log file names, `herdr:<agent>` hook tags, markers written into other
agents' config files, and text about SSH remote hosts (they install herdr itself).

Usage: python3 scripts/hivey_rebrand.py [--check]
  --check  list the files it would change and exit 1 if any, without writing
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 1. `herdr <subcommand>` in any text (help, usage, messages, comments) → `hivey <subcommand>`.
CMDS = ("pane|server|integration|agent|update|session|plugin|config|api|workspace|tab|terminal|"
        "machine|worktree|channel|status|notification|completion|scrollback|swarm|msg|"
        "--remote|--help|--machine|--skill|--session|--version|--default-config")
COMMAND_RULES = [
    (re.compile(r"(?<![\w/.:-])herdr (?=(?:%s)\b)" % CMDS), "hivey "),
    (re.compile(r"`herdr`"), "`hivey`"),
    (re.compile(r"\b(run|Run|start|Start) herdr again\b"), r"\1 hivey again"),
    (re.compile(r"\bHerdr (TUI|skill|sessions)\b"), r"hivey \1"),
    (re.compile(r"\b(run|restart) Herdr again\b"), r"\1 hivey again"),
    (re.compile(r"\brestart Herdr\b"), "restart hivey"),
    (re.compile(r"\bIf a Herdr server\b"), "If a hivey server"),
]

# 2. herdr/Herdr used as a word inside sentence text of "..." literals → hivey. Whole-literal values
#    ("herdr": config values, binary names, labels) and herdr.sock / herdr-x / herdr:x are left.
STRING = re.compile(r'"(?:[^"\\]|\\.)*"')
PROSE_WORD = re.compile(r'(?<= )[Hh]erdr(?=[ ,;)!?]|\.(?![\w])|$)|(?<=")[Hh]erdr(?= [a-z])')
PROSE_KEEP = ("Herdr API",)
PROSE_SKIP = ("src/cli/machine.rs", "src/cli/spec/machine.rs")  # remote machines run herdr

SKIP_DIRS = ("src/remote/",)  # installs and drives herdr on SSH hosts

# 3. Exact edits applied last, per file: (old, new). They put back strings that must stay herdr
#    after the rules above, and carry the hand-written hivey changes. Each is idempotent.
FIXUPS = {
    "src/integration/mod.rs": [
        # marker blocks already written into users' kimi configs
        ("# >>> hivey kimi integration", "# >>> herdr kimi integration"),
        ("# <<< hivey kimi integration", "# <<< herdr kimi integration"),
    ],
    "src/main.rs": [
        ("(fork of hivey)", "(fork of herdr)"),
        ("Attach through SSH to a remote hivey server", "Attach through SSH to a remote herdr server"),
        ('        println!("Home:   https://herdr.dev");',
         '        println!("Home:   https://github.com/alyssaaa11/hivey"); // hivey'),
    ],
    "src/cli/spec.rs": [
        ("Attach through SSH to a remote hivey server", "Attach through SSH to a remote herdr server"),
        ('    let command = Command::new("herdr")\n', '    let command = Command::new("hivey") // hivey\n'),
        ('    let mut path = vec!["herdr".to_string()];\n',
         '    let mut path = vec!["hivey".to_string()]; // hivey\n'),
        ('let mut args = vec!["herdr".to_string()];', 'let mut args = vec!["hivey".to_string()];'),
        ('"help was not handled for herdr {} {flag}"', '"help was not handled for hivey {} {flag}"'),
        ('format!("Usage: herdr {}", path.join(" "))', 'format!("Usage: hivey {}", path.join(" "))'),
        ('"unexpected help for herdr {}: {output}"', '"unexpected help for hivey {}: {output}"'),
    ],
    "src/update.rs": [
        ("your SSH machines run their own hivey and may be older:",
         "your SSH machines run their own herdr and may be older:"),
        # the command users run to reattach to the default session
        ('            attach_command: Some(if session.default {\n                "herdr".to_string()',
         '            attach_command: Some(if session.default {\n                "hivey".to_string()'),
        # test fixture for the same notice: its input is that reattach command
        ('            stop_command: "hivey server stop",\n            attach_command: Some("herdr"),',
         '            stop_command: "hivey server stop",\n            attach_command: Some("hivey"),'),
    ],
    # logs are filtered by crate name, which is hivey: "herdr=info" logged nothing at all
    "src/logging.rs": [
        ('    let filter =\n        EnvFilter::try_from_env("HERDR_LOG").unwrap_or_else(|_| EnvFilter::new("herdr=info"));',
         '    // hivey: tracing targets are the crate name.\n    let filter =\n        EnvFilter::try_from_env("HERDR_LOG").unwrap_or_else(|_| EnvFilter::new("hivey=info"));'),
    ],
    "src/terminal_effects.rs": [
        ('    let title = title.unwrap_or("herdr");', '    let title = title.unwrap_or("hivey"); // hivey'),
        # the sanitizer test feeds "herdr api" through: its output is not branding
        ('assert_eq!(output, b"\\x1b]0;hivey api\\x07");', 'assert_eq!(output, b"\\x1b]0;herdr api\\x07");'),
        ('assert_eq!(output, b"\\x1b]0;herdr\\x07");', 'assert_eq!(output, b"\\x1b]0;hivey\\x07");'),
    ],
    "src/session.rs": [
        ('        None => "herdr".to_string(),\n    }\n}\n\npub fn local_stop_command',
         '        None => "hivey".to_string(), // hivey: the binary users run\n    }\n}\n\npub fn local_stop_command'),
        ('        assert_eq!(local_attach_command(), "herdr");', '        assert_eq!(local_attach_command(), "hivey");'),
    ],
    # the test feeds "herdr api" through the title sanitizer: not branding
    "src/config/window_title.rs": [('            Some("hivey api")', '            Some("herdr api")')],
    "src/cli.rs": [
        ('''pub(crate) const AGENT_HELP_FOOTER: &str = concat!(
    "Are you an AI? Use these resources ONLY IF your task specifically asks you to:\\n",
    "  Help a human understand or set up hivey for the first time:\\n",
    "    https://herdr.dev/agent-guide.md\\n",
    "  Debug or investigate a problem with Herdr:\\n",
    "    https://herdr.dev/llms.txt\\n",
    "  Control hivey panes, agents, or workspaces:\\n",''',
         '''// hivey: points at hivey's own guide instead of herdr.dev.
pub(crate) const AGENT_HELP_FOOTER: &str = concat!(
    "Are you an AI? Use these resources ONLY IF your task specifically asks you to:\\n",
    "  Help a human understand or set up hivey, or debug a problem with it:\\n",
    "    https://github.com/alyssaaa11/hivey/blob/main/docs/hivey-guide.md\\n",
    "  Control hivey panes, agents, swarms, or workspaces:\\n",'''),
    ],
}


def rebrand(rel, text):
    if not rel.startswith(SKIP_DIRS):
        for rx, rep in COMMAND_RULES:
            text = rx.sub(rep, text)
        if rel not in PROSE_SKIP:
            def prose(m):
                s = m.group(0)
                if any(k in s for k in PROSE_KEEP) or " " not in s.strip('"'):
                    return s
                return PROSE_WORD.sub("hivey", s)
            text = STRING.sub(prose, text)
    for old, new in FIXUPS.get(rel, []):
        text = text.replace(old, new)
    return text


def main():
    check = "--check" in sys.argv[1:]
    changed = []
    # tests/ too: integration tests assert on the same user-facing messages.
    for path in sorted([*ROOT.glob("src/**/*.rs"), *ROOT.glob("tests/**/*.rs")]):
        rel = path.relative_to(ROOT).as_posix()
        text = path.read_text()
        new = rebrand(rel, text)
        if new != text:
            changed.append(rel)
            if not check:
                path.write_text(new)
    for rel in changed:
        print(("would change " if check else "rebranded ") + rel)
    if check and changed:
        sys.exit(1)


if __name__ == "__main__":
    main()

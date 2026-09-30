#!/usr/bin/env python3
"""One-shot P0 rename hooks for the hiver fork (idempotent). Kept for reapplying after upstream rebases."""
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent


def edit(rel, old, new, count=1, required=True):
    p = ROOT / rel
    s = p.read_text()
    if new in s:
        return
    if old not in s:
        if required:
            raise SystemExit(f"{rel}: pattern not found: {old[:60]!r}")
        return
    p.write_text(s.replace(old, new, count))


def sub(rel, pattern, repl, count=0):
    p = ROOT / rel
    p.write_text(re.sub(pattern, repl, p.read_text(), count=count, flags=re.M))


# Package / binary name
sub("Cargo.toml", r'^name = "herdr"$', 'name = "hiver"', 1)
sub("Cargo.toml", r'^description = .*$', 'description = "terminal workspace for swarms of AI coding agents (fork of herdr)"', 1)
sub("Cargo.toml", r'^repository = .*$', 'repository = "https://github.com/jcsancho/hiver"', 1)
sub("Cargo.toml", r'^homepage = .*\n', '', 1)
for t in ROOT.joinpath("tests").rglob("*.rs"):
    s = t.read_text()
    new = s.replace("CARGO_BIN_EXE_herdr", "CARGO_BIN_EXE_hiver")
    # Tests hardcode the app dir name (see app_dir_name below).
    new = new.replace('"herdr-dev"', '"hiver-dev"').replace('"herdr-dev/', '"hiver-dev/')
    new = new.replace('"herdr/config.toml"', '"hiver/config.toml"')
    new = new.replace('join("herdr")', 'join("hiver")')
    new = re.sub(r'^(\s*)"herdr"$', r'\1"hiver"', new, flags=re.M)
    if new != s:
        t.write_text(new)
sub("justfile", r"--bin herdr ", "--bin hiver ")
sub("justfile", r'target\}/release/herdr"', 'target}/release/hiver"')
sub("justfile", r"cargo update -p herdr ", "cargo update -p hiver ")

# Own config / socket / session dirs
edit("src/config/io.rs",
     '''    if cfg!(debug_assertions) {
        "herdr-dev"
    } else {
        "herdr"
    }''',
     '''    // hiver: own config/socket/session dirs so hiver and herdr run side by side.
    if cfg!(debug_assertions) {
        "hiver-dev"
    } else {
        "hiver"
    }''')

# main: module, env isolation, branding
edit("src/main.rs", 'pub(crate) const HERDR_ENV_VALUE: &str = "1";\n',
     'pub(crate) const HERDR_ENV_VALUE: &str = "1";\nmod hiver;\n')
edit("src/main.rs", "fn main() -> io::Result<()> {\n    let raw_args",
     "fn main() -> io::Result<()> {\n    hiver::isolate_from_parent_herdr();\n    let raw_args")
edit("src/main.rs", 'println!("herdr {}", crate::build_info::version());',
     'println!("hiver {} (herdr fork)", crate::build_info::version());')
edit("src/main.rs", 'println!("herdr — terminal workspace manager for AI coding agents");',
     'println!("hiver — terminal workspace for swarms of AI coding agents (fork of herdr)");')

# Panes get HIVER_ENV=1 next to HERDR_ENV=1
marker = "HIVER_ENV_VAR"
for rel in ["src/pane.rs", "src/pty/backend/unix.rs"]:
    p = ROOT / rel
    s = p.read_text()
    if marker not in s:
        s, n = re.subn(
            r"\n(\s*)cmd\.env\(crate::HERDR_ENV_VAR, crate::HERDR_ENV_VALUE\);",
            lambda m: m.group(0) + f"\n{m.group(1)}cmd.env(crate::hiver::HIVER_ENV_VAR, crate::HERDR_ENV_VALUE); // hiver",
            s, count=1)
        if n != 1:
            raise SystemExit(f"{rel}: pane env hook not found")
        p.write_text(s)

# No self-update / background checks against herdr.dev releases
edit("src/update.rs",
     "pub fn self_update(options: SelfUpdateOptions) -> Result<Version, String> {\n",
     "pub fn self_update(options: SelfUpdateOptions) -> Result<Version, String> {\n"
     "    // hiver: never download herdr releases over the hiver binary.\n"
     "    if !cfg!(test) {\n"
     "        let _ = options;\n"
     "        return Err(crate::hiver::SELF_UPDATE_DISABLED.into());\n"
     "    }\n")
edit("src/update.rs",
     "pub fn auto_update(events: tokio::sync::mpsc::Sender<crate::events::AppEvent>) {\n",
     "pub fn auto_update(events: tokio::sync::mpsc::Sender<crate::events::AppEvent>) {\n"
     "    // hiver: no background checks against herdr.dev release manifests.\n"
     "    if !cfg!(test) {\n"
     "        drop(events);\n"
     "        return;\n"
     "    }\n")
print("p0 rename hooks applied")

# ---------------------------------------------------------------------------
# Swarm engine hooks (src/swarm)
# ---------------------------------------------------------------------------
edit("src/main.rs", "mod hiver;\n", "mod hiver;\nmod swarm;\n")
edit("src/api/schema.rs",
     '    #[serde(rename = "agent.view.set")]\n    AgentViewSet(AgentViewSetParams),\n',
     '    // hiver: swarm engine (src/swarm); one variant keeps upstream rebases small.\n'
     '    #[serde(rename = "swarm")]\n    Swarm(crate::swarm::SwarmParams),\n'
     '    #[serde(rename = "agent.view.set")]\n    AgentViewSet(AgentViewSetParams),\n')
edit("src/api/server.rs",
     '        Method::AgentViewSet(_) => "agent.view.set",\n',
     '        Method::Swarm(_) => "swarm", // hiver\n        Method::AgentViewSet(_) => "agent.view.set",\n')
edit("src/api/server.rs",
     "        method_body => {\n            let (response_write_tx, response_write_rx)",
     "        // hiver: swarm ops are answered by the swarm engine, not the app state machine.\n"
     "        Method::Swarm(params) => {\n"
     "            let response = crate::swarm::handle_request(&request_id, &params);\n"
     "            let result = write_text_line_allow_disconnect(&mut stream, &response);\n"
     "            if result.is_ok() {\n"
     "                crate::logging::api_request_completed(\n"
     "                    &request_id,\n"
     "                    method,\n"
     "                    api_response_outcome(&response),\n"
     "                    changes_ui,\n"
     "                );\n"
     "            }\n"
     "            result\n"
     "        }\n"
     "        method_body => {\n            let (response_write_tx, response_write_rx)")
edit("src/api/server.rs",
     "    let running = Arc::new(AtomicBool::new(true));\n    let listener_running = Arc::clone(&running);\n",
     "    let running = Arc::new(AtomicBool::new(true));\n"
     "    #[cfg(not(test))]\n"
     "    crate::swarm::start(api_tx.clone()); // hiver\n"
     "    let listener_running = Arc::clone(&running);\n")
edit("src/api/mod.rs",
     "pub type ApiRequestSender = mpsc::UnboundedSender<ApiRequestMessage>;\n",
     "pub type ApiRequestSender = mpsc::UnboundedSender<ApiRequestMessage>;\n\n"
     "/// hiver: in-process API calls for the swarm engine.\n"
     "pub(crate) fn dispatch_internal(\n"
     "    request: Request,\n"
     "    api_tx: &ApiRequestSender,\n"
     "    timeout: Option<std::time::Duration>,\n"
     ") -> String {\n"
     "    server::dispatch_to_app_with_timeout(request, api_tx, timeout)\n"
     "}\n")
print("swarm engine hooks applied")

# CLI: hiver swarm … / hiver msg …
edit("src/cli.rs", '        "agent" => agent::run_agent_command(&args[2..])?,\n',
     '        "agent" => agent::run_agent_command(&args[2..])?,\n'
     '        "swarm" => swarm::run_swarm_command(&args[2..])?, // hiver\n'
     '        "msg" => swarm::run_msg_command(&args[2..])?, // hiver\n')
p = ROOT / "src/cli.rs"
s = p.read_text()
if "\nmod swarm;" not in s:
    s, n = re.subn(r"^(mod status;\n)", r"\1mod swarm; // hiver\n", s, count=1, flags=re.M)
    if n != 1:
        raise SystemExit("src/cli.rs: mod list anchor not found")
    p.write_text(s)
print("cli hooks applied")

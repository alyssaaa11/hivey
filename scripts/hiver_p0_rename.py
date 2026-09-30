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

#!/usr/bin/env bash
# Install hiver from this checkout.
#
#   git clone git@github.com:jcsancho/hiver.git ~/hiver
#   cd ~/hiver && ./install.sh
#
# Builds hiver, installs it as ~/.local/bin/hiver (HIVER_BIN_DIR to change), installs the hiver
# skill for Claude Code / Codex, links the bundled plugins (dashboard, Slack relay, GitHub,
# team template), turns on the swarm sidebar and Option keys, and sets up the hiver agent in
# ~/.hiver/agent (needs Claude Code). --no-setup skips the last two.
# Run it again any time; later updates are just `hiver update`.
#
# Needs: git, Rust (cargo, via https://rustup.rs), Zig 0.16.0 (https://ziglang.org/download,
# or set ZIG=/path/to/zig), python3. macOS or Linux.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
BIN_DIR="${HIVER_BIN_DIR:-$HOME/.local/bin}"
BIN="$BIN_DIR/hiver"
SETUP=1
for arg in "$@"; do
  case "$arg" in
    --no-setup) SETUP=0 ;;
    -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option $arg (./install.sh [--no-setup])"; exit 2 ;;
  esac
done

say() { printf '\n== %s\n' "$*"; }
fail() { printf 'install: %s\n' "$*" >&2; exit 1; }

say "check requirements"
case "$(uname -s)" in Darwin|Linux) ;; *) fail "hiver installs on macOS or Linux only" ;; esac
command -v git >/dev/null || fail "git not found"
command -v python3 >/dev/null || fail "python3 not found"
command -v cargo >/dev/null || fail "cargo not found: install Rust from https://rustup.rs, then open a new shell"
ZIG_BIN="${ZIG:-zig}"
command -v "$ZIG_BIN" >/dev/null || fail "zig not found: hiver needs Zig 0.16.0 (https://ziglang.org/download), or set ZIG=/path/to/zig"
ZIG_VERSION=$("$ZIG_BIN" version)
[ "$ZIG_VERSION" = "0.16.0" ] || fail "zig $ZIG_VERSION found, hiver needs Zig 0.16.0 exactly (set ZIG=/path/to/zig-0.16.0)"
echo "git, python3, $(cargo --version), zig $ZIG_VERSION: ok"
[ -d .git ] || fail "run this from a git clone of hiver (hiver update needs it)"

# The upstream remote is only for maintainers (hiver update --check merges herdr); never pushed to.
if ! git remote get-url upstream >/dev/null 2>&1; then
  git remote add upstream https://github.com/herdrdev/herdr
  git remote set-url --push upstream DISABLED
fi

say "build (the first build takes a few minutes)"
cargo build --release 2>&1 | grep -vE "external contributor policy|^warning: hiver@" | tail -3
[ -x target/release/hiver ] || fail "build failed"

say "install $BIN"
mkdir -p "$BIN_DIR"
# Never cp over an existing binary: macOS then kills every new exec of it (code signature).
cp target/release/hiver "$BIN.new" && mv -f "$BIN.new" "$BIN"
mkdir -p "$HOME/.local/state/hiver"
git rev-parse --short HEAD >"$HOME/.local/state/hiver/installed-commit"
"$BIN" --version

say "hiver skill (Claude Code / Codex)"
if [ -d "$HOME/.claude" ] || [ -d "$HOME/.codex" ]; then
  "$BIN" skill install
else
  echo "no Claude Code or Codex found: skipped (later: hiver skill install)"
fi

say "bundled plugins"
STARTED_SERVER=0
if ! "$BIN" plugin list >/dev/null 2>&1; then
  # Linking talks to a server: start a headless one just for this.
  "$BIN" server >/tmp/hiver-install-server.log 2>&1 &
  STARTED_SERVER=1
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    "$BIN" plugin list >/dev/null 2>&1 && break
    sleep 1
  done
fi
LINKED=$("$BIN" plugin list 2>/dev/null || true)
for manifest in plugins/*/herdr-plugin.toml; do
  dir="$ROOT/$(dirname "$manifest")"
  id=$(sed -n 's/^id *= *"\(.*\)"/\1/p' "$manifest" | head -1)
  if printf '%s' "$LINKED" | grep -q "^- $id "; then
    echo "  $id: already installed"
  elif "$BIN" plugin link "$dir" >/dev/null 2>&1; then
    echo "  $id: linked"
  else
    echo "  $id: could not link (try later: hiver plugin link $dir)"
  fi
done
if [ "$STARTED_SERVER" = 1 ]; then "$BIN" server stop >/dev/null 2>&1 || true; fi

if [ "$SETUP" = 1 ]; then
  say "swarm sidebar and keys"
  "$BIN" swarm setup

  say "the hiver agent (~/.hiver/agent)"
  if command -v claude >/dev/null; then
    # Turned on for the default session; it starts with hiver (no Slack until --slack).
    "$BIN" home setup --no-start
    echo "Slack: once the hiver.slack-relay token is set, run: hiver home setup --slack"
  else
    echo "Claude Code not found: skipped (later: hiver home setup)"
  fi
fi

say "done"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Add $BIN_DIR to your PATH, e.g.:  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc" ;;
esac
cat <<EOF
Start hiver:        hiver
New swarm:          hiver swarm new "<task>"   (from the project folder)
Update later:       hiver update
EOF

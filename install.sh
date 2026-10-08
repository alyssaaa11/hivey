#!/usr/bin/env bash
# Install hivey from this checkout.
#
#   git clone git@github.com:jcsancho/hivey.git ~/hivey
#   cd ~/hivey && ./install.sh
#
# Builds hivey, installs it as ~/.local/bin/hivey (HIVEY_BIN_DIR to change), installs the hivey
# skill and the global find-skills skill for Claude Code, links the bundled plugins (dashboard,
# Slack relay, GitHub, team template, swarm creator, agent creator, skills), turns on the swarm
# sidebar and Option keys, asks for your Slack bot token, your skills folder (default ~/SKILLS)
# and a desktop pet (macOS), and sets up the hivey agent in ~/.hivey/agent (needs Claude Code;
# Slack channel #hivey when connected). Swarms and agents keep their Obsidian wiki in their own
# folder (<folder>/obsidian), so there is no Obsidian folder to set up.
# --no-setup skips the questions and the hivey agent. Run it again any time; later updates are
# just `hivey update`.
#
# Needs: git, Rust (cargo, via https://rustup.rs), Zig 0.16.0 (https://ziglang.org/download,
# or set ZIG=/path/to/zig), python3, Node.js (npx, for skills.sh). macOS or Linux.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
BIN_DIR="${HIVEY_BIN_DIR:-$HOME/.local/bin}"
BIN="$BIN_DIR/hivey"
SETUP=1
for arg in "$@"; do
  case "$arg" in
    --no-setup) SETUP=0 ;;
    -h|--help) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option $arg (./install.sh [--no-setup])"; exit 2 ;;
  esac
done

say() { printf '\n== %s\n' "$*"; }
fail() { printf 'install: %s\n' "$*" >&2; exit 1; }

LOG_DIR="${TMPDIR:-/tmp}"; LOG_DIR="${LOG_DIR%/}"

say "check requirements"
case "$(uname -s)" in Darwin|Linux) ;; *) fail "hivey installs on macOS or Linux only" ;; esac
command -v git >/dev/null || fail "git not found"
command -v python3 >/dev/null || fail "python3 not found"
command -v cargo >/dev/null || fail "cargo not found: install Rust from https://rustup.rs, then open a new shell"
ZIG_BIN="${ZIG:-zig}"
command -v "$ZIG_BIN" >/dev/null || fail "zig not found: hivey needs Zig 0.16.0 (https://ziglang.org/download), or set ZIG=/path/to/zig"
ZIG_VERSION=$("$ZIG_BIN" version)
[ "$ZIG_VERSION" = "0.16.0" ] || fail "zig $ZIG_VERSION found, hivey needs Zig 0.16.0 exactly (set ZIG=/path/to/zig-0.16.0)"
[ -d .git ] || fail "run this from a git clone of hivey (hivey update needs it)"

. scripts/ensure-rust-toolchain.sh
ensure_rust_toolchain || fail "Rust is not ready (see above)"
echo "git, python3, $(cargo --version), zig $ZIG_VERSION: ok"

# The upstream remote is only for maintainers (hivey update --check merges herdr); never pushed to.
if ! git remote get-url upstream >/dev/null 2>&1; then
  git remote add upstream https://github.com/herdrdev/herdr
  git remote set-url --push upstream DISABLED
fi

say "build (the first build takes a few minutes)"
BUILD_LOG="$LOG_DIR/hivey-install-build.log"
if ! cargo build --release >"$BUILD_LOG" 2>&1; then
  grep -vE "external contributor policy|^warning: hivey@" "$BUILD_LOG" | grep -E -A5 "^error" | head -30 >&2
  fail "build failed (full log: $BUILD_LOG)"
fi
grep -vE "external contributor policy|^warning: hivey@" "$BUILD_LOG" | tail -3
[ -x target/release/hivey ] || fail "build finished but target/release/hivey is missing"

say "install $BIN"
mkdir -p "$BIN_DIR"
# Never cp over an existing binary: macOS then kills every new exec of it (code signature).
cp target/release/hivey "$BIN.new" && mv -f "$BIN.new" "$BIN"
mkdir -p "$HOME/.local/state/hivey"
git rev-parse --short HEAD >"$HOME/.local/state/hivey/installed-commit"
"$BIN" --version

say "hivey skill (Claude Code / Codex)"
if [ -d "$HOME/.claude" ] || [ -d "$HOME/.codex" ]; then
  "$BIN" skill install
else
  echo "no Claude Code or Codex found: skipped (later: hivey skill install)"
fi

# find-skills (skills.sh) is the one skill installed globally: creators use it to find skills an
# agent needs; those skills then go into the agent's own folder, never the global ones.
say "find-skills (global Claude Code skill)"
if [ -f "$HOME/.claude/skills/find-skills/SKILL.md" ]; then
  echo "already installed"
elif ! command -v npx >/dev/null; then
  echo "npx not found (install Node.js): skipped (later: npx -y skills add vercel-labs/skills --skill find-skills -g -a claude-code -y)"
elif npx -y skills add vercel-labs/skills --skill find-skills -g -a claude-code -y >"$LOG_DIR/hivey-find-skills.log" 2>&1; then
  echo "installed"
else
  echo "could not install (see $LOG_DIR/hivey-find-skills.log; later: npx -y skills add vercel-labs/skills --skill find-skills -g -a claude-code -y)"
fi

say "bundled plugins"
STARTED_SERVER=0
if ! "$BIN" plugin list >/dev/null 2>&1; then
  # Linking talks to a server: start a headless one just for this.
  "$BIN" server >/tmp/hivey-install-server.log 2>&1 &
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
    echo "  $id: could not link (try later: hivey plugin link $dir)"
  fi
done
# Stopped on exit: the Slack and hivey-agent steps below talk to it too.
if [ "$STARTED_SERVER" = 1 ]; then trap '"$BIN" server stop >/dev/null 2>&1 || true' EXIT; fi

if [ "$SETUP" = 1 ]; then
  say "swarm sidebar and keys"
  "$BIN" swarm setup

  say "Slack (talk to hivey and your agents from Slack; needed for Slack channels)"
  SLACK=0
  if "$BIN" slack status >/dev/null 2>&1; then
    "$BIN" slack status
    SLACK=1
  elif [ -t 0 ] || (: </dev/tty) 2>/dev/null; then
    # Asked on the terminal even when stdin is redirected (e.g. curl … | bash).
    echo "Slack isn't connected on this machine: hivey needs your Slack app's bot token"
    echo "(xoxb-…, typed hidden; the next step shows how to create the app if you have none)."
    printf 'Connect Slack now? [Y/n] '
    read -r answer </dev/tty || answer=n
    case "$answer" in
      [nN]*) echo "skipped (later: hivey slack connect)" ;;
      *) "$BIN" slack connect </dev/tty && SLACK=1 ;;
    esac
  else
    echo "skipped: no terminal to type the token in (later: hivey slack connect)"
  fi

  say "skills library (where creators pick each new agent's skills)"
  if [ -t 0 ] || (: </dev/tty) 2>/dev/null; then
    SKILLS_DIR=$("$BIN" skills dir 2>/dev/null || echo "$HOME/SKILLS")
    printf 'Your skills folder [%s]: ' "$SKILLS_DIR"
    read -r answer </dev/tty || answer=""
    SKILLS_DIR="${answer:-$SKILLS_DIR}"
    SKILLS_DIR="${SKILLS_DIR/#\~/$HOME}"
    mkdir -p "$SKILLS_DIR"
    "$BIN" skills dir "$SKILLS_DIR" || true
    printf 'Search skills.sh for skills your library lacks (asks before installing any)? [Y/n] '
    read -r answer </dev/tty || answer=""
    case "$answer" in
      [nN]*) "$BIN" skills online off ;;
      *) "$BIN" skills online on ;;
    esac
  else
    echo "skipped: no terminal (default $("$BIN" skills dir 2>/dev/null); later: hivey settings → skills)"
  fi

  say "desktop pet (optional)"
  if [ "$(uname -s)" != Darwin ]; then
    echo "pets are macOS apps: skipped"
  elif [ -t 0 ]; then
    # Builds the chosen pet (about a minute) and starts it; it then shows while hivey is open.
    "$BIN" pet choose || echo "no pet for now (later: hivey pet choose, or ⌥P)"
  else
    echo "skipped: not a terminal (later: hivey pet choose)"
  fi

  say "the hivey agent (~/.hivey/agent)"
  if command -v claude >/dev/null; then
    # Turned on for the default session; it starts with hivey. With Slack: channel #hivey.
    if [ "$SLACK" = 1 ]; then
      "$BIN" home setup --no-start --slack || {
        echo "could not set up Slack #hivey (later: hivey home setup --slack)"
        "$BIN" home setup --no-start
      }
    else
      "$BIN" home setup --no-start
      echo "Slack #hivey later: hivey slack connect, then hivey home setup --slack"
    fi
  else
    echo "Claude Code not found: skipped (later: hivey home setup)"
  fi
fi

say "done"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Add $BIN_DIR to your PATH, e.g.:  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc" ;;
esac
cat <<EOF
Start hivey:        hivey
New swarm:          hivey swarm new "<task>"   (from the project folder)
Update later:       hivey update
EOF
if [ "$SETUP" = 1 ] && [ "$SLACK" = 0 ]; then
  echo
  echo "Slack is NOT connected: agents can't get Slack channels until you run"
  echo "  hivey slack connect      (asks for the Slack bot token, then: hivey home setup --slack)"
fi

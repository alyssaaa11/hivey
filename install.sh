#!/usr/bin/env bash
# Install hivey from this checkout.
#
#   git clone git@github.com:alyssaaa11/hivey.git ~/hivey
#   cd ~/hivey && ./install.sh
#
# Builds hivey, installs it as ~/.local/bin/hivey (HIVEY_BIN_DIR to change), offers the coding
# agents it runs (Claude Code, Codex, Antigravity/Gemini, Grok) and links each one found to hivey,
# installs the hivey skills, commands and hooks (safety on; hivey safety off) and the global
# find-skills skill for Claude Code, links the bundled plugins (dashboard,
# Slack relay, GitHub, team template, swarm creator, agent creator, skills), turns on the swarm
# sidebar and Option keys, asks for your Slack bot token, checks skylls (the skills library)
# and a desktop pet (macOS), and sets up the hivey agent in ~/.hivey/agent (needs Claude Code;
# Slack channel #hivey when connected). Swarms and agents keep their Obsidian wiki in their own
# folder (<folder>/<name>-wiki), so there is no Obsidian folder to set up.
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
    -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option $arg (./install.sh [--no-setup])"; exit 2 ;;
  esac
done

say() { printf '\n== %s\n' "$*"; }
fail() { printf 'install: %s\n' "$*" >&2; exit 1; }

LOG_DIR="${TMPDIR:-/tmp}"; LOG_DIR="${LOG_DIR%/}"

# Everything this run puts on the computer is listed as it happens and again at the end.
CHANGES="$LOG_DIR/hivey-install-changes.$$"
: > "$CHANGES"
tilde() { sed "s#$HOME#~#g"; }
changed() {  # <what> <where>: one change, printed now and kept for the summary
  printf '  %-9s %s\n' "$1" "$2" | tilde | tee -a "$CHANGES"
}
# Runs a hivey command, shows its output, and keeps the lines that say what it changed:
# files new / updated / removed, plugins linked, old copies moved, wikis renamed.
tracked() {
  local out="$LOG_DIR/hivey-install-step.$$" status=0
  "$@" >"$out" 2>&1 || status=$?
  tilde <"$out"
  grep -E '^ +(new|updated|removed) +|^old (copy|hook|plugin) |^plugin .*: linked$|^wiki .*→|^hivey agent brief .*added|^ *(installed|registered) .* (to|at) |written to |^wrote ' "$out" \
    | sed -E 's/^ +//; s/^plugin (.*): linked$/linked    plugin \1/' | tilde | sed 's/^/  /' >>"$CHANGES" || true
  rm -f "$out"
  return "$status"
}

# This machine's own additions to AGENTS.md go here (git ignores it; AGENTS.md points to it).
NEW_EXTRA=0
[ -e EXTRA_AGENTS.md ] || { : > EXTRA_AGENTS.md; NEW_EXTRA=1; }

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

say "the hivey checkout ($ROOT)"
[ "$NEW_EXTRA" = 1 ] && changed new "$ROOT/EXTRA_AGENTS.md (this machine's own additions to AGENTS.md; git ignores it)"
# The upstream remote is only for maintainers (hivey update --check merges herdr); never pushed to.
if ! git remote get-url upstream >/dev/null 2>&1; then
  git remote add upstream https://github.com/herdrdev/herdr
  git remote set-url --push upstream DISABLED
  changed added "git remote upstream → https://github.com/herdrdev/herdr (fetch only, push disabled)"
else
  echo "  git remotes: as they are"
fi

say "build (the first build takes a few minutes)"
BUILD_LOG="$LOG_DIR/hivey-install-build.log"
if ! cargo build --release >"$BUILD_LOG" 2>&1; then
  grep -vE "external contributor policy|^warning: hivey@" "$BUILD_LOG" | grep -E -A5 "^error" | head -30 >&2
  fail "build failed (full log: $BUILD_LOG)"
fi
grep -vE "external contributor policy|^warning: hivey@" "$BUILD_LOG" | tail -3
[ -x target/release/hivey ] || fail "build finished but target/release/hivey is missing"

say "the hivey program"
mkdir -p "$BIN_DIR"
BEFORE=$("$BIN" --version 2>/dev/null || true)
# Never cp over an existing binary: macOS then kills every new exec of it (code signature).
cp target/release/hivey "$BIN.new" && mv -f "$BIN.new" "$BIN"
NOW=$("$BIN" --version)
if [ -z "$BEFORE" ]; then
  changed new "$BIN ($NOW)"
elif [ "$BEFORE" != "$NOW" ]; then
  changed updated "$BIN ($BEFORE → $NOW)"
else
  echo "  $BIN: $NOW (reinstalled)" | tilde
fi
mkdir -p "$HOME/.local/state/hivey"
STATE="$HOME/.local/state/hivey/installed-commit"
[ -e "$STATE" ] || NEW_STATE=1
git rev-parse --short HEAD >"$STATE"
[ "${NEW_STATE:-0}" = 1 ] && changed new "$STATE (which commit is installed)"

say "coding agents hivey runs (Claude Code, Codex, Antigravity/Gemini, Grok)"
# Each one found is linked to hivey (`hivey integration install`) so its panes show working /
# idle / blocked. Missing ones are offered (Claude Code by default: the hivey agent and the
# creators run on it).
ask_yes() {  # question, default (y|n); no terminal: the default without asking
  local answer prompt="[y/N]"
  [ "$2" = y ] && prompt="[Y/n]"
  if [ "$SETUP" = 1 ] && { [ -t 0 ] || (: </dev/tty) 2>/dev/null; }; then
    printf '%s %s ' "$1" "$prompt"
    read -r answer </dev/tty || answer=""
  fi
  case "${answer:-$2}" in [yY]*) return 0 ;; *) return 1 ;; esac
}
js_global() {  # npm package: installed with bun if present, else npm
  if command -v bun >/dev/null; then bun install -g "$1@latest"
  elif command -v npm >/dev/null; then npm install -g "$1@latest"
  else echo "needs bun or npm (Node.js)"; return 1; fi
}
install_claude() { if command -v bun >/dev/null || command -v npm >/dev/null; then js_global @anthropic-ai/claude-code; else curl -fsSL https://claude.ai/install.sh | bash; fi; }
install_codex() { js_global @openai/codex; }
install_agy() { curl -fsSL https://antigravity.google/cli/install.sh | bash; }
install_grok() { curl -fsSL https://x.ai/cli/install.sh | bash; }
for agent in "claude:Claude Code:claude:y" "codex:Codex:codex:n" "agy:Antigravity (Gemini):antigravity-cli:n" "grok:Grok:grok:n"; do
  IFS=: read -r cmd label target default <<<"$agent"
  [ -x "$HOME/.grok/bin/grok" ] && [ "$cmd" = grok ] && export PATH="$HOME/.grok/bin:$PATH"
  if ! command -v "$cmd" >/dev/null; then
    if ask_yes "$label is not installed. Install it?" "$default"; then
      if "install_$cmd"; then
        hash -r
        command -v "$cmd" >/dev/null && changed installed "$label: $(command -v "$cmd")"
      else
        echo "  could not install $label"
      fi
    else
      echo "  $label: not installed (later: rerun ./install.sh)"
      continue
    fi
  fi
  command -v "$cmd" >/dev/null || { echo "  $label: not found after installing (open a new shell, then rerun ./install.sh)"; continue; }
  echo "  $label: $("$cmd" --version 2>/dev/null | head -1)"
  tracked "$BIN" integration install "$target" | sed 's/^/    /' \
    || echo "    could not link $label to hivey (later: hivey integration install $target)"
done

say "hivey skills and commands (Claude Code ~/.claude, Codex ~/.codex)"
if [ -d "$HOME/.claude" ] || [ -d "$HOME/.codex" ]; then
  tracked "$BIN" skill install || echo "  could not install everything (later: hivey skill install)"
else
  echo "  no Claude Code or Codex found: skipped (later: hivey skill install)"
fi

# Reinstalls over an older hivey: bring its leftovers to the current conventions too.
say "hivey hooks, plugins and older leftovers (hivey migrate: safety on, wikis <name>-wiki)"
tracked "$BIN" migrate || echo "  could not migrate everything (later: hivey migrate)"

# find-skills (skills.sh) is the one skill installed globally: creators use it to find skills an
# agent needs; those skills then go into the agent's own folder, never the global ones.
say "find-skills (global Claude Code skill)"
if [ -f "$HOME/.claude/skills/find-skills/SKILL.md" ]; then
  echo "already installed"
elif ! command -v npx >/dev/null; then
  echo "npx not found (install Node.js): skipped (later: npx -y skills add vercel-labs/skills --skill find-skills -g -a claude-code -y)"
elif npx -y skills add vercel-labs/skills --skill find-skills -g -a claude-code -y >"$LOG_DIR/hivey-find-skills.log" 2>&1; then
  changed new "$HOME/.claude/skills/find-skills (from skills.sh)"
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
    changed linked "plugin $id"
  else
    echo "  $id: could not link (try later: hivey plugin link $dir)"
  fi
done
# Stopped on exit: the Slack and hivey-agent steps below talk to it too.
if [ "$STARTED_SERVER" = 1 ]; then trap '"$BIN" server stop >/dev/null 2>&1 || true' EXIT; fi

if [ "$SETUP" = 1 ]; then
  say "swarm sidebar and keys"
  tracked "$BIN" swarm setup || true

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
      *) "$BIN" slack connect </dev/tty && SLACK=1 && changed new "$HOME/.hivey/slack.json (the Slack bot token)" ;;
    esac
  else
    echo "skipped: no terminal to type the token in (later: hivey slack connect)"
  fi

  say "skills library: skylls (where creators find, save and share skills, agents and swarms)"
  if command -v skylls >/dev/null 2>&1; then
    echo "skylls $(skylls --version 2>/dev/null | awk '{print $NF}') found"
  else
    echo "skylls is not installed; install it with:"
    echo "  bash -c \"\$(curl -fsSL https://raw.githubusercontent.com/jcsancho/skylls/main/install.sh)\""
  fi
  if [ -t 0 ] || (: </dev/tty) 2>/dev/null; then
    printf 'Search skills.sh for skills skylls lacks (asks before installing any)? [Y/n] '
    read -r answer </dev/tty || answer=""
    case "$answer" in
      [nN]*) "$BIN" skills online off && changed set "$HOME/.hivey/skills.json (online skill search off)" ;;
      *) "$BIN" skills online on && changed set "$HOME/.hivey/skills.json (online skill search on)" ;;
    esac
  else
    echo "skipped: no terminal (online search stays on; later: hivey settings → skills)"
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
      tracked "$BIN" home setup --no-start --slack || {
        echo "could not set up Slack #hivey (later: hivey home setup --slack)"
        tracked "$BIN" home setup --no-start
      }
    else
      tracked "$BIN" home setup --no-start
      echo "Slack #hivey later: hivey slack connect, then hivey home setup --slack"
    fi
  else
    echo "Claude Code not found: skipped (later: hivey home setup)"
  fi
fi

say "what this install changed on this computer"
if [ -s "$CHANGES" ]; then
  sort -u "$CHANGES"
else
  echo "  nothing: everything was already up to date"
fi
rm -f "$CHANGES"

say "everything hivey has on this computer (hivey installed)"
"$BIN" installed | tilde || true

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

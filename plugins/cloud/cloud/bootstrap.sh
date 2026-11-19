#!/bin/bash
# hivey cloud bootstrap: runs inside a Claude Code cloud session (SessionStart hook, and from
# mcp-run.sh / mcp-headers.sh when an MCP server starts first). Idempotent and never fails the
# session. It decrypts .hivey-cloud/secrets.age with $HIVEY_CLOUD_AGE_KEY (set on the cloud
# environment), installs the envsave shim and exports the secrets to the shell Claude uses.
set -u
# Cloud sessions only. Claude Code on the user's Mac runs the same SessionStart hook (starting
# `claude --cloud` in a hivey-cloud worktree did), and installing the shim there replaced the real
# envsave. Claude Code sets CLAUDE_CODE_REMOTE=true in its cloud sessions.
[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0
# Never on a Mac, except the tests (they install into a temp HIVEY_CLOUD_BIN).
[ "$(uname -s)" = "Darwin" ] && [ -z "${HIVEY_CLOUD_BIN:-}" ] && exit 0
D="$(cd "$(dirname "$0")" && pwd)"
S="${HOME:-/root}/.hivey-cloud"
mkdir -p "$S" && chmod 700 "$S"
log() { printf '%s %s\n' "$(date +%H:%M:%S)" "$*" >>"$S/bootstrap.log"; }

setup() {
  [ -f "$S/ready" ] && return 0
  # The shim answers envsave get / envsave.get() from the decrypted secrets.
  # HIVEY_CLOUD_BIN / HIVEY_CLOUD_PYSITE: test overrides (defaults: the cloud VM's own).
  bin="${HIVEY_CLOUD_BIN:-/usr/local/bin}"
  install -m 755 "$D/envsave_shim.py" "$bin/envsave" 2>/dev/null \
    || { mkdir -p "$HOME/.local/bin"; install -m 755 "$D/envsave_shim.py" "$HOME/.local/bin/envsave"; }
  site="${HIVEY_CLOUD_PYSITE:-$(python3 -c 'import site; print(site.getsitepackages()[0])' 2>/dev/null)}"
  if [ -n "$site" ] && ! python3 -c 'import envsave' 2>/dev/null; then
    cp "$D/envsave_shim.py" "$site/envsave.py" 2>/dev/null || log "could not install the python shim"
  fi
  if [ -f "$D/secrets.age" ]; then
    if [ -z "${HIVEY_CLOUD_AGE_KEY:-}" ]; then
      log "secrets.age is here but HIVEY_CLOUD_AGE_KEY is not set on this cloud environment"
    else
      if ! command -v age >/dev/null 2>&1; then
        (apt-get install -y -qq age || { apt-get update -qq && apt-get install -y -qq age; }) \
          >>"$S/bootstrap.log" 2>&1
      fi
      umask 077
      printf '%s\n' "$HIVEY_CLOUD_AGE_KEY" >"$S/id"
      if age -d -i "$S/id" -o "$S/secrets.json" "$D/secrets.age" 2>>"$S/bootstrap.log"; then
        log "secrets decrypted"
      else
        log "could not decrypt secrets.age (wrong HIVEY_CLOUD_AGE_KEY?)"
      fi
      rm -f "$S/id"
    fi
  elif [ -f "$D/network.json" ]; then
    cp "$D/network.json" "$S/secrets.json"
  fi
  if [ -f "$S/secrets.json" ]; then
    python3 - "$S/secrets.json" "$S/env" <<'EOF'
import json, shlex, sys
data = json.load(open(sys.argv[1]))
lines = [f"export {k}={shlex.quote(v)}" for k, v in (data.get("vars") or {}).items()]
lines += [f"export {k}={shlex.quote('hivey-network-secret')}" for k in (data.get("network") or {})]
open(sys.argv[2], "w").write("\n".join(lines) + "\n")
EOF
    chmod 600 "$S/secrets.json" "$S/env"
  fi
  # Not ready while encrypted secrets are still undecrypted: the next call tries again.
  if [ ! -f "$D/secrets.age" ] || [ -f "$S/secrets.json" ]; then touch "$S/ready"; fi
}

if command -v flock >/dev/null 2>&1; then
  exec 9>"$S/lock"
  flock 9
fi
setup || log "setup failed"
if [ -n "${CLAUDE_ENV_FILE:-}" ] && [ -f "$S/env" ]; then
  grep -q "hivey-cloud/env" "$CLAUDE_ENV_FILE" 2>/dev/null || echo ". \"$S/env\"" >>"$CLAUDE_ENV_FILE"
fi
exit 0

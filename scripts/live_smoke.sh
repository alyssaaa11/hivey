#!/usr/bin/env bash
# Live smoke test for the hiver message bus with real Claude agents (Haiku, cheap).
# Sets up a throwaway 2-agent swarm $SLUG (coordinator + scout) in $ROOT on the
# running hiver server. Usage: scripts/live_smoke.sh [setup|teardown]
set -euo pipefail
H=${HIVER:-hiver}
ROOT=${ROOT:-/tmp/hiver-live}
MODEL=${MODEL:-haiku}
SLUG=${SLUG:-live}

json() { python3 -c "import sys,json;d=json.load(sys.stdin);print($1)"; }

start_agent() { # name pane
  local name=$1 pane=$2
  $H agent start "$name" --kind claude --pane "$pane" --timeout 60000 -- \
    --model "$MODEL" --dangerously-skip-permissions >/dev/null 2>&1 || true
  for _ in 1 2 3; do
    screen=$($H agent read "$name" --source visible 2>/dev/null || true)
    case "$screen" in
      *"trust this folder"*|*"Yes, I accept"*) $H agent send-keys "$name" down enter >/dev/null; sleep 4 ;;
      *) break ;;
    esac
  done
  $H agent wait "$name" --timeout 60000 >/dev/null 2>&1 || true
  echo "$name: $($H agent get "$name" | json "d['result']['agent']['agent_status']")"
}

brief() { # key role
  cat > "$ROOT/$1/CLAUDE.md" <<EOF
# $1 — hiver smoke-test swarm "$SLUG"

You are the **$1** ($2) of a tiny test swarm. Teammates: coordinator (master), scout (worker).
Messages from teammates arrive in your prompt starting with "[hiver". To answer, run:

    hiver msg send <agent> "<text>"        # e.g. hiver msg send coordinator "done"

Keep every reply to one short sentence. Never do other work. End each turn with DONE.
EOF
}

setup() {
  mkdir -p "$ROOT/.swarm" "$ROOT/coordinator" "$ROOT/scout"
  cat > "$ROOT/.swarm/agents.json" <<EOF
{
  "slug": "$SLUG",
  "coordinator": "$SLUG-coordinator",
  "agents": {
    "coordinator": {"herdr_name": "$SLUG-coordinator", "role": "master", "model": "$MODEL"},
    "scout": {"herdr_name": "$SLUG-scout", "model": "$MODEL"}
  }
}
EOF
  brief coordinator master
  brief scout worker
  $H swarm import "$ROOT"
  local ws p1 p2
  ws=$($H workspace create --cwd "$ROOT/coordinator" --label "$SLUG" --no-focus)
  p1=$(echo "$ws" | json "d['result']['root_pane']['pane_id']")
  p2=$($H pane split "$p1" --direction right --cwd "$ROOT/scout" --no-focus | json "d['result']['pane']['pane_id']")
  start_agent "$SLUG-coordinator" "$p1"
  start_agent "$SLUG-scout" "$p2"
  $H swarm list
}

teardown() {
  $H swarm forget "$SLUG" || true
  for name in "$SLUG-scout" "$SLUG-coordinator"; do
    pane=$($H agent get "$name" 2>/dev/null | json "d['result']['agent']['pane_id']" 2>/dev/null) || continue
    ws=${pane%%:*}
    $H workspace close "$ws" >/dev/null 2>&1 || true
  done
}

case "${1:-setup}" in
  setup) setup ;;
  teardown) teardown ;;
  *) echo "usage: $0 [setup|teardown]" >&2; exit 2 ;;
esac

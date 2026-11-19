#!/bin/bash
# hivey cloud: start an MCP server with the env vars hivey moved into the encrypted secrets.
# usage (from .mcp.json): bash .hivey-cloud/mcp-run.sh <server> -- <command> [args...]
D="$(cd "$(dirname "$0")" && pwd)"
name="$1"; shift; [ "${1:-}" = "--" ] && shift
bash "$D/bootstrap.sh" >/dev/null 2>&1
f="${HOME:-/root}/.hivey-cloud/secrets.json"
if [ -f "$f" ]; then
  eval "$(python3 - "$f" "$name" <<'EOF'
import json, shlex, sys
env = (json.load(open(sys.argv[1])).get("mcp") or {}).get(sys.argv[2], {}).get("env") or {}
print("\n".join(f"export {k}={shlex.quote(str(v))}" for k, v in env.items()))
EOF
)"
fi
exec "$@"

#!/bin/bash
# hivey cloud: headersHelper for an HTTP MCP server whose headers hivey moved into the
# encrypted secrets. Prints the headers as JSON. usage: bash .hivey-cloud/mcp-headers.sh <server>
D="$(cd "$(dirname "$0")" && pwd)"
bash "$D/bootstrap.sh" >/dev/null 2>&1
python3 - "${HOME:-/root}/.hivey-cloud/secrets.json" "$1" <<'EOF'
import json, sys
try:
    data = json.load(open(sys.argv[1]))
except (OSError, ValueError):
    data = {}
print(json.dumps((data.get("mcp") or {}).get(sys.argv[2], {}).get("headers") or {}))
EOF

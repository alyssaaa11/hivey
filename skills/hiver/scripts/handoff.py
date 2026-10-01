#!/usr/bin/env python3
"""Live-handoff a hiver session's server to the installed binary; panes and agents keep running.

usage: handoff.py [<session>]     (default session when omitted)
"""
import json
import socket
import sys
from pathlib import Path

name = sys.argv[1] if len(sys.argv) > 1 else "default"
base = Path.home() / ".config" / "hiver"
sock_path = base / "herdr.sock" if name == "default" else base / "sessions" / name / "herdr.sock"
if not sock_path.exists():
    sys.exit(f"no running session {name!r} ({sock_path})")

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(30)
s.connect(str(sock_path))
s.sendall((json.dumps({"id": "hiver:handoff", "method": "server.live_handoff", "params": {}}) + "\n").encode())
reply = b""
while not reply.endswith(b"\n"):
    chunk = s.recv(65536)
    if not chunk:
        break
    reply += chunk
print(reply.decode().strip() or "no reply")

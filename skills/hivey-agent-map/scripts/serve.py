"""Local server for the agent map pages, so their Refresh button can rerun the scan.

`hivey agent-map open` starts it (once; later opens reuse it) and opens the pages from
http://127.0.0.1:<port>/latest/. It listens on 127.0.0.1 only, serves ~/.hivey/agent-map/reports, and
has one action: POST /hv-refresh runs `agent_map.py refresh` (free: no model) and only answers the
pages it serves (Origin check), so other web pages can't trigger it. It stops after 2 hours unused.
Python standard library only.
"""
from __future__ import annotations

import http.server
import json
import os
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

STATE = Path.home() / ".hivey" / "agent-map"
REPORTS = STATE / "reports"
INFO = STATE / "server.json"
SCRIPTS = Path(__file__).resolve().parent
PREFERRED_PORT = 8754
IDLE_SECONDS = 2 * 3600
PING = "/hv-ping"
MARK = b"hivey-agent-map"


def running() -> int | None:
    try:
        port = int(json.loads(INFO.read_text())["port"])
    except (OSError, ValueError, KeyError, TypeError):
        return None
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}{PING}", timeout=1) as r:
            return port if r.read() == MARK else None
    except OSError:
        return None


def ensure() -> int | None:
    """The running server's port, starting one in the background when there is none."""
    port = running()
    if port:
        return port
    STATE.mkdir(parents=True, exist_ok=True)
    with open(STATE / "server.log", "a") as fh:
        subprocess.Popen([sys.executable, "-I", str(SCRIPTS / "agent_map.py"), "serve"], stdin=subprocess.DEVNULL,
                         stdout=fh, stderr=fh, start_new_session=True)
    for _ in range(50):
        time.sleep(0.1)
        port = running()
        if port:
            return port
    return None


class Handler(http.server.SimpleHTTPRequestHandler):
    server_version = "hivey-agent-map"
    busy = threading.Lock()
    last_used = time.time()

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(REPORTS), **kwargs)

    def log_message(self, *args) -> None:  # quiet
        pass

    def end_headers(self) -> None:
        self.send_header("Cache-Control", "no-store")  # a refresh must show the new pages
        super().end_headers()

    def answer(self, code: int, body: bytes, kind: str = "application/json") -> None:
        self.send_response(code)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        Handler.last_used = time.time()
        if self.path == PING:
            return self.answer(200, MARK, "text/plain")
        return super().do_GET()

    def do_POST(self) -> None:
        Handler.last_used = time.time()
        port = self.server.server_address[1]
        if self.path != "/hv-refresh":
            return self.answer(404, b'{"ok":false,"error":"not found"}')
        if self.headers.get("Origin") not in (f"http://127.0.0.1:{port}", f"http://localhost:{port}"):
            return self.answer(403, b'{"ok":false,"error":"only the agent map pages can refresh"}')
        start = time.time()
        with Handler.busy:  # two clicks: the second waits and then sees the fresh pages
            try:
                p = subprocess.run([sys.executable, "-I", str(SCRIPTS / "agent_map.py"), "refresh"],
                                   capture_output=True, text=True, timeout=900)
                ok, err = p.returncode == 0, (p.stderr or p.stdout).strip()[-300:]
            except (OSError, subprocess.SubprocessError) as e:
                ok, err = False, str(e)
        Handler.last_used = time.time()
        body = {"ok": ok, "seconds": round(time.time() - start)}
        if not ok:
            body["error"] = err or "the scan failed (see ~/.hivey/agent-map/reports/latest/nightly.log)"
        return self.answer(200, json.dumps(body).encode())


def serve() -> int:
    REPORTS.mkdir(parents=True, exist_ok=True)
    try:
        httpd = http.server.ThreadingHTTPServer(("127.0.0.1", PREFERRED_PORT), Handler)
    except OSError:  # taken: any free port
        httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    port = httpd.server_address[1]
    INFO.write_text(json.dumps({"port": port, "pid": os.getpid()}))

    def watchdog() -> None:
        while True:
            time.sleep(60)
            if time.time() - Handler.last_used > IDLE_SECONDS and not Handler.busy.locked():
                httpd.shutdown()
                return

    threading.Thread(target=watchdog, daemon=True).start()
    print(f"agent map pages: http://127.0.0.1:{port}/latest/agent-stats.html", flush=True)
    httpd.serve_forever()
    try:
        if json.loads(INFO.read_text()).get("pid") == os.getpid():
            INFO.unlink()
    except (OSError, ValueError):
        pass
    return 0

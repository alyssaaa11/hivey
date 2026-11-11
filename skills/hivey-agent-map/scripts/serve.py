"""Local server for the agent map pages, so their Refresh button can rerun the scan.

`hivey agent-map open` starts it (once; later opens reuse it) and opens the pages from
http://127.0.0.1:<port>/latest/. It listens on 127.0.0.1 only, serves ~/.hivey/agent-map/reports, and
has these actions, answered only for the pages it serves (Origin check), so other web pages can't
trigger them:

  POST /hv-refresh          `agent_map.py refresh` (free: no model)
  POST /hv-office-refresh   `office.py run` in the background (the agent office, /office/)
  POST /hv-office-action    {"pane": "wN:p1", "action": "focus" | "compact" | "clear"} for a desk on the
                            office page: focus opens the agent's terminal in hivey (and brings the
                            terminal app forward); compact/clear type /compact or /clear into an agent
                            that is not working

While an office page is open (it polls /office/office.json), the office is refreshed every 5 minutes.
It stops after 2 hours unused. Python standard library only.
"""
from __future__ import annotations

import http.server
import json
import os
import re
import shutil
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
PREFERRED_PORT = 42817  # unregistered and below macOS's temporary-port range, so no other app picks it
IDLE_SECONDS = 2 * 3600
PING = "/hv-ping"
MARK = b"hivey-agent-map"
VERSION = b"office-2"  # bump when the server gains actions, so `ensure` replaces an older one
OFFICE_EVERY = 300  # seconds between office refreshes while its page is open
OFFICE_WATCHED = 600  # the page counts as open for this long after its last poll
OFFICE_JSON = REPORTS / "office" / "office.json"
PANE = re.compile(r"^[\w-]{1,24}:[\w-]{1,24}$")
TERMINAL_APPS = {"Apple_Terminal": "Terminal", "iTerm.app": "iTerm", "ghostty": "Ghostty",
                 "WezTerm": "WezTerm", "WarpTerminal": "Warp", "vscode": "Visual Studio Code"}


def hivey_bin() -> str | None:
    exe = os.environ.get("HIVEY_BIN")
    return exe if exe and os.path.isfile(exe) else shutil.which("hivey")


def start_office_refresh(llm: bool = True) -> None:
    """office.py run in the background (it holds its own lock, so overlapping starts are dropped)."""
    cmd = [sys.executable, "-I", str(SCRIPTS / "office.py"), "run"] + ([] if llm else ["--no-llm"])
    with open(STATE / "office.log", "a") as fh:
        subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=fh, stderr=fh, start_new_session=True)


def request_office_refresh(port: int) -> None:
    req = urllib.request.Request(f"http://127.0.0.1:{port}/hv-office-refresh", data=b"{}", method="POST",
                                 headers={"Origin": f"http://127.0.0.1:{port}", "Content-Type": "application/json"})
    try:
        urllib.request.urlopen(req, timeout=3).read()
    except OSError:
        pass


def client_ttys() -> list[str]:
    """ttys of running hivey clients (the TUI), newest first."""
    out = subprocess.run(["ps", "-axo", "pid=,tty=,command="], capture_output=True, text=True, timeout=5).stdout
    found = []
    for line in out.splitlines():
        f = line.split(None, 2)
        if len(f) < 3 or f[1] in ("??", "-"):
            continue
        args = f[2].split()
        if os.path.basename(args[0]) == "hivey" and (len(args) == 1 or args[1] in ("attach", "--session", "-s")):
            found.append((int(f[0]), "/dev/" + f[1]))
    return [tty for _, tty in sorted(found, reverse=True)]


RAISE_TERMINAL = """
on run argv
  set wanted to item 1 of argv
  tell application "Terminal"
    repeat with w in windows
      repeat with t in tabs of w
        if tty of t is wanted then
          set selected of t to true
          set index of w to 1
          activate
          return "ok"
        end if
      end repeat
    end repeat
  end tell
  return "missing"
end run
"""
RAISE_ITERM = """
on run argv
  set wanted to item 1 of argv
  tell application "iTerm2"
    repeat with w in windows
      repeat with t in tabs of w
        repeat with s in sessions of t
          if tty of s is wanted then
            select w
            select t
            select s
            activate
            return "ok"
          end if
        end repeat
      end repeat
    end repeat
  end tell
  return "missing"
end run
"""


def raise_terminal() -> None:
    """Bring the terminal window (and tab) running hivey to the front, not just the terminal app."""
    app = TERMINAL_APPS.get(os.environ.get("TERM_PROGRAM", ""), "Terminal")
    script = {"Terminal": RAISE_TERMINAL, "iTerm": RAISE_ITERM}.get(app)
    if script:
        for tty in client_ttys():
            try:
                p = subprocess.run(["osascript", "-e", script, tty], capture_output=True, text=True, timeout=10)
            except (OSError, subprocess.SubprocessError):
                break
            if p.stdout.strip() == "ok":
                return
    subprocess.run(["open", "-a", app], capture_output=True, timeout=10)


def office_desk(pane: str) -> dict | None:
    try:
        agents = json.loads(OFFICE_JSON.read_text()).get("agents", [])
    except (OSError, ValueError):
        return None
    return next((a for a in agents if a.get("pane") == pane), None)


def live_status(hv: str, pane: str) -> str | None:
    try:
        out = subprocess.run([hv, "pane", "get", pane], capture_output=True, text=True, timeout=5).stdout
        return json.loads(out)["result"]["pane"].get("agent_status")
    except (OSError, subprocess.SubprocessError, ValueError, KeyError, TypeError):
        return None


def office_action(pane: str, action: str) -> tuple[bool, str]:
    hv = hivey_bin()
    if not hv:
        return False, "hivey not found"
    if not PANE.match(pane or "") or not office_desk(pane):
        return False, "unknown agent"
    if action == "focus":
        p = subprocess.run([hv, "agent", "focus", pane], capture_output=True, text=True, timeout=10)
        if p.returncode != 0:
            return False, (p.stderr or p.stdout).strip()[-200:] or "focus failed"
        raise_terminal()
        return True, "opened in hivey"
    if action in ("compact", "clear"):
        status = live_status(hv, pane)
        if status in ("working", "blocked"):
            return False, f"it is {status} now; try when it is idle"
        p = subprocess.run([hv, "pane", "run", pane, f"/{action}"], capture_output=True, text=True, timeout=10)
        if p.returncode != 0:
            return False, (p.stderr or p.stdout).strip()[-200:] or f"/{action} failed"
        return True, f"sent /{action}"
    return False, "unknown action"


def running(current_only: bool = True) -> int | None:
    """The port of our server; with current_only, None for an older version (it lacks new actions)."""
    try:
        port = int(json.loads(INFO.read_text())["port"])
    except (OSError, ValueError, KeyError, TypeError):
        return None
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}{PING}", timeout=1) as r:
            body = r.read()
    except OSError:
        return None
    if not body.startswith(MARK):
        return None
    return port if body == MARK + b" " + VERSION or not current_only else None


def port_free(port: int) -> bool:
    import socket
    with socket.socket() as sock:
        try:
            sock.bind(("127.0.0.1", port))
            return True
        except OSError:
            return False


def ensure() -> int | None:
    """The running server's port, starting one in the background when there is none."""
    port = running()
    if port and (port == PREFERRED_PORT or not port_free(PREFERRED_PORT)):
        return port
    if port or running(current_only=False):  # older version, or on another port while ours is free
        try:
            os.kill(int(json.loads(INFO.read_text())["pid"]), 15)
            time.sleep(0.3)
        except (OSError, ValueError, KeyError, TypeError):
            pass
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
    office_polled = 0.0

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
            return self.answer(200, MARK + b" " + VERSION, "text/plain")
        if self.path.split("?")[0] == "/office/office.json":
            Handler.office_polled = time.time()
        return super().do_GET()

    def do_POST(self) -> None:
        Handler.last_used = time.time()
        port = self.server.server_address[1]
        if self.path not in ("/hv-refresh", "/hv-office-refresh", "/hv-office-action"):
            return self.answer(404, b'{"ok":false,"error":"not found"}')
        if self.headers.get("Origin") not in (f"http://127.0.0.1:{port}", f"http://localhost:{port}"):
            return self.answer(403, b'{"ok":false,"error":"only the agent map pages can do this"}')
        if self.path == "/hv-office-refresh":
            start_office_refresh()
            return self.answer(200, b'{"ok":true,"started":true}')
        if self.path == "/hv-office-action":
            try:
                length = min(int(self.headers.get("Content-Length") or 0), 4096)
                req = json.loads(self.rfile.read(length) or b"{}")
                ok, msg = office_action(str(req.get("pane", "")), str(req.get("action", "")))
            except (ValueError, OSError, subprocess.SubprocessError) as e:
                ok, msg = False, str(e)[:200]
            return self.answer(200, json.dumps({"ok": ok, "message": msg}).encode())
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

    def office_clock() -> None:
        """Refresh the office every 5 minutes while one of its pages is open."""
        while True:
            time.sleep(15)
            if time.time() - Handler.office_polled > OFFICE_WATCHED:
                continue
            try:
                age = time.time() - OFFICE_JSON.stat().st_mtime
            except OSError:
                age = OFFICE_EVERY
            if age >= OFFICE_EVERY:
                start_office_refresh()
                time.sleep(60)  # give it time to write before checking the age again

    threading.Thread(target=watchdog, daemon=True).start()
    threading.Thread(target=office_clock, daemon=True).start()
    print(f"agent map pages: http://127.0.0.1:{port}/latest/agent-stats.html", flush=True)
    httpd.serve_forever()
    try:
        if json.loads(INFO.read_text()).get("pid") == os.getpid():
            INFO.unlink()
    except (OSError, ValueError):
        pass
    return 0

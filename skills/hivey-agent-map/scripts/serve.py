"""Local server for the agent map pages, so their Refresh button can rerun the scan.

`hivey agent-map open` starts it (once; later opens reuse it) and opens the pages from
http://127.0.0.1:<port>/latest/. It listens on 127.0.0.1 only, serves ~/.hivey/agent-map/reports, and
has these actions, answered only for the pages it serves (Origin check), so other web pages can't
trigger them:

  POST /hv-refresh          `agent_map.py refresh` (free: no model)
  POST /hv-office-refresh   `office.py run` in the background (the agent office, /office/)
  POST /hv-office-action    {"action": "session", "session": "<name>"} shows a hivey session in a terminal (a
                            new one if none shows it; `hivey session attach` starts a stopped session),
                            {"action": "stop-session", "session": "<name>"} stops a non-default session (all
                            its agents) and closes its terminal windows, or
                            {"id": "wN:p1" | "<session>/wN:p1", "action": "focus" | "compact" | "clear"} for a desk on the
                            office page: focus opens the agent's terminal in hivey (and brings the
                            terminal app forward); compact/clear type /compact or /clear into an agent
                            that is not working

While an office page is open (it polls /office/office.json), a live pass runs every minute (status,
context, screen; only desks whose status changed go to the model) and a full re-summary every 5 minutes.
The office page's ⏱ setting (POST /hv-office-settings {"every": "minute" | "5min" | "15min" | "1h" | "reload"}, saved in
~/.hivey/agent-map/office-settings.json) picks every minute (above), every 5 / 15 minutes or every hour (full passes only)
or on reload only (no clock). POST /hv-office-refresh: {} a full pass now (⟳), {"load": true} when a page
opens (a quick pass in minute mode, a full one otherwise), {"quick": true} a quick pass.
It stops after 2 hours unused. Python standard library only.
"""
from __future__ import annotations

import http.server
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

STATE = Path.home() / ".hivey" / "agent-map"
OFFICE_SETTINGS = STATE / "office-settings.json"  # {"every": "minute" | ..., "ships": {"<session>": {"x", "z"}}}
EVERY_MODES = ("minute", "5min", "15min", "1h", "reload")
FULL_EVERY = {"minute": 300, "5min": 300, "15min": 900, "1h": 3600}  # seconds between full passes per mode
REPORTS = STATE / "reports"
INFO = STATE / "server.json"
SCRIPTS = Path(__file__).resolve().parent
PREFERRED_PORT = 42817  # unregistered and below macOS's temporary-port range, so no other app picks it
IDLE_SECONDS = 2 * 3600
PING = "/hv-ping"
MARK = b"hivey-agent-map"
VERSION = b"office-12"  # bump when the server gains actions, so `ensure` replaces an older one
OFFICE_EVERY = 300  # seconds between full office refreshes (every terminal re-summarized) while its page is open
OFFICE_LIVE = 60  # seconds between live passes (status, context, screen; only changed statuses summarized)
OFFICE_WATCHED = 600  # the page counts as open for this long after its last poll
OFFICE_JSON = REPORTS / "office" / "office.json"
PANE = re.compile(r"^[\w-]{1,24}:[\w-]{1,24}$")
TERMINAL_APPS = {"Apple_Terminal": "Terminal", "iTerm.app": "iTerm", "ghostty": "Ghostty",
                 "WezTerm": "WezTerm", "WarpTerminal": "Warp", "vscode": "Visual Studio Code"}


def hivey_bin() -> str | None:
    exe = os.environ.get("HIVEY_BIN")
    return exe if exe and os.path.isfile(exe) else shutil.which("hivey")


def office_settings() -> dict:
    try:
        got = json.loads(OFFICE_SETTINGS.read_text())
        return got if isinstance(got, dict) else {}
    except (OSError, ValueError):
        return {}


def clean_ships(ships: object) -> dict:
    """{session: {x, z}} where you dragged each ship on the office page (bad entries dropped)."""
    out = {}
    for name, p in (ships.items() if isinstance(ships, dict) else []):
        try:
            x, z = float(p["x"]), float(p["z"])
        except (TypeError, KeyError, ValueError):
            continue
        if SESSION_NAME.match(str(name)) and abs(x) < 1e5 and abs(z) < 1e5:
            out[str(name)] = {"x": round(x, 2), "z": round(z, 2)}
    return out


def office_every() -> str:
    """The office page's refresh setting (every minute by default)."""
    try:
        mode = office_settings().get("every")
    except (OSError, ValueError, AttributeError):
        mode = None
    return mode if mode in EVERY_MODES else "minute"


def start_office_refresh(llm: bool = True, quick: bool = False) -> None:
    """office.py run in the background (it holds its own lock, so overlapping starts are dropped)."""
    cmd = [sys.executable, "-I", str(SCRIPTS / "office.py"), "run"] + ([] if llm else ["--no-llm"]) \
        + (["--quick"] if quick else [])
    with open(STATE / "office.log", "a") as fh:
        subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=fh, stderr=fh, start_new_session=True)


def request_office_refresh(port: int) -> None:
    req = urllib.request.Request(f"http://127.0.0.1:{port}/hv-office-refresh", data=b"{}", method="POST",
                                 headers={"Origin": f"http://127.0.0.1:{port}", "Content-Type": "application/json"})
    try:
        urllib.request.urlopen(req, timeout=3).read()
    except OSError:
        pass


def client_ttys(session: str | None = None, only: bool = False) -> list[str]:
    """ttys of running hivey clients (the TUI), newest first; those attached to `session` first
    (only: just those)."""
    out = subprocess.run(["ps", "-axo", "pid=,tty=,command="], capture_output=True, text=True, timeout=5).stdout
    found = []
    for line in out.splitlines():
        f = line.split(None, 2)
        if len(f) < 3 or f[1] in ("??", "-"):
            continue
        args = f[2].split()
        if os.path.basename(args[0]) == "hivey" and (len(args) == 1 or args[1] in ("attach", "--session", "-s")
                                                     or args[1:3] == ["session", "attach"]):
            named = (args[3] if args[1:3] == ["session", "attach"] and len(args) > 3 else None) \
                or next((args[i + 1] for i in range(len(args) - 1) if args[i] in ("--session", "-s")), None) \
                or next((a.split("=", 1)[1] for a in args if a.startswith("--session=")), None)
            mine = (named or "default") == (session or "default")
            found.append((mine, int(f[0]), "/dev/" + f[1]))
    return [tty for mine, _, tty in sorted(found, reverse=True) if mine or not only]


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


OPEN_TERMINAL = """
on run argv
  tell application "Terminal"
    activate
    do script (item 1 of argv)
  end tell
end run
"""
OPEN_ITERM = """
on run argv
  tell application "iTerm"
    activate
    create window with default profile command (item 1 of argv)
  end tell
end run
"""
CLOSE_TERMINAL = """
on run argv
  set wanted to item 1 of argv
  tell application "Terminal"
    repeat with w in windows
      repeat with t in tabs of w
        if tty of t is wanted then
          if (count of tabs of w) is 1 then close w
          return "ok"
        end if
      end repeat
    end repeat
  end tell
  return "none"
end run
"""
CLOSE_ITERM = """
on run argv
  set wanted to item 1 of argv
  tell application "iTerm"
    repeat with w in windows
      repeat with t in tabs of w
        repeat with s in sessions of t
          if tty of s is wanted then
            close s
            return "ok"
          end if
        end repeat
      end repeat
    end repeat
  end tell
  return "none"
end run
"""
SESSION_NAME = re.compile(r"^[\w.-]{1,64}$")


def open_session_terminal(session: str | None) -> bool:
    """A new terminal window running hivey on this session (`hivey session attach` also starts a stopped one)."""
    exe = hivey_bin()
    if not exe or (session and not SESSION_NAME.match(session)):
        return False
    cmd = shlex.quote(exe) if not session or session == "default" else \
        f"{shlex.quote(exe)} session attach {shlex.quote(session)}"
    cmd = f"exec {cmd}"  # the window ends with hivey (when the session is stopped or you quit)
    app = TERMINAL_APPS.get(os.environ.get("TERM_PROGRAM", ""), "Terminal")
    script = OPEN_ITERM if app == "iTerm" else OPEN_TERMINAL
    try:
        p = subprocess.run(["osascript", "-e", script, cmd], capture_output=True, text=True, timeout=15)
    except (OSError, subprocess.SubprocessError):
        return False
    return p.returncode == 0


def raise_terminal(session: str | None = None) -> str:
    """Bring the terminal window (and tab) showing this hivey session to the front; when no terminal shows
    it, open a new one on it. Returns "raised", "opened" or "" (nothing worked)."""
    app = TERMINAL_APPS.get(os.environ.get("TERM_PROGRAM", ""), "Terminal")
    mine = client_ttys(session, only=True)
    if not mine:
        return "opened" if open_session_terminal(session) else ""
    script = {"Terminal": RAISE_TERMINAL, "iTerm": RAISE_ITERM}.get(app)
    if script:
        for tty in mine:
            try:
                p = subprocess.run(["osascript", "-e", script, tty], capture_output=True, text=True, timeout=10)
            except (OSError, subprocess.SubprocessError):
                break
            if p.stdout.strip() == "ok":
                return "raised"
    subprocess.run(["open", "-a", app], capture_output=True, timeout=10)
    return "raised"


def office_desk(desk_id: str) -> dict | None:
    """The desk on the office page with this id (pane id, or "<session>/<pane>" outside the default session)."""
    try:
        agents = json.loads(OFFICE_JSON.read_text()).get("agents", [])
    except (OSError, ValueError):
        return None
    return next((a for a in agents if a.get("id") == desk_id), None) \
        or next((a for a in agents if a.get("pane") == desk_id and not a.get("sock")), None)


INHERITED = ("HERDR_SOCKET_PATH", "HERDR_CLIENT_SOCKET_PATH", "HERDR_SESSION", "HERDR_PANE_ID", "HERDR_TAB_ID",
             "HERDR_WORKSPACE_ID", "HERDR_PANE_RUNTIME_ID")


def hv(exe: str, sock: str | None, *args: str, timeout: int = 10) -> subprocess.CompletedProcess:
    """Run a hivey command against one session's server (sock None: the one we inherited)."""
    env = None
    if sock:
        env = {k: v for k, v in os.environ.items() if k not in INHERITED}
        env["HERDR_SOCKET_PATH"] = sock
    return subprocess.run([exe, *args], capture_output=True, text=True, timeout=timeout, env=env)


def live_status(exe: str, sock: str | None, pane: str) -> str | None:
    try:
        return json.loads(hv(exe, sock, "pane", "get", pane, timeout=5).stdout)["result"]["pane"].get("agent_status")
    except (OSError, subprocess.SubprocessError, ValueError, KeyError, TypeError):
        return None


def waiting_in_prompt(exe: str, sock: str | None, pane: str, command: str) -> bool:
    """True while the command still sits typed in the agent's input box (its Enter not taken yet)."""
    try:
        out = hv(exe, sock, "pane", "read", pane, "--source", "recent", "--lines", "15", timeout=5).stdout
    except (OSError, subprocess.SubprocessError):
        return False
    try:  # the CLI answers JSON or plain text depending on the version
        out = json.loads(out)["result"].get("text", out)
    except (ValueError, KeyError, TypeError, AttributeError):
        pass
    return any(re.match(r"^[│|\s]*[>❯›]\s*" + re.escape(command) + r"\s*[│|]?\s*$", line)
               for line in str(out).splitlines()[-10:])


def send_command(exe: str, sock: str | None, pane: str, command: str) -> tuple[bool, str]:
    """Type a slash command into an agent and press Enter as its own key press, so the CLI does not take
    text and Enter as one paste (or let its slash-command menu swallow the Enter); press Enter again
    while the command is still waiting in the input box."""
    p = hv(exe, sock, "pane", "send-text", pane, command)
    if p.returncode != 0:
        return False, (p.stderr or p.stdout).strip()[-200:] or f"{command} failed"
    for wait in (0.5, 1.2, 1.5):
        time.sleep(wait)
        if wait > 0.5 and not waiting_in_prompt(exe, sock, pane, command):
            return True, f"sent {command} and Enter"
        p = hv(exe, sock, "pane", "send-keys", pane, "Enter")
        if p.returncode != 0:
            return False, (p.stderr or p.stdout).strip()[-200:] or "Enter failed"
    time.sleep(1.5)
    if waiting_in_prompt(exe, sock, pane, command):
        return False, f"{command} is typed but the agent has not taken it; press Enter in its terminal"
    return True, f"sent {command} and Enter"


def stop_session(session: str) -> tuple[bool, str]:
    """Stop a (non-default) hivey session: its server and every agent in it, then close its terminal windows."""
    exe = hivey_bin()
    if not exe or not SESSION_NAME.match(session or ""):
        return False, "unknown session"
    if session == "default":
        return False, "the default session is not stopped from here (it runs this office); quit it in hivey"
    ttys = client_ttys(session, only=True)
    p = subprocess.run([exe, "session", "stop", session], capture_output=True, text=True, timeout=60)
    if p.returncode != 0:
        return False, (p.stderr or p.stdout).strip()[-200:] or "stop failed"
    time.sleep(1.5)  # let its hivey clients exit
    app = TERMINAL_APPS.get(os.environ.get("TERM_PROGRAM", ""), "Terminal")
    script = {"Terminal": CLOSE_TERMINAL, "iTerm": CLOSE_ITERM}.get(app)
    closed = 0
    for tty in ttys if script else []:
        try:
            r = subprocess.run(["osascript", "-e", script, tty], capture_output=True, text=True, timeout=10)
            closed += r.stdout.strip() == "ok"
        except (OSError, subprocess.SubprocessError):
            pass
    return True, f"session {session} stopped" + (f", {closed} terminal closed" if closed else "")


def session_action(session: str) -> tuple[bool, str]:
    """Click on a ship: show that hivey session in a terminal (a new one if none shows it; starts it if stopped)."""
    if not SESSION_NAME.match(session or ""):
        return False, "unknown session"
    how = raise_terminal(session)
    return (True, {"opened": f"opened session {session} in a new terminal", "raised": f"session {session} is in front"}[how]) \
        if how else (False, f"could not open a terminal; run: hivey session attach {session}")


def office_action(desk_id: str, action: str, session: str = "") -> tuple[bool, str]:
    if action == "session":
        return session_action(session)
    if action == "stop-session":
        return stop_session(session)
    exe = hivey_bin()
    if not exe:
        return False, "hivey not found"
    desk = office_desk(desk_id)
    pane = (desk or {}).get("pane") or ""
    if not desk or not PANE.match(pane):
        return False, "unknown agent"
    sock = desk.get("sock") or None
    if desk.get("asleep"):  # an agent of a stopped session: start the session (compact/clear wait until then)
        if action == "focus":
            return session_action(desk.get("hv_session") or "")
        return False, f"session {desk.get('hv_session')} is stopped; open it first"
    if action == "focus":
        p = hv(exe, sock, "agent", "focus", pane)
        if p.returncode != 0:
            return False, (p.stderr or p.stdout).strip()[-200:] or "focus failed"
        how = raise_terminal(desk.get("hv_session"))
        where = desk.get("hv_session") or "default"
        return True, {"opened": f"opened in a new terminal on session {where}",
                      "raised": "opened in hivey"}.get(how, "focused in hivey (bring its terminal forward yourself)")
    if action in ("compact", "clear"):
        status = live_status(exe, sock, pane)
        if status in ("working", "blocked"):
            return False, f"it is {status} now; try when it is idle"
        return send_command(exe, sock, pane, f"/{action}")
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
    office_full = 0.0  # last full (all re-summarized) office refresh started by this server

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
        if self.path not in ("/hv-refresh", "/hv-office-refresh", "/hv-office-action", "/hv-office-settings"):
            return self.answer(404, b'{"ok":false,"error":"not found"}')
        if self.headers.get("Origin") not in (f"http://127.0.0.1:{port}", f"http://localhost:{port}"):
            return self.answer(403, b'{"ok":false,"error":"only the agent map pages can do this"}')
        if self.path in ("/hv-office-refresh", "/hv-office-settings"):
            try:
                length = min(int(self.headers.get("Content-Length") or 0), 4096)
                req = json.loads(self.rfile.read(length) or b"{}")
                req = req if isinstance(req, dict) else {}
            except ValueError:
                req = {}
            if self.path == "/hv-office-settings":
                cur = office_settings()
                if "every" in req:
                    if req.get("every") not in EVERY_MODES:
                        return self.answer(400, b'{"ok":false,"error":"every: minute, 5min, 15min, 1h or reload"}')
                    cur["every"] = req["every"]
                if "space" in req:  # the background: "random" (new each load) or a scene "1".."6"
                    if str(req["space"]) not in ("random", "1", "2", "3", "4", "5", "6"):
                        return self.answer(400, b'{"ok":false,"error":"space: random or 1..6"}')
                    cur["space"] = str(req["space"])
                if "track" in req:  # the music: "random" (new each load, then the next) or a track "1".."10"
                    if not re.fullmatch(r"random|[1-9]|10", str(req["track"])):
                        return self.answer(400, b'{"ok":false,"error":"track: random or 1..10"}')
                    cur["track"] = str(req["track"])
                if "ships" in req:  # the ship positions you dragged on the office page
                    cur["ships"] = clean_ships(req["ships"])
                OFFICE_SETTINGS.write_text(json.dumps(cur))
                return self.answer(200, json.dumps({"ok": True, "every": office_every(), "space": cur.get("space"), "track": cur.get("track"),
                                                    "ships": cur.get("ships", {})}).encode())
            every = office_every()
            # a page load: a quick pass when updating every minute, a full one otherwise; ⟳: always full
            quick = bool(req.get("quick")) or (bool(req.get("load")) and every == "minute")
            Handler.office_polled = time.time()
            if not quick:
                Handler.office_full = time.time()
            start_office_refresh(quick=quick)
            return self.answer(200, json.dumps({"ok": True, "started": True, "every": every,
                                                "space": office_settings().get("space"), "track": office_settings().get("track"),
                                                "ships": clean_ships(office_settings().get("ships"))}).encode())
        if self.path == "/hv-office-action":
            try:
                length = min(int(self.headers.get("Content-Length") or 0), 4096)
                req = json.loads(self.rfile.read(length) or b"{}")
                ok, msg = office_action(str(req.get("id") or req.get("pane") or ""), str(req.get("action", "")),
                                        str(req.get("session") or ""))
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
        """While an office page is open: a live pass every minute, a full re-summary every 5 minutes."""
        while True:
            time.sleep(10)
            every = office_every()
            if every == "reload" or time.time() - Handler.office_polled > OFFICE_WATCHED:
                continue
            try:
                age = time.time() - OFFICE_JSON.stat().st_mtime
            except OSError:
                age = OFFICE_EVERY
            if time.time() - Handler.office_full >= FULL_EVERY.get(every, OFFICE_EVERY):
                Handler.office_full = time.time()
                start_office_refresh()
                time.sleep(30)  # give it time to write before checking the age again
            elif every == "minute" and age >= OFFICE_LIVE:
                start_office_refresh(quick=True)
                time.sleep(20)

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

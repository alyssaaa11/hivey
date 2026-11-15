"""The dashboards your agents keep: one list, shown on the fleet page's 🖥 Dashboards tab.

Each entry is {"id", "name", "url", "agent", "note", "token", "token_param"}; they live in
~/.hivey/agent-map/fleet-settings.json (owner-only) under "dashboards", so every browser sees the same list.
The page lists them on the left and shows the one you pick in a frame on the right; it adds, edits and
removes them (POST /hv-fleet-settings {"dashboards": [...]}). An agent that builds or deploys a dashboard
registers it from the shell:

  python3 -I dashboards.py add "Trading dashboard" https://example.vercel.app --agent trading-strategies
  python3 -I dashboards.py add "Trading dashboard" https://example.vercel.app --token default
  python3 -I dashboards.py token es_…                       (your default dashboard token; `token` shows it)
  python3 -I dashboards.py list [--json]
  python3 -I dashboards.py edit "Trading dashboard" --name "Paper trading" --url https://… --note "…"
  python3 -I dashboards.py remove "Trading dashboard"      (by name, id or URL)

The same commands run as `hivey agent-map dashboard add|list|edit|remove …` (the hivey-dashboards skill).
`add` with a name or URL already in the list updates that entry.

Logins: a dashboard can carry a token. Opening it (GET /hv-dashboard-open?id=…) redirects to its URL with
the token added as a query parameter (`token_param`, default `token`), so a dashboard with token login
(the hivey-dashboards skill's references/token-login.md) logs you in by itself. The token is a literal value,
an EnvSave opaque key (`es_…`, read with `envsave get` when the dashboard opens; no Touch ID), or `default`:
your default dashboard token (one EnvSave key, `dashboard_token` in the settings file, else the one built in
for your user in USER_TOKENS) that opens all your dashboards, so changing it updates every one. A new
dashboard uses it unless you say it needs no login (`--no-token`). Tokens are never sent to the page's list.
Python standard library only.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import getpass
import json
import os
import pwd
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

SETTINGS = Path.home() / ".hivey" / "agent-map" / "fleet-settings.json"
MAX = 100
LIMITS = {"name": 80, "url": 2000, "agent": 80, "note": 300}
DEFAULT = "default"  # the token value meaning "my default dashboard token"
PARAM = re.compile(r"^[A-Za-z0-9_.-]{1,64}$")
FIELDS = ("name", "url", "agent", "note", "token", "token_param")
# Each known user's default dashboard token (their DASHBOARD_TOKEN_<USER> EnvSave key), used until they save
# their own (`dashboard token es_…`, or the Edit form). Matched on the Mac login and full name. Anyone else
# types theirs once in the form, and it is saved for next time.
USER_TOKENS = (
    (("jcsancho", "jose"), "es_AN_xOxHk9IU6KbcaRxXEndz0xHcHj0NKIRnb79Iq2QV7Hr6f"),
    (("alyssa",), "es_8AC8PzeSobAcZMD1PV6gnZwo7bezsQkiQoswZu7BZqbgecKb"),
)


def _settings() -> dict:
    try:
        got = json.loads(SETTINGS.read_text())
        return got if isinstance(got, dict) else {}
    except (OSError, ValueError):
        return {}


def valid_url(url: str) -> bool:
    p = urllib.parse.urlsplit(url)
    return p.scheme in ("http", "https") and bool(p.netloc) and not any(c.isspace() for c in url)


def _slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:40] or "dashboard"


def clean(items: object, old: list[dict] | None = None) -> list[dict]:
    """Keep well-formed entries only: an http(s) URL, a name (the host when missing), short fields, unique ids.

    With `old` (the saved list), an entry whose id is saved and that has no "token" key keeps its saved
    token: the page never sees tokens, so it sends none back unless you typed a new one ("" removes it)."""
    saved = {d["id"]: d for d in old or []}
    out, ids = [], set()
    for d in items if isinstance(items, list) else []:
        if not isinstance(d, dict):
            continue
        e = {k: str(d.get(k) or "").strip()[:n] for k, n in LIMITS.items()}
        if not valid_url(e["url"]):
            continue
        e["name"] = e["name"] or urllib.parse.urlsplit(e["url"]).netloc
        prev = saved.get(str(d.get("id") or ""), {})
        token = d["token"] if "token" in d else prev.get("token", "")
        e["token"] = str(token or "").strip()[:4000]
        param = str(d.get("token_param") or (prev.get("token_param") if "token_param" not in d else "") or "").strip()
        e["token_param"] = param if PARAM.match(param) else ""
        base = _slug(str(d.get("id") or "") or e["name"])
        i, n = base, 2
        while i in ids:
            i, n = f"{base}-{n}", n + 1
        ids.add(i)
        out.append({"id": i, **e})
        if len(out) >= MAX:
            break
    return out


def public(items: list[dict]) -> list[dict]:
    """The list for the page: tokens replaced by what kind of login each one has."""
    out = []
    for d in items:
        p = {k: v for k, v in d.items() if k != "token"}
        tok = d.get("token") or ""
        p["login"] = "" if not tok else DEFAULT if tok == DEFAULT else "envsave" if tok.startswith("es_") else "token"
        out.append(p)
    return out


def load() -> list[dict]:
    return clean(_settings().get("dashboards"))


def write(cur: dict) -> None:
    """Save the settings file readable by you only (it can hold dashboard tokens)."""
    SETTINGS.parent.mkdir(parents=True, exist_ok=True)
    tmp = SETTINGS.with_suffix(".tmp")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(json.dumps(cur))
    os.chmod(tmp, 0o600)
    tmp.replace(SETTINGS)


def save(items: list[dict]) -> list[dict]:
    cur = _settings()
    cur["dashboards"] = clean(items, clean(cur.get("dashboards")))
    write(cur)
    return cur["dashboards"]


def _match(d: dict, key: str) -> bool:
    k = key.strip().lower()
    return k in (d["id"].lower(), d["name"].lower(), d["url"].lower().rstrip("/"), d["url"].lower())


def _changes(name=None, url=None, agent=None, note=None, token=None, token_param=None) -> dict:
    ch = {k: v for k, v in dict(name=name, url=url, agent=agent, note=note, token=token, token_param=token_param).items()
          if v is not None}
    if "url" in ch and not valid_url(ch["url"]):
        raise ValueError(f"not an http(s) URL: {ch['url']}")
    if ch.get("token_param") and not PARAM.match(ch["token_param"]):
        raise ValueError(f"bad token parameter name: {ch['token_param']}")
    return ch


def add(name: str, url: str, agent: str = "", note: str = "", token: str | None = None,
        token_param: str | None = None) -> dict:
    ch = _changes(name=name, url=url, agent=agent or None, note=note or None, token=token, token_param=token_param)
    items = load()
    for i, d in enumerate(items):
        if _match(d, name) or _match(d, url.rstrip("/")):
            items[i] = {**d, **ch, "id": d["id"]}
            break
    else:
        items.append({"token": DEFAULT, **ch})  # a new dashboard logs in with your default token unless told not to
    saved = save(items)
    return next(d for d in saved if d["url"] == url or d["name"] == name)


def edit(key: str, **changes) -> dict | None:
    """Change the given fields of the dashboard named key (name, id or URL); None if there is none."""
    ch = _changes(**changes)
    items = load()
    for i, d in enumerate(items):
        if _match(d, key):
            items[i] = {**d, **ch}
            return save(items)[i]
    return None


def remove(key: str) -> bool:
    items = load()
    kept = [d for d in items if not _match(d, key)]
    if len(kept) == len(items):
        return False
    save(kept)
    return True


def builtin_token() -> str:
    """The default dashboard token built in for this Mac's user (USER_TOKENS), "" for anyone else."""
    try:
        user = getpass.getuser()
        who = f"{user} {pwd.getpwnam(user).pw_gecos}".lower()
    except (KeyError, OSError):
        who = os.environ.get("USER", "").lower()
    words = set(re.findall(r"[a-z0-9]+", who))
    return next((tok for names, tok in USER_TOKENS if any(n in words for n in names)), "")


def default_token() -> str:
    """Your default dashboard token: the one you saved, else your built-in one; an EnvSave key (es_…) or a
    value, "" when there is none."""
    return str(_settings().get("dashboard_token") or "").strip() or builtin_token()


def set_default_token(token: str) -> None:
    cur = _settings()
    if token:
        cur["dashboard_token"] = token.strip()[:4000]
    else:
        cur.pop("dashboard_token", None)
    write(cur)


_resolved: dict[str, tuple[float, str]] = {}


def resolve(token: str) -> str:
    """A literal token as is; an EnvSave opaque key read with `envsave get es_…` (cached for 10 minutes);
    "default" is your default dashboard token."""
    if token == DEFAULT:
        token = default_token()
        if not token or token == DEFAULT:
            raise RuntimeError("no default dashboard token yet: `hivey agent-map dashboard token es_…`")
    if not token.startswith("es_"):
        return token
    hit = _resolved.get(token)
    if hit and time.time() - hit[0] < 600:
        return hit[1]
    exe = shutil.which("envsave") or next((p for p in ("/opt/homebrew/bin/envsave", "/usr/local/bin/envsave")
                                           if os.path.isfile(p)), None)
    if not exe:
        raise RuntimeError("envsave is not installed, so the dashboard's es_ token cannot be read")
    p = subprocess.run([exe, "get", token], capture_output=True, text=True, timeout=20, stdin=subprocess.DEVNULL)
    value = p.stdout.strip()
    if p.returncode or not value:
        raise RuntimeError(f"envsave get {token[:10]}… failed: {(p.stderr or 'empty').strip()[:120]}")
    _resolved[token] = (time.time(), value)
    return value


def open_url(d: dict) -> str:
    """The URL to open: with its token as a query parameter when it has one."""
    if not d.get("token"):
        return d["url"]
    param = d.get("token_param") or "token"
    extra = [(param, resolve(d["token"]))]
    u = urllib.parse.urlsplit(d["url"])
    q = [(k, v) for k, v in urllib.parse.parse_qsl(u.query, keep_blank_values=True) if k not in dict(extra)]
    return urllib.parse.urlunsplit(u._replace(query=urllib.parse.urlencode(q + extra)))


def find(key: str) -> dict | None:
    return next((d for d in load() if d["id"] == key), None)


def _frameable(headers) -> bool:
    """Whether the page lets another site show it in a frame (X-Frame-Options, CSP frame-ancestors)."""
    if (headers.get("X-Frame-Options") or "").strip().upper() in ("DENY", "SAMEORIGIN"):
        return False
    m = re.search(r"frame-ancestors([^;]*)", headers.get("Content-Security-Policy") or "", re.I)
    if not m:
        return True
    srcs = m.group(1).split()
    return "*" in srcs or any(s.rstrip("/") in ("http://127.0.0.1:*", "http://localhost:*", "http:") for s in srcs)


def _probe(d: dict) -> dict:
    """Is the dashboard up, and can it show in the frame? Any answer below 500 counts as up."""
    start = time.time()
    try:
        url = open_url(d)
    except (RuntimeError, OSError, subprocess.SubprocessError) as e:
        return {"ok": False, "error": str(e)[:160], "ms": 0}
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 hivey-fleet-dashboards"})
    try:
        with urllib.request.urlopen(req, timeout=8) as r:
            code, final, headers = r.status, r.geturl(), r.headers
    except urllib.error.HTTPError as e:
        code, final, headers = e.code, e.geturl() or url, e.headers
    except (urllib.error.URLError, OSError, ValueError) as e:
        return {"ok": False, "error": str(getattr(e, "reason", e))[:120], "ms": round((time.time() - start) * 1000)}
    out = {"ok": code < 500, "code": code, "ms": round((time.time() - start) * 1000), "frame": _frameable(headers)}
    if urllib.parse.urlsplit(final).netloc != urllib.parse.urlsplit(d["url"]).netloc:
        out["elsewhere"] = urllib.parse.urlsplit(final).netloc  # sent to another site, e.g. a hosting login wall
    return out


def status() -> dict:
    """{id: {ok, code | error, ms, frame, elsewhere?}} for every dashboard, checked in parallel."""
    items = load()
    if not items:
        return {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=min(8, len(items))) as pool:
        return {d["id"]: r for d, r in zip(items, pool.map(_probe, items))}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="hivey agent-map dashboard", description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    def login_args(p) -> None:
        p.add_argument("--token", help="login token: default (your default dashboard token), an EnvSave key es_… "
                                       "or a value (\"\" removes it)")
        p.add_argument("--token-param", help="the query parameter the dashboard reads it from (default token)")
        p.add_argument("--no-token", dest="token", action="store_const", const="",
                       help="it needs no login token (a new dashboard uses your default token otherwise)")

    a = sub.add_parser("add", help="add a dashboard (or update the one with this name or URL)")
    a.add_argument("name")
    a.add_argument("url")
    a.add_argument("--agent", default="", help="the agent that keeps it")
    a.add_argument("--note", default="", help="one line: what it shows")
    login_args(a)
    ls = sub.add_parser("list")
    ls.add_argument("--json", action="store_true")
    ed = sub.add_parser("edit", help="change a dashboard's name, URL, agent, note or token")
    ed.add_argument("key", help="name, id or URL")
    for f in ("name", "url", "agent", "note"):
        ed.add_argument(f"--{f}")
    login_args(ed)
    rm = sub.add_parser("remove")
    rm.add_argument("key", help="name, id or URL")
    tk = sub.add_parser("token", help="show or set your default dashboard token (used by --token default)")
    tk.add_argument("value", nargs="?", help="an EnvSave key es_… (best) or the token itself")
    tk.add_argument("--clear", action="store_true")
    args = ap.parse_args(argv)
    if args.cmd == "token":
        if args.clear or args.value:
            set_default_token("" if args.clear else args.value)
        cur = default_token()
        built = cur and not str(_settings().get("dashboard_token") or "").strip()
        print(f"default dashboard token: {cur[:8] + '…' if cur.startswith('es_') else ('set (a raw value)' if cur else 'none')}"
              + (" (built in for this user)" if built else "")
              + ("" if cur else "  (set it: hivey agent-map dashboard token es_…)"))
        return 0
    try:
        if args.cmd == "add":
            d = add(args.name, args.url, args.agent, args.note, args.token, args.token_param)
            print(f"saved {d['name']} → {d['url']}{' (with login token)' if d['token'] else ''} (fleet page → 🖥 Dashboards)")
            return 0
        if args.cmd == "edit":
            d = edit(args.key, name=args.name, url=args.url, agent=args.agent, note=args.note, token=args.token,
                     token_param=args.token_param)
            if not d:
                print(f"dashboards: no dashboard named {args.key}", file=sys.stderr)
                return 1
            print(f"saved {d['name']} → {d['url']}{' (with login token)' if d['token'] else ''}")
            return 0
    except ValueError as e:
        print(f"dashboards: {e}", file=sys.stderr)
        return 1
    if args.cmd == "list":
        items = public(load())
        if args.json:
            print(json.dumps(items, indent=1))
        else:
            for d in items:
                print(f"{d['name']:<28} {d['url']}" + (f"  [{d['agent']}]" if d["agent"] else "")
                      + (f"  🔑 {d['login']}" if d["login"] else ""))
            if not items:
                print("no dashboards yet (fleet page → 🖥 Dashboards → + Add, or `hivey agent-map dashboard add NAME URL`)")
        return 0
    if args.cmd == "remove":
        if remove(args.key):
            print(f"removed {args.key}")
            return 0
        print(f"dashboards: no dashboard named {args.key}", file=sys.stderr)
        return 1
    return 2


if __name__ == "__main__":
    sys.exit(main())

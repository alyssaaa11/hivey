---
name: hivey-dashboards
description: >
  Create dashboards that open logged in from the hivey agent fleet, and manage the fleet page's
  🖥 Dashboards tab: add, list, edit and remove dashboards (Vercel sites, local servers, anything
  with a URL) and their login tokens. Use whenever you build, deploy or change a dashboard, report
  site or web view for the user or an agent (give it the token login in references/token-login.md,
  then register it, without being asked); and whenever the user says "add this dashboard to the
  agent fleet", "register my dashboard", "remove/rename that dashboard", "change its token",
  "which dashboards do I have", or gives a dashboard URL to keep track of. Works from any agent
  hivey runs (Claude Code, Codex, Grok, Antigravity).
allowed-tools:
  - Bash
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# hivey dashboards

The agent fleet page (hivey menu → agent fleet, or `hivey agent-map fleet`) has a **🖥 Dashboards** tab:
the user's dashboards listed on the left with a live up/down dot, and the one they pick loaded in a frame
on the right, logged in. The same list is edited from the page (**+ Add**, Edit, Remove) and from the
shell, so this skill and the page always agree.

## How the login works

Each user has one **dashboard token** (EnvSave key `DASHBOARD_TOKEN_<USER>`, 32+ characters) that opens
all their dashboards. hivey keeps it as the *default dashboard token*: Jose's and Alyssa's keys are built in
(`USER_TOKENS` in `dashboards.py`, matched on the Mac user), anyone else types theirs once in the form and it
is saved for next time; either can be changed later in a dashboard's Edit form or with `dashboard token es_…`.
A new dashboard logs in with it by default; untick "login token" (or `--no-token`) for one that needs none; a dashboard registered with
`--token default` opens as `https://<site>/?token=<it>`, and the site trades the token for its own session
cookie. No hosting login (no Vercel Deployment Protection): it refuses frames, and the token login protects
the site instead. Tokens never reach the page; the local fleet server adds them when a dashboard opens.

## Building a dashboard

When you create or change a dashboard (any web page that shows the user's data):

1. **Give it the token login** exactly as in `references/token-login.md` (Next.js or a static page with
   Vercel functions; the contract applies to any stack): `?token=` → `/api/auth/token` → signed
   `SameSite=None; Partitioned` session cookie, token stripped from the URL, every data endpoint 401
   without the cookie, no `X-Frame-Options`, the host's own login wall off.
2. **The token on the server** comes from the user's `DASHBOARD_TOKEN_<USER>` EnvSave key
   (`envsave.get("es_…")`). If you don't know the key, ask the user for it; never ask for the token
   itself. Pushing it to Vercel (`envsave vercel …`) unlocks the vault: tell the user first and let them run it.
3. **Check it** with the two `curl` lines at the end of the reference (a wrong token gets 401, nothing
   forbids framing).
4. **Register it** (below) with `--token default`, and tell the user it is on the 🖥 Dashboards tab.

## Managing the list

```bash
hivey agent-map dashboard list                     # name, URL, agent, 🔑 login  (--json for the fields)
hivey agent-map dashboard add "Trading dashboard" https://trading.example.app \
    --agent trading-strategies --note "paper accounts, trades and job status" --token default
hivey agent-map dashboard edit "Trading dashboard" --name "Paper trading" --url https://new.example.app
hivey agent-map dashboard edit "Trading dashboard" --token default   # or es_… for its own token, "" for none
hivey agent-map dashboard add "Status page" https://status.example.app --no-token   # opens without a login
hivey agent-map dashboard remove "Paper trading"   # by name, id or URL; removes the card only
hivey agent-map dashboard token                    # is a default dashboard token set?
hivey agent-map dashboard token es_…               # set it (the user's DASHBOARD_TOKEN_<USER> key)
```

- `add` with a name or URL already in the list updates that entry instead of adding a twin. Run
  `list` first so you edit the existing card rather than adding a duplicate under a new name.
- URLs must be `http://` or `https://` (local servers like `http://localhost:3000` are fine). Prefer the
  stable production URL (`vercel project ls` in the project folder, its README) over a preview URL.
- `--agent` is the agent that keeps the dashboard (its folder name); leave it out for the user's own.
  `--note` is one short line on what it shows.
- `--token default` uses the default dashboard token, so changing that key later updates every dashboard.
  A dashboard that needs a different token gets its own EnvSave key (`--token es_…`); a raw value works
  but lands in the settings file and your shell history. `--token-param` changes the URL parameter
  (default `token`). Never put a token in the name, note or URL.
- No default token yet: ask the user for their `DASHBOARD_TOKEN_<USER>` EnvSave key, or tell them to pick
  "my dashboard token" in the page's form, which asks for it once.
- If `hivey` is not on PATH: `python3 -I ~/.claude/skills/hivey-agent-map/scripts/dashboards.py …` (same
  arguments).

## When a dashboard won't show in the frame

The page says why. "Sends you to another site": a hosting login wall (turn it off and use the token login);
"does not allow being shown inside other pages": remove `X-Frame-Options` / `frame-ancestors` from the
site. A red dot means the site did not answer, or its token could not be read.
claude.ai artifact links (`claude.ai/artifact/…`, `claude.ai/public/artifacts/…`, `/embed` too) can never
show in the frame (claude.ai sends `frame-ancestors 'self'`). Registering them is fine: the page says so and
its "Open in a new tab" button opens them.

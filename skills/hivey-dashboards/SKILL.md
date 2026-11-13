---
name: hivey-dashboards
description: >
  Add, list, edit and remove the dashboards shown on the hivey agent fleet page's 🖥 Dashboards
  tab: one place to open every dashboard your agents keep (Vercel sites, local servers, Baserow
  views, Grafana, anything with a URL). Use whenever the user says "add this dashboard to the
  agent fleet", "register my dashboard", "remove/rename that dashboard", "which dashboards do I
  have", "add a login token to that dashboard", or gives a dashboard URL to keep track of; and,
  without being asked, right after you
  build or deploy a dashboard, report page or web view for an agent, so it shows up in the fleet.
  Works from any agent hivey runs (Claude Code, Codex, Grok, Antigravity).
allowed-tools:
  - Bash
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# hivey dashboards

The fleet page (hivey menu → agent fleet, or `hivey agent-map fleet`) has a **🖥 Dashboards** tab: the
dashboards listed on the left with a live up/down dot, and the one you pick loaded in a frame on the right. The same list is
edited from the web page (**+ Add dashboard**, Edit, Remove) and from the shell, so this skill and the
page always agree. It lives in `~/.hivey/agent-map/fleet-settings.json` under `"dashboards"`.

```bash
hivey agent-map dashboard list                     # name, URL, agent  (--json for the fields)
hivey agent-map dashboard add "Trading dashboard" https://trading.example.app \
    --agent trading-strategies --note "paper accounts, trades and job status"
hivey agent-map dashboard edit "Trading dashboard" --name "Paper trading" --url https://new.example.app
hivey agent-map dashboard remove "Paper trading"   # by name, id or URL
```

- `add` with a name or URL already in the list updates that entry instead of adding a twin.
- URLs must be `http://` or `https://` (local servers like `http://localhost:3000` are fine).
- `--agent` is the agent that keeps the dashboard (its folder name, e.g. `trading-strategies`);
  leave it out for the user's own. `--note` is one short line on what it shows.
- If `hivey` is not on PATH, run the script directly:
  `python3 -I ~/.claude/skills/hivey-agent-map/scripts/dashboards.py add …` (same arguments).

## How to do it

1. **Get the URL.** When the user names a project but not its URL, find it: the deployed URL (e.g.
   `vercel project ls` in that folder, a `README.md`, `.vercel/project.json`) or the local dev
   address. Prefer the stable production URL over a per-deploy preview URL.
2. **Check the list first** (`dashboard list`) so you edit the existing card instead of adding a
   duplicate under a new name.
3. **Run the command**, then tell the user the card is on the fleet page's 🖥 Dashboards tab
   (an open page shows it the next time the tab opens, or after a reload).
4. **Removing:** confirm which one when the name is ambiguous; `remove` deletes only the card,
   never the dashboard itself.

## Logins (tokens)

The page lists dashboards on the left and shows the picked one in a frame on the right. A private
dashboard can open logged in with a token:

```bash
hivey agent-map dashboard edit "X post review" --token es_… --token-param key   # ?key=<token>
hivey agent-map dashboard edit "Trading dashboard" --token es_… --vercel        # Vercel Deployment Protection
hivey agent-map dashboard edit "X post review" --token ""                       # remove the token
```

- Opening the dashboard goes through the local fleet server, which adds the token to the URL as a
  query parameter (`--token-param`, default `token`). The dashboard has to accept a token that way.
- **Use an EnvSave opaque key (`es_…`) for the token**, never the raw value: it stays in the
  user's vault and is read with `envsave get` when the dashboard opens. A raw token works, but
  it lands in the owner-only settings file and in your shell history. Never ask for a token in
  chat; ask the user for the `es_` key, or tell them to paste the token in the page's form
  (Edit → Login token), which no agent sees.
- **Sites behind Vercel's login** (a Vercel project with Deployment Protection) refuse to load in
  the frame; the page says so. Their "Protection Bypass for Automation" secret (Vercel project →
  Settings → Deployment Protection) with `--vercel` opens them in the frame, logged in.
- Sites that forbid framing (X-Frame-Options / frame-ancestors) show an **Open in tab** button;
  the browser's own saved login then applies.
- Never put a token in the name, the note or the URL itself.

# Giving each agent its skills (hivey.skills)

Follow this for every agent you create, after its mission is clear and before you show the plan.
Skills go **only into the agent's own folder** (`<agent folder>/.claude/skills/`), never into
the global Claude Code skills: global skills load into every session and crowd its context.

1. **skylls first.** The skills library is skylls: the user's published skills and the ones
   friends shared with them. `hivey skills` says whether skylls is installed and whether online
   search is on (if skylls is missing, ask the user before installing it; `hivey skills` shows
   how). Search once per capability the agent needs, one or two words each (e.g. `slack`,
   `pdf`, `browser`, `stock`):
   `skylls --json find <words> --limit 5` (one JSON object per line: `name`, `owner`,
   `summary`, `installed`). Pick only what the agent will really use (usually 2–6); each
   extra skill costs context in every one of its sessions.
2. **Missing capabilities.** If skylls has nothing for something the agent needs and
   `hivey skills` says online search is on, search skills.sh, following the `find-skills`
   skill (https://www.skills.sh/vercel-labs/skills/find-skills; installed globally):
   - first the leaderboard, https://skills.sh/ (popular, battle-tested skills by domain);
   - then `hivey skills find <query>` (runs `npx skills find <query>`), one query per
     capability, e.g. `stock market news`, `pdf extraction`;
   - see what a repo offers before choosing: `npx skills add <owner/repo> -l`.
   Judge results: prefer 1,000+ installs, official or well-known sources (`vercel-labs`,
   `anthropics`, the tool's own vendor) and GitHub stars; skip unmaintained or obscure ones.
   Don't install anything yet.
3. **Put the choice in the plan**: per agent, `Skills: a, b (skylls) · c (skylls, from
   <owner>) · owner/repo@skill (skills.sh, to install)`. Ask the user to confirm; online
   skills always need a yes, since they run with the agent's permissions.
4. **Install, once the agent's folder exists:**
   - skylls skills: `(cd <agent folder> && skylls add <name> -a claude)` (no `-g`: into that
     folder only; add `--from <owner>` when several people have one with that name).
   - skills.sh skills: `hivey skills add <agent folder> <owner/repo> --skill <name>`
     (installs into that folder only).
   Check `ls <agent folder>/.claude/skills`.
5. **Tell the agent.** In its CLAUDE.md, list its skills under Method with one line on when to
   use each, and pass them to `hivey swarm launch … --skills a,b` (or `hivey swarm profile`) so
   `hivey swarm directory` shows them.
6. **New skills are saved with skylls.** When you write a new skill for an agent (with
   `/skill-creator`), also offer to publish it so the user can reuse and share it: confirm
   first, then `skylls push <agent folder>/.claude/skills/<name> -m "<what it does>"`. Push
   scans for secrets and personal data and refuses if it finds any: remove them, never pass
   `--skip-scan`. Sharing (`skylls share <name> @user`) only when the user asks.

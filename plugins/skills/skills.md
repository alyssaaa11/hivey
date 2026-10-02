# Giving each agent its skills (hiver.skills)

Follow this for every agent you create, after its mission is clear and before you show the plan.
Skills go **only into the agent's own folder** (`<agent folder>/.claude/skills/`), never into
the global Claude Code skills: global skills load into every session and crowd its context.

1. **Library first.** `hiver skills` shows the user's skills library (default `~/SKILLS`; the
   user can change it in settings → skills) and whether online search is on.
   `hiver skills list` lists every skill with its description; narrow with
   `hiver skills list --grep <word>`, one word per capability the agent needs (e.g. `slack`,
   `pdf`, `browser`, `stock`). Pick only what the agent will really use (usually 2–6); each
   extra skill costs context in every one of its sessions.
2. **Missing capabilities.** If the library has nothing for something the agent needs and
   `hiver skills` says online search is on, search skills.sh, following the `find-skills`
   skill (https://www.skills.sh/vercel-labs/skills/find-skills; installed globally):
   - first the leaderboard, https://skills.sh/ (popular, battle-tested skills by domain);
   - then `hiver skills find <query>` (runs `npx skills find <query>`), one query per
     capability, e.g. `stock market news`, `pdf extraction`;
   - see what a repo offers before choosing: `npx skills add <owner/repo> -l`.
   Judge results: prefer 1,000+ installs, official or well-known sources (`vercel-labs`,
   `anthropics`, the tool's own vendor) and GitHub stars; skip unmaintained or obscure ones.
   Don't install anything yet.
3. **Put the choice in the plan**: per agent, `Skills: a, b (library) · owner/repo@skill
   (skills.sh, to install)`. Ask the user to confirm; online skills always need a yes, since
   they run with the agent's permissions.
4. **Install, once the agent's folder exists:**
   - library skills: `hiver skills copy <agent folder> <skill> [<skill>…]`
   - skills.sh skills: `hiver skills add <agent folder> <owner/repo> --skill <name>`
     (installs into that folder only).
   Check `ls <agent folder>/.claude/skills`.
5. **Tell the agent.** In its CLAUDE.md, list its skills under Method with one line on when to
   use each, and pass them to `hiver swarm launch … --skills a,b` (or `hiver swarm profile`) so
   `hiver swarm directory` shows them.

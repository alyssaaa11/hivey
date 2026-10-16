# Retrospective: improving this skill

Every swarm teaches something about running swarms. Capture it so the skill gets better, but
**don't edit the skill yourself**: write proposals for the user to review.

When the swarm finishes (after the final report), or the user stops it for good:
1. Review the run from evidence, not impressions: `.swarm/events.jsonl` (timeline, alerts,
   check failures, how long tasks sat in `review`), the task board, `hivey msg log --swarm
   <name>`, and your own interventions. Useful questions:
   - Where did agents stall, duplicate work or misunderstand?
   - Which watcher alerts were noise?
   - What did you do by hand that a script or rule could do?
   - Were the team shape, CLIs and models right? (The dashboard's tokens per agent: which agents
     used the most for what they delivered?)
   - Which skills were missing or unused?
2. Write `<vault>/wiki/lessons/swarm-retrospective_YYYY-MM-DD.md`: what worked, what didn't (with
   evidence from events and messages), and a **"Proposed improvements to the swarm creator"**
   section. Each proposal names the file to change (`SKILL.md`,
   `references/agent-claude-md.md`, a script), the change, and the problem it fixes. Link it from
   `index.md`.
3. Tell the user where the proposals are. They decide which to apply. Changes for this machine
   only go in this skill's `EXTRA_SKILL.md` (hivey updates never touch it); improvements for
   everyone belong in hivey itself.

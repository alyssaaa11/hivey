# Archify visual report (only if the archify skill is installed)

Use this only if the `archify` skill is installed; otherwise skip it, the prose final report is
enough.

A final report in prose is hard to scan. Alongside it, draw the project with `archify`, so the
user sees at a glance how the swarm and what it built fit together, and where each piece stands.
Follow archify's own authoring and finalize steps, saving to
`<root>/diagrams/<type>-<name>-<YYYYMMDD-HHMMSS>/` (one folder per diagram).

Draw at least:
1. **How the swarm works** (`architecture`): coordinator, every agent with its role and CLI, the
   message bus and Slack mirror, the wiki, and the deliverables each agent produced, with each
   agent's state in its label (finished / blocked / approved r2 …).
2. **Where the work stands** (`workflow`): the hand-off chain and critique loop, with each
   deliverable's status (approved, in review, in progress, not started) and open blockers.

Add a third when it helps: an `architecture` of the system the swarm built (from the real code),
or a `dataflow` for a research or data pipeline.

Then make it reachable from the wiki: export one light PNG per diagram into the vault as
`assets/<diagram>_YYYY-MM-DD.png`, embed it in `wiki/swarm-final-report.md` (`![[…png]]`), and
give the user the path of the interactive HTML. Earlier folders stay, so progress can be
compared over time.

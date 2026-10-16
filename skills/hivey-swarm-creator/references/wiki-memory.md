# Building the swarm's wiki memory

Read this when step 8 points here. The vault is `<root>/<name>-wiki`, inside the swarm folder.
Only the coordinator writes `wiki/`; agents write reports to `raw/reports/<agent>/`.

- **Build the memory as reports arrive.** When an agent writes a report to
  `raw/reports/<agent>/` (at DONE milestones and always at FINISHED), ingest it with the Ingest
  workflow in `<vault>/CLAUDE.md` (`/hivey-wiki-ingest <report>` finds the vault from inside the
  swarm folder). The wiki is the swarm's **connected** memory, so don't just file each report as
  its own page:
  - Break the report into findings, decisions, entities/concepts and lessons, and file each in
    `findings/`, `decisions/` or `lessons/` (concept pages as needed). **Merge into an existing
    page** when another agent already covered the topic, instead of making a near-duplicate.
  - **Connect across agents.** When two agents' findings relate (the researcher's data behind
    the builder's feature choice, the critic's issue with a design decision), link them both ways
    with `[[wiki-links]]`. Where they contradict, record it in a `> [!warning]` callout on both
    pages and in `lessons/`.
  - Every page cites its source report(s). Update `wiki/agents/<agent>.md` (status,
    deliverables, report links, the pages its work fed), then `index.md` and `log.md`.
- **When an agent sends FINISHED, ingest its final report before anything else.** A worker only
  counts as finished once its final report is in the wiki, fully linked, and its agent page says
  `finished`. A watcher "stopped" alert is often the moment to check this.
- **Connection pass.** When every worker has sent FINISHED (which needs critic approval), the
  critic has written its final review, and all final reports are ingested, run the Lint workflow
  in `<vault>/CLAUDE.md` (`/hivey-wiki-lint`) to find orphan pages, missing cross-links,
  contradictions and concepts that deserve their own page. Fix what it finds. Then write
  `wiki/swarm-final-report.md` (outcome vs. the original request, per-agent results linking
  their agent pages and reports, key findings linking their pages, open risks, next steps) and
  link it from `swarm-overview.md` and `index.md`, so it's the hub everything connects to. If the
  `archify` skill is installed, add the visual report (`references/archify-report.md`).

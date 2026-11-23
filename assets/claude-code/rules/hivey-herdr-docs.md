---
paths:
  - "docs/**"
  - "skills/herdr/**"
  - "distribution/**"
  - "README.md"
  - "CHANGELOG.md"
---

<!-- hivey: generated from assets/claude-code by scripts/claude-code-setup.sh; hivey update rewrites it. Put local changes in EXTRA_AGENTS.md. -->

# Herdr docs trees (from AGENTS.md "Docs")

- `docs/next/website/src/content/docs/`: unreleased docs. Update here when a user-facing change needs
  docs. `docs/next/README.md` stages root README changes.
- Don't edit `docs/next/CHANGELOG.md`, root `README.md`, root `CHANGELOG.md`, `docs/versions/`
  (published), `docs/preview/` (CI-owned) or `distribution/latest.json` / `preview.json` in normal
  work. The exceptions are focused corrections to published docs, or an explicit request.
- `skills/herdr/SKILL.md` tracks the latest stable herdr release. Don't edit it in feature work.
- Local plans and PRDs go in `.local/prd/` (gitignored).

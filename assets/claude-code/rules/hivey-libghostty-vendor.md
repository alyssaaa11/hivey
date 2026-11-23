---
paths:
  - "vendor/**"
---

<!-- hivey: generated from assets/claude-code by scripts/claude-code-setup.sh; hivey update rewrites it. Put local changes in EXTRA_AGENTS.md. -->

# Vendored libghostty-vt (from AGENTS.md)

- `vendor/libghostty-vt.vendor.json` records the vendored upstream commit.
- Every local patch is a file under `vendor/patches/libghostty-vt/` and an entry in
  `vendor/libghostty-vt.patches.md`. The entry gives the reason, herdr issue, upstream PR/discussion,
  base commit, touched files, verification, and exact removal condition.
- When updating: check every active patch. Drop the ones upstream now contains (with their index entry)
  and rerun their verification. Reapply the rest on the new source.
- `just check` verifies that every patch is indexed and reverse-applies cleanly. Never leave a patch
  unindexed or unapplied.
- Herdr needs Zig 0.16.0 to build it.

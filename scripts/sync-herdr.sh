#!/usr/bin/env bash
# Bring the latest herdr (upstream) into hiver.
#
#   scripts/sync-herdr.sh            merge upstream/master on branch sync-herdr, reapply the hiver
#                                    renames, build, run the swarm tests and a smoke test in a
#                                    throwaway session. Stops before touching main or anything live.
#   scripts/sync-herdr.sh --finish   merge sync-herdr into main, push origin, install the binary,
#                                    refresh the hiver skill and live-hand-off every running session
#                                    (agents keep running). Reattach open hiver windows afterwards.
#
# On a merge conflict it stops: fix the files, `git commit`, then run it again.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BRANCH=sync-herdr
BIN="$HOME/.local/bin/hiver"
HANDOFF="$HOME/.claude/skills/hiver/scripts/handoff.py"

say() { printf '\n== %s\n' "$*"; }

install_binary() {
  # cp over the running binary reuses its inode and macOS then SIGKILLs every new exec.
  cp target/release/hiver "$BIN.new" && mv -f "$BIN.new" "$BIN"
}

finish() {
  [ "$(git branch --show-current)" = "$BRANCH" ] || { echo "run without --finish first (not on $BRANCH)"; exit 1; }
  [ -z "$(git status --porcelain)" ] || { echo "uncommitted changes on $BRANCH"; exit 1; }
  say "merge $BRANCH into main and push"
  git switch -q main
  git merge --ff-only "$BRANCH" || git merge --no-edit "$BRANCH"
  git push origin main
  git branch -d "$BRANCH"
  say "install"
  cargo build --release
  install_binary
  "$BIN" --version
  "$BIN" skill install
  say "live handoff of running sessions"
  "$BIN" session list | awk '$2 == "running" {print $1}' | while read -r session; do
    printf '%s: ' "$session"
    python3 "$HANDOFF" "$session" | tail -1
  done
  say "done: detach (⌥Q) and reattach open hiver windows to get the new client"
}

[ "${1:-}" = "--finish" ] && { finish; exit 0; }

[ -z "$(git status --porcelain)" ] || { echo "commit or stash your changes first"; git status --short; exit 1; }
say "fetch upstream"
git fetch -q upstream
NEW=$(git rev-list --count HEAD..upstream/master)
if [ "$NEW" = 0 ]; then echo "hiver already has every herdr commit"; exit 0; fi
git log --oneline HEAD..upstream/master

if [ "$(git branch --show-current)" != "$BRANCH" ]; then
  git switch -q -c "$BRANCH" 2>/dev/null || git switch -q "$BRANCH"
fi
say "merge upstream/master ($NEW commits)"
if ! git merge --no-edit upstream/master; then
  echo
  echo "Conflicts in:"; git diff --name-only --diff-filter=U
  echo "Keep herdr's change and put the hiver part back on top, then: git add … && git commit && $0"
  exit 1
fi

say "reapply hiver renames"
python3 scripts/hiver_hooks.py
if [ -n "$(git status --porcelain)" ]; then
  git commit -qam "fix: reapply hiver renames after herdr merge"
fi

say "build and test"
cargo build --release
# nextest runs each test in its own process; plain `cargo test` in parallel is flaky here
# (tests share global state).
if cargo nextest --version >/dev/null 2>&1; then
  if OUT=$(cargo nextest run --release --no-fail-fast 2>&1); then STATUS=0; else STATUS=1; fi
  echo "$OUT" | grep -E "Summary|FAIL \[" || true
  [ "$STATUS" = 0 ] || { echo "tests failed (cargo nextest run --release)"; exit 1; }
else
  OUT=$(cargo test --release swarm -- --test-threads=1 2>&1 || true)
  echo "$OUT" | grep -E "^test result: ok. [1-9]|FAILED|panicked" || true
  if echo "$OUT" | grep -q FAILED; then echo "swarm tests failed"; exit 1; fi
fi

say "smoke test in a throwaway session"
SESSION="sync-test-$$"
NEWBIN="$ROOT/target/release/hiver"
"$NEWBIN" --session "$SESSION" server >/tmp/hiver-$SESSION.log 2>&1 &
sleep 4
"$NEWBIN" --version
"$NEWBIN" --session "$SESSION" swarm directory
"$NEWBIN" --session "$SESSION" swarm providers
"$NEWBIN" session stop "$SESSION" >/dev/null && "$NEWBIN" session delete "$SESSION" >/dev/null

say "ready: review, then run $0 --finish"

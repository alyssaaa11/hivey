#!/usr/bin/env bash
# Update hiver from source. `hiver update` runs this script.
#
#   hiver update            pull the fork (origin), merge new herdr commits (upstream) on branch
#                           sync-herdr, reapply the hiver renames, build, test, smoke-test in a
#                           throwaway session, then ask once before: merging into main + push,
#                           installing ~/.local/bin/hiver, refreshing the hiver skill and live
#                           handoff of every running session (agents keep running).
#   hiver update --yes      same, without the question.
#   hiver update --check    only show what's new (fork and herdr); change nothing.
#
# On a merge conflict it stops on branch sync-herdr: fix the files, `git commit`, then run
# `hiver update` again and it continues from there.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BRANCH=sync-herdr
BIN="$HOME/.local/bin/hiver"
HANDOFF="$HOME/.claude/skills/hiver/scripts/handoff.py"
YES=0
CHECK=0
for arg in "$@"; do
  case "$arg" in
    --yes|-y) YES=1 ;;
    --check) CHECK=1 ;;
    --finish) YES=1 ;;  # older name: continue and publish without asking
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option $arg (hiver update [--yes] [--check])"; exit 2 ;;
  esac
done

say() { printf '\n== %s\n' "$*"; }
# Known failures that also fail on main before any merge (local agent-detection override and
# live-handoff test setup on this machine); anything else failing stops the update.
KNOWN='test(server_reload_agent_manifests_reports_runtime_override) | test(agent_explain_evaluates_with_server_manifest_cache) | test(/^live_handoff_(ignores_leaked_default|preserves_client_socket_env|preserves_installed_plugins|preserves_http_servers|preserves_named_session)/)'

say "fetch"
git fetch -q origin
git fetch -q upstream
CURRENT=$(git branch --show-current)
FORK_NEW=$(git rev-list --count main..origin/main)
HERDR_NEW=$(git rev-list --count HEAD..upstream/master)
echo "fork (origin/main): $FORK_NEW new commit(s)"
echo "herdr (upstream):   $HERDR_NEW new commit(s)"
[ "$HERDR_NEW" = 0 ] || git log --oneline HEAD..upstream/master | sed 's/^/  /'
[ "$CHECK" = 1 ] && exit 0

if [ -e .git/MERGE_HEAD ]; then
  echo "a merge is in progress: fix the conflicts, git add … && git commit, then run hiver update again"
  git diff --name-only --diff-filter=U
  exit 1
fi
[ -z "$(git status --porcelain)" ] || { echo "the hiver repo ($ROOT) has uncommitted changes:"; git status --short; exit 1; }

if [ "$CURRENT" = main ]; then
  if [ "$FORK_NEW" != 0 ]; then
    say "pull the fork"
    git merge --ff-only origin/main
  fi
  if [ "$HERDR_NEW" != 0 ]; then
    git switch -q -c "$BRANCH" 2>/dev/null || git switch -q "$BRANCH"
  fi
elif [ "$CURRENT" != "$BRANCH" ]; then
  echo "on branch $CURRENT: switch to main (or $BRANCH to continue an update) first"; exit 1
fi

if [ "$(git branch --show-current)" = "$BRANCH" ] && [ "$(git rev-list --count HEAD..upstream/master)" != 0 ]; then
  say "merge herdr"
  if ! git merge --no-edit upstream/master; then
    echo
    echo "Conflicts in:"; git diff --name-only --diff-filter=U
    echo "Keep herdr's change and put the hiver part back on top, then: git add … && git commit && hiver update"
    exit 1
  fi
fi

say "reapply hiver renames"
python3 scripts/hiver_hooks.py >/dev/null
if [ -n "$(git status --porcelain)" ]; then
  git commit -qam "fix: reapply hiver renames after herdr merge"
fi

say "build"
cargo build --release 2>&1 | grep -vE "external contributor policy" | tail -2
NEWBIN="$ROOT/target/release/hiver"
UNPUSHED=$(git rev-list --count origin/main..HEAD)
if cmp -s "$NEWBIN" "$BIN" && [ "$UNPUSHED" = 0 ]; then
  say "hiver is up to date ($("$BIN" --version))"
  [ "$(git branch --show-current)" = "$BRANCH" ] && git switch -q main && git branch -d "$BRANCH" >/dev/null
  exit 0
fi

say "test"
if cargo nextest --version >/dev/null 2>&1; then
  if OUT=$(cargo nextest run --release --no-fail-fast -E "not ($KNOWN)" 2>&1); then STATUS=0; else STATUS=1; fi
  echo "$OUT" | grep -E "Summary|FAIL \[" | sort -u || true
else
  # Plain `cargo test` in parallel is flaky here (shared state); the swarm tests run serially.
  if OUT=$(cargo test --release swarm -- --test-threads=1 2>&1); then STATUS=0; else STATUS=1; fi
  echo "$OUT" | grep -E "^test result: ok. [1-9]|FAILED" || true
fi
[ "$STATUS" = 0 ] || { echo "tests failed: nothing was installed"; exit 1; }

say "smoke test in a throwaway session"
SESSION="update-test-$$"
"$NEWBIN" --session "$SESSION" server >"/tmp/hiver-$SESSION.log" 2>&1 &
sleep 4
"$NEWBIN" --version
"$NEWBIN" --session "$SESSION" swarm directory >/dev/null && echo "server answers"
"$NEWBIN" session stop "$SESSION" >/dev/null && "$NEWBIN" session delete "$SESSION" >/dev/null

RUNNING=$("$BIN" session list 2>/dev/null | awk '$2 == "running" {print $1}' | tr '\n' ' ')
if [ "$YES" != 1 ]; then
  echo
  echo "Ready. Next: $( [ "$(git branch --show-current)" = "$BRANCH" ] && echo "merge into main, " )push to origin,"
  echo "install $BIN and hand off running sessions: ${RUNNING:-none} (agents keep running)."
  if [ ! -t 0 ]; then echo "not interactive: rerun with --yes"; exit 1; fi
  read -r -p "Go ahead? [Y/n] " answer
  case "$answer" in n|N|no|No) echo "stopped; run hiver update again to continue"; exit 0 ;; esac
fi

if [ "$(git branch --show-current)" = "$BRANCH" ]; then
  say "merge into main"
  git switch -q main
  git merge --ff-only "$BRANCH" || git merge --no-edit "$BRANCH"
  git branch -d "$BRANCH" >/dev/null
fi
if [ "$(git rev-list --count origin/main..main)" != 0 ]; then
  say "push"
  git push -q origin main && echo "pushed to origin/main"
fi

say "install"
# cp over the running binary reuses its inode and macOS then SIGKILLs every new exec.
cp "$NEWBIN" "$BIN.new" && mv -f "$BIN.new" "$BIN"
"$BIN" --version
"$BIN" skill install

say "live handoff of running sessions"
for session in $RUNNING; do
  printf '%s: ' "$session"
  python3 "$HANDOFF" "$session" | tail -1
done
say "done: detach (⌥Q) and reattach open hiver windows to get the new client"

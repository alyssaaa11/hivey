#!/usr/bin/env bash
# Publishes the tarballs build-packages.mjs --pack wrote, in its ORDER (platform packages first,
# then @hivey/cli, which depends on them). Versions already on npm are skipped, so a failed
# release can simply be run again.
#
#   scripts/npm/publish.sh <tarballs dir> [--dry-run]
#
# In GitHub Actions it authenticates with trusted publishing (no token). On a computer, run
# `npm login` first (the first release of each package has to be published this way).
set -euo pipefail

DIR="${1:?usage: scripts/npm/publish.sh <tarballs dir> [--dry-run]}"
shift
DRY=()
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY=(--dry-run) ;;
    *) echo "publish: unknown option $arg" >&2; exit 2 ;;
  esac
done
[ -f "$DIR/ORDER" ] || { echo "publish: no $DIR/ORDER (run build-packages.mjs --pack)" >&2; exit 1; }

while IFS= read -r tarball; do
  [ -n "$tarball" ] || continue
  manifest=$(tar -xOzf "$DIR/$tarball" package/package.json)
  name=$(node -e 'const p = JSON.parse(process.argv[1]); console.log(p.name)' "$manifest")
  version=$(node -e 'const p = JSON.parse(process.argv[1]); console.log(p.version)' "$manifest")
  if [ "$(npm view "$name@$version" version 2>/dev/null)" = "$version" ]; then
    echo "$name@$version: already on npm"
    continue
  fi
  echo "$name@$version: publishing"
  npm publish "$DIR/$tarball" --access public ${DRY[@]+"${DRY[@]}"}
done <"$DIR/ORDER"

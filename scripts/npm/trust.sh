#!/usr/bin/env bash
# Lets the release workflow publish every hivey npm package with trusted publishing (GitHub
# OIDC, no token). Run once, after `npm login` and the first publish (a package must exist
# before it can trust a workflow). Each package asks for 2FA.
#
#   scripts/npm/trust.sh [owner/repo]     (default: alyssaaa11/hivey)
set -euo pipefail

REPO="${1:-alyssaaa11/hivey}"
WORKFLOW=publish-npm.yml
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

packages=$(node -e '
  const platforms = require(process.argv[1]);
  for (const key of Object.keys(platforms)) console.log(`@hivey/cli-${key}`);
  console.log("@hivey/cli");
' "$ROOT/npm/platforms.json")

for package in $packages; do
  echo "$package: trust $REPO .github/workflows/$WORKFLOW"
  npm trust github "$package" --file "$WORKFLOW" --repo "$REPO" --allow-publish --yes
done

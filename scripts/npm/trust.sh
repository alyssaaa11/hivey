#!/usr/bin/env bash
# Lets the release workflow publish @hivey/cli with trusted publishing (GitHub OIDC, no
# token). Run once, after `npm login` and the first publish (a package must exist before it
# can trust a workflow). Asks for 2FA.
#
#   scripts/npm/trust.sh [owner/repo]     (default: alyssaaa11/hivey)
set -euo pipefail

REPO="${1:-alyssaaa11/hivey}"
WORKFLOW=publish-npm.yml

echo "@hivey/cli: trust $REPO .github/workflows/$WORKFLOW"
npm trust github @hivey/cli --file "$WORKFLOW" --repo "$REPO" --allow-publish --yes

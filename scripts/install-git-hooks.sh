#!/usr/bin/env bash
# Activates this repo's own tracked git hooks (.githooks/) by pointing
# `core.hooksPath` at them -- `.git/hooks` itself is never tracked by
# git, so without this, the hooks in .githooks/ exist in the repo but
# do nothing. Idempotent; safe to re-run.
#
# Usage: scripts/install-git-hooks.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

chmod +x .githooks/pre-commit .githooks/commit-msg
git config core.hooksPath .githooks

echo "Git hooks installed: core.hooksPath -> .githooks"
echo "  pre-commit : cargo fmt --check + cargo clippy on every commit"
echo "  commit-msg : requires and executes a 'Tests-Run:' trailer on every commit"
echo
echo "See .githooks/commit-msg's own header comment for the Tests-Run convention,"
echo "and scripts/local-ci.sh for the full local pre-push verification script."

#!/usr/bin/env bash
# Local CI runner: everything a contributor (human or agent) must run
# and see pass *before* pushing to `origin main`, not after. This repo
# has no PR gate -- a push goes straight to CI on the trunk -- so a red
# GitHub Actions run discovered after the fact is a self-inflicted
# delay, not a normal part of the workflow. Mirrors, as closely as a
# single local machine can, the `rust` job in `.github/workflows/ci.yml`
# (the job that gates every push): `cargo fmt --check`, `cargo clippy`,
# `cargo test --workspace`, then the nim-planner unit tests.
#
# What this script does NOT cover, on purpose -- said out loud so a
# green run here is never mistaken for a green full CI matrix:
#   - the `windows` job (Windows-specific smoke tests; this script can
#     only run what the host OS actually is)
#   - the `docker-bootstrap` and `nlvm-experiment` jobs
#   - the many fixture-build / real-toolchain-invocation steps later in
#     the `rust` job (wide-parallel-graph, direct-native-link
#     experiments, D1-b1/D1-b2 evidence runners, ...) -- these are
#     expensive and exercised by `cargo test --workspace` itself where
#     they matter; this script does not re-run them as separate steps.
# Real CI is still the authority for the full matrix. This script's job
# is to catch the large majority of failures *before* a push, cheaply,
# on this one machine.
#
# Usage:
#   scripts/local-ci.sh            # fmt + clippy + full cargo test --workspace + nim-planner tests
#   scripts/local-ci.sh --fast     # fmt + clippy only (seconds, for an inner dev loop)
#   scripts/local-ci.sh --test-only "<cargo test filter>"
#                                   # fmt + clippy + `cargo test <filter>` (not the full suite)
#
# Exit status is non-zero if anything fails. Every step's own command
# and output stays visible -- no swallowed output on failure.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

MODE="full"
TEST_FILTER=""
if [[ "${1:-}" == "--fast" ]]; then
  MODE="fast"
elif [[ "${1:-}" == "--test-only" ]]; then
  MODE="filtered"
  TEST_FILTER="${2:-}"
  if [[ -z "$TEST_FILTER" ]]; then
    echo "scripts/local-ci.sh --test-only requires a cargo test filter argument" >&2
    exit 2
  fi
fi

step() { echo; echo "==> $*"; }

step "cargo fmt --all -- --check"
cargo fmt --all -- --check

step "cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

if [[ "$MODE" == "fast" ]]; then
  echo
  echo "local-ci.sh --fast: fmt + clippy clean. Full test suite NOT run -- do not push on this alone."
  exit 0
fi

if [[ "$MODE" == "filtered" ]]; then
  step "cargo test $TEST_FILTER"
  cargo test "$TEST_FILTER"
  echo
  echo "local-ci.sh --test-only: fmt + clippy + '$TEST_FILTER' clean. This is NOT the full suite --"
  echo "run scripts/local-ci.sh with no arguments before pushing a change with wider blast radius."
  exit 0
fi

step "cargo test --workspace"
cargo test --workspace

if command -v nim >/dev/null 2>&1; then
  step "nim-planner unit tests (planning_kernel, incremental_kernel)"
  (
    cd nim-planner
    nim c -r --path:src --nimcache:nimcache tests/test_planning_kernel.nim
    nim c -r --path:src --nimcache:nimcache tests/test_incremental_kernel.nim
  )
else
  echo
  echo "WARNING: nim not on PATH -- nim-planner unit tests were skipped locally." >&2
  echo "         Real CI still runs them; do not treat this run as a full substitute." >&2
fi

echo
echo "local-ci.sh: fmt + clippy + full workspace tests clean."
echo "Still NOT a full CI mirror -- see this script's own header for what real CI covers that this doesn't."

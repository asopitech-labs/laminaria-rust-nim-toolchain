#!/usr/bin/env bash
# Reverts scripts/apply-edit.sh: restores crates/leaf-b/src/lib.rs from
# the pristine lib.baseline.rs backup.
set -euo pipefail
cd "$(dirname "$0")/.."
cp crates/leaf-b/src/lib.baseline.rs crates/leaf-b/src/lib.rs
echo "reverted: crates/leaf-b is back to the 'baseline' variant"

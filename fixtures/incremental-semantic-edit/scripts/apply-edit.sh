#!/usr/bin/env bash
# Applies this fixture's designated single-line semantic edit: swaps
# crates/leaf-b/src/lib.rs for the committed lib.edited.rs variant. See
# ../EDIT.md for what should (and should not) recompile afterward.
set -euo pipefail
cd "$(dirname "$0")/.."
cp crates/leaf-b/src/lib.edited.rs crates/leaf-b/src/lib.rs
echo "applied: crates/leaf-b is now the 'edited' variant"

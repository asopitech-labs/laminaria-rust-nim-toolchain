#!/usr/bin/env bash
# issue #47 B-H3 formalization, question 2 (root-set confirmation cost): can the FFI root set
# that alopex-cli's TLS usage actually reaches into aws-lc-sys/bcm.c be confirmed as a Phase 1
# (semantic-determinable) fact using `cargo check` (type-check only, no codegen) at a cost small
# relative to bcm.c's own Phase 2 (execution-cost-opaque) compile cost of 12.30s (see
# measure-bcm-unity-build-cost.sh)?
#
# Builds an isolated crate depending only on aws-lc-rs (matching alopex-cli's resolved feature
# set: default-features = false + prebuilt-nasm, see measure-aws-lc-sys-feature-cost.sh) and runs
# `cargo check` from a clean target directory, with --verbose to show whether aws-lc-sys's
# build.rs (which invokes `cc` on bcm.c) is actually executed during a type-check-only build.
#
# Usage: scripts/research/measure-phase1-root-set-cost.sh
set -euo pipefail

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

cat > "$WORKDIR/Cargo.toml" <<'EOF'
[package]
name = "measure-issue47-q2"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
aws-lc-rs = { version = "=1.17.1", default-features = false, features = ["aws-lc-sys", "alloc", "ring-io", "ring-sig-verify", "prebuilt-nasm"] }

[profile.dev]
opt-level = 0
EOF
mkdir -p "$WORKDIR/src"
echo 'fn main() {}' > "$WORKDIR/src/main.rs"

cd "$WORKDIR"
cargo fetch >/dev/null 2>&1

echo "=== cargo check wall time (cold, type-check only, no explicit codegen requested) ==="
rm -rf target
START=$(date +%s.%N)
cargo check >/tmp/measure-issue47-q2-check.log 2>&1
END=$(date +%s.%N)
awk -v s="$START" -v e="$END" 'BEGIN{printf "wall_time_seconds: %.2f\n", e-s}'

echo "=== does cargo check invoke aws-lc-sys's build.rs (which compiles bcm.c via cc)? ==="
rm -rf target
LOG="$WORKDIR/check-verbose.log"
cargo check --verbose > "$LOG" 2>&1
if grep -q "Running .*build/aws-lc-sys-[0-9a-f]*/build-script-main" "$LOG"; then
  echo "YES: aws-lc-sys's build.rs (build-script-main, which compiles bcm.c via cc) is EXECUTED as part of \`cargo check\` -- Phase 1 (type-check) cannot be obtained without paying Phase 2's cost (native bcm.c compile) under the current Cargo/build.rs model."
else
  echo "NO: aws-lc-sys's build-script-main was not observed executing in this run"
fi

rm -f /tmp/measure-issue47-q2-check.log

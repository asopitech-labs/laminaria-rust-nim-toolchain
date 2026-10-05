#!/usr/bin/env bash
# Measure LAMINARIA's owned AArch64-Darwin compiler against the current
# Cargo+rustc reality baseline on exactly the same source file.
#
# This is a measurement procedure, not a compiler-selection gate. It makes
# four independently persisted `laminaria run` records per repetition:
#
#   * clean Cargo+rustc build;
#   * clean LAMINARIA owned-native build;
#   * Cargo+rustc rebuild after one identical source edit; and
#   * LAMINARIA owned-native rebuild after that edit.
#
# Each record carries the same Run-envelope/toolchain fingerprint and Level 1
# process/resource trace. The compiler bootstrap below is deliberately outside
# the timed commands: a compiler executable must exist before either target
# build can be measured. The measured Cargo command builds the workload, not
# LAMINARIA itself.
#
# Usage:
#   scripts/measure-owned-native-darwin.sh OUTPUT_DIRECTORY [REPETITIONS]
#
# OUTPUT_DIRECTORY must not exist. The script preserves it on success and
# failure so an operator can inspect every Run record and captured process
# trace. It never writes below the repository other than the ignored target/
# directory used to bootstrap the LAMINARIA CLI.

set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 OUTPUT_DIRECTORY [REPETITIONS]" >&2
  exit 2
fi

REPORT_ROOT="$1"
REPETITIONS="${2:-5}"
if [[ ! "$REPETITIONS" =~ ^[1-9][0-9]*$ ]]; then
  echo "REPETITIONS must be a positive integer, got: $REPETITIONS" >&2
  exit 2
fi
if [[ -e "$REPORT_ROOT" ]]; then
  echo "OUTPUT_DIRECTORY already exists: $REPORT_ROOT" >&2
  exit 2
fi
if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "owned native comparison requires an arm64 macOS host" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE_ROOT="$REPO_ROOT/fixtures/owned-native-call-compare"
source "$REPO_ROOT/scripts/activate-pinned-toolchains.sh"

mkdir -p "$REPORT_ROOT/runs" "$REPORT_ROOT/work"
COMPILER="$REPO_ROOT/target/release/laminaria"

require_clean_checkout() {
  local dirty_entries
  dirty_entries="$(git -C "$REPO_ROOT" status --porcelain)"
  if [[ -n "$dirty_entries" ]]; then
    echo "refusing to mix a dirty checkout into a comparable measurement:" >&2
    printf '%s\n' "$dirty_entries" >&2
    exit 1
  fi
}

# A later Run records the Git fingerprint at its own start. Recheck before
# and after every observation so unrelated test output or operator activity
# cannot silently turn one repetition set into a mixed clean/dirty report.
require_clean_checkout

# This is the bootstrap compiler, not either side of the measured workload.
cargo build --release -p laminaria-cli
if [[ ! -x "$COMPILER" ]]; then
  echo "expected bootstrap compiler is missing: $COMPILER" >&2
  exit 1
fi
require_clean_checkout

SDK_ROOT="$(xcrun --show-sdk-path)"
RESULTS="$REPORT_ROOT/artifact-exit-statuses.txt"
MANIFEST="$REPORT_ROOT/measurement-manifest.txt"
"$COMPILER" --version > "$REPORT_ROOT/laminaria-version.txt"
printf '%s\n' "workload=fixtures/owned-native-call-compare" > "$MANIFEST"
printf '%s\n' "repetitions=$REPETITIONS" >> "$MANIFEST"
printf '%s\n' "platform=$(uname -s) $(uname -m)" >> "$MANIFEST"
printf '%s\n' "sdk_root=$SDK_ROOT" >> "$MANIFEST"
printf '%s\n' "compiler=$COMPILER" >> "$MANIFEST"

record_run() {
  local label="$1"
  local observed_root="$2"
  shift 2
  "$COMPILER" run \
    --json \
    --workload-id owned-native-call-compare \
    --scenario-id "$label" \
    --requested-artifact owned-native-call-compare \
    --runs-root "$REPORT_ROOT/runs" \
    --lock "$REPO_ROOT/toolchains.lock.toml" \
    --repo-root "$REPO_ROOT" \
    --probe-level level1 \
    --observe "$observed_root" \
    -- "$@" > "$REPORT_ROOT/$label.run.json"
}

check_exit_status() {
  local label="$1"
  local executable="$2"
  local expected_status="$3"
  local actual_status
  set +e
  "$executable"
  actual_status=$?
  set -e
  printf '%s=%s\n' "$label" "$actual_status" >> "$RESULTS"
  if [[ "$actual_status" -ne "$expected_status" ]]; then
    echo "$label exited $actual_status, expected $expected_status" >&2
    exit 1
  fi
}

for repetition in $(seq 1 "$REPETITIONS"); do
  REPEAT_ROOT="$REPORT_ROOT/work/repetition-$repetition"
  CARGO_ROOT="$REPEAT_ROOT/cargo"
  OWNED_ROOT="$REPEAT_ROOT/owned"
  mkdir -p "$REPEAT_ROOT"
  cp -R "$FIXTURE_ROOT" "$CARGO_ROOT"
  cp -R "$FIXTURE_ROOT" "$OWNED_ROOT"

  record_run \
    "clean-cargo-rustc-r$repetition" \
    "$CARGO_ROOT/target" \
    cargo build --locked --release --manifest-path "$CARGO_ROOT/Cargo.toml" --target-dir "$CARGO_ROOT/target"
  require_clean_checkout
  record_run \
    "clean-laminaria-owned-r$repetition" \
    "$OWNED_ROOT/out" \
    "$COMPILER" owned-native-build --source "$OWNED_ROOT/src/main.rs" --output-dir "$OWNED_ROOT/out" --linker /usr/bin/ld --sdk-root "$SDK_ROOT" --minimum-macos-version 11.0 --json
  require_clean_checkout
  check_exit_status \
    "clean-cargo-rustc-r$repetition" \
    "$CARGO_ROOT/target/release/owned-native-call-compare" \
    75
  check_exit_status \
    "clean-laminaria-owned-r$repetition" \
    "$OWNED_ROOT/out/laminaria-main" \
    75

  # A single semantic source edit changes the expected process result from
  # 75 to 86 in both routes, while retaining their respective build outputs.
  sed -i '' 's/wrapping_add(1)/wrapping_add(2)/' "$CARGO_ROOT/src/main.rs"
  sed -i '' 's/wrapping_add(1)/wrapping_add(2)/' "$OWNED_ROOT/src/main.rs"
  if ! cmp -s "$CARGO_ROOT/src/main.rs" "$OWNED_ROOT/src/main.rs"; then
    echo "the Cargo and LAMINARIA incremental sources diverged" >&2
    exit 1
  fi
  shasum -a 256 "$CARGO_ROOT/src/main.rs" >> "$MANIFEST"

  record_run \
    "incremental-cargo-rustc-r$repetition" \
    "$CARGO_ROOT/target" \
    cargo build --locked --release --manifest-path "$CARGO_ROOT/Cargo.toml" --target-dir "$CARGO_ROOT/target"
  require_clean_checkout
  record_run \
    "incremental-laminaria-owned-r$repetition" \
    "$OWNED_ROOT/out" \
    "$COMPILER" owned-native-build --source "$OWNED_ROOT/src/main.rs" --output-dir "$OWNED_ROOT/out" --linker /usr/bin/ld --sdk-root "$SDK_ROOT" --minimum-macos-version 11.0 --json
  require_clean_checkout
  check_exit_status \
    "incremental-cargo-rustc-r$repetition" \
    "$CARGO_ROOT/target/release/owned-native-call-compare" \
    86
  check_exit_status \
    "incremental-laminaria-owned-r$repetition" \
    "$OWNED_ROOT/out/laminaria-main" \
    86
done

printf '%s\n' "measurement records: $REPORT_ROOT"
printf '%s\n' "run envelopes and Level 1 traces: $REPORT_ROOT/runs"
printf '%s\n' "artifact semantic checks: $RESULTS"

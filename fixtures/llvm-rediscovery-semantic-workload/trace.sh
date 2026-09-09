#!/usr/bin/env bash
# Issue #25's first experiment (docs/llvm-rediscovery-research.md):
# trace the semantic contract in ../CONTRACT.md through each compiler's
# own stages, for Rust and Nim independently, starting from source --
# not from pre-existing merged LLVM IR (that's fixtures/
# rust-nim-llvm-lto-compatibility/, #17's fixture, a separate thing).
#
# Stage-provenance chain this captures, matching the design doc's own
# notation:
#   semantic contract (../CONTRACT.md)
#   -> compiler/source representation (rustc's own MIR; no Nim
#      equivalent was found -- see NOTES.md's "unresolved" section)
#   -> lowering (--emit=llvm-ir)
#   -> LLVM-facing attributes/metadata (llvm-dis + grep "attributes")
#   -> optimization result (opt -O2 with --pass-remarks-output)
#   -> artifact (llc + link, for the Rust side locally; Nim/nlvm needs
#      the CI runner nlvm actually ships binaries for)
set -euo pipefail
cd "$(dirname "$0")"

: "${OPT_BIN:?OPT_BIN must point at an opt matching the target rustc bundled LLVM version}"
: "${LLVM_DIS_BIN:?LLVM_DIS_BIN must point at a matching-version llvm-dis}"

OUT_DIR="trace-out"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/rust"

echo "=== Rust: compiler/source representation (rustc's own MIR, unstable/inspection-only) ==="
rustc --crate-type=staticlib --emit=mir -o "$OUT_DIR/rust/add.mir" rust-src/add.rs
cat "$OUT_DIR/rust/add.mir"

echo "=== Rust: lowering (--emit=llvm-ir, no native codegen) ==="
rustc --crate-type=staticlib --emit=llvm-ir -O -o "$OUT_DIR/rust/add.ll" rust-src/add.rs

echo "=== Rust: LLVM-facing attributes/metadata on add() itself ==="
grep -A1 '^define.*@add(' "$OUT_DIR/rust/add.ll"
grep '^attributes' "$OUT_DIR/rust/add.ll"

echo "=== Rust: optimization result (opt -O2, capturing LLVM's own remarks) ==="
"$OPT_BIN" -O2 "$OUT_DIR/rust/add.ll" -o "$OUT_DIR/rust/add-opt.bc" \
  --pass-remarks='.*' --pass-remarks-missed='.*' --pass-remarks-analysis='.*' \
  --pass-remarks-output="$OUT_DIR/rust/remarks.yaml"
"$LLVM_DIS_BIN" "$OUT_DIR/rust/add-opt.bc" -o "$OUT_DIR/rust/add-opt.ll"
cat "$OUT_DIR/rust/add-opt.ll"
echo "--- remarks (LLVM's own record, not our inference) ---"
cat "$OUT_DIR/rust/remarks.yaml"

echo "=== Rust: artifact (native object, this platform) ==="
rustc --crate-type=staticlib -O -o "$OUT_DIR/rust/libadd.a" rust-src/add.rs
file "$OUT_DIR/rust/libadd.a" 2>/dev/null || true

if [ -z "${NLVM_BIN:-}" ]; then
  echo ""
  echo "=== Nim (via nlvm) skipped: NLVM_BIN not set (no macOS nlvm binary exists;"
  echo "=== run this script with NLVM_BIN set, e.g. in CI, for the Nim side) ==="
  exit 0
fi

mkdir -p "$OUT_DIR/nim"

echo ""
echo "=== Nim (via nlvm): compiler/source representation ==="
echo "no equivalent to rustc's --emit=mir was found in Nim's/nlvm's own CLI --"
echo "see NOTES.md's unresolved-questions section. Skipped, not fabricated."

echo "=== Nim (via nlvm): lowering (emit LLVM IR, no C anywhere, no native codegen) ==="
rm -rf ~/.cache/nim/add_d
"$NLVM_BIN" c -c --app:staticlib --noMain --nimcache:nimcache -o:"$OUT_DIR/nim/add" nim-src/add.nim
NIM_LL=$(find . -maxdepth 3 -iname 'add.ll' | head -1)
if [ -z "$NIM_LL" ]; then
  echo "no add.ll produced by nlvm -- see nlvm's own output above for where it went"
  find . -iname '*.ll'
  exit 1
fi
TARGET_LL="$OUT_DIR/nim/add.ll"
if [ "$(cd "$(dirname "$NIM_LL")" && pwd)/$(basename "$NIM_LL")" != "$(cd "$(dirname "$TARGET_LL")" && pwd)/$(basename "$TARGET_LL")" ]; then
  cp "$NIM_LL" "$TARGET_LL"
else
  TARGET_LL="$NIM_LL"
fi
echo "nim LLVM IR at: $NIM_LL"

echo "=== Nim (via nlvm): LLVM-facing attributes/metadata on add() itself ==="
grep -A1 '^define.*@add(' "$TARGET_LL" || echo "(no @add definition found -- see full add.ll)"
grep '^attributes' "$TARGET_LL" || true

echo "=== Nim (via nlvm): optimization result (opt -O2, capturing LLVM's own remarks) ==="
"$OPT_BIN" -O2 "$TARGET_LL" -o "$OUT_DIR/nim/add-opt.bc" \
  --pass-remarks='.*' --pass-remarks-missed='.*' --pass-remarks-analysis='.*' \
  --pass-remarks-output="$OUT_DIR/nim/remarks.yaml"
"$LLVM_DIS_BIN" "$OUT_DIR/nim/add-opt.bc" -o "$OUT_DIR/nim/add-opt.ll"
cat "$OUT_DIR/nim/add-opt.ll"
echo "--- remarks (LLVM's own record, not our inference) ---"
cat "$OUT_DIR/nim/remarks.yaml"

echo "=== side-by-side: does add()'s attribute set differ between the two sides? ==="
diff <(grep '^attributes' "$OUT_DIR/rust/add.ll" || true) <(grep '^attributes' "$TARGET_LL" || true) \
  && echo "identical attribute sets" || echo "(difference shown above -- expected; see NOTES.md)"

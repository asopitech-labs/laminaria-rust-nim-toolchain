#!/usr/bin/env bash
# Issue #25's candidate-substrate prototype (see CONTRACT.md): builds four
# independent programs for the same add_or_double/double workload -- the
# real Rust binary, the real Nim binary, the substrate's own reference
# evaluator, and the substrate's own LLVM-IR-projected-and-compiled
# binary -- and diffs their printed output byte-for-byte, then runs the
# allowed-vs-rejected inlining demo.
set -euo pipefail
cd "$(dirname "$0")"

: "${LLC_BIN:?LLC_BIN must point at an llc matching the target rustc bundled LLVM version}"

OUT_DIR="trace-out"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

echo "=== Real Rust binary ==="
rustc -O -o "$OUT_DIR/rust-binary" rust-src/add_or_double.rs
"$OUT_DIR/rust-binary" | tee "$OUT_DIR/rust.out"

echo "=== Substrate: reference evaluator ==="
(cd substrate && cargo build -q)
substrate/target/debug/substrate eval | tee "$OUT_DIR/substrate-eval.out"

echo "=== Substrate: unit tests (representation/evaluator/inliner self-checks) ==="
(cd substrate && cargo test -q)

echo "=== Substrate: LLVM-IR backend projection (LLVM 22, matching this repo's pinned version) ==="
substrate/target/debug/substrate emit-ir > "$OUT_DIR/substrate.ll"
# -relocation-model=pic: llc's default is non-PIC object code, which a
# modern Linux system cc/ld (PIE executables by default on Ubuntu 22.04+)
# refuses to link ("relocation R_X86_64_32 ... can not be used when making
# a PIE object") -- a real failure this fixture's first CI run hit, not
# reproduced locally on macOS (ld64 tolerates it there). Requesting PIC
# unconditionally is correct on every platform this fixture runs on, not
# just Linux -- it doesn't change any test input's output, since the
# grammar this emitter covers has no absolute-address-dependent behavior.
"$LLC_BIN" -relocation-model=pic -filetype=obj "$OUT_DIR/substrate.ll" -o "$OUT_DIR/substrate.o"
cc "$OUT_DIR/substrate.o" -o "$OUT_DIR/substrate-ir-binary"
"$OUT_DIR/substrate-ir-binary" | tee "$OUT_DIR/substrate-ir.out"

echo "=== Cross-check: Rust binary vs. substrate reference evaluator vs. substrate LLVM-IR binary ==="
diff "$OUT_DIR/rust.out" "$OUT_DIR/substrate-eval.out" \
  && echo "rust == substrate-eval: identical" \
  || { echo "MISMATCH: rust vs substrate-eval"; exit 1; }
diff "$OUT_DIR/rust.out" "$OUT_DIR/substrate-ir.out" \
  && echo "rust == substrate-ir: identical" \
  || { echo "MISMATCH: rust vs substrate-ir (LLVM-IR-projected binary)"; exit 1; }

echo "=== Allowed-vs-rejected inlining demo ==="
substrate/target/debug/substrate demo-inline

if [ -z "${NIM_BIN:-}" ]; then
  echo ""
  echo "=== Nim binary skipped: NIM_BIN not set (run this script with NIM_BIN set, e.g. in CI) ==="
  exit 0
fi

echo "=== Real Nim binary ==="
"$NIM_BIN" c -d:release --hints:off --nimcache:"$OUT_DIR/nimcache" \
  -o:"$OUT_DIR/nim-binary" nim-src/add_or_double.nim
"$OUT_DIR/nim-binary" | tee "$OUT_DIR/nim.out"

echo "=== Cross-check: Nim binary vs. Rust binary ==="
diff "$OUT_DIR/rust.out" "$OUT_DIR/nim.out" \
  && echo "rust == nim: identical" \
  || { echo "MISMATCH: rust vs nim"; exit 1; }

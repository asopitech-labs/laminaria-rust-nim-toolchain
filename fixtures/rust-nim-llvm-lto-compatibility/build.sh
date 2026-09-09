#!/usr/bin/env bash
# #11's "Rust/Nim 2/Nimony shared LLVM/LTO compatibility workload" —
# the minimal first proof: merge Rust's own LLVM IR and Nim's own LLVM
# IR (via nlvm) into ONE module with llvm-link, before either side
# reaches native codegen, then run LLVM's own optimizer on the merged
# module and inspect the result. See NOTES.md.
#
# Requires: rustc (this repo's pinned toolchain), nlvm (see
# fixtures/direct-native-link/build.sh for how it's fetched in CI),
# llvm-link/opt/llvm-dis matching nlvm's pinned LLVM version (see
# NOTES.md for why the version has to match).
set -euo pipefail
cd "$(dirname "$0")"

: "${NLVM_BIN:?NLVM_BIN must point at the nlvm binary}"
: "${LLVM_LINK_BIN:?LLVM_LINK_BIN must point at a matching-version llvm-link}"
: "${OPT_BIN:?OPT_BIN must point at a matching-version opt}"
: "${LLVM_DIS_BIN:?LLVM_DIS_BIN must point at a matching-version llvm-dis}"

rm -f rust-src/rust.ll nim-src/main.ll merged.bc merged-opt.bc merged-opt.ll

echo "--- rustc: emit LLVM IR (no native codegen) ---"
rustc --crate-type=staticlib --emit=llvm-ir -O -o rust-src/rust.ll rust-src/lib.rs

echo "--- nlvm: emit LLVM IR (no native codegen) ---"
"$NLVM_BIN" c -c --nimcache:nimcache -o:nim-src/main nim-src/main.nim
# nlvm's `-c` output location isn't documented precisely; find whatever
# .ll it actually produced rather than assuming a fixed path.
NIM_LL=$(find . -maxdepth 3 -iname 'main.ll' | head -1)
if [ -z "$NIM_LL" ]; then
  echo "no main.ll produced by nlvm -- see nlvm's own output above for where it went"
  find . -iname '*.ll'
  exit 1
fi
echo "nim LLVM IR at: $NIM_LL"

echo "--- llvm-link: merge both modules into one, before native codegen ---"
"$LLVM_LINK_BIN" "$NIM_LL" rust-src/rust.ll -o merged.bc

echo "--- opt: run LLVM's own optimizer on the merged module ---"
"$OPT_BIN" -O2 merged.bc -o merged-opt.bc

echo "--- llvm-dis: inspect the result ---"
"$LLVM_DIS_BIN" merged-opt.bc -o merged-opt.ll
cat merged-opt.ll

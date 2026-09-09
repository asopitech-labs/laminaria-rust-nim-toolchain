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
: "${LLC_BIN:?LLC_BIN must point at a matching-version llc}"

rm -f rust-src/rust.ll nim-src/main.ll merged.bc merged-opt.bc merged-opt.ll merged-opt.o merged-native remarks.yaml

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

echo "--- opt: run LLVM's own optimizer on the merged module, capturing its own internal optimization-remarks instrumentation ---"
# White-box evidence, not black-box process observation: opt's own pass
# manager reports every inlining/optimization decision it makes, as
# structured YAML, with real cost/threshold numbers -- this answers
# "did cross-language inlining/optimization actually happen" directly
# from LLVM's own internals, replacing/backing up manual disassembled-IR
# reading with the same real evidence a human would otherwise have to
# infer by eye. Verified locally first (a synthetic two-function Rust
# case, unrelated to this fixture) that --pass-remarks-output produces
# real, non-empty YAML with actual inlining cost/threshold data before
# relying on it here -- see NOTES.md.
"$OPT_BIN" -O2 merged.bc -o merged-opt.bc \
  --pass-remarks='.*' --pass-remarks-missed='.*' --pass-remarks-analysis='.*' \
  --pass-remarks-output=remarks.yaml

echo "--- optimization remarks mentioning rust_add (LLVM's own record of what it decided, not our inference) ---"
grep -B3 -A8 "rust_add" remarks.yaml || echo "(rust_add did not appear in any remark -- see full remarks.yaml)"

echo "--- llvm-dis: inspect the result ---"
"$LLVM_DIS_BIN" merged-opt.bc -o merged-opt.ll
cat merged-opt.ll

echo "--- llc: compile the merged+optimized module to a native object ---"
# -relocation-model=pic: llc defaults to a non-PIC relocation model,
# which produces R_X86_64_32S relocations against .rodata that the
# system linker rejects when building a PIE (Ubuntu's default) --
# "relocation ... can not be used when making a PIE object." Rust's own
# emitted IR already carries `!{i32 8, !"PIC Level", i32 2}`; llc still
# needs this told explicitly since it doesn't read that module flag as
# a relocation-model default on its own.
"$LLC_BIN" -relocation-model=pic -filetype=obj merged-opt.bc -o merged-opt.o

echo "--- cc: link the native object into an executable (system driver, standard crt/libs) ---"
cc merged-opt.o -o merged-native -lpthread

echo "--- run the fully-merged, natively-compiled binary ---"
./merged-native

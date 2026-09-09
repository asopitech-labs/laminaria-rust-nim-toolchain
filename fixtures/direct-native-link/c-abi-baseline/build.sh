#!/usr/bin/env bash
# Layer 5 required evidence: builds the "conventional C ABI baseline"
# counterpart to ../nim-bin/main.nim, against the *same* unmodified
# ../rust-lib static library, and reports the comparison evidence
# (object/binary size, generated-C size, symbol/relocation inspection,
# build step cost) docs/rust-nim-native-linking.md's Layer 5 requires.
# See ../NOTES.md for how to read the output.
set -euo pipefail
cd "$(dirname "$0")"

: "${CBINDGEN_BIN:=cbindgen}"

command -v "$CBINDGEN_BIN" >/dev/null 2>&1 || {
  echo "error: cbindgen not found on PATH (install with: cargo install cbindgen --locked)" >&2
  exit 1
}

# --- Step 1: build the shared, unmodified rust-lib. Timed separately so
# this cost is excluded from the header-generation cost measured below
# -- it's identical work the direct route also pays.
cargo build --manifest-path ../rust-lib/Cargo.toml --release

# --- Step 2: generate the conventional C ABI contract. This step, and
# only this step, has no counterpart on the direct route -- it is the
# actual measured build-cost delta between the two paths.
rm -f bindings.h
HEADER_GEN_START=$(date +%s%N)
"$CBINDGEN_BIN" --crate rust-lib --lang c -o bindings.h ../rust-lib
HEADER_GEN_END=$(date +%s%N)
HEADER_GEN_MS=$(( (HEADER_GEN_END - HEADER_GEN_START) / 1000000 ))
echo "cbindgen header generation: ${HEADER_GEN_MS}ms, $(wc -l <bindings.h | tr -d ' ') lines, $(wc -c <bindings.h | tr -d ' ') bytes"

# --- Step 3: compile Nim against the generated header, linking the
# same static library the direct route uses.
LIB_DIR="../rust-lib/target/release"
OUT_BIN="c_abi_baseline_out"
rm -f "${OUT_BIN}"

nim c \
  --nimcache:nimcache \
  --passC:"-I." \
  --passL:"-L${LIB_DIR} -lrustlib" \
  -o:"${OUT_BIN}" \
  main.nim

"./${OUT_BIN}"

# --- Step 4: comparison evidence. Run only after ../build.sh has
# already produced ../nim-bin/direct_native_link_out, so both binaries
# being compared were built in the same environment in the same run.
echo ""
echo "--- comparison evidence (see ../NOTES.md) ---"
if [ -f "../nim-bin/direct_native_link_out" ]; then
  echo "direct route binary:"
  ls -l "../nim-bin/direct_native_link_out"
  size "../nim-bin/direct_native_link_out" 2>/dev/null || true
  echo "c-abi-baseline route binary:"
  ls -l "${OUT_BIN}"
  size "${OUT_BIN}" 2>/dev/null || true
else
  echo "(../nim-bin/direct_native_link_out not present -- run ../build.sh first for a same-run comparison)"
fi

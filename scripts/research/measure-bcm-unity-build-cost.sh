#!/usr/bin/env bash
# issue #64 P0-beta: measure whether aws-lc-sys's `bcm.c` unity-build translation unit
# (crypto/fipsmodule/bcm.c, which #includes ml_dsa.c/ml_kem.c/pqdsa.c/p_kem.c/p_pqdsa.c
# unconditionally -- no feature gate, see cc_builder source lists in the aws-lc-sys crate)
# spends a disproportionate share of its compile time on post-quantum code (ML-DSA/ML-KEM),
# as a structural-cost explanation (hypothesis beta) distinct from the reachability/feature-flag
# hypothesis (alpha, refuted in issue #64 P0).
#
# Compiles bcm.c standalone (a) unmodified and (b) with the five post-quantum #include lines
# stripped, timing each with `cc -c` (object generation only, no link -- undefined symbols from
# the strip are irrelevant since nothing is linked). Requires a local `cc` and the aws-lc-sys
# 0.42.0 source tree already fetched into the Cargo registry cache (run after
# scripts/research/measure-aws-lc-sys-feature-cost.sh or any workspace `cargo fetch`/`cargo build`
# that resolves aws-lc-sys).
#
# Usage: scripts/research/measure-bcm-unity-build-cost.sh
set -euo pipefail

REG_SRC="$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -maxdepth 1 -type d -iname 'index.crates.io-*' | head -1)"
D="$REG_SRC/aws-lc-sys-0.42.0"
if [ ! -d "$D" ]; then
  echo "aws-lc-sys-0.42.0 not found under $REG_SRC -- run cargo fetch against a crate depending on aws-lc-sys first" >&2
  exit 1
fi
BASE="$D/aws-lc/crypto/fipsmodule"

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

cp "$BASE/bcm.c" "$WORKDIR/bcm_no_pq.c"
sed -i '/#include "ml_dsa\/ml_dsa.c"/d; /#include "ml_kem\/ml_kem.c"/d; /#include "pqdsa\/pqdsa.c"/d; /#include "evp\/p_kem.c"/d; /#include "evp\/p_pqdsa.c"/d' "$WORKDIR/bcm_no_pq.c"

CFLAGS=(-c -O2 -I"$D/aws-lc/include" -I"$D/aws-lc/crypto" -I"$D/aws-lc/crypto/fipsmodule" -DOPENSSL_NO_ASM)

echo "=== preprocessed line counts (comments/blank stripped) ==="
echo "bcm.c full: $(cc -E "${CFLAGS[@]:1}" "$BASE/bcm.c" 2>/dev/null | grep -v '^#' | grep -cv '^\s*$')"
echo "ml_dsa.c standalone: $(cc -E -I"$D/aws-lc/include" -I"$D/aws-lc/crypto" -I"$D/aws-lc/crypto/fipsmodule" "$BASE/ml_dsa/ml_dsa.c" 2>/dev/null | grep -v '^#' | grep -cv '^\s*$')"
echo "ml_kem.c standalone: $(cc -E -I"$D/aws-lc/include" -I"$D/aws-lc/crypto" -I"$D/aws-lc/crypto/fipsmodule" "$BASE/ml_kem/ml_kem.c" 2>/dev/null | grep -v '^#' | grep -cv '^\s*$')"

echo "=== compile time: bcm.c full (3 runs) ==="
for i in 1 2 3; do
  START=$(date +%s.%N)
  cc "${CFLAGS[@]}" -o "$WORKDIR/bcm_full.o" "$BASE/bcm.c" 2>/dev/null
  END=$(date +%s.%N)
  awk -v s="$START" -v e="$END" 'BEGIN{printf "%.2f\n", e-s}'
done

echo "=== compile time: bcm.c with post-quantum #includes stripped (3 runs) ==="
for i in 1 2 3; do
  START=$(date +%s.%N)
  cc "${CFLAGS[@]}" -I"$BASE" -o "$WORKDIR/bcm_no_pq.o" "$WORKDIR/bcm_no_pq.c" 2>/dev/null
  END=$(date +%s.%N)
  awk -v s="$START" -v e="$END" 'BEGIN{printf "%.2f\n", e-s}'
done

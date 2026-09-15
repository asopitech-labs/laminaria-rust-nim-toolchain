#!/usr/bin/env bash
# issue #47 B-H3 formalization, question 1 (decomposability): for aws-lc-sys's bcm.c unity-build
# translation unit (118 member .c files #included into one TU), how many members can be compiled
# as INDEPENDENT translation units (standalone `cc -c`) without pulling in the rest of bcm.c, and
# how many require true structural coupling (static-scoped cross-member symbols, e.g. via
# fipsmodule/delocate.h's DEFINE_METHOD_FUNCTION/DEFINE_LOCAL_DATA macros, or `static inline`
# helpers such as aes/mode_wrappers.c's aes_hw_encrypt_wrapper)?
#
# This tests B-H3's "summary/body separation" decision variable concretely: if member M is not
# reached by the FFI root set alopex-cli's TLS usage pulls (Phase 1 fact), can M's body be
# omitted from materialization without pulling in bcm.c as a whole? A member that fails standalone
# compilation even with matched include paths and macro predefinitions is NOT independently
# omittable -- its cost is entangled with bcm.c's other members regardless of reachability.
#
# Method: for each of bcm.c's 118 #include members, attempt `cc -c` in isolation (no link),
# first with only the same include paths bcm.c itself uses, then (for failures) with
# cpucap/internal.h force-included via -include to match bcm.c's actual macro-visibility order
# (bcm.c #includes cpucap/internal.h partway through its own member list, before most members that
# need SET_DIT_AUTO_RESET -- omitting this in a naive per-file compile is a harness artifact, not
# real coupling). Remaining failures are reported as genuinely structurally coupled, with the
# first compiler error as evidence.
#
# Usage: scripts/research/measure-bcm-decomposability.sh
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

grep '^#include "' "$BASE/bcm.c" | sed 's/#include "//;s/"//' > "$WORKDIR/members.txt"

INC=(-I"$D/aws-lc/include" -I"$D/aws-lc/crypto" -I"$BASE")
FORCE_INC=(-include "$BASE/cpucap/internal.h")

ok=0
fail=0
echo "member,result,first_error"
while read -r f; do
  case "$f" in ../internal.h) continue;; esac
  path="$BASE/$f"
  [ -f "$path" ] || continue
  out="$WORKDIR/$(echo "$f" | tr '/' '_').o"
  errfile="$WORKDIR/$(echo "$f" | tr '/' '_').err"
  if cc -c -O2 -DOPENSSL_NO_ASM "${INC[@]}" "${FORCE_INC[@]}" -o "$out" "$path" 2>"$errfile"; then
    echo "$f,OK,"
    ok=$((ok+1))
  else
    firsterr=$(grep -m1 "error:" "$errfile" | tr ',' ';')
    echo "$f,FAIL,$firsterr"
    fail=$((fail+1))
  fi
done < "$WORKDIR/members.txt"

echo "# decomposable (standalone-compilable): $ok" >&2
echo "# structurally coupled (requires bcm.c unity build): $fail" >&2

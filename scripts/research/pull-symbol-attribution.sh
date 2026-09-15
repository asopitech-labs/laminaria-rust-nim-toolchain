#!/usr/bin/env bash
# Issue #63 P0 (pull-driven reframing): attribute alopex-cli's final linked binary symbols to
# their origin crate, purely from the produced artifact (pull side) -- no reference to
# Cargo.toml `links`/build-dependency declarations (push side) at this stage. The push-side
# comparison is a *separate*, later step, not the starting question.
#
# Run inside the laminaria-issue62-monoitems container against a built
# /work/target/debug/alopex binary.
set -euo pipefail

BIN=/work/target/debug/alopex
OUT_DIR=${1:-/work/p0-pull-symbols}
mkdir -p "$OUT_DIR"

echo "== binary size ==" | tee "$OUT_DIR/summary.txt"
stat -c '%s bytes' "$BIN" | tee -a "$OUT_DIR/summary.txt"

echo "== extracting defined symbols (nm) ==" >&2
nm -C --defined-only "$BIN" > "$OUT_DIR/nm-defined-demangled-cxxfilt.txt" || true
nm --defined-only "$BIN" > "$OUT_DIR/nm-defined-raw.txt"

echo "== demangling with rustfilt (Rust v0/legacy accurate) ==" >&2
awk '{print $3}' "$OUT_DIR/nm-defined-raw.txt" | grep -E '^_Z|^_R' > "$OUT_DIR/mangled-names.txt" || true
rustfilt -i "$OUT_DIR/mangled-names.txt" -o "$OUT_DIR/demangled-names.txt"

echo "== attributing each defined symbol to its leading path segment (crate name) ==" >&2
# A demangled Rust symbol is typically `crate_name::module::path::to::Item::method`.
# Foreign (C/C++) symbols demangle to themselves (no `::`) and are bucketed separately.
python3 - "$OUT_DIR/demangled-names.txt" "$OUT_DIR/crate-attribution.tsv" <<'PY'
import sys, re, collections

in_path, out_path = sys.argv[1], sys.argv[2]
counts = collections.Counter()
total = 0
CRATE_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

with open(in_path, errors="replace") as f:
    for line in f:
        name = line.rstrip("\n")
        if not name:
            continue
        total += 1
        # Two shapes cover the vast majority of demangled Rust items:
        #   1. plain path:              crate_name::module::Item::method
        #   2. impl-block-qualified:    <crate_name::Type as other_crate::Trait>::method
        #      or generic instantiation: <crate_name::Type>::method::<other_crate::Arg>
        # Form 2's crate name is the first identifier *inside* the leading `<`, not the
        # symbol's own first token -- a naive `^ident::` match misses every such symbol,
        # which is most trait-method/generic monomorphizations (the initial version of this
        # script misclassified 150015/191484 symbols this way; see issue #63 P0 pull-driven
        # rewrite notes for the correction).
        if name.startswith("<"):
            m = CRATE_RE.match(name[1:])
        else:
            m = CRATE_RE.match(name)
        if m and "::" in name:
            counts[m.group(0)] += 1
        elif m:
            # A bare identifier with no `::` at all (e.g. a single exported C symbol name
            # that happens to demangle to itself) is not a Rust path -- keep it separate
            # from actual crate-path attribution instead of guessing.
            counts["<no-path-symbol (likely foreign/C or single-ident)>"] += 1
        else:
            counts["<unparseable>"] += 1

with open(out_path, "w") as f:
    f.write(f"total_defined_symbols\t{total}\n")
    for crate, n in counts.most_common():
        f.write(f"{crate}\t{n}\n")
print(f"wrote {out_path}: {total} symbols across {len(counts)} buckets", file=sys.stderr)
PY

echo "== top 30 crate buckets by defined-symbol count (Rust-mangled symbols) ==" | tee -a "$OUT_DIR/summary.txt"
tail -n +2 "$OUT_DIR/crate-attribution.tsv" | sort -t$'\t' -k2 -rn | head -30 | tee -a "$OUT_DIR/summary.txt"

echo "== extracting non-mangled (bare C ABI) defined symbols ==" >&2
awk '{print $3}' "$OUT_DIR/nm-defined-raw.txt" | grep -vE '^_Z|^_R' > "$OUT_DIR/c-abi-names.txt" || true

echo "== attributing C ABI symbols to known vendored C-library prefixes ==" >&2
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
python3 "$SCRIPT_DIR/classify-c-abi-symbols.py" "$OUT_DIR/c-abi-names.txt" "$OUT_DIR/c-abi-attribution.tsv" \
  | tee -a "$OUT_DIR/summary.txt"

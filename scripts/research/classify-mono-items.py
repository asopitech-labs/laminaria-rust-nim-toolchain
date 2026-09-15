#!/usr/bin/env python3
"""Issue #63 P0 (pull-driven rewrite, follow-up): attribute `-Zprint-mono-items=yes` output lines
to their originating crate via the codegen-unit (cgu) name, which `cargo build
-p alopex-cli --bin alopex` (with RUSTFLAGS=-Zprint-mono-items=yes) emits to stdout as
`MONO_ITEM ... @@ <crate_name>.<hash>-cgu.<n>[Linkage]`.

Correction to issue #62 P0's note that "cgu names are hashed so exact crate-origin boundaries
cannot be determined": the crate name IS present as a literal prefix before the hash in the
common case. The one real limitation is a build-script binary or a crate's own root/main cgu,
which appears as a bare hash with no crate-name prefix at all (`@@ <hash>[Linkage]`) -- those
lines are bucketed separately rather than misattributed.
"""
import collections
import re
import sys

CGU_RE = re.compile(r"@@ ([A-Za-z0-9_]+)\.[0-9a-f]+-cgu")


def main() -> int:
    in_path = sys.argv[1]
    out_path = sys.argv[2]
    counts = collections.Counter()
    total = 0
    with open(in_path, errors="replace") as f:
        for line in f:
            if not line.startswith("MONO_ITEM"):
                continue
            total += 1
            m = CGU_RE.search(line)
            if m:
                counts[m.group(1)] += 1
            else:
                counts["<unparseable-cgu (build-script or crate-root cgu)>"] += 1

    with open(out_path, "w") as f:
        f.write(f"total_mono_items\t{total}\n")
        for crate, n in counts.most_common():
            f.write(f"{crate}\t{n}\n")

    print(f"wrote {out_path}: {total} mono-items across {len(counts)} buckets", file=sys.stderr)
    for crate, n in counts.most_common(40):
        print(f"{n}\t{crate}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

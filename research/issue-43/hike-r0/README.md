# Issue #43 R0 — pinned Hike browser path

This directory preserves the source, commands, intermediate LLVM IR, linked
Wasm, generated JavaScript bridge, structural inspection, and Node execution
evidence for Hike revision
`6402155652fa61692818fe7985193f0fd97513f5`. The selected upstream workload is
`examples/browser`: it exercises strings, four DOM-facing imports, recursive
Fibonacci, and exported browser functions.

## Result

The pinned revision's unmodified one-command path does **not** link this
workload. Its wasm32 runtime replacement emits calls to `strlen32` but omits
the definition. [`upstream-build.stderr.txt`](upstream-build.stderr.txt)
preserves the normalized Clang failure, and
[`main.generated.ll`](main.generated.ll) is the unmodified compiler output.

The generated browser artifacts also disagree with each other: `index.html`
constructs `HikeRuntime`, while generated `runtime.js` exports only
`HikeConcurrentRuntime`. [`bridge-contract.txt`](bridge-contract.txt) records
that incompatibility. The generated bridge therefore cannot drive this fixed
workload as published.

To finish the Wasm reference path without changing the pinned Hike checkout,
[`main.compat.ll`](main.compat.ll) appends only the missing `strlen32`
definition from that same revision's native `runtime.ll`. The normal Hike
Wasm linker options then produce a valid module. This compatibility artifact
instantiates under Node, calls `InitApp`, returns `6912` for
`AddNumbers(1234, 5678)` and `55` for `Fib(10)`, performs the expected host
callbacks, and keeps linear memory at two pages. This functional check uses a
direct WebAssembly API harness and explicitly bypasses the incompatible
generated bridge. See [`execution.stdout.txt`](execution.stdout.txt).

| Artifact | Bytes |
| --- | ---: |
| `main.wasm` | 1,478 |
| `main.wasm.gz` (`gzip -9`) | 829 |
| `main.wasm.zst` (`zstd -19`) | 839 |
| generated `runtime.js` | 11,159 |
| Wasm + generated bridge | 12,637 |

The Wasm has four imports, 21 exports, ten code bodies, and one data segment.
Its largest payload sections are Code (505 bytes), Data (451 bytes), and
Export (336 bytes). Exact signatures, symbol sizes, section offsets, hashes,
commands, package versions, and the pinned container base digest are in
[`report.json`](report.json), [`wasm-objdump.txt`](wasm-objdump.txt), and
[`wasm-objdump.headers.txt`](wasm-objdump.headers.txt).

## Difference from the reported 2.56 KB

The reproduced 1,478-byte compatibility Wasm is not a byte-for-byte
reproduction of the article's 2.56 KB artifact. The evidence identifies the
producers rather than treating the values as directly comparable:

- the fixed revision's `examples/browser/main.hike` is not byte-identical to
  the article excerpt;
- the article does not publish the exact Go, Clang, and LLD versions or its
  binary, so its toolchain contribution cannot be isolated;
- the fixed revision has the `strlen32` wasm32 runtime regression described
  above, requiring the explicitly separated compatibility IR;
- the fixed revision generates an 11,159-byte unified worker/browser bridge,
  whereas the article shows an earlier, smaller `HikeRuntime` bridge; the
  generated bridge and checked-in `index.html` also name different runtime
  classes.

Consequently, `2.56 KB` remains the article's workload/toolchain result; this
R0 result establishes the auditable state of the issue-pinned revision and
explains why its observable producers differ.

## Reproduce on Windows

From this worktree's repository root:

```powershell
wslc build --progress plain -f docker/hike-r0.Dockerfile -t laminaria-hike-r0 .
wslc run --rm --pull never --volume "${PWD}:/work" --entrypoint python3 laminaria-hike-r0 scripts/hike_r0_reproduce.py --hike-source /work/.reference/hike-lang --output /work/research/issue-43/hike-r0
```

The Hike checkout is created and verified through
`scripts/reference_projects.py setup hike-lang`; its URL and revision have a
single source of truth in `reference-projects.lock.json`.

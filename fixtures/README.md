# Fixtures

Committed, versioned reference workloads for the LAMINARIA measurement spine
(issue [#11](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/11)'s
"Core workloads"), and what issue
[#18](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/18)'s
last acceptance-criteria line ("a fresh environment can reproduce the ...
tool subsets required by the initial benchmark fixtures") is checked
against.

Each fixture is a small but real, independently buildable workspace — not
part of LAMINARIA's own `Cargo.toml` workspace, so it can be built with
whatever toolchain a scenario selects rather than whichever one builds
LAMINARIA itself.

## Committed so far

- `rust-heavy-workspace/` — a 3-crate Cargo workspace (`fixture-core` →
  `fixture-mid` → `fixture-bin`) exercising a real dependency chain.
- `nim-heavy-workspace/` — a 3-module Nim project (`primes` → `geometry` →
  `fixture`) computing the same thing, for a rough cross-language sanity
  comparison.
- `rust-nim-c-abi-baseline/` — the "conventional C ABI baseline" workload:
  `nim-lib/nimlib.nim` compiles to a static library (`nim c --app:staticlib
  --noMain`) exposing only scalar `cint`/`clong` functions, and
  `rust-bin/build.rs` compiles and links it into a Rust binary that calls
  it via `extern "C"`. No Nim `seq`/`string`/GC type ever crosses the FFI
  boundary — deliberately, since that constraint is exactly what future
  direct native-link research (issue #4, "Rust–Nim native linking without
  a mandatory C ABI boundary") exists to relax, and this fixture is the
  reference point that research will be compared against. Computes the
  same prime-grid/cluster values as the two fixtures above, so all three
  produce identical output for cross-checking.

All three are built (not just version-checked) as part of CI — natively on
`ubuntu-latest`/`macos-latest`, and inside `docker/bootstrap.Dockerfile`'s
container environment — which is the actual evidence for #18's fixture
reproduction criterion.

## Still open (per #11's full "Core workloads" list)

Mixed Rust/Nim native executable (Rust and Nim linked into one binary
without the fixture-per-language split above), direct native-link workload,
backend-route variant workload, backend checkpoint-economics workload, LLVM
pass/pipeline observation workload, ThinLTO/DTLTO dynamic backend-job
workload, wide parallel graph, deep critical-path graph, boundary-heavy
workload, incremental semantic edit, unchanged/no-op workspace, worktree
reuse, mixed-language WASM workload, Wasm link/post-link/component
invalidation workload, Rust/Nim 2/Nimony shared LLVM/LTO compatibility
workload, compiler/backend-work-elimination fixture. Most of these need the
Run/scenario machinery from #19-21 to be meaningful, not just a buildable
workspace.

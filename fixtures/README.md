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
- `wide-parallel-graph/` — the "wide parallel graph" workload: 8 mutually
  independent leaf crates (`leaf-fibonacci`, `leaf-factorial`, `leaf-gcd`,
  `leaf-sum-of-squares`, `leaf-palindrome`, `leaf-bubble-sort`,
  `leaf-binary-search`, `leaf-matrix-sum`, each with real logic and unit
  tests) with no dependencies on each other, all depended on by one
  `aggregator` binary. Deliberately the opposite topology from
  `rust-heavy-workspace`'s linear chain — a correct build scheduler can
  compile all 8 leaves in parallel before linking `aggregator`, which is
  exactly what future scheduler research (#6) needs a fixture to exercise.
- `deep-critical-path-graph/` — the "deep critical-path graph" workload: 12
  crates (`stage-01` .. `stage-12`) in a strict linear dependency chain,
  each depending only on the one before it, plus a `fixture-bin` that pulls
  the whole chain. Each stage applies one distinct, real, unit-tested
  transform (avalanche mix, bit reversal, Collatz step-count fold, modular
  exponentiation, digital-root fold, etc.) to a running `u64` value; the
  final chained value is asserted in `fixture-bin` against a committed
  reference constant. Deliberately the opposite topology from
  `wide-parallel-graph`'s fan-in and deeper than `rust-heavy-workspace`'s
  3-crate chain — a scheduler cannot shorten this critical path with
  parallelism, which is exactly what future critical-path/queue-wait
  measurement (#19, #21) needs a fixture to exercise.

- `mixed-rust-nim-executable/` — the "mixed Rust/Nim native executable"
  workload: unlike `rust-nim-c-abi-baseline` (deliberately scalar-only),
  both languages contribute real algorithmic work over a shared array
  buffer crossing the boundary by pointer + length. Rust generates
  deterministic data and computes its own checksum, Nim reads the same
  buffer for statistics (`nim_array_stats`) and then mutates it in place
  (`nim_array_scale_evens`), and Rust re-checksums the mutated buffer —
  every stage's result is asserted against a committed reference constant.
  Exercises the everyday array-marshaling FFI pattern that #4's baseline
  deliberately avoids.

All six are built (not just version-checked) as part of CI — natively on
`ubuntu-latest`/`macos-latest`, and inside `docker/bootstrap.Dockerfile`'s
container environment — which is the actual evidence for #18's fixture
reproduction criterion.

## Still open (per #11's full "Core workloads" list)

Direct native-link workload, backend-route variant workload, backend
checkpoint-economics workload, LLVM pass/pipeline observation workload,
ThinLTO/DTLTO dynamic backend-job workload, boundary-heavy workload,
incremental semantic edit, unchanged/no-op workspace, worktree reuse,
mixed-language WASM workload, Wasm link/post-link/component invalidation
workload, Rust/Nim 2/Nimony shared LLVM/LTO compatibility workload,
compiler/backend-work-elimination fixture. Most of these need the
Run/scenario machinery from #19-21 to be meaningful, not just a buildable
workspace.

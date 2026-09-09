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

See [`STATE-CONTRACTS.md`](STATE-CONTRACTS.md) for this issue's "cold/
warm/no-op have explicit reproducible state contracts" acceptance
criterion — precise, verified definitions of those three states for
every fixture below, including two non-obvious pitfalls found while
verifying them (a content-identical file copy is not a cache no-op;
Nim's default cache lives outside the repo entirely).

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

- `boundary-heavy-workload/` — the "boundary-heavy workload" workload:
  crosses the Rust/Nim FFI boundary once per loop iteration for
  1,000,000 iterations, with a deliberately trivial per-call payload
  (two `u32`s in, one `u32` out via `nim_fold_step`) and trivial per-call
  computation, so the *count* of boundary crossings dominates cost rather
  than data volume or per-call work — unlike `mixed-rust-nim-executable`'s
  handful of calls over a whole array. Runs the identical FNV-1a-style
  fold natively in pure Rust over the same input sequence as a same-logic
  comparison point; both final accumulators are asserted equal to each
  other and to a committed reference constant.

- `unchanged-noop-workspace/` — the "unchanged/no-op workspace" workload:
  deliberately the smallest, simplest fixture here — a single crate, one
  real unit-tested checksum, no cross-crate or cross-language structure.
  Its point isn't the computation; it's the *scenario* built on top once
  a Run harness exists (#19, #21): build once, then rebuild with the
  source completely unchanged, and characterize the metadata/hash/I/O/
  process-launch overhead a correct build system still pays on a full
  no-op, without that cost being confounded by graph shape the way it
  would be on any of the other fixtures. `cargo build` immediately after
  `cargo test`/`cargo run` with no source changes already demonstrates
  the no-op (`Finished ... in 0.00s`, nothing recompiled).

- `incremental-semantic-edit/` — the "incremental semantic edit" workload:
  three independent leaf crates (`leaf-a`, `leaf-b`, `leaf-c`) plus an
  `aggregator` that sums them, with one *designated* single-line semantic
  edit to `leaf-b` (`scripts/apply-edit.sh` / `scripts/revert-edit.sh`
  swap its `src/lib.rs` between the committed `lib.baseline.rs` and
  `lib.edited.rs` variants) and a documented expected invalidation set —
  `leaf-b` and `aggregator` should recompile, `leaf-a`/`leaf-c` should
  not. See `EDIT.md` for the full scenario and expected values; this is
  the shape a future Run/scenario harness (#19-21) needs to check this
  issue's "unexpected extra actions" acceptance criterion.

- `direct-native-link/` — the "direct native-link workload" workload: the
  minimal Layer 1 proof from `docs/rust-nim-native-linking.md` ("one
  Rust-produced object and one Nim-produced object in the same link, with
  an intentionally simple symbol relationship and no generated C header
  contract"). Reverses every other Rust/Nim fixture's direction — Nim is
  the final linked binary (`nim-bin/main.nim`) and links directly against
  a Rust static library (`rust-lib`) via a hand-named `importc`/
  `#[no_mangle] extern "C"` symbol, no header generator involved. See
  `NOTES.md` for `nm` symbol-inspection evidence (undefined in Nim's own
  object, defined in Rust's, resolved in the final binary) — the fixture
  future direct native-link research (#4) builds its deeper Layers 2-6 on
  top of, not that research itself.

- `rust-nim-llvm-lto-compatibility/` — the "Rust/Nim 2/Nimony shared
  LLVM/LTO compatibility workload": the minimal first proof from
  `docs/research-program.md` Track J. Not "can the linker resolve
  symbols across two native objects" (every other fixture here) but
  "can Rust's own LLVM IR and Nim's own LLVM IR (via `nlvm`) be merged
  into *one module* with `llvm-link`, before either side reaches native
  codegen." Reachable now because `direct-native-link/`'s `nlvm`
  investigation (issue #4) established `nlvm`'s pinned LLVM version is
  exactly `22.1.8`, matching this project's own pinned `rustc`'s
  bundled LLVM precisely. See `NOTES.md` for method and status.

All eleven are built (not just version-checked) as part of CI — natively
on `ubuntu-latest`/`macos-latest`, and inside `docker/bootstrap.Dockerfile`'s
container environment — which is the actual evidence for #18's fixture
reproduction criterion.

## Still open (per #11's full "Core workloads" list)

Backend-route variant workload, backend checkpoint-economics workload,
LLVM pass/pipeline observation workload, ThinLTO/DTLTO dynamic backend-job
workload, worktree reuse, mixed-language WASM workload, Wasm
link/post-link/component invalidation workload, compiler/backend-work-
elimination fixture. Every one of these needs backend/Run infrastructure
from #4, #12, or #19-21 to be more than a placeholder, not just a
buildable workspace.

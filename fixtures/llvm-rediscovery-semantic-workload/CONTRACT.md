# Semantic contract: `add`

## Evidence classification correction (2026-09-10)

This file preserves historical fixture/measurement and implementation evidence, not the current research delivery order. Existing-compiler builds and driver self-builds recorded below are **reference/bootstrap/delegated-build baselines**, not proof of LAMINARIA compiler ownership or independent self-hosting. The [compiler ownership contract](../../docs/compiler-ownership-contract.md) governs current issue acceptance; historical checklists do not close the revised requirements.


Issue #25's first experiment, per `docs/llvm-rediscovery-research.md`'s
rediscovery method ("start from semantic workloads... do not begin
research from pre-existing LLVM IR"). Separate from `fixtures/
rust-nim-llvm-lto-compatibility/` (#17's fixture, which starts from
already-emitted `.ll` files) — this fixture starts from source on both
sides and traces forward.

## The contract, stated precisely

> Given two 32-bit two's-complement integers `a` and `b`, compute their
> sum modulo 2^32 (wrapping on overflow), with no other observable
> effect (no panic, no trap, no I/O).

Deliberately the narrowest possible arithmetic contract — plain
wrapping addition, not Rust's default checked-in-debug/wrapping-in-
release `+` operator — so this first experiment isolates
**compiler/target default policy** differences (attributes, codegen
choices applied uniformly regardless of what the function's own source
asked for) from **language-semantics** differences (what happens on
overflow, which Rust's checked `+` and Nim's own arithmetic would
answer differently and is deliberately out of scope for *this*
experiment — a natural second workload the charter's own "integer
arithmetic and overflow" category names explicitly).

## What is deliberately NOT specified by this contract

- Calling convention beyond "C-compatible, two `i32`/`cint` arguments,
  one `i32`/`cint` return" — whatever each compiler's default C ABI
  lowering produces for this signature on the target.
- Any exploit-mitigation, sanitizer, or stack-safety posture. This is
  exactly the axis the first traced difference (see `NOTES.md`) turned
  out to live on — not specified by the contract at all, applied by
  each compiler's own target-level default policy independently of
  anything this function's source says.

## Rust side

`rust-src/add.rs`: `#[no_mangle] pub extern "C" fn add(a: i32, b: i32)
-> i32 { a.wrapping_add(b) }` — `wrapping_add` makes the no-panic,
modulo-2^32 contract explicit in the source itself, not dependent on
`-C overflow-checks`/debug-vs-release defaults.

## Nim side

`nim-src/add.nim`: `proc add(a, b: int32): int32 {.exportc, cdecl.} =
a +% b` — Nim's `+%` operator is its own explicit wrapping-addition
primitive (`system/arithmetics.nim`: `proc \`+%\`*(x, y: int32): int32`,
"treats x and y as unsigned and adds them... implements modulo
arithmetic, no overflow errors are possible" — read from Nim's real
source, not assumed), as opposed to plain `+`, whose overflow behavior
depends on `--overflowChecks`. `int32` rather than `cint` for the
parameter/return types: both lower to the same 32-bit C-compatible
integer, but `+%` is defined directly on `int32` in Nim's own source,
and using it avoids an extra implicit-conversion question this
experiment isn't about.

Compiled via `nlvm` (LLVM IR emission, no C anywhere — see `fixtures/
direct-native-link/NOTES.md` for why this route was established as
genuinely C-free), not `nim c` — because the destination representation
this experiment traces toward is LLVM IR, and `nim c`'s C-generation
route would insert a second compiler (the system C compiler) into the
chain between Nim's own semantic decisions and the LLVM IR that
compiler produces from Nim-generated C, muddying which compiler made
which decision.

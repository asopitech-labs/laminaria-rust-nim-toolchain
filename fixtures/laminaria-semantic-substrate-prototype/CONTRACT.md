# `laminaria-semantic-substrate-prototype` — semantic contract

Issue #25's own instruction (`docs/llvm-rediscovery-research.md`) requires
prototyping "a candidate LAMINARIA semantic/optimization substrate" and
demonstrating a backend-route projection -- not just observing more
Rust/Nim IR differences. This fixture is that first prototype attempt,
built alongside a concrete transformation experiment (per issue #25's
follow-up instruction: build the observation infrastructure *together
with* real disassembly/reassembly work, not as a standalone checklist
item).

## Workload

Two functions, given two 32-bit two's-complement integers `a`, `b` and a
boolean-as-i32 `use_double`:

- `double(x) = x +% x` (wrapping addition of `x` to itself)
- `add_or_double(a, b, use_double) = if use_double != 0 { double(a) }
  else { a +% b }`

Deliberately small: one function call, one branch, one wrapping
arithmetic op -- just enough structure to need a *representation* (not
just a value), while staying hand-verifiable.

## What this fixture builds

1. **A tiny candidate representation** (`substrate/src/repr.rs`): an
   `Expr`/`Stmt` AST for exactly this workload's grammar (parameter
   reference, wrapping-add, not-equal-zero, function call, if, return),
   plus an explicit per-function fact set (`FnFact`: parameter/return bit
   widths, and a `has_side_effects` flag) -- the minimal semantic
   information issue #25 asks a candidate substrate to carry.
2. **A reference evaluator** (`substrate/src/eval.rs`): interprets the
   representation directly, given concrete integer inputs.
3. **A backend-route projection** (`substrate/src/llvm_ir.rs`): emits
   literal LLVM IR text from the *same* representation -- not from Rust or
   Nim source, and not by reading rustc's/nlvm's own emitted IR. Compiled
   via `llc` (LLVM 22, matching the rest of this repo's pinned version)
   and linked into a real native executable.
4. **One transformation, tested both ways** (`substrate/src/inline.rs`):
   inlining `double(a)` into `a +% a` at the call site. Tested against a
   version of `double` with `has_side_effects: false` (the real one --
   inlining must be semantically transparent) and a version with
   `has_side_effects: true` (a hypothetical `double` that also has an
   observable effect the tracked fact records -- inlining must be refused
   *because the fact says so*, not because of any heuristic).

## What faithfulness to Rust/Nim actually means here

`add_or_double`'s real Rust (`rust-src/add_or_double.rs`) and Nim
(`nim-src/add_or_double.nim`) implementations, and the representation's
own reference evaluator and LLVM-IR projection, are cross-checked against
each other by running all of them (as four separate compiled programs)
against the *same* fixed set of test inputs and diffing their printed
output byte-for-byte. This is a black-box behavioral cross-check (four
programs, one shared "print `a,b,use_double,result`" convention), not a
claim that the representation captures everything about Rust's or Nim's
real compiled functions -- see `NOTES.md` for exactly what is and isn't
established by four programs agreeing on four test inputs.

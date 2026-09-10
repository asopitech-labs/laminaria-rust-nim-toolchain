# `laminaria-semantic-substrate-prototype` — issue #25's first candidate-substrate experiment

## Evidence classification correction (2026-09-10)

This file preserves historical fixture/measurement and implementation evidence, not the current research delivery order. Existing-compiler builds and driver self-builds recorded below are **reference/bootstrap/delegated-build baselines**, not proof of LAMINARIA compiler ownership or independent self-hosting. This fixture contains hand-authored IR and a limited evaluator/transform; its LLVM projection is a comparison experiment, not a Rust/Nim source frontend or independent target generator. The [compiler ownership contract](../../docs/compiler-ownership-contract.md) governs current issue acceptance; historical checklists do not close the revised requirements.

**Real, source-derived successor (2026-09-10): `crates/laminaria-ir`.** This
fixture's own `repr.rs` names its central limitation directly ("transcribed
by hand ... not derived mechanically" from `rust-src/add_or_double.rs`/
`nim-src/add_or_double.nim`). `crates/laminaria-ir` parses that *same*
workload for real, through hand-written frontends for a declared Rust/Nim
subset (`rust_frontend.rs` uses the `syn` crate for syntax-only parsing per
`docs/compiler-ownership-contract.md`'s lexical-parsing-reuse boundary;
`nim_frontend.rs` is grounded directly in the real Nim compiler's own
`lexer.nim`/`parser.nim`), carries real source provenance on every IR node,
and replaces this fixture's bare `has_side_effects: bool` fact with an
actual executable effect trace. It also adds the explicit `let`-binding IR
node (`Expr::Let`) this fixture's own `inline.rs` names as missing, and
implements two compared candidate transformations (`checked_inline`,
generalizing this fixture's own approach, and `anf_insert`, a new
structural alternative) against the same duplication/dropped/reordering
hazards this fixture's `inline.rs` tests cover. This fixture's own code and
history are left exactly as they were -- nothing here was rewritten or
deleted.


**Status: not complete, not claimed complete.** This is a first prototype
of "a candidate LAMINARIA semantic/optimization substrate" and a
backend-route projection, per issue #25's own instruction -- covering
exactly one small workload (a function call plus a branch, added to the
existing wrapping-addition workload from
`fixtures/llvm-rediscovery-semantic-workload/`). None of issue #25's ten
acceptance criteria are checked off by this fixture alone.

## What was actually built and verified, not assumed

Four independent programs, all producing the workload's
`add_or_double(a, b, use_double)` result for the same four test inputs,
diffed byte-for-byte via `trace.sh`:

1. The real Rust binary (`rust-src/add_or_double.rs`, `rustc -O`).
2. The real Nim binary (`nim-src/add_or_double.nim`, `nim c -d:release`).
3. `substrate`'s reference evaluator (`substrate/src/eval.rs`), directly
   interpreting the candidate representation (`substrate/src/repr.rs`).
4. `substrate`'s LLVM-IR backend projection (`substrate/src/llvm_ir.rs`),
   emitted from the *same* representation (never from rustc's/nlvm's own
   IR), compiled with `llc` (LLVM 22.1.8, this repo's already-pinned
   version) and `cc`, then run as a real native executable.

All four agree, byte-for-byte, on all four test inputs:

```text
3,4,0,7
3,4,1,6
2147483647,1,0,-2147483648
-5,10,1,-10
```

This is real evidence the representation is faithful to the workload's
documented semantic contract (`CONTRACT.md`) *for these four inputs and
this one workload* -- not a general claim the representation is correct
for arbitrary programs. See "What this does not establish" below.

## The allowed-vs-rejected transformation, issue #25's explicit ask

`substrate/src/inline.rs` implements one transformation -- inlining a call
to `double` at its call site inside `add_or_double` -- gated on exactly
one tracked fact, `FnFact::has_side_effects`:

- **Allowed**: `double`'s real fact has `has_side_effects: false`.
  Inlining is permitted, and the transformed representation was actually
  re-evaluated against the reference evaluator across the same test
  inputs (plus one extra, `[7, -7, 1]`) to confirm the transformation is
  behavior-preserving -- not merely assumed correct because the
  substitution was mechanical.
- **Rejected**: a hypothetical `double` variant
  (`repr::double_with_side_effect_fact`) with `has_side_effects: true`.
  Inlining is refused *because the tracked fact says so*, verified by a
  test asserting the refusal actually happens (`inline::tests::
  rejected_case_inlining_is_refused_when_the_side_effect_fact_is_set`),
  not just documented as an intention.

This is the same *shape* of decision `fixtures/
llvm-rediscovery-semantic-workload/NOTES.md` found LLVM's real inliner
making on `rust_add` (refusing inlining over an attribute-compatibility
fact, categorically, before any cost heuristic) -- reproduced here inside
this project's own representation, on this project's own facts, not
observed inside LLVM.

## What this does not establish

Against issue #25's own acceptance criteria: this is one workload
(criterion 2, extended from `llvm-rediscovery-semantic-workload`'s
wrapping-addition-only workload to include a call and a branch, but still
only one workload), one candidate representation prototyped (criterion 5,
first instance, not validated against a second, differently-shaped
workload), one backend-route projection demonstrated (criterion 6, LLVM
IR only -- no second backend route attempted), and zero LLVM concepts
independently re-derived or rejected from scratch (criterion 3 needs
five; this fixture's representation reuses SSA-like temporaries and
basic-block control flow directly, closer to "adopting an LLVM-shaped
idea" than "re-deriving or rejecting one" -- an honest limitation, not
glossed over).

## Real, named limitations of the representation and this experiment

- **Hand-transcribed, not mechanically extracted.** `repr::
  workload_program()` was written by hand to match `rust-src/
  add_or_double.rs`/`nim-src/add_or_double.nim`'s documented contract --
  nothing checks the representation actually matches either source's real
  compiler output (MIR, LLVM IR) structurally. The four-way output
  cross-check is behavioral (same printed results), not structural.
- **`has_side_effects` is a bare boolean**, not a description of what the
  effect is, where it's observable, or whether it commutes with anything
  else. Real effect systems (regions, capabilities, points-to) are a
  large design space this prototype does not enter -- it only shows that
  *even a single boolean fact*, tracked and checked explicitly, already
  changes a transformation's correctness, which is the minimal point
  issue #25 asks this prototype to make.
- **The inliner only handles a single-`Return`-bodied callee.** `double`'s
  real body happens to be exactly that shape; a callee with its own
  branch would need a real inlining strategy (parameter substitution
  through control flow, not just through an expression tree) this
  experiment doesn't attempt.
- **The LLVM-IR emitter has no target triple/datalayout of its own** --
  it relies on `llc`'s host-default inference, verified locally
  (`aarch64-apple-darwin`) and matching CI's runners
  (`x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`). A cross-compiled
  target would need this made explicit; not attempted here.
- **`TEST_INPUTS` is duplicated by hand** across `rust-src/
  add_or_double.rs`, `nim-src/add_or_double.nim`, and `substrate/src/
  main.rs` -- nothing mechanically enforces the three lists stay in sync,
  a real, small maintenance risk, named rather than hidden.

## Correction: the inliner could duplicate a side-effecting argument

An external review caught a real bug in `inline.rs`'s transformation:
`substitute_params` copies the actual argument expression verbatim into
*every* occurrence of the corresponding parameter in the callee's body --
but `inline_call` only ever checked the *callee's own* `has_side_effects`
fact, never whether the argument expression itself contained a call that
inlining would then duplicate. Since `double`'s own body (`x +% x`)
references its one parameter twice, `double(effect(x))` (with a
hypothetical `effect` function registered `has_side_effects=true`) was
permitted to inline, silently invoking `effect` twice -- exactly the
class of correctness bug a real inliner has to avoid, and exactly what
this fixture's own "allowed vs. rejected" demonstration was supposed to
be about, just missing this second dimension of the hazard.

Fixed: `inline_call` now also refuses whenever a multiply-referenced
parameter's actual argument expression contains any `Call` -- fails
closed ("cannot prove this argument is safe to duplicate"), not "assume
it's pure unless a fact says otherwise." Verified both that the
duplicating case is now refused, and that a singly-referenced parameter
receiving a call argument is still permitted (nothing to duplicate there),
so the fix is scoped to the actual hazard, not an overbroad "never inline
a call argument" rule. See `inline.rs`'s own doc comment on `inline_call`
for the full reasoning.

## Second correction: the inliner could also silently drop an unused argument's evaluation

A further external review caught a second, related bug in the same
occurrence-count check: it refused inlining when a parameter was
referenced *more than once* (the duplication hazard above), but permitted
an occurrence count of *zero* -- an unused parameter. Since
`substitute_params` only ever substitutes at the occurrence sites that
actually exist in the callee's body, a call argument corresponding to an
unreferenced parameter is dropped from the result entirely, never
evaluated. Reproduced directly: for `pick(x, y) = x` (a function that
ignores its second parameter), inlining `pick(x, effect(x))` (with
`effect` registered `has_side_effects=true`) transformed to just `x`, and
the call to `effect` disappeared from the result -- exactly as much an
observable-behavior change as duplicating it, just in the opposite
direction.

Fixed: the guard changed from `occurrences > 1` to `occurrences != 1`,
covering both hazards under one check, with distinct error wording for
each (`inline.rs`'s own doc comment on `inline_call` has the full
reasoning). Verified with a dedicated test reproducing the exact
`pick`/`effect` scenario, alongside the existing tests confirming both the
duplication case and the singly-referenced-parameter case (which has
nothing to duplicate or drop) behave as before.

This still does not cover every hazard in this class: an argument
evaluated exactly once, but whose position relative to another argument's
own effects changes (evaluation *order*, not count), is invisible to an
occurrence-count check. Naive substitution can still reorder side effects
relative to the caller's original left-to-right argument evaluation.
Fixing that would need a single-evaluation, order-preserving binding form
(a `let`-like construct) this small experiment does not implement --
named as an explicit, still-open limitation rather than attempted here.

## Relation to `fixtures/llvm-rediscovery-semantic-workload`

That fixture traces one difference (`"probe-stack"`) all the way to
rustc's real target-policy source, and candidate-lists what LAMINARIA
should preserve -- but never attempts a representation or a
transformation of its own. This fixture is the next, complementary step:
same workload family (extended with a call and a branch), but building
forward from "what would LAMINARIA's own substrate need to track, and
what would it let LAMINARIA safely do with that" rather than backward
from "what does LLVM already do and why."

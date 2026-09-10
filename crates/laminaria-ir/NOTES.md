# `laminaria-ir` — Task 1 (issues #25 + #3, joint with #6/#8's contract shape)

Implements a source-derived owned IR for a declared Rust/Nim subset (fixed-
width integers, explicit wrapping arithmetic, function calls, conditionals,
local bindings), replacing `fixtures/laminaria-semantic-substrate-prototype`'s
hand-transcribed representation for the same `add_or_double`/`double`
workload, per `docs/compiler-ownership-contract.md`'s corrected research
order (`docs/issue-plan.md`: "#25 + #3 + #6 + #8 together"). See
`crates/laminaria-ir/src/lib.rs`'s own top-level doc comment for the full
scope statement and the Task 2 seam this is built around.

## What's real here, not assumed

- **Parsed from actual source, not hand-transcribed.** Both
  `fixture_parity_tests` in `lib.rs` parse the *existing*
  `rust-src/add_or_double.rs`/`nim-src/add_or_double.nim` files through
  `rust_frontend`/`nim_frontend` and diff the resulting IR's interpreter
  output against a genuinely `rustc`/`nim`-compiled binary of those same
  files, byte for byte, for all four of the fixture's own `TEST_INPUTS`.
  This is the concrete answer to the fixture's own honest limitation
  ("transcribed by hand ... not derived mechanically").
- **`nim_frontend.rs` is grounded in the real Nim compiler**, not invented
  from memory of "how Nim generally looks" -- `.reference/Nim/compiler/
  lexer.nim`/`parser.nim` were read first (see that module's own doc
  comment for the specific mechanisms mirrored: no INDENT/DEDENT tokens,
  just a per-token indent column and a parser-side expected-indent value;
  operators as a generic character-run, not individually hard-coded;
  `funcName(x)` vs. `funcName (x)` as genuinely distinct grammar
  productions gated on leading whitespace).
- **`rust_frontend.rs` uses `syn` for syntax only** (tokenizing/parsing into
  a `syn::File`/`syn::Expr` tree; zero semantic analysis, zero type
  checking, zero macro expansion) -- explicitly within
  `docs/compiler-ownership-contract.md`'s lexical/syntactic-parsing-reuse
  boundary (added to that doc as part of this task, after the user
  resolved an open question about exactly where that boundary sits). Every
  semantic fact is derived by this crate's own code walking that tree.
- **Real source provenance on every IR node** (`types::Provenance`: file +
  line/column span + source language) -- the hand-transcribed fixture had
  none at all.
- **An actual effect model, not a declared boolean.** The fixture's
  `has_side_effects: bool` is replaced by `interpreter::EvalOutcome`'s
  effect trace: the real, ordered sequence of function calls executed
  during evaluation (`interpreter::CallEvent`). A function call is this
  subset's only source of an effect an outside observer could see
  (arithmetic/conditionals/bindings are pure by construction), so this is
  the concrete, checkable notion of "effect" the task asked for, not a
  hypothetical fact nothing in the representation could actually verify.

## The two-candidate comparison (the actual research question)

Issue #25's own third comment on GitHub names the concrete open gap this
addresses directly: "a real single-evaluation order-preserving `let`-like
binding form remains unimplemented." Rather than assuming a `let`-based
(ANF-style) answer is correct upfront, both a checker-based and a
structural candidate were implemented against the same IR and run through
the identical four-case battery (`transform::tests`): duplication
(`double(x) = x +% x`, called with an effectful argument), dropping
(`pick(x, y) = x`, `y` never referenced), reordering (`reverse(x, y) = y +%
x`, callee body references parameters out of order), and a positive
control (`combine(x, y) = x +% y`, same order, no hazard).

**Result: `checked_inline` (candidate B, generalizing the fixture's own
`inline.rs` approach) correctly rejects all three hazards and accepts the
control. `anf_insert` (candidate A) correctly accepts *all four* cases**,
including the three `checked_inline` must refuse -- confirmed by actually
running the transformed programs through `interpreter::eval_function` and
comparing both the return value and the full effect trace against the
untransformed baseline, not merely by argument.

### Why, concretely

`checked_inline` substitutes a callee's parameter references verbatim with
the caller's argument expressions. Verbatim substitution can only preserve
an argument's evaluation semantics when the callee references that
parameter *exactly once*, in the same relative order the caller evaluated
it. Any other shape genuinely breaks something (duplicates re-evaluate the
argument's effect twice; a dropped reference never evaluates it at all;
reordering the argument expressions relative to sibling arguments really
does reorder their observable effects) -- so the check is not overly
conservative, it is correct. Its actual limitation is coverage, not
correctness: it must reject transformations that are perfectly safe to
perform *differently*.

`anf_insert` sidesteps the whole hazard class by never substituting an
argument expression into the callee body at all. Instead, every argument
is bound to a fresh local via an explicit `Let`, in the caller's own
left-to-right order, evaluated exactly once at the position the original
call occupied -- then the callee body's parameter references become
references to those locals. Because the *binding* (which runs the
argument's real evaluation, including any effects) is separated from the
*use* (which only ever reads an already-computed value), how many times or
in what order the callee body's arithmetic happens to reference a
parameter can no longer affect how many times, or in what order, the
argument's own effects actually run. Duplication, dropping, and reordering
all become structurally unrepresentable rather than merely detected.

A genuinely interesting confirmed detail:
`dropped_argument_hazard_checked_inline_rejects_anf_insert_accepts`'s test
asserts `anf_insert`'s transformed program still evaluates the *unused*
argument's effect exactly once, matching what a real, uninlined function
call would always do (every argument is evaluated, whether or not the
callee's body happens to reference it) -- verbatim substitution cannot
preserve this at all for an unreferenced parameter, since the parameter
reference the argument would have been substituted into simply doesn't
exist in the callee body's tree. `anf_insert` isn't only safer here; it is
the only one of the two that is *correct* with respect to real call
semantics for a statically-unused-but-effectful argument.

### What this means for choosing between them going forward

Neither is a strictly worse choice in every dimension:

- `anf_insert` expands the safely-transformable range and, per the point
  above, is strictly more correct for the unused-argument case -- but it
  restructures the callee body's shape at every inlined call site (nested
  `Let`s instead of a flat substituted expression), which is more surface
  area for a later optimization pass (or issue #6/#8's own scheduling) to
  reason about, and a `Program` with `anf_insert`'s output has more nodes
  than the original.
- `checked_inline` keeps the substituted expression's shape minimal
  wherever it's actually safe to do so, at the cost of simply refusing a
  real class of transformations `anf_insert` can perform.

Given issue #25's explicit ask to widen "the range that can be safely
transformed" rather than only add more rejection conditions, and given the
concrete correctness gap above (the unused-argument case), **`anf_insert`'s
structural approach is the one this project adopts going forward** as the
IR's primary single-evaluation discipline for effectful arguments --
`checked_inline` is kept, not deleted, as the generalized fixture-parity
baseline and as a live comparison point for later work, not as dead code.

This is not adopted as "ANF is the final answer" in the SSA-vs-ANF sense
`docs/llvm-rediscovery-research.md` warns against assuming -- only as "for
*this* specific hazard (single-evaluation/order preservation of call
arguments across a substitution), the structural binding-insertion
candidate dominates the post-hoc-checking candidate on the evidence
actually gathered here." A later task may find a different representation
dominates for a different transformation.

## Scope not attempted here (explicitly, not silently)

- **Task 2's own scope** (issues #6/#8): wiring this crate's
  `parse_and_lower`/`transform` functions into `laminaria-plan`'s
  `ActionKind`/the Nim Planning Kernel as real, scheduled compiler work.
  This crate does not import or depend on `laminaria-plan`,
  `laminaria-run`, `nim-planner`, or `laminaria-cli` at all.
- **Target code generation** (Task 3): `interpreter.rs` is verification
  evidence for the IR/transformation research, not a code generator; no
  LLVM IR, no native codegen is attempted by this crate.
- **A richer callee body shape for `transform`**: both candidates currently
  only substitute a callee whose body is a single `Return(expr)` (matching
  the existing fixture's own `double`-shaped example exactly) --
  `TransformError::UnsupportedCalleeShape` names this explicitly rather
  than silently mishandling a richer body. Extending either candidate to a
  callee with its own internal `Let`/`If` is a direct, bounded follow-up.
- **Additional integer widths, more wrapping/comparison operators, `elif`,
  the same-line `if x: y else: z` Nim form, command-call syntax**: named
  as deliberate restrictions in each frontend's own doc comment, not
  silently assumed unnecessary. Each is a mechanical grammar extension
  under the same "diagnostic, never a fallback" contract already
  established, not a design change.
- **Self-application to LAMINARIA's own real Rust/Nim source** (the
  eventual #2 self-hosting goal): this task's declared subset was chosen
  to match one small, already-studied workload, not LAMINARIA's own
  codebase.

## Verification

`cargo test -p laminaria-ir`: 24 tests, all real (no mocked frontend/
interpreter/transform behavior) -- frontend positive/negative-construct
coverage for both languages, three interpreter effect-trace tests, the
four-case transform comparison battery, and the two real-toolchain parity
tests described above. `cargo clippy --workspace --all-targets -- -D
warnings` and `cargo fmt --check` clean.

## Second pass: four real bugs found against real input, not just design

A review of the first pass's actual behavior (not just its design) found
and reproduced four concrete bugs before this crate's own 24 tests could
be trusted as sufficient evidence:

1. **P1: `anf_insert`'s fresh `LocalId` counter started at 0
   unconditionally, colliding with a pre-existing local already bound in
   the caller's body.** Because the interpreter's `Let`-scoping restores
   "whatever was bound before" purely by numeric id, a colliding fresh
   binding can transiently overwrite a same-numbered outer variable's slot
   while it's in scope -- and if a *later* hoisted argument in the same
   call site needs to read that outer variable's real value (not the
   fresh binding's), it silently reads the wrong one instead. Reproduced
   directly: with `caller() { let a = 1; let b = 2; combine(b, a) }`
   (`a`/`b` = `LocalId(0)`/`LocalId(1)` from real frontend numbering) and
   `combine`'s own two fresh locals also picked as `0`/`1`, the second
   hoisted argument's value expression (meant to read outer `a`) instead
   read the just-rebound slot holding `b`'s value, computing `4` instead
   of the correct `3`. Fixed by seeding `next_local` from
   `types::max_local_id_in_stmt(caller_body) + 1` (a new helper walking
   both `Let` binding sites and `Local` use sites for the maximum id in
   scope) instead of `0` -- re-scanned fresh on every `anf_insert` call, so
   a second, sequential call also can't collide with the *first* call's
   own introduced locals (confirmed by a dedicated repeated-transformation
   test).
2. **P1: Nim's `+%`/`-%`/`*%` were parsed in one same-precedence,
   left-to-right loop**, so `a +% b *% c` lowered as `(a +% b) *% c`
   instead of real Nim's actual semantics, `a +% (b *% c)` -- confirmed
   directly against `lexer.nim`'s own `getPrecedence` (`MulPred = 9` for
   `*%`, strictly above `PlusPred = 8` for `+%`/`-%`). Fixed with the
   standard two-level precedence-climbing split (`parse_expr` for `+%`/
   `-%`, calling `parse_term` for the tighter-binding `*%`) -- the
   grounding research this frontend was already built on named the actual
   precedence values; the first pass simply didn't apply them. While
   fixing this, `parse_condition`'s two sides were also upgraded from
   `parse_atom_expr` to the full `parse_expr` (a condition like `a +% b !=
   0` needs a real expression on each side, matching how
   `rust_frontend::lower_condition` already lowers its own inner
   expression generally, not just a single atom).
3. **P1: an unsupported/malformed Nim body could be read only part way
   through and still reported as a whole-file success.** `parse_body_block`
   returned as soon as it successfully parsed a tail form (an `if` or a
   plain expression), without checking whether *another* statement
   followed at the same indentation -- so two same-indent tail-shaped
   lines in a row silently lowered using only the first line, with the
   second left unconsumed and then quietly skipped by
   `lower_nim_source`'s top-level "not a `proc` declaration" loop (which
   only recognizes `proc` at column 1 and otherwise just advances token by
   token). `rust_frontend::lower_block` already rejects the equivalent
   shape explicitly (`if tail.is_some() { return Err(...) }`); this pass
   adds the matching check for the indentation-based grammar
   (`p.same_indent()` checked immediately after the tail is parsed,
   before restoring the caller's own indent level).
4. **Separately, CI-only: the `windows` job failed on `cargo clippy`**
   (`unused_imports`/`dead_code` under `-D warnings`) because only the two
   real-toolchain parity test *functions* in `lib.rs` were
   `#[cfg(unix)]`-gated, while their shared `use` imports and helpers
   (`repo_root`, `TEST_INPUTS`) were not -- so on a platform where neither
   test compiles, those items become genuinely unused. Fixed by gating
   the whole `fixture_parity_tests` module with `#[cfg(all(test, unix))]`
   instead of gating each function individually, matching how other
   real-toolchain test modules in this workspace are structured.

Six new regression tests (two in `transform/mod.rs`, three in
`nim_frontend.rs`, none needed for the CI-only fix). Each of the three P1
fixes was confirmed to actually matter by temporarily reverting it and
re-running its new test before restoring the fix -- all three failed with
the exact wrong value/behavior predicted, not a hypothetical concern.

`cargo test --workspace`: 216 passed (up from 211). Clippy and fmt clean.

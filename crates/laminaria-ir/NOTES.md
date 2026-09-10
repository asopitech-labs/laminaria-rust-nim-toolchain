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

## Third pass: composition safety and stricter acceptance, 8 more bugs

A review of the second pass's own fixes -- using additional inputs the
existing 29 tests didn't cover -- found four confirmed-broken behaviors
plus four further residual gaps in the same round, all fixed here:

1. **P1: composing an already-transformed callee could numerically
   collide with its own embedded locals.** Both transforms only
   considered the *caller's* pre-existing `LocalId`s when picking safe new
   ids (or, for `checked_inline`, considered none at all, since it
   introduces no fresh ids of its own) -- neither accounted for a callee
   body that *itself* already contains embedded `Let`s from an earlier
   inlining pass. Reproduced directly: `add(x,y)=x+y`, `g(x)=add(1,x)`,
   `f()=g(10)`; inlining `add` into `g` gives `g` two internal `Let`s
   (ids 0, 1, its own fresh numbering); inlining `g` into `f` then picked
   a fresh id *also* starting at 0 (since `f` itself has no pre-existing
   locals), colliding with `g`'s own embedded id 0 and computing `2`
   instead of the correct `11`. Fixed with a real alpha-rename pass
   (`transform::alpha_rename_callee_body`, used by both candidates via
   `prepare_callee_body_for_grafting`): every `Let` the callee body itself
   already contains is renamed to a fresh id, starting above *both* the
   caller's own maximum id and the callee body's own maximum id, before
   either candidate does anything else with it -- provably disjoint from
   anything already meaningful at the graft site, regardless of how many
   prior transformations either side has been through. Confirmed to
   matter by reverting to the narrower (caller-only) fix and re-running
   the new regression test: it reproduced the exact same `2` instead of
   `11`.
2. **P1: a callee's own internal effects were recorded with a
   per-call-relative `order_index`, not a globally monotonic one.**
   `eval_function` creates a fresh `EvalState` per call, whose effect
   trace starts counting from 0 for that call alone; splicing it into the
   caller's own trace without renumbering produced duplicate/out-of-order
   indices (`[0, 1, 0, 1]` in the constructed regression test, matching
   the review's own `[0, 0, 2, 0]`-shaped report) whenever the callee's
   own body made more than one call and the caller already had earlier
   effects recorded -- a shape the existing
   `nested_effects_are_recorded_in_real_execution_order` test never
   exercised (there, the nested call is an *argument*, evaluated through
   the same shared state, never through a fresh one). Fixed by
   renumbering a callee's spliced-in effects to continue from the
   caller's own current trace length. Confirmed via revert.
3. **P1: Nim's new same-indent trailing-statement check missed same-line
   and deeper-indent leftovers.** The check added in the second pass only
   compared against `Some(cur_indent)` exactly, so (a) unsupported
   trailing syntax on the *same source line* as the tail (`x div 2`,
   where `div` isn't a supported operator: `parse_expr` on `x` silently
   returns just `x`, leaving ` div 2` unconsumed as continuation tokens,
   `indent: None`, which never equals `Some(cur_indent)`) and (b) content
   left at a *deeper* indent (`Some(c) if c > cur_indent`) both slipped
   through undetected. Replaced with a genuine block-boundary check: the
   only tokens that legitimately follow a block's tail are EOF or a
   token starting a new line at strictly *less* indentation (a
   sibling/enclosing construct correctly dedenting out); anything else
   (a continuation token, or a token at this block's own indent or
   deeper) is unconsumed content.
4. **P2 (Rust): `async`, `unsafe`, `extern`, generic parameters, and
   non-doc attributes on a requested function were silently ignored.** A
   generic function whose parameter/return types happen to be `i32`
   (`fn identity<T>(x: i32) -> i32 { x }`) previously slipped through
   entirely -- the *existing* generic-rejection test only ever caught
   `identity<T>(x: T)` by accident, via the unrelated "parameter type
   other than i32" check on `T`, never a genuine generics check. Fixed
   with explicit checks on `sig.asyncness`/`unsafety`/`abi`/
   `generics.params`/`generics.where_clause`, and rejecting any function
   attribute that isn't `#[doc = ...]` (what a `///` comment desugars to,
   allowed through since it has no semantic effect).
5. **P2 (Rust): a bare `!=`/`==` was accepted as a general, `i32`-valued
   expression.** `fn f(x: i32) -> i32 { x != 0 }` lowered successfully,
   even though real Rust rejects it outright (`x != 0` has type `bool`,
   not `i32`, and cannot be returned from an `i32`-declared function).
   The general expression-lowering arm that constructed `NotEqZero` from
   a bare comparison has been removed entirely; `NotEqZero` is now only
   ever reachable through `lower_condition`, called from `lower_if`'s
   condition position -- the one place this subset actually has a
   boolean-shaped value to consume.
6. **P2 (Rust): calling a name shadowed by a local silently resolved to
   the differently-scoped function instead of being rejected.** Real Rust
   would reject `let double = a; double(a)` outright (calling a plain
   `i32` local is a type error; this subset has no function-valued locals
   at all), but the call-lowering code only ever checked
   `declared_functions`, never whether the callee name is *also* bound as
   a local/parameter in scope. Fixed by checking `ctx.locals`/`ctx.params`
   before `declared_functions` and rejecting the shadowed case explicitly.
7. **P2 (Rust): the unary-negation path skipped the integer-suffix check,
   and separately rejected the valid `i32::MIN` literal.**
   `syn::LitInt::base10_parse::<i32>()` only inspects the digits, never
   the suffix, so `-1u64` (an unsigned 64-bit literal, a different type
   entirely) silently became `IntLit(-1, I32)` through the negation arm,
   while the *non-negated* literal path already correctly checked the
   suffix. Separately, `-2147483648` (`i32::MIN`) was rejected outright:
   Rust's surface syntax has no single "negative literal" token -- `-N`
   is unary negation of the separately-tokenized *positive* digits
   `2147483648`, which do not themselves fit `i32` (`i32::MAX` is
   `2147483647`), so parsing the pre-negation digits directly as `i32`
   failed before negation could bring the value back into range. Both
   literal-parsing call sites now go through one `parse_i32_literal`
   helper: parse the digits as `i64` (wide enough for any value the
   literal could name), negate in `i64` if applicable, *then*
   range-check the final value against `i32::MIN..=i32::MAX` -- correctly
   accepting `-2147483648` while still rejecting `-1u64` and a genuinely
   out-of-range value.
8. **P2 (Nim): an `'i32`-suffixed literal outside `i32`'s actual range was
   accepted as-is.** `lex_number` parses the digit sequence into `i64`
   (wide enough to hold values far beyond `i32`), but nothing checked the
   *result* actually fits `i32` even when the suffix explicitly claims it
   does. Fixed with an explicit range check once the suffix is confirmed
   to be `i32`.

Fourteen new regression tests (two in `transform/mod.rs`, one in
`interpreter.rs`, two in `nim_frontend.rs`'s trailing-content coverage,
one more Nim range test plus its `i32::MIN`-acceptance mirror, and eight
in `rust_frontend.rs` covering attributes/generics/async/unsafe, the
bool-vs-i32 type check, name-shadowing, and both integer-literal fixes).
The three most structurally involved fixes (composition/alpha-rename,
effect-order renumbering, Nim trailing-content) were each confirmed to
actually matter by reverting the fix and re-running its new test before
restoring it -- all three reproduced the exact wrong value/behavior the
review reported, not a hypothetical concern.

`cargo test --workspace`: 231 passed (up from 216). Clippy and fmt clean.

## Known residual scope, not yet addressed

Explicitly out of scope for this round, named rather than silently
dropped -- a genuinely exhaustive Rust/Nim subset frontend (full
name-resolution scoping beyond simple shadowing-in-call-position, a real
type system beyond "everything is i32," complete input-consumption
verification for every grammar production rather than the specific gaps
this round closed) is not this task's goal; the task is a narrow, honest
slice proving the source-derived-IR/effect-model/transform-comparison
pipeline end to end, not a production-grade compiler frontend. Further
gaps of the same general character (type/shape/consumption checks this
declared subset doesn't yet enforce) should be expected and fixed
incrementally as they're found, the same way this round's were, rather
than treated as a one-time completeness gate before proceeding to Task 2
(#6/#8 planner/scheduler integration).

## Fourth pass: issue #27's semantic-gate (A), grounded in real reference code

[Issue #27](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/27)
reframes Task 2 as a joint milestone that does not wait for #3/#25's
research to fully close, and names its own "A" gate: fix the three
concrete defects its reference-code table identifies, add finite
regression/composition tests, and stop there (no new language features).
Its reference table names exact revisions/functions this round actually
read before deciding what to fix -- see the issue body itself for the
full list; summarized here only where a fix follows directly from one:

1. **Per-graft-site alpha-rename, instead of one rename shared across every
   call site of the same callee.** `checked_inline`/`anf_insert` used to
   call `prepare_callee_body_for_grafting` *once*, before scanning for call
   sites, producing a single alpha-renamed callee body reused verbatim at
   every match `rewrite_calls_in_stmt` finds. When the same callee is
   called more than once inside one caller, every one of those call sites
   grafts into the *same* caller scope -- reusing identical `LocalId`s for
   each graft's own embedded `Let`s is exactly the "a scope's own bindings
   must not leak into a sibling scope opened afterward" hazard issue #27
   names via Nim's own `openScope`/`rawCloseScope`. Fixed with
   `alpha_rename_for_one_graft`, called fresh (a brand-new, empty `remap`)
   inside the per-call-site closure itself, threading one monotonically
   advancing `next_local` counter across every site an operation performs.
   Honestly: this round's own fuzz battery (below) did not find a
   currently-reachable wrong *value* from the single-shared-rename version
   for any construction attempted (`Expr::Let`'s value-before-bind, fully-
   unwind-before-return evaluation order happens to protect every
   nesting/sibling shape tried) -- this is recorded as a structural
   invariant fix following the reference discipline directly, not a
   confirmed-by-reverting wrong-value bug the way the next two are.
2. **A callee body's own attributes were checked, but a `let` statement's
   or a function parameter's were not.** Verified directly against rustc's
   own `rustc_expand::config::StripUnconfigured::configure` and
   `rustc_expand::expand`'s `flat_map_stmt`/`flat_map_param` (issue #27's
   reference table): `cfg` acts on statements and parameters, not only
   items. `syn` happily parses `#[cfg(test)] let y = x;` and
   `fn f(#[cfg(test)] x: i32)` into `Local.attrs`/`PatType.attrs`, and
   `rust_frontend` never inspected either field -- confirmed by reverting
   the fix and re-running two new regression tests: both silently
   `Ok(Program{..})`'d instead of rejecting. Fixed with a shared
   `reject_unsupported_attrs` helper, now also called on every `Local` and
   `FnArg::Typed`, plus `ExprIf.attrs` for completeness (an `if` used at a
   block's tail can carry its own attribute too).
3. **`checked_inline` only ever compared arguments against each other,
   never against a call the callee body makes on its own.** A callee like
   `f(x) = mark(2) +% x` called as `caller() = f(mark(1))` must run
   `mark(1)` before `mark(2)` (every argument evaluates before the callee
   body runs at all), but verbatim substitution produces
   `mark(2) +% mark(1)`, evaluated left-to-right as `[2, 1]` --
   `count_param_occurrences`/`param_evaluation_order` never looked at any
   `Call` embedded directly in the callee body, only at `Param`
   references, so this reordering was silently accepted. Fixed with
   `ObservedEvent`/`observed_event_order` (unifying `Param` references and
   the callee's own internal calls into one evaluation-order sequence) and
   a new `TransformError::WouldReorderRelativeToCalleeCall`, checked
   alongside the pre-existing argument-to-argument checks. Confirmed by
   reverting and re-running: the dedicated regression test failed with
   observed order `[2, 1]` against baseline `[1, 2]`, exactly as predicted.

**A new generative battery**
(`transform::composition_fuzz`) directly answers issue #27's A4 checklist
rather than only adding hand-picked cases: a deterministic, seeded
(splitmix64, no external `rand` dependency) generator builds 120 random
`mark`/`helper`/`g`/`f`/`entry` programs -- covering 0-2 prior compositions,
nested and sibling multi-call-sites of the same callee, unused/duplicated/
reordered parameter references (occurring naturally from uniformly random
parameter-index selection, not hand-picked per case), and edge/random
`i32` values -- and asserts both candidates either match the baseline
interpreter's value *and* observed `mark(..)` sequence exactly, or (for
`checked_inline` only) conservatively refuse; a coverage self-check fails
the test outright if a run never exercises both `checked_inline`'s
acceptance and rejection paths, or never produces any effect to compare at
all. **This fuzzer independently rediscovered defect 3 above at a randomly
generated seed** when run against the pre-fix code (confirmed directly:
reverting only the transform files and re-running failed at seed 2 with
observed marks `[0, 1, 0, 0]` against expected `[1, 0, 0, 0]`) -- real
corroborating evidence, not merely a plausible-sounding battery. A second,
smaller fuzz test (`source_level_shadowing_fuzz_preserves_value`, 40
seeds) generalizes the existing hand-written shadowing regression test
through the real `nim_frontend` across a random number of pre-existing
caller locals.

`cargo test --workspace`: 237 passed (up from 231; +6 in `laminaria-ir`
proper: 4 new hand-written regression tests plus these 2 fuzz batteries).
Clippy and fmt clean workspace-wide.

**Issue #27's A is not fully closed by this round alone**: A1's full
subset acceptance table (mapping every accept/reject/ignore branch across
file/function/argument/let/expression positions) and the source/
transformed/legal-subset real-compiler comparison-recording split are
still open, tracked directly in issue #27 rather than restated here.

## Fifth pass: issue #27 A2 -- a validated-Program API boundary

A2 asked for the "unvalidated `Program`" vs. "checked input" distinction
this crate's own API previously lacked entirely: `ValidateIr` was added as
an `ActionKind` in `laminaria-plan` (issue #27 B) with nothing on the
`laminaria-ir` side to actually back it. New `validate.rs`:

- `ValidatedProgram`, a newtype with no public constructor other than
  `validate_program` -- a caller cannot manufacture one without the
  checks actually having run.
- `validate_program` checks every function: every `Local` reference
  resolves to a binding in scope at that exact position (tracked via the
  same insert-before-body/remove-after-body discipline
  `interpreter::eval_stmt`/`eval_expr` themselves use for `Let`, so this
  validates against the scoping rule the interpreter actually relies on,
  not an invented stricter/looser one); source-level shadowing (two
  distinct `Let`s at different numeric `LocalId`s) is unaffected, checked
  directly against the real `nim_frontend`. Every `Call` names a declared
  function with a matching argument count. Every `IntLit` fits its own
  declared `IntWidth`. Every `NotEqZero` (this subset's only condition
  form) appears only directly as an `Stmt::If`'s own `cond` -- a
  condition nested inside an arithmetic expression is rejected as "used
  as a value," this subset having no general boolean type.
- Wired as a genuine **postcondition**, not merely an optional checker a
  caller could skip: `rust_frontend::lower_rust_source`/
  `nim_frontend::lower_nim_source` now run it before ever returning `Ok`
  ("lowering後"), and `transform::apply_inlined_caller` -- the one shared
  choke point both `anf_insert` and `checked_inline` funnel their final
  `Program` through -- runs it before returning `Ok` ("変換後"),
  converting a violation into `LoweringError::PostconditionViolated`/
  `TransformError::PostconditionViolated` respectively. Every existing
  frontend/transform test still passes unchanged, confirming this is a
  live regression guard, not a currently-known gap being closed by
  fiat -- none of this crate's own correctly-implemented logic was ever
  expected to trip it.
- Deliberately does *not* change `interpreter::eval_function`'s own
  signature to require a `ValidatedProgram` -- that function is this
  crate's test harness for semantic-preservation across hundreds of
  existing call sites, a different concern (does a transform preserve
  behavior?) than the one this module closes (must an executor be able
  to silently run unvalidated IR? -- no such executor exists in this
  crate yet; issue #27 stage C is where `ValidatedProgram` will actually
  gate execution). Also does not introduce a new IR representation or
  SSA form, per A2's own explicit "新IR体系やSSAの導入は条件にしない."

13 new tests (well-formed programs from both real frontends and both
transforms validating; unbound-local, unknown-callee, arity-mismatch,
param-out-of-range, value-out-of-range, and condition-used-as-value each
rejected; a correct condition-in-`If`-position and source-level shadowing
both still validating). `cargo test -p laminaria-ir`: 64 passed (was 51).
Workspace total: 272 (was 259). Clippy/fmt clean.

## Sixth pass: A's remaining named residuals -- acceptance table, shared
## observation contract, independent-IR/parity-test separation

Closes the three items still open from A ("Aの既存残件を閉じる"): a
subset acceptance table with test correspondence, a shared observation
contract instead of a per-test filter, and verified independent-IR/
external-compiler-parity test separation. No new language features (per
the review's own "言語対応範囲を広げる指摘ではありません").

- **New `SUBSET.md`**: every accept/reject/ignore branch in
  `rust_frontend.rs`/`nim_frontend.rs`, built by reading both files in
  full (not from memory), mapped to the implementing function/match-arm
  and the test that exercises it -- across file, function-declaration,
  parameter, let-binding, statement/block-shape, `if`/`else`,
  expression, and (Nim-only) unknown-token levels. Rows with no test are
  named explicitly as gaps, not glossed over; several genuine Rust/Nim
  frontend asymmetries (the `==`-swap condition form being Nim-
  unsupported, the call-shadowing check being Rust-only, `elif`/same-line
  `if`/command-call-syntax being deliberate Nim scope exclusions) are
  named as asymmetries, not silently presented as parallel coverage.
- Closed most of the table's own named gaps with 31 new regression tests
  (25 in `rust_frontend.rs`, 6 in `nim_frontend.rs`, including genuinely
  new positive-acceptance tests for the literal-first zero-comparison
  form and an `else if` chain) -- each targets a distinct code path the
  table identified as previously untested, not a restatement of existing
  coverage. 9 rows remain intentionally open (documented in `SUBSET.md`'s
  own "What this table is not" section): several are structurally
  unreachable from stable surface syntax, several share an
  already-exercised code path, and a couple are genuinely open follow-up
  work, named as such rather than silently left off the table.
- **Shared observation contract**: `interpreter::observed_calls`, a new
  public function with its own doc comment declaring the actual contract
  (filter by function name, compare by evaluated argument value in
  occurrence order, `order_index` deliberately excluded since it's
  splicing bookkeeping, not part of "the same observed behavior"; not a
  claim to model general I/O/effects). `transform::tests::effects_of` and
  `composition_fuzz::observed_marks` -- previously two independent
  copies of the identical filter -- now both delegate to it, closing the
  exact duplication a review named directly.
- **Verified, not merely argued, independent-IR/parity-test separation**:
  confirmed `fixture_parity_tests` is the *only* module in this crate
  invoking an external `rustc`/`nim` process (grepped), then empirically
  built this crate's test binary normally and ran the *already-compiled*
  binary with `rustc`/`nim` invisible on `PATH`
  (`PATH=/usr/bin:/bin ... --skip fixture_parity_tests`) -- every
  remaining test passed. Documented directly in `lib.rs`'s own comment on
  that module, not left as an implicit structural fact.

`cargo test -p laminaria-ir`: 96 passed (was 65). Workspace total: 304
(was 272). Clippy/fmt clean.

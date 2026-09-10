# laminaria-plan notes

## Issue #27 B: the compiler-work handoff contract (first slice)

[Issue #27](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/27)'s
B section asks for a versioned contract for `LowerSource -> ValidateIR ->
TransformFunction -> ValidateIR` (and an `EvaluateEvidence` step) to be
designed and schema-tested *now*, in parallel with A's fixes, without
waiting for #3/#25's research to close. This adds the first real slice:

- Four new `ActionKind` variants (`LowerSource`, `ValidateIr`,
  `TransformFunction`, `EvaluateEvidence`), added *alongside* the existing
  delegated-build kinds (`NimBuild`/`CargoBuild`/`Integrate`), never
  replacing them -- the existing self-build/project-build executors keep
  their own role, unaware of the new kinds beyond a `match` arm that
  refuses to execute them (`laminaria-run::self_build`/`project_build`:
  "compiler-work action kinds are not executable via this legacy
  delegated-build executor").
- `compiler_work.rs`: `CompilerWorkDescriptor` carrying every field issue
  #27 names as required -- a descriptor schema version, the operation's
  own implementation version (versioned independently of the descriptor
  shape), semantic input artifact ids, requested-function/transform
  parameters, source provenance, a resource request, and a budget/
  cancellation token reference. Attached to `Action` as an `Option`,
  omitted entirely (not `null`) when absent, so every existing
  delegated-build action's JSON is byte-for-byte unchanged.
- **Artifact identity, the issue's own explicit requirement**: never a
  path, memory address, or whole-plan digest. `compute_artifact_id`
  (a hand-rolled FNV-1a, not `std::collections::hash_map::DefaultHasher`
  -- that hasher's algorithm is explicitly not guaranteed stable across
  Rust versions, and this workspace avoids a new dependency for it,
  matching `planning_kernel.computePlanId`'s own "structural, not a
  cache/security-grade content address" framing on the Nim side) derives
  an id from the operation name, its own implementation version, and its
  full semantic parameter list. Four "change tests" per issue #27's own
  requirement: same inputs -> same id; changing any one semantic
  parameter (source snapshot, language, requested-function set, subset
  version, or the operation's own version) -> a different id; two
  operations with textually-identical parameters do not collide with each
  other; and two different part-boundaries that would concatenate to the
  same string do not collide (the `write_str` null-separator).
- `nim-planner/src/contract.nim` mirrors the wire shape by hand (as it
  already does for every other type in this contract) -- `PlanSchemaVersion`
  bumped `0.1.0 -> 0.2.0`. The Nim kernel never constructs a
  `CompilerWorkDescriptor` or inspects its contents beyond
  encode/decode -- `planning_kernel.plan`'s dependency derivation is
  already purely `inputs`/`outputs`-based and needed **zero** changes for
  the new `ActionKind`s, confirmed by not touching that file at all.
- **A genuine cross-language round-trip test**
  (`nim_planner_client::tests::a_compiler_work_descriptor_round_trips_through_the_real_planner_binary`)
  sends a `TransformFunction` action with a populated descriptor through
  the *real* `laminaria-planner` binary (not a hand-simulated JSON
  string) and asserts the descriptor comes back byte-for-byte identical,
  and that a compiler-work action's dependency on a delegated action
  (here, a `LowerSource` action producing the artifact `TransformFunction`
  consumes) still orders correctly through the same purely-artifact-based
  dependency derivation every other `ActionKind` uses.

`cargo test --workspace`: 246 passed (was 237). `nim-planner`'s own
`nim c -r tests/test_planning_kernel.nim`: all suites green, including
three new tests for the descriptor's own encode/decode and an unknown-
`ActionKind` rejection. Clippy/fmt clean workspace-wide.

## Second pass: 2 real bugs + 1 gap a review found, all confirmed and closed

A review, grounded in Buck2's own `build_action_no_redirect`
(`action.inputs()` is exactly what gets staged/waited-on via
`ensure_artifact_group_staged` before an action runs at all), found this
first slice declared the right *fields* and computed artifact ids, but
validated neither against the other:

1. **Bug: `validate::validate`'s `ActionShapeMismatch` check compared
   `kind`/`inputs`/`outputs` but never `compiler_work`.** A plan echoing
   back a *mutated* descriptor (a different `caller`/`callee`, say) for an
   otherwise-unchanged action id passed silently. Confirmed with a
   dedicated test before fixing (added `compiler_work` to the comparison).
2. **Bug: nothing recomputed or checked `Action.id` against the artifact
   id its own descriptor's content implies.** This crate's own doc
   comments claimed "the id is derived, not chosen," but nothing enforced
   it -- an arbitrary hand-picked id, or a descriptor mutated *after* its
   id was computed, passed through untouched. Closing this honestly also
   surfaced a real gap the artifact-id functions themselves had:
   `LowerSource` needs `language`/`subset_version` and `EvaluateEvidence`
   needs `test_inputs_digest`/`observation_contract_version` to recompute
   their own id, but the descriptor never stored them -- added
   `language`, `contract_version` (one field, meaning depends on
   operation), and `test_inputs_digest` to `CompilerWorkDescriptor` so
   every operation's id is fully reconstructable from the descriptor
   alone. New `recompute_work_id` + `validate_compiler_work_action`
   (`compiler_work.rs`) close this, called from `validate::validate` for
   every action in a plan.
   - **A genuine bug was found and fixed *while adding these fields*:**
     `nim-planner/src/contract.nim`'s `CompilerWorkDescriptor` object type
     and `toJson`/`compilerWorkDescriptorFromJson` were never updated for
     the three new fields, so `language`/`contract_version`/
     `test_inputs_digest` silently round-tripped to `none` through the
     real Nim binary regardless of what was sent -- caught directly by
     the strengthened `nim_planner_client` integration test (which now
     also calls `validate::validate` on the round-tripped plan, not only
     comparing the descriptor by equality), not by inspection. Fixed on
     both the type definition and the encode/decode functions, plus a
     dedicated Nim-side round-trip test for exactly these three fields.
3. **Gap: `semantic_input_artifact_ids` was declared but never checked
   against `Action.inputs`.** Nothing stopped a work item from naming a
   semantic input its own declared dependencies never cover -- the exact
   "what's actually read must be covered by what's waited-on-for-
   readiness" invariant Buck2's own input-preparation step enforces
   structurally. `validate_compiler_work_action` now rejects any
   `semantic_input_artifact_ids` entry absent from `Action.inputs`'s own
   `Declared` set.

Twelve new tests added across `compiler_work.rs` (presence-per-kind in
both directions, missing-required-field, schema-version mismatch,
descriptor-mutated-after-id-computed, arbitrary-hand-picked-id,
undeclared-semantic-input) and `validate.rs` (the `ActionShapeMismatch`
fix, a well-formed compiler-work action validating end-to-end, and
`ValidationError::CompilerWork` propagation), plus one Nim-side test for
the three newly-wired fields. `cargo test --workspace`: 259 passed (was
246). Clippy/fmt clean; Nim suite green.

### What this slice deliberately does not do

- **No planner/executor wiring.** No code anywhere constructs a
  `LowerSource`/`ValidateIr`/`TransformFunction`/`EvaluateEvidence`
  `Action` as part of an actual plan request; this is the schema and its
  identity scheme only, exercised through hand-built `PlanningInput`
  values in tests. Wiring `laminaria-ir::rust_frontend`/`nim_frontend`/
  `transform` into real dispatch, and building the in-process executor
  itself, is issue #27's C, gated on A also being complete.
- **No resource accounting, admission control, or cancellation
  mechanism.** `ResourceRequest`/`budget_token` are wire fields only --
  nothing anywhere reads or enforces them yet. Issue #27's own C
  acceptance criteria (CPU-budget equivalence, real concurrent execution,
  structured `resource_exhausted`, cancellation) own that work.
- **No IR payload storage.** `semantic_input_artifact_ids`/`work_id`s are
  bare strings on the wire; the actual `laminaria_ir::types::Program`
  values they'd eventually name are not stored, looked up, or even
  constructed by anything in this slice -- issue #27 B's own text places
  that in "Rust側のin-memory store," not yet built.
- **The four new `ActionKind`s' own field defaults
  (`requested_functions: []`, `transform: None`,
  `source_provenance: None`) are not semantically validated against which
  operation actually needs which field** (e.g. nothing rejects a
  `TransformFunction` action whose `transform` field is absent) -- that
  validation belongs with the executor these fields will actually drive,
  not the wire contract alone.
- Per issue #27's own "未確定事項": the exact enum names/hash encoding
  here are this round's first fixed choice, not claimed final -- a future
  contract version may still change them, covered by the change tests
  above rather than left as an unstated assumption.

## Third pass: a review's P1 finding -- identity propagation across a producer/consumer edge

Grounded in Buck2's own artifact-identity model (`BuildArtifact`: an
artifact's identity *is* its producing action's key,
`app/buck2_artifact/src/artifact/build_artifact.rs`) -- one more real gap
the second pass's own fixes left open, confirmed by reproducing it
directly against `valid_transform_action()`'s own test fixture (its
`outputs` named an unrelated literal string, `"out-1"`, with zero
relationship to the action's own verified `id`):

**`validate_compiler_work_action` verified `action.id` was internally
self-consistent with its own descriptor, but never checked that
`action.id` is actually *published* anywhere `action.outputs`
declares.** Without this, a producer's own semantic (content) change --
which does change its recomputed `action.id` -- would never propagate
into what its output is actually *called* on the wire: a downstream
consumer's stale reference to the producer's *old* identity could still
silently resolve to this same action's current (but now semantically
different) output, since nothing ties "the id I claim" to "the id I
actually publish."

Fixed with a new `CompilerWorkContractError::OutputIdentityNotPublished`,
checked right after the existing `WorkIdMismatch` check: `action.outputs`
must contain `ArtifactRef::Declared { artifact_id: action.id }` (an
*additional* output alongside it is fine -- the requirement is presence,
not exclusivity). This closes the loop with `validate::validate`'s
already-existing producer/consumer matching: once a producer's own
output id is *required* to be its real content-derived identity, a
consumer still referencing a *stale* id (from before the producer's
content changed) no longer resolves to any real producer in the plan at
all, and the existing `UnknownProducer` check catches it structurally,
without needing a bespoke cross-action check of its own.

Every existing compiler-work test fixture across `compiler_work.rs`,
`validate.rs`, and `nim_planner_client.rs` had this same
non-self-referential `outputs` shape and needed the same one-line fix
(publish the action's own `id`, not an unrelated literal) -- fixed
consistently, plus 2 new dedicated tests (the rejection, and the positive
control confirming an *additional* output alongside the published
identity still validates).

`cargo test -p laminaria-plan`: 40 passed (was 38). Workspace total: 311
(was 304, combined with the `laminaria-ir` fixes in the same round).
Clippy/fmt clean; real-planner-binary round-trip test still green.

## Fourth pass: `test_inputs` -- a real gap issue #27 stage C's first executor surfaced

Building `laminaria-run::compiler_work_executor` (issue #27 stage C's
first slice) found that `EvaluateEvidence`'s descriptor had no field
actually carrying the finite test-input *values* to run -- only
`test_inputs_digest`, an identity for *which* inputs, useful for a
stable artifact id but useless for actually dispatching the evaluation.
Added `test_inputs: Vec<Vec<i64>>`, deliberately kept separate from and
*not* hashed into the artifact id (matching `resource_request`/
`budget_token`'s own carry-along-but-not-identity role) -- the digest
still owns identity, the new field owns what an executor actually runs.
Mirrored in `nim-planner/src/contract.nim` by hand from the start this
time (not forgotten the way the second pass's three fields were), with
its own dedicated round-trip test.

## Fifth pass: a review found the evidence id and the additional-output check were both unsound

Two more real findings, confirmed directly:

1. **P1: `evaluate_evidence_artifact_id` never actually depended on the
   function name or the real test-input values.** It hashed a
   caller-*supplied* `test_inputs_digest` string -- nothing checked that
   string against `test_inputs`'s real content, or even considered which
   function was being evaluated at all. Two different functions
   evaluated against the same validated program (or the same function
   against genuinely different inputs) could claim the same digest and
   share an artifact id. Fixed by removing `test_inputs_digest` entirely
   and hashing `requested_functions`'s first entry (the function name)
   and a canonical stringification of `test_inputs` directly (see
   `canonical_test_inputs`) -- the id can now only ever be recomputed
   from, and can therefore only ever match, the actual evidence a work
   item produces. 4 new "変更テスト"-style tests (function name, input
   values, input row count, and row/value grouping each independently
   changing the id).
2. **P2: `validate_compiler_work_action` allowed any number of
   *additional* declared outputs beyond the required one.** The second
   pass's own positive-control test
   (`an_additional_output_alongside_the_published_identity_still_validates`)
   passed happily, but no dispatch arm in
   `laminaria_run::compiler_work_executor` (built in the very next
   round) ever produces more than the one artifact keyed by its own
   `action.id` -- so an additional output would validate successfully
   yet resolve to nothing at execution time, a gap only surfacing at
   runtime. Tightened to require *exactly* one output, matching
   `action.id` exactly; the old positive-control test is now a negative
   one (`an_additional_output_alongside_the_published_identity_is_rejected`).

`cargo test -p laminaria-plan`: 44 passed (was 40). Workspace total: 320
(was 315, combined with the `laminaria-run` fix in the same round).
`nim-planner/src/contract.nim`'s now-unused `testInputsDigest` field
removed to match (never had independent identity meaning once
`test_inputs` itself is hashed directly).

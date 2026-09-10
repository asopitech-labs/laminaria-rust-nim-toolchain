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

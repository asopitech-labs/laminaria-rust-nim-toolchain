# Self-build: stage0 → stage1 (issues #8, #6, #4 first slice)

This document describes the concrete, implemented protocol behind the
`plan(PlanningInput) -> ExecutionPlan` contract named in
`docs/research-foundations.md` section 7 and the Phase 3/Phase 7 roadmap
items ("Stabilize `PlanningInput` and `ExecutionPlan`"; "Use LAMINARIA to
build its own Rust host and Nim planning kernel"). Before this work,
neither existed as code — this is the first slice that makes them real,
shared jointly by issues #8 (the Nim Planning Kernel), #6 (the Rust
Action Graph/scheduler), and #4 (the Rust↔Nim integration boundary).

## Scope of this slice

- A real, non-fixture Nim package (`nim-planner/`) implementing
  `plan(PlanningInput) -> ExecutionPlan`.
- A real Rust crate (`crates/laminaria-plan`) that is the *only* way the
  Rust host ever obtains a plan — there is no Rust-computed substitute
  anywhere in this codebase.
- `laminaria self-build`, which uses that plan to actually build
  LAMINARIA's own Rust host and Nim planner from source, into a
  generation root.
- Local, sequential execution only. No caching/reuse (issues #7/#12), no
  distributed execution, and no stage1→stage2 self-rebuild — all
  explicitly deferred to later work.

## Grounded in real prior art, not invented

`docs/project-proposal.md` section 7/11 names four reference projects
for exactly this Action-Graph/planner problem: **Buck2**, **Bazel**,
**Pants**, and **Nx**. All four were shallow-cloned into `.reference/`
(see `.reference/README.md`) and their real source was read before any
of this was designed. Concretely:

- **Dependency edges are artifact-mediated, not a hand-declared
  `depends_on` list.** Buck2's `BuildArtifact` carries the `ActionKey` of
  the action that produced it
  (`app/buck2_artifact/src/artifact/build_artifact.rs` in
  `.reference/buck2`); an action's inputs are `ArtifactGroup`s
  (`app/buck2_build_api/src/artifact_groups.rs`) that resolve to either a
  source artifact (a leaf) or another action's output. This project's own
  `Action` (`crates/laminaria-plan/src/types.rs`,
  `nim-planner/src/contract.nim`) declares only `inputs`/`outputs` as
  `ArtifactRef`s; the Nim kernel derives the dependency graph itself by
  matching inputs against declared outputs.
- **Cycle detection is an explicit path-tracking DFS**, ported from
  Bazel's `SimpleCycleDetector`
  (`skyframe/SimpleCycleDetector.java` in `.reference/bazel`) and Nx's
  `findCycle`/`_findCycle`
  (`packages/nx/src/tasks-runner/task-graph-utils.ts` in `.reference/nx`).
  Both keep an explicit path list and report the *exact* cycle the
  moment a node already on that path is revisited. Buck2's Dice engine
  instead uses a Kosaraju-SCC-based cycle terminator
  (`InnerGraph::terminate_cycles`), but that exists only because Dice's
  graph is large, live, and mutating while nodes run concurrently — not
  a fit for a single, small, upfront-computed `ExecutionPlan`.
- **Deterministic ordering is Kahn's algorithm with a lexicographic
  tie-break**, ported directly from Nx's `walkTaskGraph`
  (same file as above): compute in-degree from the derived edges,
  repeatedly take the lexicographically-smallest-id zero-in-degree
  action, decrement dependents, repeat. This is what makes "same input →
  identical output" (issue #8's own acceptance criterion) a structural
  guarantee rather than an accident of hash-map iteration order — the
  same determinism concern Pants solves in its `rule_graph` crate by
  using `BTreeSet`/`BTreeMap`/`IndexSet` everywhere instead of raw hash
  collections (`src/rust/rule_graph/src/builder.rs`, `rules.rs:14` in
  `.reference/pants`).
- **No demand-driven/pull-based scheduling** — a deliberate divergence
  from Buck2 and Bazel's actual execution model. Buck2's action
  execution has no explicit scheduler at all: it is recursive async
  calls memoized by its Dice incremental-computation engine
  (`ActionCalculation::build_action` in
  `app/buck2_build_api/src/actions/calculation.rs`), and Bazel's
  Skyframe is the same shape (a `SkyFunction` returns `null` when a
  dependency isn't ready yet and is restarted once it is,
  `skyframe/SkyFunction.java`). Both are optimized for large, long-lived,
  incrementally-cached graphs. This slice has explicitly no caching and a
  fixed, tiny action set produced once by a single Nim subprocess call
  across a process boundary — a demand-driven pull model doesn't fit an
  IPC contract that must hand back one complete document. The full order
  is computed once (previous bullet) and executed sequentially instead.
- **Structured rejection reasons are a tagged enum, not formatted
  strings** — modeled on Buck2's project-wide `buck2_error` pattern
  (`#[derive(buck2_error::Error)]` + `#[buck2(tag = ...)]`, e.g.
  `ConfiguredGraphCycleError { cycle: Arc<Vec<...>> }` in
  `app/buck2_configured/src/cycle.rs`) and Pants's internal
  `NodePrunedReason`/`EdgePrunedReason` enums
  (`src/rust/rule_graph/src/builder.rs:194-207`) that name *why* a
  node/edge was rejected before being rendered to text.
- **`plan_id` is an honest, LAMINARIA-specific addition, not a copied
  idiom.** None of the four reference projects keep a single whole-graph
  digest as their primary node identity: Pants relies on structural
  `Eq`+`Hash`+interning with no `Digest`/`Fingerprint` type in
  `rule_graph` at all; Bazel's `Artifact` identity is path+owner-based,
  with content digest as a *separate* change-detection value; Buck2
  gives each *action* its own remote-execution `ActionDigest`, but there
  is no single digest for a whole `ActionGraph`; Nx's `Task.id` is a
  plain `project:target` string, and `Task.hash` is a separate per-task
  cache value. `plan_id` exists purely because issue #6 asks for a
  recorded "plan ID" for lineage/evidence in `laminaria-run`'s `Run`
  records — it is a structural (non-cryptographic) hash of the
  canonicalized `PlanningInput` (`nim-planner/src/
  planning_kernel.nim`'s `computePlanId`), sufficient for that evidence
  purpose, not a cache/security-grade content address.
- **Future critical-path work should reuse Buck2's real algorithm**
  rather than being invented later either:
  `app/buck2_critical_path/src/potential.rs`'s
  `compute_critical_path_potentials` (topological-order dynamic
  programming, `cost[v] = weight[v] + max(cost[dep])`, run once forward
  and once on the reversed graph) is the concrete reference for when
  critical-path analysis is actually implemented — out of scope for this
  slice.

## The contract

`PlanningInput -> ExecutionPlan`, versioned (`schema_version = "0.1.0"`
today). Rust owns the canonical type definitions
(`crates/laminaria-plan/src/types.rs`); Nim mirrors them by hand
(`nim-planner/src/contract.nim`) since there is no shared schema
generator — every JSON key is a literal snake_case string chosen to
match Rust's default serde output. The two sides are kept honest by tests
on both ends: `nimble test` under `nim-planner/`, and
`cargo test -p laminaria-plan`'s tests that deserialize a literal capture
of the real Nim binary's stdout (not a value this crate re-serialized
itself).

- `ArtifactRef`: `{"kind": "source", "path": "..."}` (an external,
  pre-existing input) or `{"kind": "declared", "artifact_id": "..."}` (an
  id some action in the same input must declare as an output).
- `Action`: `id`, `kind` (`nim_build` | `cargo_build` | `integrate` in
  this slice), `command_identity` (a logical description, not
  necessarily the literal argv executed), `inputs`, `outputs`.
- `ExecutionPlan` (on success): `schema_version`, `produced_by` (always
  `"laminaria-nim-planning-kernel"` on a real Nim-produced plan —
  `laminaria_plan::validate` checks this so a plan that didn't actually
  come from the real Nim binary can never be silently accepted),
  `producer_version`, `plan_id`, `ordered_actions`, `actions`.
- `PlanRejection` (a valid, complete answer — not a crash):
  `reason_kind` (`cycle` | `unsupported_input` | `missing_producer` |
  `duplicate_producer` | `invalid_contract_version`), `reason_detail`,
  `cycle_path` (populated only for `cycle`, rendered `a -> b -> c -> a`).
- The top-level answer from `laminaria-planner`'s stdout is
  `{"outcome": "planned"|"rejected", "data": ...}`.

## The Rust↔Nim boundary: subprocess + JSON, not FFI

Issue #4 permits "an explicit validated adapter" as a pinned bootstrap
route without settling the wider ABI-free research question. This slice
uses a subprocess boundary: Rust spawns the compiled `laminaria-planner`
Nim executable, writes `PlanningInput` JSON to its stdin, reads
`ExecutionPlan`/rejection JSON from its stdout
(`crates/laminaria-plan/src/nim_planner_client.rs`). This is deliberately
*not* claimed as "direct" or "ABI-free" — it sidesteps Nim's ARC/ORC
runtime-lifecycle and panic/exception-unwind-boundary questions that
native FFI linking would force, while matching issue #8's requirement
that "the planner performs no process launch, environment probing,
filesystem or network side effects" (the Nim binary only reads
stdin/writes stdout). `laminaria-planner` exits `0` for *both* a
successful plan and a well-formed rejection; only a genuinely
unparseable input exits nonzero
(`nim-planner/src/laminaria_planner.nim`).

## Layout

- `nim-planner/` — the Nim package. `src/contract.nim` (the mirrored
  contract), `src/planning_kernel.nim` (`plan`/`planFromJson`),
  `src/laminaria_planner.nim` (the `laminaria-planner` binary's entry
  point), `tests/test_planning_kernel.nim` (`nimble test`).
- `crates/laminaria-plan/` — `types.rs` (the contract's Rust source of
  truth), `nim_planner_client.rs` (the subprocess client — the *only*
  source of a `PlanOutcome` anywhere in this crate; a missing/failing
  planner binary is always `Err`, never a fallback plan),
  `validate.rs` (a lightweight structural re-check of the ordering
  property before any plan is trusted for execution — not a
  reimplementation of Nim's solver).
- `crates/laminaria-run/src/self_build.rs` — builds the self-build's
  `PlanningInput` (three actions: `compile-nim-planner`,
  `compile-rust-host`, `integrate`), calls the planner, validates, and
  executes `ordered_actions` strictly sequentially (concurrency bound 1,
  explicitly conservative — issue #6 permits this for the first local
  slice; nested Cargo/`nim c` parallelism is left at each tool's own
  default and named, not hidden, in each action's `Run` evidence).
  `nim_build`/`cargo_build` actions run through the same `run_and_record`
  RUSTC-wrapper/CC-wrapper tracer paths every other traced command in
  this crate already uses — even this slice's coarse ("whole `cargo
  build`") actions are backed by real per-compiler-invocation evidence,
  never a bare `Command::output()`. `integrate` is filesystem assembly,
  not a compiler invocation, so it produces no traced `Run` — wrapping it
  in a synthetic one would misrepresent it as compiler evidence it isn't.
  Each action's `Run` is patched with `plan_id`, the planner binary's own
  resolved digest, and a `generation` lineage label before being
  re-persisted (issue #6: "recording the invoked build-driver identity,
  planner identity, plan ID, action results and generation lineage"). A
  failed action aborts everything depending on it; no partial generation
  is ever returned as success.
- `crates/laminaria-cli` — `laminaria plan-self-build` (plan only, no
  execution) and `laminaria self-build --generation-root <dir>` (plan +
  execute). Both resolve `laminaria-planner` next to the running
  executable by default (`--planner` overrides), with no fallback if it
  can't be found.

## The stage0 → stage1 protocol

**stage0** is an ordinary, un-planned build produced by an external tool
— literally just `nim c` (or `nimble build`) on `nim-planner/` and
`cargo build --workspace --release` at the repo root, run by hand or in
CI, *without* going through `laminaria self-build` at all. This is the
bootstrap seed, the same role a previous release compiler plays in a
traditional compiler bootstrap.

**stage1** is produced by running stage0's own `laminaria` binary:

```bash
# stage0, built by an external tool (not this project's own CLI):
cd nim-planner && nim c --path:src -o:bin/laminaria-planner src/laminaria_planner.nim && cd ..
cargo build --workspace --release

# stage1, produced through the real plan+execute pipeline, using
# stage0's own planner to plan it:
./target/release/laminaria self-build \
  --planner nim-planner/bin/laminaria-planner \
  --generation-root target/laminaria-gen/stage1 \
  --generation-label stage0-to-stage1
```

(`--planner` is required here because `target/release/laminaria` itself
has no sibling `laminaria-planner` next to it — only `nim-planner/bin/`
does; `--planner` is what points stage0's `laminaria` at stage0's own
planner. stage1's own `laminaria` binary, once produced by `integrate`,
*does* have a sibling planner and needs no such flag — see below.)

`self-build` gathers the self-build `PlanningInput`, calls stage0's own
`laminaria-planner` binary (resolved next to the running `laminaria`
executable), validates the returned `ExecutionPlan`, and executes it:
`compile-nim-planner` (`nim c` directly — *not* `nimble build`, which was
found during this work to print a build-failure message but still exit
`0` on a genuine Nim compile error; `nim c`'s own exit code correctly
reflects success/failure, and using it directly also means the action is
recognized by this crate's own `is_nim_c_command` check and gets real
CC-wrapper tracing that `nimble build`'s internal `nim c` invocation
would have hidden), `compile-rust-host` (`cargo build --workspace
--release`), and `integrate` (copies both binaries plus
`laminaria-rustc-wrapper`/`laminaria-cc-wrapper` into
`target/laminaria-gen/stage1/`, so every sibling-binary lookup finds what
it needs next to whichever binary is running).

**Proving stage1's planner actually works** (not linked-but-unused):
stage1's own `laminaria` binary, run with no flags at all, finds its own
sibling `laminaria-planner` and plans successfully:

```bash
target/laminaria-gen/stage1/laminaria plan-self-build --json
```

This is exercised as an automated test, not just a manual step:
`crates/laminaria-run/src/self_build.rs`'s
`stage0_produces_a_stage1_whose_own_planner_actually_works` builds a real
stage0, runs a real `run_generation` to produce stage1, and then
independently invokes stage1's own freshly built planner binary and
asserts it returns a well-formed `ExecutionPlan` with
`produced_by == "laminaria-nim-planning-kernel"`.

## What is *not* claimed by this slice

- **stage1 → stage2** and generation-to-generation plan/artifact
  comparison are explicitly the *next* piece of work, not this one.
- **No caching/reuse**: every `run_generation` call fully rebuilds both
  the Nim planner and the Rust host from source (issues #7/#12's
  `reuse.rs` decision core is not wired into self-build at all).
- **No distributed execution, no fine-grained (per-translation-unit)
  compiler scheduling**: actions are coarse (a whole `cargo build`, a
  whole `nim c` invocation), which issue #6 explicitly permits for this
  first local slice ("Coarse compiler actions may establish the first
  self-build, with opaque regions exposed honestly. They do not prove
  fine-grained compiler scheduling.") — what this slice does prove is
  that those coarse actions are the *real* causal path (no hidden outer
  build, real Nim planning, real Rust-executed actions with real
  per-invocation evidence), not that the scheduler is optimal.

## Verified negative behavior (issue #8/#6: reject, don't succeed)

All of the following are automated tests, not just described behavior:

- **Determinism**: `laminaria-plan`'s
  `call_planner_against_the_real_binary_produces_a_deterministic_plan`
  calls the real `laminaria-planner` binary twice with identical input
  and asserts byte-identical `ExecutionPlan` output; `nim-planner`'s own
  `nimble test` asserts the same in-process.
- **Cycle rejection**: both `nimble test` (a two-action cycle and a
  self-cycle) and `laminaria-plan`'s
  `call_planner_reports_a_structured_cycle_rejection_from_the_real_binary`
  assert a `cycle` rejection with the exact `cycle_path`.
- **Invalid/unsupported input**: `unsupported_input`/`missing_producer`/
  `duplicate_producer`/`invalid_contract_version` rejections are each
  covered by `nimble test`, and `invalid_contract_version` is checked to
  happen *before* any other field is even decoded.
- **No Rust fallback plan**: `laminaria-plan`'s
  `call_planner_against_a_missing_binary_is_a_structural_error_never_a_fallback_plan`
  and `laminaria-run`'s
  `run_generation_against_a_missing_planner_binary_fails_without_producing_a_generation`
  both assert a missing planner binary is a structural `Err`, never a
  silently-computed substitute.
- **Compile failures abort the generation**: `laminaria-run`'s
  `a_compile_failure_aborts_the_generation_without_producing_a_stage_output`
  introduces a genuine Nim syntax error in a throwaway copy of
  `nim-planner/` (never the tracked repo) and asserts the generation
  fails at exactly that action, `integrate` never runs, and no stage
  output binary is produced.

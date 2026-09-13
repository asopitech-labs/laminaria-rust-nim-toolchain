# Research Issue Plan

This file maps the GitHub issue set to the research program. It is an issue projection, not the project work authority. The [project work portfolio](project-portfolio.md) manages outcomes, capabilities, unissued gaps, decisions, experiments, implementation, verification, release work, and later expansion. GitHub issues track only bounded portions of that work.

The [research issue prioritization and minimal-hypothesis policy](../01-foundations/research-prioritization-policy.md) governs how work is selected and when an issue may stop. Existing acceptance lists are research backlogs and evidence menus, not requests for finished products or mandatory all-at-once completion. Project-wide priority follows non-substitutability and falsification power, not issue number or unchecked-box count.

The [project progression and near-term research goal](../near-term-research-program.md) is the canonical current roadmap. The active milestone is G1–G3: jointly resolve package/source-semantic/language-and-intermediate-IR/artifact/ABI/symbol/link obligations, discharge or externalize them into an ordinary runnable native artifact, then compare resolution algorithms by time, peak memory, explored states, pruning, and recomputation. A single language call path is not dependency-graph completion. Completing the G1–G3 issue set is also not completion of the project portfolio.

The portfolio has two coupled research lanes: **Lane A — Semantic and Artifact Closure** and **Lane B — Efficient Compiler Computation**. Source semantics, IR, identity, and provenance form their shared substrate rather than duplicated lane-local models. Current GitHub tracking surfaces are #45 as shared gate M1, #48 for Lane A A1/G1, #46 for Lane A A2/G2, and #47 for Lane B B1/G3. Existing #3/#5-#8/#10-#12/#18-#22/#42/#44 retain their subsystem responsibilities. Work without a suitable issue remains explicitly listed in the portfolio rather than being forced into the nearest existing issue.

## Compiler ownership and execution order — corrected 2026-09-10

The [compiler ownership contract](../01-foundations/compiler-ownership-contract.md) governs this plan. The current artifact goal is a native executable backed by a resolved Cargo/Nimble/C/C++ graph. The independent Rust/Nim compiler, IR, and scheduler must ultimately compile LAMINARIA itself and its transitive dependencies. External-compiler orchestration/self-build is reference/bootstrap evidence, not the primary delivery milestone; WebAssembly is an optional target track.

1. **#8 + #22 + #44, with required parts of #3/#4/#5/#7/#18:** ingest Cargo/Nimble/C/C++ metadata and resolve one typed package/source/artifact/toolchain/ABI/symbol/link closure, preserving ecosystem identity and rejecting one conflict before compilation.
2. **#6 + #4 + #5 + #44:** execute that closure through the production Nim planner/Rust runtime and produce and run one ordinary native executable with explicit compile/adapter/archive/final-link edges.
3. **#7/#8/#11/#12 + #19–#24 as needed:** compare eager and demand-driven resolution for correctness, wall-clock, peak memory, state expansion/pruning/merging, invalidation, and avoided work.
4. **#3/#25/#26:** broaden source semantics and partition/fusion only against counterexamples found in the real dependency graph. Rust-only, Nim-only, and mixed inputs use the same owned path.
5. **#2:** expand surviving source/dependency coverage to the real Rust + Nim implementation through independent stage0 → stage1 → stage2 generations.

#4 runtime/ABI integration supports delivery but is not a replacement for compiler development. #5/#13 define owned target routes and compiler-work boundaries; #44, defined by `docs/02-research-areas/compiler/nim-c-cpp-library-integration.md`, owns foreign declarations, native dependency production, explicit adapter generation and final-link participation. #14–#17 remain prior-art/comparison experiments feeding #25. #22–#24 distinguish owned compiler profiles from external reference/bootstrap matrices.

## Measurement foundation — supporting track

#11 comprises #18 environment/fingerprints, #19 Run/lifecycle tracing, #20 artifact/compiler evidence and #21 scenario/comparison discipline. Respect dependencies within this track without postponing the small independent compiler slice. Retain historical measured results at their demonstrated scope.

The detailed designs are documented in:

- `docs/02-research-areas/measurement/measurement-foundation.md`
- `docs/02-research-areas/measurement/measurement-foundation_ja.md`
- `docs/02-research-areas/toolchains/multi-version-toolchains.md`
- `docs/02-research-areas/toolchains/multi-version-toolchains_ja.md`
- `docs/02-research-areas/toolchains/validated-toolchain-profiles.md`
- `docs/02-research-areas/toolchains/validated-toolchain-profiles_ja.md`
- `docs/02-research-areas/toolchains/agent-oriented-toolchain-ux.md`
- `docs/02-research-areas/toolchains/agent-oriented-toolchain-ux_ja.md`

## Horizontal distribution research

Horizontal distribution is a cross-cutting research subject, not a deployment assumption. The canonical charter is documented in:

- `docs/02-research-areas/execution/horizontal-distribution-research.md`
- `docs/02-research-areas/execution/horizontal-distribution-research_ja.md`

The comparison must keep three partition levels separate:

1. source/build-graph translation-unit, object and archive distribution;
2. global-summary/index followed by backend-job distribution, as in ThinLTO/DTLTO;
3. action-level remote execution.

Kbuild, distcc/icecream, LLVM ThinLTO/DTLTO and Bazel Remote Execution are prior-art baselines. None defines the canonical LAMINARIA semantic partition. The candidate partition must be derived from preserved Rust/Nim semantic facts, global requirements, invalidation boundaries, resource constraints and explanation needs.

## Core tracks

2. Compiler pipeline decomposition across supported toolchain versions — #3
3. Rust–Nim native linking without mandatory C ABI boundary — #4
4. Backend route selection and capability constraints — #5
5. Nim C/C++ library reuse, foreign declarations and native dependency/link integration — #44
6. Unified Action Graph and resource-aware scheduling — #6
7. Artifact identity, incremental invalidation and CAS — #7
8. Variant-space control in the Nim Planning Kernel — #8
9. Optional WASM mixed-language integration/topology — #9
10. Agent-oriented explainability and evidence schema — #10
11. Work elimination, execution correctness and no-op build invariants — #12
12. Multi-version Rust/Nim toolchain selection and artifact compatibility — #22
13. Validated toolchain profiles and progressive configuration — #23
14. Agent-oriented bounded/explainable toolchain planning and UX — #24

## Backend pipeline white-boxing expansion

14. Expand backend routes into nested observable/checkpoint/execution graphs and define checkpoint economics — #13
15. White-box LLVM pass/codegen/LTO pipeline boundaries without pass-per-process decomposition — #14
16. Map ThinLTO/DTLTO dynamic backend jobs into the LAMINARIA scheduler and cache graph — #15
17. Optionally decompose the WebAssembly target pipeline through `wasm-ld`, Binaryen, WIT and componentization — #16
18. Evaluate shared LLVM IR/LTO convergence across Rust, Nim 2 and Nimony routes — #17

The detailed architecture is documented in:

- `docs/02-research-areas/compiler/backend-pipeline-whiteboxing.md`
- `docs/02-research-areas/compiler/backend-pipeline-whiteboxing_ja.md`

## Responsibility boundaries

### #18 versus #22

#18 owns installation/discovery, named toolchain sets, exact resolution and ToolchainFingerprint generation. #22 owns how compiler/toolchain version participates in Variant Graph resolution, capability constraints, artifact compatibility, and cross-version reuse policy.

### #22 versus #23

#22 owns the broad internal version/compatibility search space. #23 owns the narrower user-facing qualification layer: `recommended`, `latest-validated`, `long-term`, `preview`, progressive presets and advanced overrides.

A combination accepted by #22 is only a candidate for #23. Static compatibility does not imply recommendation.

### #23 versus #24

#23 defines evidence-backed profiles and progressive configuration surfaces. #24 defines the coding-agent and human interaction policy that consumes those profiles: cost-ordered resolution, reusable negative knowledge, ranked viable plans, bounded exploration, and UX metrics.

#24 must not build a separate constraint solver. It uses #8's variant-space machinery, #22 compatibility facts, #23 qualification evidence, and #10 structured explanations.

### #24 versus #8/#10

#8 owns how candidate states are represented, merged, pruned and bounded in the Nim Planning Kernel. #10 owns the machine-readable explanation schema. #24 owns the UX-level success condition: those capabilities must prevent coding agents from externalizing combinatorial search as repeated failing compiler/build attempts.

### #23 versus #11/#18–#21

#23 does not invent a separate trust system. Profile qualification must consume the common Measurement Spine evidence from #11/#18–#21. Profile aliases resolve to exact ToolchainFingerprint bundles and immutable profile revisions before execution.

### #22 versus #3/#7/#20

#22 defines the common multi-version model. #3 maps actual compiler-pipeline boundaries per toolchain capability, #7 applies the exact toolchain identity to cache/artifact compatibility, and #20 adapts version-specific native telemetry into the common Run schema.

### #11/#18–#21 versus later research

#11 and #18–#21 own the common evidence model: environment/toolchain identity, Run/process/resource trace, artifact/telemetry records, scenario/repetition/comparison semantics and observer-overhead measurement.

Later research may extend these schemas with backend-specific data, but must not create incompatible benchmark/evidence stores.

### #5 versus #13

#5 answers **which backend route is valid and selected**. #13 answers **how the selected backend expands into internal computation and which boundaries become observable/checkpoint/execution nodes**.

### Nim C/C++ integration versus #4/#5/#26

#44 preserves Nim's ability to reuse foreign libraries when LAMINARIA skips Nim-generated C/C++. It owns `importc`/`importcpp`-style semantic declarations, foreign source/prebuilt artifact/adapter actions, native toolchain identity and final-link edges. #4 owns Rust–Nim cross-language runtime/ABI research; #5 owns the target-generation route for LAMINARIA-produced code; #26 owns public project compilation through the common owned path. C/C++ compilation of a declared foreign dependency is not a fallback implementation of the Nim target unit.

### Optional WASM track: #9 versus #16

#9 compares mixed-language WebAssembly integration topologies and boundary costs. #16 white-boxes the target production pipeline itself: relocatable Wasm, `wasm-ld`, Core Wasm, Binaryen, WIT/adapters and componentization. Both are optional target research after the native dependency-closure milestone; neither defines the project goal or blocks G1–G3.

### #4 versus #17

#4 tests direct Rust–Nim native object/link contracts without a mandatory C ABI boundary. #17 tests whether Rust/Nim-origin LLVM artifacts can participate in a shared LLVM/LTO plan and explicitly separates backend artifact compatibility from language/runtime ABI compatibility.

### #6/#7/#12 versus #15

#15 is not a separate scheduler/cache architecture. It is the ThinLTO/DTLTO stress case that must use #6 scheduling, #7 identity/CAS and #12 work-elimination semantics. DTLTO's externally described backend jobs are used to test dynamic graph expansion rather than adding a hidden nested scheduler.

### Horizontal distribution versus #6/#7/#13–#17/#25

The horizontal-distribution charter compares object-level, LLVM-derived backend-level and action-level remote partitions. #6 owns placement/resource accounting, #7 owns identity/invalidation/reuse, #13 owns logical/checkpoint/execution boundary economics, #14–#17 supply optional LLVM/ThinLTO/WASM baselines, and #25 owns the independent semantic-fact-derived partition. Remote execution is evidence about placement and cost; it must not silently become a new semantic substrate or hidden scheduler.

Persistence is part of that decision. #7 must distinguish logical artifacts from physical replicas and compare keeping, materializing, replicating, transferring and recomputing them across memory, local storage, peer caches and remote durable stores. The relevant cost includes CPU, memory, storage I/O, network I/O, serialization, hashing, consistency, recovery and retention—not only cache-hit rate.

Node heterogeneity is another explicit dimension. The scheduler must separate `execute-on` from `produces-for` and qualify Windows, macOS and Raspberry Pi nodes by host OS/ISA, target OS/ISA, ABI, sysroot/SDK, linker, target features, runtime and trust. Cross-compilation actions may run concurrently across heterogeneous nodes when their contracts are independent, while native tests, target-specific linking and performance measurements must be placed on compatible nodes.

## Tool UX rule

Tool UX is a first-class project objective, not only CLI cosmetics.

LAMINARIA intentionally accepts a broad internal combination space, but normal human/coding-agent usage should follow:

```text
requirements + intent
  -> profiles / compatibility / negative knowledge
  -> constraint resolution and pruning
  -> small ranked viable-plan set
  -> structured explanation
  -> execution
```

Execution must not be the default combinatorial search mechanism.

The normal agent path should be bounded. Known-incompatible variants are not executed; equivalent rejected states are pruned/merged; validated candidates are preferred; unresolved cases return validation gaps and ranked next actions instead of continuing retries indefinitely.

Broad brute-force exploration remains available explicitly for research mode.

UX evaluation includes external build attempts avoided, failed toolchain attempts, time-to-first-viable-plan, explored/pruned/merged states, negative-knowledge reuse, fallback count, and agent log/context volume.

## Toolchain profile rule — role-separated qualification

LAMINARIA intentionally has two different surfaces:

```text
Internal: broad candidate variant space
  ↓ constraint compatibility
User-facing: narrow evidence-backed profiles
```

Default user-facing profiles should include at least `recommended`, `latest-validated`, `long-term`, `preview`, and `custom`.

Upstream freshness/support and LAMINARIA qualification are separate dimensions. In particular, a newest stable release is not automatically recommended until qualified.

Rust's normal upstream model is stable/beta/nightly; LAMINARIA must not imply an upstream Rust LTS channel. A `long-term` Rust-containing profile is a LAMINARIA-maintained bundle with its own support policy.

Configuration is progressively disclosed:

1. profile only;
2. intent preset;
3. advanced overrides;
4. expert graph constraints.

Any override must re-evaluate qualification status rather than inheriting the base profile's validation badge.

## Multi-version toolchain rule — external reference/bootstrap matrices

Compiler version is a graph dimension, not an ambient machine setting.

- Rust and Nim toolchains are resolved from selectors to exact ToolchainFingerprint values;
- Cargo `rust-version`, Rust edition, selected rustc, Cargo resolver behavior and nightly capability are separate constraints;
- one connected normal Cargo/Rust crate graph normally resolves to one Rust toolchain;
- cross-version `rmeta`/`rlib`/internal compiler artifacts are not assumed compatible;
- lower-level object/archive/Wasm/native boundaries may be studied separately with explicit compatibility evidence;
- Nim 2/Nim 3 and multiple Rust versions use the same high-level toolchain-version abstraction while preserving language-specific metadata.

## Environment rule

Reproducibility and canonical performance isolation are separate concerns.

- container/Nix-like environments may reproduce bootstrap/correctness;
- canonical performance baselines normally execute natively on the measured environment;
- WSL, native Linux, macOS and other host classes have distinct EnvironmentFingerprint values;
- different fingerprints are non-comparable by default unless an explicit cross-environment study says otherwise;
- resolved tool executables/revisions are recorded rather than only requested version labels.

## Optimization order

LAMINARIA should prefer:

1. eliminating unnecessary compiler/backend actions or stages;
2. reusing valid artifacts/checkpoints;
3. reducing invalidation scope;
4. exposing parallelism;
5. globally scheduling the remaining work;
6. optimizing individual remaining actions.

The research program must distinguish these effects in evidence. Parallelizing work that should not have executed is not equivalent to eliminating it.

## Boundary rule

Backend white-boxing must not equate graph visibility with process granularity. Every backend boundary is classified independently as:

1. logical stage;
2. observation boundary;
3. checkpoint/artifact boundary;
4. execution boundary;
5. dynamic graph-expansion point.

A compiler pass may be visible and measured without being separately serialized or scheduled.

## Evidence rule

Each issue must provide reproducible evidence. Passing functional tests alone is not sufficient for architecture, performance, scheduling, cache, compiler/backend-boundary, reduced-work, linking, profile qualification, or tool UX claims. Controlled incremental tests should validate the expected execution set as well as the final artifact.

Single wall-clock samples are insufficient for architecture decisions. The measurement spine stores raw samples, characterizes environment noise, records explicit cold/warm/no-op/cache state, and measures observer overhead.

Backend checkpoint work must measure both benefit and cost. At least one overly fine checkpoint candidate must be allowed to fail the economics test; increasing graph granularity is not itself a success criterion.

Tool UX work likewise must measure whether planning actually avoids unnecessary external build attempts; a sophisticated resolver that still makes an agent try dozens of compiler combinations is not a successful UX result.

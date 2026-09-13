# LAMINARIA Project Work Portfolio

This document manages the work needed to reach LAMINARIA's project outcomes. It is not an index of existing GitHub issues. The [near-term research program](../near-term-research-program.md) is the canonical source for project direction; this portfolio turns that direction into capabilities, decisions, experiments, implementation slices, verification, and later expansion.

GitHub issues are a projection of bounded work that benefits from external tracking. A missing issue does not mean missing work is out of scope, and an open issue does not make that work a current priority.

**Cross-cutting invariant:** a binary without an executable test contract is not a completed artifact. Testability has its own research lane and participates in every milestone gate; it is not a post-build CI concern.

## Management model

```text
project outcome
  -> required capability
  -> unresolved decision or falsifiable hypothesis
  -> bounded experiment
  -> production implementation slice
  -> direct executable verification
  -> evidence-backed decision
  -> expansion, reformulation, retirement, or release
```

Work is managed at every level of this chain. An issue checklist, design document, fixture, validator, benchmark, or passing test is never a substitute for the outcome above it.

## Portfolio rules

1. Start from the requested native artifact and long-term self-hosting outcome, not from the available issue list.
2. Preserve separate but connected package, source-semantic, language/intermediate-IR, artifact, ABI, symbol, link, execution, and runtime concerns.
3. Record work that has no issue yet. Open an issue only when the boundary, decision, owner relationship, and stop condition are clear enough to track.
4. Treat implementation, direct verification, measurement, release qualification, documentation, and removal of obsolete paths as distinct work.
5. Do not keep work active merely because its issue is open. Do not close a project gap merely because a related issue or fixture was completed.
6. Use direct executable tests against production paths as verification authority. YAML catalogs, fixture-only validators, and validator tests may organize evidence but cannot be the source of truth.
7. Reassess the portfolio whenever an experiment changes the graph model, compiler boundary, artifact contract, or project goal.
8. Bind test results to the exact subject artifact, harness, environment, and raw evidence. An instrumented or test-profile artifact cannot silently certify a different production artifact.

## Three research lanes

The project has three coupled research lanes. They answer different questions, but none can deliver LAMINARIA alone.

```text
Lane A — Semantic and Artifact Closure
  What package/source/IR/ABI/link obligations must be solved,
  transformed, discharged, externalized, or rejected
                         |
                         | shared typed graph, identities,
                         | semantic facts, roots, provenance
                         v
Lane B — Efficient Compiler Computation
  How the required compiler computation is discovered, partitioned,
  pruned, incrementally recomputed, scheduled, retained, and placed
                         |
                         | test demands, selection, execution events,
                         | failures, observations, raw evidence
                         v
Lane C — Executable Verification and Testability
  How exact artifacts are controlled, observed, isolated, tested,
  falsified, selected for retest, and qualified

                    shared milestone gate
                         |
                         v
              runnable native artifact + evidence
```

Lane A owns artifact meaning and completeness. Lane B owns computation strategy and resource behavior. Lane C owns executable verification and testability. Source semantics and language/intermediate IR are not assigned exclusively to one lane: Lane A uses them to decide what is required and whether an obligation is satisfied; Lane B uses them to decide what computation exists and how it should execute; Lane C uses them to derive test obligations, controls, observations, oracles, and retest impact. Duplicating separate graphs, IR summaries, or expectation manifests per lane is prohibited.

The research baselines and falsifiable hypotheses are maintained in the [Lane A foundation report](../02-research-areas/toolchains/lane-a-semantic-artifact-closure-foundations_ja.md), [Lane B foundation report](../02-research-areas/execution/lane-b-efficient-compiler-computation-foundations_ja.md), and [Lane C foundation report](../02-research-areas/toolchains/lane-c-executable-verification-foundations_ja.md). This portfolio selects work from those findings; it does not duplicate their literature survey.

### Lane A — Semantic and Artifact Closure

| Milestone | Decision and evidence | Current tracking |
| --- | --- | --- |
| A0 — demand and obligation contract | Native artifact demand plus typed package, semantic, lowering, artifact, ABI, symbol, link, runtime, and provenance obligations; unresolved obligations prevent production | Portfolio gap under #46 |
| A1 — coupled mixed-ecosystem resolution | Cargo/Nimble/C/C++ package choices receive feedback from source/module/type/FFI and IR lowering; one positive and one pre-compilation rejection | G1 #48 |
| A2 — dependency-discharged native artifact | Owned and foreign actions produce a directly runnable native artifact; every obligation is discharged or externalized; clean-environment execution succeeds | G2 #46 |
| A3 — representative ecosystem coverage | Counterexample-driven package, language, C/C++, runtime, platform, security, and license coverage across real projects | #3/#22/#23/#26/#42/#44 plus newly discovered tasks |
| A4 — self-hosted artifact lineage | Stage0 produces stage1 and stage1 produces stage2 through the owned path, including required transitive dependencies | #2/#26 and bounded successors |

### Lane B — Efficient Compiler Computation

| Milestone | Decision and evidence | Current tracking |
| --- | --- | --- |
| B0 — common observation and identity contract | The same production graph exposes causal events, identity, resource use, retained/pruned work, and artifact provenance without observer-defined semantics | #7/#10/#11/#19-#21 |
| B1 — demand-driven and pruned first graph | Eager, demand-driven, compiler-DCE, linker-GC, and cross-layer-pruned strategies are correctness-equivalent; at least one algorithm decision follows from time/memory/work evidence | G3 #47; #8/#12 |
| B2 — incremental and memory-bounded computation | No-op and controlled edits recompute only valid slices; retention, spill, reload, and recomputation are selected under memory constraints | #6/#7/#37/#38/#39 |
| B3 — semantic partition, fusion, and scheduling | Partition/fusion and resource-aware schedules are derived from preserved semantics and measured against coarse compiler/action boundaries | #25/#27/#29-#34 |
| B4 — resilient physical placement | Local/remote retention, transfer, recomputation, failure recovery, and heterogeneous execute-on/produces-for placement preserve logical identity | #6/#7/#39-#41 |

### Lane C — Executable Verification and Testability

| Milestone | Decision and evidence | Current tracking |
| --- | --- | --- |
| C0 — test contract and subject identity | `TestContract`, exact subject/harness/environment identities, controls, observations, oracle, isolation, target execution requirements, and raw result schema are fixed | M0/M1 work; #49 and test-harness contract |
| C1 — exact native artifact harness | The exact M1 production binary is exercised in a clean target environment; instrumented artifacts remain separate identities | M1 #49 |
| C2 — semantic, ABI, and failure coverage | Cross-language semantics, ABI/symbol/runtime contracts, negative dependencies, crash/timeout/resource failures, and pruning equivalence are directly tested | G1-G3 integration; #10/#20/#44 |
| C3 — incremental and distributed test selection | Source/IR/artifact/runtime changes derive the valid rebuild/retest set; execution is placed only on qualified target nodes and flakiness is measured | #7/#11/#19-#21/#40/#41 plus newly discovered tasks |
| C4 — self-host and release qualification | Stage lineage/conformance and exact release candidate install/relocation/update/rollback/provenance contracts are executable and repeatable | #2/#23/#26 and future release tasks |

### Shared milestone gates

The lanes synchronize at outcome gates. A lane-local experiment can finish without waiting for unrelated work, but a project milestone passes only when both required sides are present.

| Gate | Required Lane A state | Required Lane B state | Required Lane C state | Result |
| --- | --- | --- | --- | --- |
| M0 — contract lock | A0 native demand and obligation types fixed | B0 event/identity/measurement contract fixed | C0 test subject/control/observation/oracle contract fixed for the same graph | One falsifiable executable experiment contract |
| M1 — first tested native artifact | A1 and A2 produce the exact production artifact | B1 produces correctness-equivalent measurements and one algorithm decision | C1 and the required part of C2 test the exact binary, cross-language path, ABI/runtime, negative dependency, and pruning equivalence | First tested dependency-discharged native artifact; current GitHub milestone |
| M2 — representative tested native projects | A3 supports or explicitly rejects selected real projects | B2 bounds invalidation, memory, and publication behavior | C2/C3 bound test coverage, retest selection, environment matrix, and flakiness | Reusable tested native toolchain subset with known limits |
| M3 — tested self-hosted native toolchain | A4 proves stage lineage and dependency discharge | B3 makes self-build computation explainable and resource-bounded | C4 proves stage conformance and excludes copied/external artifacts | Stage1/stage2 owned self-hosting evidence |
| M4 — qualified resilient release | O8 packaging/update/rollback contracts pass | B4 or an explicitly local-only profile passes recovery and placement requirements | C4 tests exact release candidates across each qualified operational profile | Releasable artifact profile with stated operational scope |

M0–M4 are evidence gates, not calendar phases. Optional WASM or advanced optimization work may inform a gate but cannot replace its native evidence.

## Outcome portfolio

| Outcome | Completion evidence | Current condition | Horizon |
| --- | --- | --- | --- |
| O1 — dependency-discharged native artifact | A mixed Cargo/Nimble/C/C++ project produces a harness-tested exact production native artifact; every package/source/IR/ABI/symbol/link obligation is discharged, externalized, or rejected with production evidence | Active research milestone; no complete production path yet | Now |
| O2 — owned Rust/Nim compiler path | Supported Rust and Nim source semantics lower through LAMINARIA-owned IR, legal transformations, target generation, and runtime support without hidden compiler fallback | Small source/IR/interpreter/WASM evidence exists; native coverage is incomplete | Now and Next |
| O3 — efficient incremental computation | Correctness-equivalent demand resolution, pruning, invalidation, reuse, memory planning, and scheduling improve measured cost over eager/coarse baselines | Planning, tracing, reuse, and incremental pieces exist; coupled package-to-link evidence is missing | Now and Next |
| O4 — practical ecosystem coverage | Real package metadata, generated inputs, C/C++ libraries, runtime capabilities, platform contracts, diagnostics, and security/license constraints are supported or rejected explicitly | Mostly reference/delegated paths and design contracts | Next |
| O5 — LAMINARIA builds LAMINARIA | Stage0 uses the owned path to produce stage1; stage1 produces stage2; both implementation languages and required dependencies participate | Existing self-build is delegated/bootstrap evidence only | Later, grown one supported slice at a time |
| O6 — resilient physical execution | Compiler work can be placed, retained, spilled, transferred, recovered, and verified across memory/storage/nodes without changing semantic identity | Research and issue inventory exists; not a gate for the first local artifact | Later |
| O7 — optional targets and advanced optimization | WASM, shared libraries, LLVM/LTO comparisons, polyhedral/fusion/autotuning, and other targets consume the same semantic and dependency substrate where evidence supports them | Several bounded experiments exist; none defines the current goal | Optional / evidence-triggered |
| O8 — usable and releasable toolchain | A user or coding agent can select, explain, build, verify, package, and update qualified artifacts without brute-force retries or hidden prerequisites | Toolchain/profile/UX and release work is fragmented | Next, after O1 has a real artifact contract |

These horizons express decision order, not a promise that every item in one row completes before useful work in another begins. A supporting slice may move earlier when it is required to falsify the active hypothesis.

## Current capability and gap map

| Capability | Repository evidence now | Unresolved project work | Issue projection |
| --- | --- | --- | --- |
| Cargo/Nim project discovery | `laminaria-run` discovers Cargo manifests and Nim entry points; delegated builds are tested | Parse and normalize Cargo and Nimble dependency semantics without using their target builds as the resolver | G1 #48; related #8/#22 |
| Cross-ecosystem package model | Variant and toolchain models exist in bounded forms | Represent Cargo/Nimble/C/C++ identities, versions, features, providers, host/target roles, build dependencies, generated inputs, and conflicts in one typed model | G1 #48; related #8/#18/#22/#44 |
| Source and semantic model | `laminaria-ir` has restricted Rust/Nim frontends, validation, interpretation, transformation, and dependency discovery | Expand only the constructs needed by the selected mixed workload and feed discovered module/type/FFI facts back into resolution | G1 #48; related #3/#25 |
| Language/intermediate-IR coupling | Owned IR operations and limited transforms exist | Define typed obligations between language semantics, lowering choices, intermediate forms, target capabilities, and invalidation | G1 #48 and G2 #46; related #3/#5/#7 |
| Dependency-obligation lifecycle | Artifact contract defines `Unresolved`, `Selected`, `Satisfied`, `Discharged`, `Externalized`, and `Rejected` | Implement these states and their causal provenance in production graph types | **Unissued implementation gap** under G2 #46 |
| C/C++ foreign dependencies | Reference native-link fixtures and delegated compiler telemetry exist | Model headers, C/C++ compile/adapter/archive/shared-library production, ABI, symbols, link order, and runtime deployment explicitly | G1/G2; #44 |
| Native target production | Direct native-link baselines exist; the owned target experiment is currently WASM-shaped | Select and implement the smallest owned native object/code generation path and explicit final linker action | G2 #46; #5 must be re-evaluated for native priority |
| Runtime capability derivation | Runtime needs are documented and #42 exists | Derive only demanded startup, allocation, unwind, panic, TLS, and platform support; externalize unsupported system contracts | **Required slice not yet attached to G2**; related #42 |
| Clean artifact consumption | Artifact and closure contract is documented | Inspect actual loader dependencies/symbols/resources and execute in a clean environment without build tools or sources | G2 #46; related #20 |
| First-class artifact test harness | Unit/integration tests and process evidence exist, but no production graph `TestContract` binds an exact subject artifact, harness, target environment, controls, observations, and raw result | Model test demands/dependencies separately from release dependencies; execute exact production artifacts; derive incremental test selection and release qualification from the same graph | Lane C #49 for M1; later C3/C4 gaps remain unissued |
| Cross-layer pruning | Reachability contract and compiler/linker baselines are documented | Implement shared roots and conservative retention from package candidates through source, IR, object, symbol, section, and runtime | G3 #47; #12 |
| Incrementality and resource evidence | Incremental planner, executor, tracing, reuse, and benchmark fixtures exist | Apply them to the same coupled graph and separate avoided work, cache reuse, invalidation, scheduling, and memory effects | G3 #47; #6/#7/#11/#12/#19-#21 |
| Explanation and negative knowledge | Structured plan rejection mechanisms exist | Explain package-to-artifact selection, discharge, externalization, pruning, and rejection without log archaeology | G1-G3; #10/#24 |
| Release and update contract | Bootstrap/project-build guides and CI exist | Define qualification, packaging profiles, provenance/SBOM export, compatibility, rollback, and update policy for dependency-discharged artifacts | **Unissued outcome gap** under O8 |
| Self-hosting progression | Delegated stage-like self-build evidence exists | Replace one supported compiler/dependency slice at a time and prove stage producer lineage before broadening coverage | O5; #2/#26/#27 |
| Physical distribution and persistence | Research contracts and scheduler/storage components exist | Decide retention, spill, recomputation, remote placement, atomic publication, recovery, and heterogeneous qualification from measurements | O6; #38-#41 plus #6/#7/#39 |

## Active shared gate: M1

The active gate is not “finish issues #45–#48.” It is to decide whether Lane A can produce a dependency-discharged native artifact, Lane B can demonstrate a correctness-equivalent measurable computation strategy, and Lane C can directly falsify or qualify the exact artifact on the same graph. The issues are only bounded reporting surfaces.

### M1-W0 — freeze the observable artifact demand

- Native executable entry point, target/host, export/dynamic roots, runtime profile, and expected behavior.
- One Cargo crate, one Nimble package, one C library, and one C++ library whose results are all observable.
- Positive graph, one cross-layer feedback case, and one pre-compilation incompatibility.
- Exact production subject identity, test contract, harness controls/observations, target environment, and negative dependency behavior.

### M1-W1 — implement the common obligation vocabulary

- Typed obligation kinds for package, semantic, lowering, artifact, ABI, symbol, link, runtime, and provenance relationships.
- State transition and causal-operation evidence for discharge, externalization, rejection, and proven irrelevance.
- An unresolved obligation must make artifact production fail.

This is a concrete implementation gap even though no dedicated legacy issue describes it. G2 #46 is its current tracking surface; split a dedicated issue only after the graph-type boundary is decided.

### M1-W2 — ingest and couple ecosystem facts

- Normalize package metadata while preserving Cargo/Nimble/C/C++ identity and semantics.
- Feed source/module/type/FFI and lowering results back into candidate choice.
- Reject opaque target-build execution as “resolution.”

### M1-W3 — produce foreign and owned native artifacts

- Owned Rust/Nim lowering and native target output for the supported subset.
- Explicit C/C++ compile, adapter/instantiation, archive/shared-library, symbol, ABI, and link actions.
- Explicit runtime-capability generation or external runtime contract.

### M1-W4 — discharge and execute

- Prove every retained obligation is discharged or externalized.
- Preserve the original graph as provenance rather than a consumer re-resolution graph.
- Execute the produced artifact in a clean environment and inspect its actual loader, symbol, resource, and provenance behavior.

### M1-W5 — compare computation strategies

- Compare eager expansion, demand-driven resolution, early cross-layer pruning, compiler DCE, linker GC, invalidation, and reuse on the same semantics.
- Measure elapsed time, CPU, peak RSS, IR bytes, artifact size, expanded/pruned/merged/recomputed nodes, external actions, and conservative retention.
- Adopt, reject, or reformulate at least one graph representation or algorithm.

## Next outcome decisions

M1 evidence selects the next work in both lanes; issue age does not.

1. If package/semantic/IR feedback is the limiting factor, expand O2 source and semantic coverage using #3/#25/#29-#34 only for the counterexample found.
2. If native production or runtime support is limiting, advance #5/#42/#44 and create a dedicated native-runtime or linker task with the observed contract.
3. If state explosion or memory is limiting, advance #8/#38 before broadening ecosystem coverage.
4. If invalidation, identity, or publication correctness is limiting, advance #7/#37/#39.
5. If the local artifact works, broaden practical project inputs through #26/#28 and then begin an O5 self-hosting generation slice.
6. Activate #13-#17, #40/#41, or optional WASM work only when it tests a discovered architecture decision; do not use it to replace the native path.

## Issue lifecycle

An issue may be created when all of the following are known:

- the parent outcome and capability gap;
- the decision or implementation boundary it owns;
- required production evidence;
- dependencies and non-goals;
- a stop condition that does not require completing an entire subsystem.

An issue is closed only when its bounded decision is made or its implementation evidence exists. The capability row remains open in this portfolio until the project outcome is satisfied. Superseded issues are closed with a pointer to the replacing decision; they are not silently reinterpreted.

## Review cadence

At each meaningful result:

1. update the relevant outcome and capability condition;
2. record the newly discovered gap even if it has no issue;
3. decide whether active work remains the highest-falsification next step;
4. create, split, merge, defer, or close issue projections;
5. update direct tests and evidence references;
6. check whether release, migration, documentation, or obsolete-path removal work was created by the change.

The portfolio is successful when it makes missing work and changed decisions visible. A tidy issue list by itself is not a project outcome.

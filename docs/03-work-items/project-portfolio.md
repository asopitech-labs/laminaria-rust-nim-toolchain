# LAMINARIA Project Work Portfolio

This document manages the work needed to reach LAMINARIA's project outcomes. It is not an index of existing GitHub issues. The [near-term research program](../near-term-research-program.md) is the canonical source for project direction; this portfolio turns that direction into capabilities, decisions, experiments, implementation slices, verification, and later expansion.

GitHub issues are a projection of bounded work that benefits from external tracking. A missing issue does not mean missing work is out of scope, and an open issue does not make that work a current priority.

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

## Outcome portfolio

| Outcome | Completion evidence | Current condition | Horizon |
| --- | --- | --- | --- |
| O1 — dependency-discharged native artifact | A mixed Cargo/Nimble/C/C++ project produces and runs a native artifact; every package/source/IR/ABI/symbol/link obligation is discharged, externalized, or rejected with production evidence | Active research milestone; no complete production path yet | Now |
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
| Cross-layer pruning | Reachability contract and compiler/linker baselines are documented | Implement shared roots and conservative retention from package candidates through source, IR, object, symbol, section, and runtime | G3 #47; #12 |
| Incrementality and resource evidence | Incremental planner, executor, tracing, reuse, and benchmark fixtures exist | Apply them to the same coupled graph and separate avoided work, cache reuse, invalidation, scheduling, and memory effects | G3 #47; #6/#7/#11/#12/#19-#21 |
| Explanation and negative knowledge | Structured plan rejection mechanisms exist | Explain package-to-artifact selection, discharge, externalization, pruning, and rejection without log archaeology | G1-G3; #10/#24 |
| Release and update contract | Bootstrap/project-build guides and CI exist | Define qualification, packaging profiles, provenance/SBOM export, compatibility, rollback, and update policy for dependency-discharged artifacts | **Unissued outcome gap** under O8 |
| Self-hosting progression | Delegated stage-like self-build evidence exists | Replace one supported compiler/dependency slice at a time and prove stage producer lineage before broadening coverage | O5; #2/#26/#27 |
| Physical distribution and persistence | Research contracts and scheduler/storage components exist | Decide retention, spill, recomputation, remote placement, atomic publication, recovery, and heterogeneous qualification from measurements | O6; #38-#41 plus #6/#7/#39 |

## Active outcome: O1

The active outcome is not “finish issues #45–#48.” It is to decide whether the coupled model can produce a dependency-discharged native artifact. The issues are only bounded reporting surfaces.

### A0 — freeze the observable artifact demand

- Native executable entry point, target/host, export/dynamic roots, runtime profile, and expected behavior.
- One Cargo crate, one Nimble package, one C library, and one C++ library whose results are all observable.
- Positive graph, one cross-layer feedback case, and one pre-compilation incompatibility.

### A1 — implement the common obligation vocabulary

- Typed obligation kinds for package, semantic, lowering, artifact, ABI, symbol, link, runtime, and provenance relationships.
- State transition and causal-operation evidence for discharge, externalization, rejection, and proven irrelevance.
- An unresolved obligation must make artifact production fail.

This is a concrete implementation gap even though no dedicated legacy issue describes it. G2 #46 is its current tracking surface; split a dedicated issue only after the graph-type boundary is decided.

### A2 — ingest and couple ecosystem facts

- Normalize package metadata while preserving Cargo/Nimble/C/C++ identity and semantics.
- Feed source/module/type/FFI and lowering results back into candidate choice.
- Reject opaque target-build execution as “resolution.”

### A3 — produce foreign and owned native artifacts

- Owned Rust/Nim lowering and native target output for the supported subset.
- Explicit C/C++ compile, adapter/instantiation, archive/shared-library, symbol, ABI, and link actions.
- Explicit runtime-capability generation or external runtime contract.

### A4 — discharge and execute

- Prove every retained obligation is discharged or externalized.
- Preserve the original graph as provenance rather than a consumer re-resolution graph.
- Execute the produced artifact in a clean environment and inspect its actual loader, symbol, resource, and provenance behavior.

### A5 — compare computation strategies

- Compare eager expansion, demand-driven resolution, early cross-layer pruning, compiler DCE, linker GC, invalidation, and reuse on the same semantics.
- Measure elapsed time, CPU, peak RSS, IR bytes, artifact size, expanded/pruned/merged/recomputed nodes, external actions, and conservative retention.
- Adopt, reject, or reformulate at least one graph representation or algorithm.

## Next outcome decisions

O1 evidence selects the next work; issue age does not.

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

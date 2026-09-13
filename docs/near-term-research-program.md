# LAMINARIA Project Progression and Near-Term Research Goal

This is the canonical starting point for LAMINARIA documentation and task selection. The [documentation map](README.md) organizes every other document downstream as a foundation, research area, work item, guide, or historical record.

## Long-term goal

LAMINARIA resolves the package, source, artifact, toolchain, ABI, and link relationships contributed by the Cargo, Nimble, C, and C++ ecosystems as one explainable dependency graph, then uses that resolution to produce an **ordinary native executable** efficiently, quickly, and with low memory use. Ultimately the same path builds LAMINARIA and its transitive dependencies.

LAMINARIA owns the supported Rust/Nim source semantics, IR, transformations, work partition/fusion, scheduler, and native target generation. Cargo, Nimble, and C/C++ ecosystem metadata, manifests, lockfiles, source acquisition, and system-library facts may be inputs. Package resolution must not be confused with opaque build scripts, compilers, or linkers launched by a package manager.

## Artifact and target priority

The current primary target is a native executable that the host OS can launch directly. A vertical slice counts only when objects, archives, shared libraries, runtimes, system libraries, and final link relationships are explicit in the graph and produce that executable.

WebAssembly is one optional future target. It is not the current goal, a required milestone, or the architectural default. Existing owned-WASM evidence remains a bounded target-generation experiment; it cannot substitute for native-binary delivery or cross-ecosystem dependency resolution.

## Artifact value

LAMINARIA does not merely solve Cargo, Nimble, C, and C++ package choices and distribute that result. It jointly resolves package obligations, source/module/type/FFI semantics, language-to-intermediate-IR lowering, artifacts, toolchains, ABIs, symbols, and links, then **discharges** each obligation at build time through specialization, lowering, code generation, static linking, embedding, or explicit externalization. The user runs the resulting native artifact without reconstructing the original package-manager, compiler, header, feature, ABI, or link-order graph.

The original dependency graph remains as derivation, reproducibility, and audit provenance, not as a runtime topology the user must resolve again. This does not claim physical independence from the OS, kernel, drivers, or every dynamic library; unavoidable requirements are externalized as explicit, verifiable runtime contracts. Pruning is an important optimization that proves some obligations irrelevant and avoids their work, but it is not the source of this artifact property.

## Current evidence and central uncertainty

The repository has bounded evidence for source-derived IR, owned validation/interpretation/transformation, Nim planning with Rust execution, incremental discovery, demand-driven execution, identity, measurement, and native/LLVM/WASM routes.

Making one fixed call path work is not evidence that a real project dependency graph can be resolved. A transitive closure contributed by package managers includes versions, features, target conditions, build dependencies, generated sources, native libraries, headers, link order, ABI, and toolchain constraints. This is a different problem from composing one semantic call.

Cross-ecosystem package resolution now has direct formal prior art in *Package Managers à la Carte*. The largest non-substitutable uncertainty is therefore narrower and deeper: whether LAMINARIA can couple those package choices with source-derived module/type/FFI facts, multi-level IR lowering, native artifacts, ABI, symbols, and link order in one demand-driven typed graph, then control candidate explosion and minimize time, peak memory, and recomputation while resolving the closure required by a native executable.

## Near-term goal

> For one fixed project containing external dependencies injected by Cargo, Nimble, C, and C++ ecosystems, incrementally couple package choices, source/semantic facts, language-to-intermediate-IR lowering, artifact/toolchain/ABI/symbol/link relationships in one typed dependency graph. Discharge, externalize, or reject every dependency obligation with evidence, and produce an ordinary native executable that users run without re-resolving the original ecosystem graph. Evaluate a method including early unreachable-work pruning by resolution time, peak memory, output size, and expanded/pruned/recomputed states against naive eager expansion.

This does not mean completing all Cargo/Nimble semantics, all C/C++ build systems, every platform, the fastest compiler, a production package manager, WASM support, distributed builds, or self-hosting. The milestone requires a correct closure, a runnable native binary, a negative case, resource measurements, and one architectural decision for a fixed realistic mixed-dependency workload.

## Near-term research program

1. **G1 — cross-ecosystem dependency graph (#8/#22/#44; related #3/#4/#5/#7/#18).** Map at least one Cargo crate, Nimble package, C library, and C++ library through the Package Calculus or document the exact semantic divergence, then connect the result to source/semantic and native-artifact constraints in a typed graph while retaining ecosystem identity. Demand-expand only the closure required by the executable. Resolve one consistent case and reject one incompatible version/feature/ABI/symbol/toolchain case before compilation.
2. **G2 — native-executable vertical slice (#6/#4/#5/#44; related #3/#10/#12/#20).** Send G1's closure through the production Nim planner and Rust runtime, execute explicit semantic-validation/lowering/compile/adapter/archive/link actions, and directly test a native executable. Require every package/source/IR/ABI/link obligation to reach `Discharged`, `Externalized`, or `Rejected` with evidence. Record every input/producer identity, executed and skipped action, final link input, and retained/pruned symbol or section. Preserve FFI exports, constructors, dynamic-retention roots, and runtime support conservatively.
3. **G3 — resolution efficiency, pruning, and incrementality (#8/#7/#11/#12; related #6/#19–#24).** Compare eager candidate/code expansion with demand-driven constraint propagation, cross-layer reachability pruning, canonicalization, memoization, equivalent-state merging, dominance pruning, and SCC condensation. Measure cold resolution, no-op, leaf-change, root-set change, and feature/target-condition changes by wall time, peak RSS, output size, package/source/IR/artifact/symbol pruning, recomputation, and avoided external-tool executions.

G1–G3 are the current milestone. Semantic fusion/splitting and the WASM target pipeline remain useful separate research, but neither is a serial gate for this milestone.

## Progression after the current milestone

1. Expand Cargo/Nimble/C/C++ semantics and native platform coverage one counterexample at a time.
2. Re-derive compiler-work partition/fusion, incremental invalidation, and resource-aware scheduling from real resolved graphs.
3. Compare memory, local-disk, peer, and remote-durable placement for the same logical graph.
4. Expand the native-executable source/dependency path toward stage0→stage1→stage2 self-hosting.
5. Evaluate WASM, shared libraries, and other outputs as optional target variants consuming the same typed graph.

## Supporting tracks

Semantic IR, target pipelines, measurement, identity, diagnostics, toolchain profiles, CI, platform compatibility, UX, and baselines are activated only as needed to make G1–G3 trustworthy. Completion of an optional target or a single compiler path cannot stand in for the dependency-resolution milestone.

## Task-selection rule

Fix the requested native artifact, retained export/dynamic roots, and host/target; enumerate the participating ecosystems and constraints; choose the smallest positive and negative graph that can falsify the largest uncertainty; measure correctness together with time, peak memory, output size, retained/pruned work, explored states, and recomputation; stop at a decision and update the graph model, algorithm, and next goal.

The current next task is therefore G1's cross-ecosystem dependency-graph experiment.

## Documents for the current decision

- Governing constraints: [compiler ownership contract](01-foundations/compiler-ownership-contract.md) and [research prioritization policy](01-foundations/research-prioritization-policy.md)
- Core research: [cross-ecosystem dependency graph research](02-research-areas/toolchains/cross-ecosystem-dependency-graph.md)
- Prior art and research gap: [cross-ecosystem dependency and compiler-IR resolution survey](02-research-areas/toolchains/cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md)
- Pruning contract: [cross-layer reachability pruning](02-research-areas/toolchains/cross-layer-reachability-pruning_ja.md)
- Artifact obligation discharge: [dependency-discharge artifact contract](02-research-areas/toolchains/dependency-resolved-artifact-closure_ja.md)
- Whole-project outcomes, capabilities, and unissued gaps: [project work portfolio](03-work-items/project-portfolio.md)
- Execution map: [research issue plan](03-work-items/issue-plan.md)
- Current experiment: [first cross-ecosystem dependency-graph experiment](03-work-items/design/cross-ecosystem-dependency-graph-first-experiment.md)
- All supporting and historical material: [documentation map](README.md)

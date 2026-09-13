# First Cross-Ecosystem Dependency-Graph Experiment

## Decision being tested

Can LAMINARIA demand-resolve a typed dependency closure spanning Cargo, Nimble, C, and C++ and use that closure to build and run one ordinary native executable without hiding target compilation inside package-manager commands?

This is G1/G2 of the current [near-term research program](../../near-term-research-program.md). A single Rust/Nim call path and an owned WASM module do not satisfy this decision.

The experiment starts from the [prior-art survey](../../02-research-areas/toolchains/cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md). It must evaluate the Package Calculus as the package-level formal baseline and record any semantic extension or divergence instead of claiming cross-ecosystem package resolution itself as novel. Spack's ASP concretizer is the implementation baseline for version/variant/compiler/architecture solving; Bazel/Buck2 are action-graph baselines; rustc/Salsa/Pluto and MLIR/ThinLTO are incremental-semantic and IR-summary baselines.

## Fixed positive graph

Use one minimal project containing:

1. one Cargo package with an explicit feature and target condition;
2. one Nimble package consumed by the program;
3. one C library supplied as source or an identified archive;
4. one C++ library requiring one explicit adapter or instantiation unit; and
5. one native executable whose observable result depends on all four inputs.

Also include one deliberately unreachable package/provider candidate, one unused source/module or translation unit, one unused semantic/IR item, one unused archive member or symbol, and one apparently unreferenced item that must remain live because of an FFI export, constructor, linker directive, or runtime contract.

The graph must preserve package/version/feature identity, host-versus-target role, source/header inputs, generated adapter provenance, toolchain/ABI constraints, artifact producers, symbols, link order, and the final executable demand.

The graph must also contain a first-class `TestContract` for the exact production executable, plus any separately identified test executable, harness executable, test data, controls, observations, and target-execution requirements. Test-only dependencies must be distinguishable from production dependencies.

## Required executable evidence

Production resolver/planner/executor tests must directly establish that:

- only the demanded closure is expanded;
- every package, source-semantic, language/intermediate-IR, ABI, symbol, and link obligation reaches `Discharged`, `Externalized`, or `Rejected`, with the production operation that justifies the transition;
- every compile, adapter, archive, and link action has explicit inputs and outputs;
- the native executable is produced and run with the expected result;
- the recorded final-link inputs account for every required foreign artifact; and
- Cargo/Nimble metadata ingestion does not silently execute an opaque target build;
- no parse/lowering/compile action runs for items proven unreachable early;
- final symbol/section evidence excludes dead code while retaining FFI, constructor, dynamic, and runtime roots; and
- every retained or pruned node has a root path or a conservative-retention/pruning reason from the production graph.
- the exact production executable digest, not only an instrumented/test-profile variant, is executed by the harness;
- the harness exercises the Rust/Nim/C/C++ observable path and checks exit/output plus required ABI/symbol/runtime behavior;
- one missing or incompatible dependency is not recovered accidentally from the host environment; and
- test-only package, symbol, hook, and runtime dependencies do not enter the release artifact.

The resulting artifact must also be exercised in a clean runtime environment without Cargo, Nimble, Rust/Nim/C/C++ compilers, project sources, or build-only dependencies. This test is evidence that the original ecosystem obligations were discharged, not merely copied into a package-manager or store closure. A self-contained or relocatable-bundle profile must run using only its declared runtime artifacts. A system-integrated profile must enumerate and preflight its external ABI/symbol/runtime contracts.

Add one negative case that makes exactly one version, feature, target, ABI, symbol, or toolchain constraint incompatible. The resolver must return a structured explanation before compilation starts.

At least one case must require feedback across layers: a source-derived module/type/FFI fact or an IR-lowering result changes or rejects a provisional package/toolchain/artifact choice. A one-way `package resolve -> compile -> link` pipeline does not test the coupled-resolution hypothesis.

## Efficiency comparison

For the same graph, compare no pruning, linker GC only, compiler DCE plus linker GC, and cross-layer early pruning plus DCE/linker GC. Record wall-clock time, peak RSS, executable size, package/source/semantic/IR/artifact/symbol nodes retained and pruned, recomputed nodes, and external actions avoided for cold, no-op, leaf-change, root-set-change, and feature/target-change scenarios. Compare performance only when observable behavior is identical.

## Stop condition

Stop after obtaining a correct positive closure, evidence that all obligations were discharged or externalized, a pre-compilation rejection, a harness-tested exact production native binary, and enough measurements to adopt, reject, or reformulate one graph representation or resolution algorithm. Do not extend the experiment to complete ecosystem coverage, WASM, distributed execution, or self-hosting.

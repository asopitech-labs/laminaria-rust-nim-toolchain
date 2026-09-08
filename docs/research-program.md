# LAMINARIA Research Program

## Purpose

LAMINARIA is a research and development project for treating Rust and Nim compilation, dependency resolution, code generation, artifacts, linking, caching, and execution as one computational system.

This document defines the research program used to turn that project direction into falsifiable engineering work. It complements `research-foundations.md`: the foundations document describes the architecture hypothesis; this document defines the research tracks, evidence, and completion criteria.

## Research policy

LAMINARIA does not count an implementation as successful merely because it builds or passes a functional test. Every research track must record the execution path actually used, the artifacts actually produced, and the resource behavior observed.

For performance-sensitive work, evidence must include wall time together with relevant CPU, memory, I/O, cache, artifact, and critical-path measurements. A fast result produced through an unintended fallback path is not evidence for the intended design. A correct result produced by a stub, bypass, hidden delegation to an opaque outer tool, or test-specific path is likewise not completion.

Reference projects are used as measurement baselines and design evidence, not as slogans. If an implementation is materially slower, more resource-hungry, or structurally further from the intended computation model than the selected reference, the implementation itself must be reconsidered.

## Track A — Compiler pipeline decomposition

### Question

Which Rust and Nim compiler stages can be represented as explicit graph nodes with stable enough inputs, outputs, and invalidation semantics to be useful outside the compiler invocation?

### Work

- inventory Rust frontend, analysis, MIR, monomorphization, codegen-unit, backend, object, archive, and link boundaries;
- inventory Nim frontend, semantic processing, backend generation, generated source, native compilation, object, archive, and link boundaries;
- classify each boundary as public/stable, observable but internal, experimentally exposable, or opaque;
- define artifact identities and producer/consumer relationships for useful boundaries;
- preserve a valid coarse-grained execution path when a fine-grained boundary is unavailable.

### Evidence

- compiler/version/target matrix;
- observed stage artifacts and dependency edges;
- invalidation experiments after controlled source changes;
- execution traces proving which stages ran;
- comparison against ordinary Cargo/rustc and Nim/Nimble execution.

## Track B — Rust/Nim native linking without a C ABI boundary

### Question

Can Rust and Nim translation units participate in one native link with a direct, explicitly modeled cross-language contract instead of first reducing the boundary to a conventional exported C ABI surface?

This does not assume that every Rust or Nim language feature can cross the boundary directly. The research must separate what can be represented natively from what still requires an adapter, generated shim, or restricted contract.

The dedicated design and experiment plan is in `rust-nim-native-linking.md`.

## Track C — Backend Graph

### Question

Can backend choice be represented as a constrained graph variant rather than as a fixed property of the source language toolchain?

### Work

- model LLVM, Cranelift, GCC-family Rust codegen routes where available;
- model Nim C/C++/Objective-C/JavaScript backend families;
- model downstream native compiler and linker requirements;
- include target, optimization, ABI, debug-info, artifact and linker compatibility constraints;
- measure backend-switch invalidation and cache reuse.

## Track D — Unified Action Graph and scheduling

### Question

Can Rust codegen work, Nim-generated native compilation, binding/shim generation, object generation, archive creation, and linking share one resource-aware scheduler without nested tool schedulers competing for the same machine?

### Evidence

- explicit ready/running/blocked action state;
- CPU, peak memory and I/O accounting per action class;
- critical-path calculation before and after scheduling decisions;
- proof that the selected actions actually ran through the unified scheduler;
- comparison with nested Cargo + Nim build execution on the same workload.

## Track E — Artifact identity, incremental invalidation and CAS

### Question

Can semantic, generated-source, backend, object and final-artifact identities be normalized so equivalent work can be reused across worktrees, CI checkouts and compatible machines?

### Work

- define semantic versus machine artifact identity;
- exclude physical checkout paths from identity where not semantically relevant;
- measure invalidation after source, feature, backend, compiler and linker changes;
- record cache hits together with the exact identity explanation;
- reject cache reuse when toolchain or configuration compatibility is not provable.

## Track F — Variant-space control

### Question

Can target, profile, features, host/target role, backend, compiler, linker, artifact kind and cross-language variants be expanded on demand without materializing their Cartesian product?

### Work

Use constraint propagation, canonicalization, memoization, equivalent-state merging, SCC condensation, demand propagation and pruning. Measure explored, merged and rejected states.

## Track G — WASM

### Question

What advantages become possible when Rust and Nim compilation are planned as one graph for WebAssembly targets, especially when the integration is not constrained to a conventional C ABI boundary?

### Work

- distinguish single-module, multi-module/component, and native-link-equivalent WASM production paths;
- identify which cross-language artifacts can be combined before final module production;
- measure boundary overhead, generated shims, code size, duplicate runtime support, linker behavior and cache identity;
- do not claim support from theoretical compiler flags alone: produce and execute artifacts.

## Track H — Explainability for agents and humans

Every important graph decision must have a structured explanation. At minimum LAMINARIA should be able to explain:

- why a dependency or variant was selected;
- why an action rebuilt;
- why an artifact was reused or rejected from cache;
- why a backend/linker combination was accepted or rejected;
- which action is on the critical path;
- which compiler stage was opaque and therefore executed coarsely.

## Evaluation workloads

The research suite must contain workloads that isolate different graph properties:

1. Rust-heavy workspace;
2. Nim-heavy generated-native-source workspace;
3. mixed Rust/Nim native executable;
4. direct native-link boundary workload;
5. conventional C-ABI baseline workload;
6. backend-variant workload;
7. wide parallel graph;
8. deep critical-path graph;
9. FFI/boundary-heavy graph;
10. incremental semantic edit workload;
11. worktree reuse workload;
12. WASM mixed-language workload.

## Required metrics

The exact metrics vary by track, but the evaluation framework must be able to capture:

- graph construction time;
- node/edge counts;
- explored/pruned/merged variant states;
- requested-artifact critical-path duration;
- action wall time;
- CPU time/utilization;
- peak and time-weighted memory;
- relevant I/O volume and wait;
- generated source/object/archive/module sizes;
- semantic/codegen/object/final-artifact reuse;
- cache-hit and cache-miss reasons;
- invalidation set size;
- linker inputs and selected symbols;
- fallback/delegation path usage;
- reference-baseline ratio.

## Completion rule

A research issue is complete only when its claim can be reproduced from committed code, commands, fixtures and evidence. Passing tests alone is insufficient where the issue is about architecture, scheduling, performance, resource use, compiler boundaries, linking, or cache behavior.

Unexpected results are valid research outcomes. If a hypothesis fails, record why it failed and adjust the architecture or research direction instead of preserving the original claim through a weaker benchmark or a hidden fallback.

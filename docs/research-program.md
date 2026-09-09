# LAMINARIA Research Program

## Purpose

LAMINARIA is a research and development project for treating Rust and Nim compilation, dependency resolution, code generation, artifacts, linking, caching, and execution as one computational system.

This document defines the research program used to turn that project direction into falsifiable engineering work. It complements `research-foundations.md`: the foundations document describes the architecture hypothesis; this document defines the research tracks, evidence, and completion criteria. Detailed backend-internal research is defined in `backend-pipeline-whiteboxing.md`.

## Research policy

LAMINARIA does not count an implementation as successful merely because it builds or passes a functional test. Every research track must record the execution path actually used, the artifacts actually produced, and the resource behavior observed.

Functional correctness and execution correctness are separate requirements. A correct final artifact produced by rebuilding too much work, using the wrong backend/scheduler path, or silently delegating to an opaque outer tool or backend does not prove an incremental, scheduling, native-linking, backend-white-boxing or work-elimination claim.

For performance-sensitive work, evidence must include wall time together with relevant CPU, memory, I/O, cache, artifact, critical-path and executed/skipped-action measurements. A fast result produced through an unintended fallback path is not evidence for the intended design.

Reference projects are used as measurement baselines and design evidence, not as slogans. If an implementation is materially slower, more resource-hungry, performs materially more work, or is structurally further from the intended computation model than the selected reference, the implementation itself must be reconsidered.

LAMINARIA prefers optimization in this order:

1. eliminate unnecessary compiler/backend actions or stages;
2. reuse already-valid artifacts;
3. reduce invalidation scope;
4. expose useful parallelism;
5. schedule globally under resource constraints;
6. optimize individual remaining actions.

Parallelizing work that should not have executed is not equivalent to eliminating it.

## Track A — Compiler pipeline decomposition

### Question

Which Rust and Nim compiler stages can be represented as explicit graph nodes with stable enough inputs, outputs, and invalidation semantics to be useful outside the compiler invocation?

### Work

- inventory Rust frontend, analysis, MIR, monomorphization, codegen-unit, backend-handoff, object, archive, and link boundaries;
- inventory Nim frontend, semantic processing, backend generation, generated source, native compilation, backend-handoff, object, archive, and link boundaries;
- classify each boundary as public/stable, observable but internal, experimentally exposable, or opaque;
- define artifact identities and producer/consumer relationships for useful boundaries;
- preserve a valid coarse-grained execution path when a fine-grained boundary is unavailable.

## Track B — Rust/Nim native linking without a mandatory C ABI boundary

Can Rust and Nim translation units participate in one native link with a direct, explicitly modeled cross-language contract instead of first reducing the boundary to a conventional exported C ABI surface?

This does not assume that every Rust or Nim language feature can cross the boundary directly. The dedicated design and experiment plan is in `rust-nim-native-linking.md`.

## Track C — Backend Route and Backend Pipeline Graph

Backend handling is split into two different problems:

```text
Backend Route Selection
  ↓
Backend Pipeline Expansion
```

### C1 — Backend Route Selection

Model LLVM, Cranelift, GCC-family Rust codegen routes and Nim C/C++/Objective-C/JavaScript families as constrained variants. Include target, optimization, ABI, debug-info, native compiler, LTO mode, artifact and linker compatibility.

### C2 — Backend Pipeline White-boxing

Do not collapse a selected backend back into one opaque action. Project lowering, optimization, LTO, target codegen, linking and post-link work into a backend-specific nested graph.

Distinguish:

1. Logical Stage;
2. Observation Boundary;
3. Checkpoint / Artifact Boundary;
4. Execution Boundary;
5. Dynamic Graph Expansion Point.

White-boxing does not mean one process per compiler pass. LLVM New Pass Manager and similar systems intentionally group work to preserve analysis state, cache locality and optimization quality. Checkpoints are therefore selected using measured economics:

```text
benefit = eliminated work + reuse + reduced invalidation + scheduling/distribution gain
cost = serialization + reload + hashing/I/O + process/IPC + lost analysis/locality + optimization risk
```

See `backend-pipeline-whiteboxing.md` and issues #13, #14 and #15.

## Track D — Unified Action Graph and scheduling

Can Rust codegen work, Nim-generated native compilation, backend jobs, binding/shim generation, object generation, archive creation, linking and post-link work share one resource-aware scheduler without nested tool schedulers competing for the same machine?

Track queue wait, dependency/resource wait, execution time, CPU, memory and I/O. Where backend jobs are discovered dynamically, as with DTLTO, investigate explicit dynamic graph expansion rather than hidden nested scheduling.

Horizontal distribution is a first-class research subject across the scheduler, cache, backend and LLVM-rediscovery tracks. Compare Kbuild-shaped object distribution, LLVM ThinLTO/DTLTO backend distribution and action-level remote execution without assuming that any of their partition units is the canonical LAMINARIA semantic partition. See `horizontal-distribution-research.md` and `horizontal-distribution-research_ja.md`.

## Track E — Artifact identity, incremental invalidation and CAS

Define identities for semantic artifacts, generated source, backend IR/bitcode, LTO indexes, backend outputs, native objects, Core Wasm, optimized Wasm, components and final artifacts.

Exclude physical checkout paths where not semantically relevant. Backend checkpoint identity must include relevant producer/toolchain information such as LLVM/backend version, target/data layout/features, optimization/pass pipeline, LTO mode, profile input, debug configuration and plugins.

Distinguish artifact reuse from work elimination/no-op behavior.

## Track F — Variant-space control

Avoid eagerly materializing:

`target × profile × features × host/target role × backend route × backend pipeline mode × compiler × linker × post-link optimizer × composition model × artifact kind × cross-language boundary`

Use constraint propagation, canonicalization, memoization, equivalent-state merging, SCC condensation, demand propagation and pruning.

## Track G — WebAssembly Target Pipeline

WebAssembly is not modeled as a peer backend value to LLVM or Cranelift. Separate:

```text
Backend Engine
× Target ISA / Object Model
× Link Model
× Post-link Optimizer
× Composition Model
```

A representative LLVM path is:

```text
LLVM IR
→ LLVM optimization
→ WebAssembly target codegen
→ relocatable Wasm object
→ wasm-ld
→ Core Wasm module
→ Binaryen / wasm-opt
→ optimized Core Wasm module
→ WIT metadata / adapters
→ componentization
→ WebAssembly Component
```

`wasm-ld`, Binaryen, WIT embedding/adaptation and componentization must remain visible as independent candidate stages/artifacts. Binaryen pass visibility follows the same rule as LLVM: observe pass structure without assuming per-pass external processes.

Support claims require actually generated and executed artifacts. See #9 and #16.

## Track H — Explainability for agents and humans

LAMINARIA must explain, in structured form:

- dependency and variant choices;
- rebuild causes and cache identity differences;
- skipped/eliminated compiler/backend stages;
- backend route and nested pipeline expansion;
- logical versus observable versus checkpoint versus execution boundaries;
- dynamic child actions created during graph expansion;
- backend/linker/post-link combination choices;
- critical-path contribution and queue/dependency/resource/execution delay;
- opaque/coarse fallback use.

## Track I — Work elimination and no-op invariants

Apply work elimination at compiler and backend-stage granularity. Test direct artifact handoff, backend checkpoint reuse, partial ThinLTO backend invalidation, skipped `wasm-ld`/Binaryen/componentization work where inputs permit, and unchanged/no-op behavior.

When source content, relevant configuration, toolchain identity and compatible environment inputs are unchanged, compiler/codegen/backend/link/post-link execution actions should be zero unless an explicitly documented environment-sensitive action requires otherwise.

A 100% cache-hit statistic is insufficient if no-op evaluation still performs substantial hashing, I/O, graph traversal or process startup.

## Track J — Cross-language LLVM / LTO convergence

Evaluate Rust, Nim 2 and Nim 3/Nimony LLVM routes separately rather than collapsing them into a generic `Rust/Nim → LLVM` path.

Candidate paths:

```text
Rust → rustc LLVM bitcode
Nim 2 → nlvm → LLVM IR
Nim 2 → generated C → Clang → LLVM IR/bitcode
Nim 3/Nimony → Leng/lengc → LLVM IR
```

Test shared LTO/ThinLTO participation separately from language ABI/runtime compatibility. Verify target triple, data layout, symbol visibility, calling convention, runtime initialization, allocator, panic/exception behavior, TLS and ownership. Link compatibility is not evidence of cross-language inlining or semantic ABI compatibility. See #17.

## Evaluation workloads

The research suite should include:

1. Rust-heavy workspace;
2. Nim-heavy generated-native-source workspace;
3. mixed Rust/Nim native executable;
4. direct native-link boundary workload;
5. conventional C-ABI baseline;
6. backend-route variant workload;
7. backend checkpoint-economics workload;
8. LLVM pass/pipeline observation workload;
9. ThinLTO/DTLTO dynamic backend-job workload;
10. wide parallel graph;
11. deep critical-path graph;
12. boundary-heavy graph;
13. incremental semantic edit workload;
14. unchanged/no-op workload;
15. worktree reuse workload;
16. compiler/backend work-elimination fixture;
17. mixed-language WebAssembly workload;
18. Wasm link/post-link/component invalidation workload;
19. Rust/Nim 2/Nimony shared LLVM/LTO compatibility workload.

## Required metrics

The evaluation framework must be able to capture, as applicable:

- graph construction time and node/edge counts;
- logical/observation/checkpoint/execution boundary counts;
- dynamic graph-expansion time and child-action counts;
- explored/pruned/merged variant states;
- critical-path duration;
- action wall time and queue/dependency/resource wait;
- CPU time/utilization;
- peak and time-weighted memory;
- I/O volume and wait;
- bytes serialized/deserialized/hashed/read/written;
- generated source/IR/bitcode/object/archive/module/component sizes;
- semantic/codegen/backend/object/final-artifact reuse;
- executed/skipped compiler/backend stage counts;
- LLVM pass-group timing and optimization remarks where available;
- ThinLTO backend-job count, indexes and invalidation set;
- linker inputs and symbols;
- Binaryen pass timing/module metrics where available;
- WIT/adaptation/componentization artifacts and timing;
- no-op metadata/hash/read/process-launch overhead;
- cache-hit/miss reasons;
- fallback/delegation path usage;
- reference-baseline ratio;
- checkpoint benefit versus checkpoint cost.

## Completion rule

A research issue is complete only when its claim can be reproduced from committed code, commands, fixtures and evidence. Passing tests alone is insufficient where the issue is about architecture, scheduling, performance, resource use, compiler/backend boundaries, linking, cache behavior, incremental execution or work elimination.

Backend white-boxing is not complete when internals are merely visualized. At least one checkpoint must demonstrate measured work-elimination/reuse value, and at least one overly fine candidate boundary must be measured and rejected when its overhead or optimization damage exceeds its benefit.

Controlled incremental tests should validate the expected execution set as well as the final artifact. Unexpected results are valid research outcomes and should change the implementation or hypothesis rather than weaken the benchmark or hide fallback behavior.

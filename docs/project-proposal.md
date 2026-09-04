# LAMINARIA

## Rust Nim Unified Toolchain

### Research and development of a unified toolchain that treats the compiler pipelines, dependencies, artifacts, and execution plans of Rust and Nim as a single computational graph

---

## 1. Project Overview

LAMINARIA is a research and development project that treats software development in Rust and Nim not as a collection of build systems separated by language, but as a single, unified computational system.

The scope of research is not limited to unifying package management or build commands. It decomposes the compiler pipelines of Rust and Nim themselves, and treats the following computational process as a common graph composed of dependencies, artifacts, constraints, and actions.

```text
Source
↓
Semantic Analysis
↓
Language IR
↓
Specialization / Transformation
↓
Code Generation
↓
Backend
↓
Machine Artifact
↓
Archive / Link
```

LAMINARIA builds a multi-level model — **Package Graph → Program Graph → Variant Graph → Artifact Graph → Action Graph** — and performs dependency resolution, combinatorial search, incremental computation, cache identity, critical-path analysis, and resource-aware scheduling on top of it.

LAMINARIA itself is implemented in Rust and Nim. Rust is responsible for the CLI, application logic, OS interaction, process execution, caching, storage, and runtime scheduling, while Nim is responsible for the planning kernel: graph resolution, constraint propagation, variant exploration, graph transformation, critical-path analysis, and combinatorial optimization. LAMINARIA itself serves as the reference implementation of the Rust+Nim unified compiler/build architecture that is the subject of this research.

## 2. Background

When Rust and Nim are used together in the same project, multiple independent computational systems actually exist.

```text
Cargo dependency resolution Nimble dependency resolution
Rust compiler pipeline Nim compiler pipeline
Rust codegen backend Nim backend generation
C / C++ compiler and linker FFI and binding generation
compiler cache / build cache CI scheduler
```

Typical build orchestration connects these as opaque commands such as `cargo build`, `nimble build`, `nim c`, `clang`, and `link`. However, inside each command there is further dependency structure and parallelism.

Conceptually, Rust has a pipeline of parsing / expansion, HIR, type analysis, MIR, MIR analysis / optimization, monomorphization, codegen units, codegen backend, object files, and archive / link. Nim also has a transformation chain of semantic processing, backend generation, C / C++ / Objective-C / JavaScript, native compilation, object files, and archive / link.

If a per-language build command is treated as the unit of execution, LAMINARIA cannot make use of this internal parallelism and cross-language dependency structure. LAMINARIA extends the scope of unification to include the internal boundaries within the compilers themselves.

## 3. Central Problem

The central question of LAMINARIA is whether the dependency semantics, compiler pipeline, backend, and artifact generation that Rust and Nim each independently possess can be reconstructed, without losing semantic information, into a single computational graph.

Furthermore, it examines whether making that graph sufficiently fine-grained can enable incremental computation, caching, and scheduling that cross language boundaries and compiler boundaries. `cargo build` and `nim c` are not treated as the final unit of computation, but as an entry point for discovering a finer-grained graph.

## 4. Research Goals

### 4.1 Unified Program Graph

Integrates not just package dependencies but the program structure recognized by the compiler into a common graph. This covers packages, crates, Nim modules, targets, features, generic instances, generated sources, codegen units, FFI artifacts, metadata, objects, and archives. Rather than directly unifying the internal representations of Rust and Nim, each compiler's representation is mapped onto LAMINARIA's common semantic model.

### 4.2 Compiler Pipeline Decomposition

Rather than treating a single compiler invocation as one Action, its internal pipeline is modeled as multiple computational stages.

```text
Rust: Frontend → HIR / semantic representation → MIR → MIR transformation
→ monomorphization collection → codegen units → backend lowering
→ backend optimization → object generation → link

Nim: Frontend → semantic representation → backend transformation
→ generated C / C++ / Objective-C / JavaScript → native compiler
→ object generation → link
```

Both are expanded onto a common Action Graph.

### 4.3 Backend-Agnostic Compilation Model

Rather than treating LLVM as a fixed, Rust-only backend, the backend itself is modeled as an independent graph primitive.

```text
Language IR → Backend Action → Backend IR / Machine Artifact
```

For Rust, this covers the LLVM, Cranelift, and GCC backends operating on the MIR / monomorphized program; for Nim, it covers the C, C++, Objective-C, and JavaScript backend family operating on the semantic program. Backend selection becomes a subject of variant resolution, treating `language × target × optimization × backend × native compiler × linker` as a unified constraint problem.

### 4.4 Unified Action Graph

The common Action can express the following:

```text
RustFrontend / RustAnalysis / RustMIR / RustMonomorphization / RustCodegenUnit
NimFrontend / NimAnalysis / NimBackendGeneration
BackendLowering / BackendOptimization
CCompile / CppCompile / ObjectGeneration
BindingGeneration / Archive / Link / Test / CodeGeneration
```

The language name is not an attribute that partitions the scheduler queue, but metadata that describes the meaning of that Action.

### 4.5 Codegen Unit Scheduling

This studies whether codegen units internal to the Rust compiler and native-source compilation generated by Nim can be treated as the same scheduling problem.

```text
Rust CGU A ─── LLVM ─────→ A.o
Rust CGU B ─── Cranelift → B.o
Nim C unit C ─ clang ────→ C.o
Nim C unit D ─ clang ────→ D.o
```

The goal is not to maximize separate parallelism, but parallel execution that minimizes the critical path to the final artifact.

### 4.6 Combinatorial Graph Resolution

This addresses the state space `Package × Target × Profile × Feature × Generic Instance × Host/Target × Backend × Native Compiler × Artifact Type × FFI Configuration`. Rather than generating the full Cartesian product of states, only the necessary graph is generated through lazy expansion, constraint propagation, canonicalization, memoization, equivalent-state merging, dominance pruning, SCC condensation, demand-driven artifact resolution, and incremental recomputation. This planning kernel is implemented in Nim.

### 4.7 Artifact-Oriented Dependency Model

Dependencies are treated not only as edges between packages, but in the following form:

```text
Producer Action → Artifact → Consumer Action
```

Artifacts include Rust metadata, MIR-related compiler metadata, object files, LLVM bitcode, generated C / C++, C headers, static archives, dynamic libraries, and executables. A distinction is drawn between the artifacts needed to understand dependencies and the artifacts needed to generate machine code.

### 4.8 Semantic Build / Check Separation

The compiler pipeline is decomposed to separate semantic correctness from machine artifact generation. `check`-style operations compute dependency resolution, semantic analysis, type checking, and FFI compatibility analysis, constructing an execution graph that does not require machine code generation. Build / check / test are expressed not as separate command implementations, but as different artifact demands.

### 4.9 FFI as a Graph Primitive

FFI between Rust and Nim is treated not as a side effect of an external build script, but as a first-class relationship on the Artifact Graph.

```text
Rust semantic representation → C ABI surface → header / binding representation → Nim consumer
Nim exported representation → C ABI surface → header / binding representation → Rust consumer
```

ABI invalidation, binding regeneration, rebuild propagation, compatibility checking, and cache invalidation are integrated into ordinary graph operations.

### 4.10 Cross-Language Critical Path Scheduling

The scheduler's objective is not maximum CPU utilization, but **minimizing the wall-clock time until the requested artifact is complete**. Planning draws on graph dependencies, estimated action duration, CPU / memory requirements, IO characteristics, backend cost, cache-hit probability, critical path, and artifact availability. The Nim planning kernel analyzes the global graph, and the Rust runtime scheduler carries out execution using the machine's actual resource state.

### 4.11 Incremental Compiler Graph

Rather than invalidating an entire package on a file change, this studies a model that can track the following propagation:

```text
Changed source → Affected semantic node → Affected specialization
→ Affected codegen unit → Affected backend action → Affected object → Affected final artifact
```

Incrementality is treated as three layers: workspace incrementality, compiler incrementality, and artifact incrementality.

### 4.12 Unified Cache Identity

Content identity is assigned not just per Action, but to each stage of the compiler pipeline.

```text
Identity = operation + semantic inputs + relevant configuration
+ toolchain identity + dependency artifacts
```

Using identity that is independent of the physical workspace path or worktree path, this studies artifact reuse across repositories, branches, worktrees, CI checkouts, and machines.

### 4.13 Agent-Oriented Compiler Toolchain

This studies whether AI coding agents can directly query the internal state of the compiler/build system.

```text
laminaria dependency-graph laminaria program-graph
laminaria action-graph laminaria compiler-pipeline
laminaria codegen-units laminaria critical-path
laminaria explain-dependency laminaria explain-rebuild
laminaria explain-codegen laminaria explain-backend-selection
laminaria explain-cache-miss
```

Rather than having the agent infer compiler output, the compiler/build graph itself is made observable.

## 5. The Roles of Rust and Nim

### Nim Planning Kernel

Responsible for graph construction, normalization, variant resolution, constraint solving, artifact demand propagation, SCC decomposition, lazy expansion, state merging, pruning, critical-path computation, and planning optimization. This is not confined to Nim-related processing; it handles the overall LAMINARIA computation problem, including the Rust compiler graph.

### Rust Runtime

Responsible for the CLI, application logic, compiler/tool discovery, filesystem, process lifecycle, async execution, resource accounting, cache / CAS, daemon, IPC, sandboxed execution, native process scheduling, and diagnostics transport. This is not confined to Rust-related processing; it manages the execution infrastructure, including the Nim compiler and LLVM.

## 6. LAMINARIA Compiler Topology

```text
Source Graph
├─ Rust Frontend → Rust Semantic IR → MIR / Specialization ┐
└─ Nim Frontend → Nim Semantic IR → Transformation ├→ Backend Graph
│ ├─ LLVM
│ ├─ Cranelift
│ ├─ GCC
│ ├─ C / C++
│ └─ JS
↓
Artifact Graph
↓
Objects / Metadata / Archive / Link
↓
Final Artifact
```

This entire graph is handled by a single planner and scheduler.

## 7. Points of Comparison

| Subject | Points of Comparison |
| --- | --- |
| Bun | unified developer interface, toolchain ownership of package/build/test/run |
| Cargo / rustc | dependency semantics, feature/target resolution, unit graph, query model, MIR, monomorphization, codegen units, backend abstraction, metadata / rlib |
| `rustc_codegen_ssa` / LLVM / Cranelift / GCC | backend abstraction, MIR lowering, codegen interface, backend-specific optimization, machine artifact generation |
| Nim compiler | semantic pipeline, C/C++/Objective-C/JavaScript backend, generated source, native compiler integration, nimcache, compile/link boundary |
| Buck2 | Action Graph, critical path, action digest, CAS, local/remote execution, incremental daemon architecture |
| Bazel | explicit action semantics, hermetic execution, remote execution, content-addressed artifacts |
| Pants | dependency inference, fine-grained invalidation, source-level graph |
| Nx | project graph, task graph, affected analysis |
| sccache | compiler invocation cache, Rust/C/C++ reuse |

What distinguishes LAMINARIA is not merely operating the compiler from above the build system, but pulling the compiler's internal semantic/codegen boundary up into the build graph itself.

## 8. Research Hypotheses

- **Hypothesis A:** A package/task graph alone is too coarse for cross-language optimization; exposing the internal compiler pipeline as an Action Graph yields new parallelism and cache reuse.
- **Hypothesis B:** Rather than fixing LLVM as a Rust-only backend, treating backend selection as a variant on the graph generalizes the compiler/toolchain architecture.
- **Hypothesis C:** Placing Rust codegen units and Nim generated-native-source compilation on the same scheduler achieves a shorter global critical path than nested parallelism.
- **Hypothesis D:** Separating semantic artifacts from machine artifacts unifies check, build, test, and other operations as different artifact demands.
- **Hypothesis E:** Treating FFI as a graph primitive integrates incremental invalidation across language boundaries into ordinary dependency propagation.
- **Hypothesis F:** Content identity at the granularity of compiler stages enables artifact reuse finer-grained than at the crate/package level.
- **Hypothesis G:** Exploring the combinatorial state space demand-driven, rather than pre-generating it, controls variant explosion.
- **Hypothesis H:** Exposing the compiler graph as a structured interface lets AI agents directly analyze build failures, cache misses, backend selection, and the critical path.

## 9. Evaluation Workloads

- **Rust-heavy Compiler Graph:** a configuration with many crates, generic specialization, and multiple codegen units.
- **Nim-heavy Backend Graph:** a configuration with many Nim modules and a large volume of generated C/C++.
- **Mixed Codegen Graph:** a configuration where Rust codegen units and Nim-generated native compilation coexist.
- **Backend Variant Workload:** a configuration involving backend differences such as LLVM / Cranelift.
- **FFI-heavy Graph:** a configuration with multiple ABI boundaries between Rust and Nim.
- **Deep Critical Path / Wide Compiler Graph:** a configuration with a long artifact chain or many independent codegen actions.
- **Variant-heavy Graph:** a configuration with many combinations of feature, target, backend, artifact type, and so on.
- **Incremental Semantic Change:** a configuration involving semantic changes that do not affect the final machine artifact.
- **Git Worktree Workload:** builds across multiple worktrees sharing the same source history.

## 10. Evaluation Metrics

- graph construction cost, graph node / edge growth, variant exploration count
- incremental invalidation range, semantic / codegen / object cache reuse
- critical-path duration, CPU utilization, peak memory, backend switching cost
- FFI invalidation precision, worktree cache reuse, explanation completeness

Rather than a simple full-build benchmark alone, the primary metric is **how much computation could be skipped**.

## 11. Positioning within the Research Landscape

LAMINARIA connects Bun's unified toolchain, Cargo / rustc's language-aware compiler semantics, Nim's explicit multi-backend compilation pipeline, Buck2's Action Graph / execution, Bazel's artifact/action identity, Pants' dependency inference, Nx's affected-graph analysis, and sccache's compiler cache to the concrete subject of a Rust + Nim compiler pipeline.

## 12. Ultimate Research Goal

What LAMINARIA aims for is not a collection of independent processes — Rust build, Nim build, C compilation, LLVM codegen, linking, and FFI generation. It reduces these to a single graph as combinations of **Input → Transformation → Artifact → Dependency**.

Ultimately, LAMINARIA studies a state in which it can answer, from a single computational model: what needs to be computed, what already exists, which semantic information has changed, which specializations are affected, what backend work is required, what can be executed in parallel, what is blocking the requested artifact, and why an Action was executed.

## Project Statement

**LAMINARIA — Rust Nim Unified Toolchain**

LAMINARIA researches and implements a unified computational model for Rust and Nim toolchains, decomposing language frontends, semantic stages, code generation, compiler backends, artifacts, caching, and execution into a single explainable action graph for cross-language planning and resource-aware scheduling.

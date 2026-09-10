# LAMINARIA

## Ownership correction (2026-09-10)

The [compiler ownership contract](compiler-ownership-contract.md) governs research objectives and acceptance.

Own compiler/IR/scheduler development is the main path, not optional later integration. Cargo/Nim ecosystem tools may resolve dependencies; existing compilation routes below are reference/observation or external-bootstrap baselines, not target-build alternatives. The Action Graph is not a substitute for a language IR.

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

### 4.1 Unified Program Graph and owned IR

LAMINARIA itself processes Rust/Nim source and implements representations for types, values, control/data dependencies, ownership, effects, overflow and runtime obligations with provenance. This is not merely mapping each existing compiler's output into planning metadata. Package/artifact relationships do not replace program semantics.

### 4.2 Compiler Pipeline Decomposition

Existing rustc/Nim stage maps are reference observations and information-loss probes. Derive and implement the required analysis, transformation and invalidation boundaries from language semantics. MIR, CGUs and Nim-generated C do not define the default partition.

```text
Rust source / Nim source / both
  → LAMINARIA source processing + semantic facts
  → LAMINARIA-owned IR(s) + provenance
  → legal analysis / transformation / specialization
  → demand-driven partition and resource plan
  → LAMINARIA target lowering / code generation
  → target artifacts + explicit runtime/link contract
```

### 4.3 Backend-Agnostic Compilation Model

Research LAMINARIA-owned target lowering and code generation. LLVM/Cranelift/GCC and Nim C/JS routes are comparison paths, not substitutes for the compiler. Represent target, ABI, runtime and optimization contracts and justify architectural decisions with implementations and evidence.

### 4.4 Unified Action Graph

Represent owned semantic processing, analysis dependency updates, legality checks, transformations, specialization, target generation and artifact retention/transfer/recomputation. Actions need not be external processes or separate language queues. An ordered command list is not a compiler IR.

### 4.5 Compiler-work Scheduling

Study which IR computations remain integrated within one process/memory space and which become parallel or remote work. Consider semantic dependencies, analysis state, critical path, core/cache/NUMA topology, memory bandwidth/capacity and I/O/network cost together. Scheduling existing Rust CGUs and Nim C units remains a comparison baseline.

### 4.6 Combinatorial Graph Resolution

This addresses the state space `Package × Target × Profile × Feature × Generic Instance × Host/Target × Backend × Native Compiler × Artifact Type × FFI Configuration`. Rather than generating the full Cartesian product of states, only the necessary graph is generated through lazy expansion, constraint propagation, canonicalization, memoization, equivalent-state merging, dominance pruning, SCC condensation, demand-driven artifact resolution, and incremental recomputation. This planning kernel is implemented in Nim.

### 4.7 Artifact-Oriented Dependency Model

Dependencies are treated not only as edges between packages, but in the following form:

```text
Producer Action → Artifact → Consumer Action
```

Independent-path artifacts include LAMINARIA semantic, analysis, transformation and target representations with their identities. Reference paths also record Rust metadata/MIR, LLVM bitcode and Nim-generated C, without treating them as interchangeable with the owned IR.

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

The Rust runtime owns CLI, OS interaction, resource accounting, storage, IPC, diagnostics and execution of LAMINARIA compiler computations. External process execution is a separate reference/bootstrap role, not the target compilation engine.

## 6. LAMINARIA Compiler Topology

```text
Rust source / Nim source / both
  → LAMINARIA source processing + semantic facts
  → LAMINARIA-owned IR(s) + provenance
  → legal analysis / transformation / specialization
  → demand-driven partition and resource plan
  → LAMINARIA target lowering / code generation
  → target artifacts + explicit runtime/link contract
```

The Nim planner and Rust runtime scheduler handle compiler work on this path. A build graph without owned source/IR processing is bootstrap/reference infrastructure. Rust-only/Nim-only input does not change ownership.

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

- **Hypothesis A:** An owned IR can preserve semantic dependencies absent from package/task graphs and derive legal analysis, transformation, reuse and parallelism.
- **Hypothesis B:** Rather than fixing LLVM as a Rust-only backend, treating backend selection as a variant on the graph generalizes the compiler/toolchain architecture.
- **Hypothesis C:** Resource-aware grouping and partitioning of owned compiler work may reduce critical path or resource use relative to nested existing-compiler schedules; test rather than assume that benefit.
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

LAMINARIA researches and implements owned semantic analysis, IR, transformations, code generation, planning and execution for Rust/Nim. Existing compilers serve separate reference/bootstrap roles; the goal includes independently compiling its own Rust + Nim implementation.

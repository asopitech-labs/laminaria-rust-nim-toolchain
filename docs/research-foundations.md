# LAMINARIA Research Foundations

## Rust Nim Unified Toolchain

### Status

This document consolidates the design research that established LAMINARIA's current direction. It is a research agenda and architecture hypothesis, not a claim that every described compiler boundary is already available through a stable public API.

LAMINARIA studies whether Rust and Nim development can be represented as one explainable computational graph spanning dependency resolution, compiler stages, backend selection, artifacts, foreign-function boundaries, caching, and execution.

The project statement is:

> LAMINARIA researches and implements a unified computational model for Rust and Nim toolchains, decomposing language frontends, semantic stages, code generation, compiler backends, artifacts, caching, and execution into a single explainable action graph for cross-language planning and resource-aware scheduling.

## 1. Motivation

Projects that combine Rust and Nim repeatedly reconstruct the same infrastructure:

- separate Cargo and Nimble dependency resolution;
- custom build scripts for Rust-to-Nim and Nim-to-Rust integration;
- generated C or C++ compilation;
- header and binding generation;
- coordination between `cargo`, `rustc`, Nim, native compilers, and linkers;
- duplicated local and CI cache configuration;
- nested parallelism across independently scheduled tools;
- workspace-specific toolchain discovery and diagnostics; and
- one-off explanations for why a target rebuilt or failed.

The problem grows with every additional mixed-language repository. The same dependency, FFI, scheduling, and cache logic is implemented repeatedly, while each language tool continues to make local decisions without visibility into the other language's work.

LAMINARIA treats this as an infrastructure problem rather than a collection of project-specific scripts.

## 2. Core thesis

LAMINARIA is not intended to be a thin command wrapper around `cargo build` and `nimble build`. An outer task runner cannot fully coordinate tools that each own an internal dependency graph, compiler pipeline, and parallel scheduler.

The central thesis is that useful cross-language optimization requires progressively exposing the work hidden behind language-level build commands:

```text
Source Graph
    ↓
Compiler Pipeline
    ↓
Unified Program Graph
    ↓
Variant Graph
    ↓
Artifact Graph
    ↓
Action Graph
    ↓
Nim Planning Kernel
    ↓
Rust Runtime Scheduler
```

This model separates five concerns that are often collapsed into a single build invocation:

1. what source and package entities exist;
2. which semantic and compiler transformations are required;
3. which variants and artifacts are demanded;
4. which executable actions can produce those artifacts; and
5. how those actions should run on the current machine.

## 3. Research questions

LAMINARIA is organized around the following questions:

1. Can Rust and Nim compiler pipelines be projected into a shared graph without discarding language-specific semantics?
2. What is the minimum stable contract between compiler analysis, artifact planning, and execution?
3. Can backend choice be modeled as a graph variant rather than a fixed property of a language toolchain?
4. Can Rust codegen units and Nim-generated native compilation participate in one resource-aware schedule?
5. Can FFI generation and ABI validation become ordinary graph dependencies with precise invalidation?
6. Can content identity be defined at compiler-stage and artifact boundaries so results can be reused across worktrees, CI checkouts, and machines?
7. Can demand-driven expansion control the combinatorial space of targets, profiles, features, backends, host/target roles, artifact kinds, and FFI variants?
8. Can the resulting system explain dependency choice, rebuilds, backend selection, cache misses, and critical paths to both humans and software agents?

## 4. Graph hierarchy

LAMINARIA does not use one undifferentiated graph. Each layer answers a different question.

### 4.1 Source Graph

The Source Graph records packages, crates, Nim modules, local workspaces, generated sources, and cross-language source relationships. It preserves the identity of the originating ecosystem instead of forcing Rust and Nim into an artificial common syntax.

### 4.2 Compiler Pipeline Graph

The Compiler Pipeline Graph models the transformations that a language tool performs. A compiler invocation may begin as an opaque action during bootstrap, but the research direction is to expose progressively finer boundaries where those boundaries are observable and useful.

Conceptually, the Rust path includes:

```text
source
  → parsing and expansion
  → HIR and type analysis
  → MIR construction and transformation
  → monomorphization collection
  → codegen-unit partitioning
  → backend lowering and optimization
  → object generation
  → archive and link
```

The Nim path includes:

```text
source
  → frontend and semantic processing
  → backend transformation
  → C / C++ / Objective-C / JavaScript generation
  → native compilation where applicable
  → object generation
  → archive and link
```

These sequences are research maps. LAMINARIA must distinguish stable integration points from compiler-internal or experimental interfaces.

### 4.3 Unified Program Graph

The Unified Program Graph is the shared semantic planning layer. It does not attempt to make MIR and Nim's internal representations identical. Instead, each compiler projects relevant entities and relationships into a common contract:

- logical program units;
- dependency edges;
- demanded capabilities;
- specialization or variant dimensions;
- required and produced artifacts;
- source and diagnostic provenance; and
- invalidation relationships.

Language-specific details remain attached as typed metadata.

### 4.4 Variant Graph

The Variant Graph represents choices such as:

```text
package
× target
× profile
× feature set
× generic or specialized instance
× host/target role
× code-generation backend
× native compiler
× artifact kind
× FFI configuration
```

LAMINARIA should not eagerly materialize this Cartesian product. It should use demand-driven expansion, constraint propagation, canonicalization, memoization, equivalent-state merging, SCC condensation, and pruning to construct only relevant states.

### 4.5 Artifact Graph

Dependencies are modeled as producer-artifact-consumer relationships:

```text
Producer Action → Artifact → Consumer Action
```

Artifacts may include semantic metadata, Rust metadata, generated C or C++, headers, bindings, object files, backend IR, bitcode, static archives, dynamic libraries, executables, test results, and diagnostic reports.

This makes `check`, `build`, and `test` different artifact demands rather than unrelated command implementations.

### 4.6 Action Graph

The Action Graph contains executable work. Candidate action kinds include:

```text
RustFrontend
RustAnalysis
RustMIR
RustMonomorphization
RustCodegenUnit
NimFrontend
NimAnalysis
NimBackendGeneration
BackendLowering
BackendOptimization
CCompile
CppCompile
BindingGeneration
ObjectGeneration
Archive
Link
Test
CodeGeneration
Custom
```

Language is metadata on an action, not a reason to place it in a separate scheduling universe.

## 5. Backend Graph

LLVM is neither excluded nor treated as the fixed foundation of LAMINARIA. LLVM, Cranelift, GCC-based code generation, and future routes are modeled as selectable backend components where the source compiler permits such selection.

```text
Language representation
    → Backend selection
    → Backend lowering
    → Backend optimization
    → Machine artifact
```

For Nim, C, C++, Objective-C, and JavaScript generation are likewise backend-family choices with their own downstream artifact and execution requirements.

The research problem is not merely choosing the fastest backend. It is expressing backend choice, toolchain compatibility, produced artifact types, diagnostic quality, cache identity, and downstream linking requirements as constraints in the same plan.

## 6. FFI as a graph primitive

FFI must not remain an incidental side effect of custom build scripts. LAMINARIA models it as a first-class relationship with explicit intermediate artifacts.

Nim-to-Rust may include:

```text
Nim exported surface
    → C ABI description
    → header generation
    → native object or archive
    → Rust binding generation
    → Rust compilation and link
```

Rust-to-Nim may include:

```text
Rust exported surface
    → staticlib or cdylib
    → C header generation
    → Nim import representation
    → Nim compilation and link
```

This makes ABI changes, binding regeneration, compatibility checks, rebuild propagation, and cache invalidation ordinary graph operations.

## 7. Planning and execution boundary

LAMINARIA is itself a Rust and Nim system. The division of responsibility follows the computational model, not the language being processed.

### Nim Planning Kernel

The Nim component owns predominantly deterministic computation:

- graph normalization and traversal;
- cycle detection and SCC decomposition;
- constraint propagation;
- variant expansion and pruning;
- state canonicalization and merging;
- artifact-demand propagation;
- critical-path analysis;
- candidate-plan generation; and
- combinatorial optimization.

Its desired contract resembles a coarse-grained function:

```text
plan(PlanningInput) → ExecutionPlan
```

The planning kernel should avoid direct filesystem, network, process, and operating-system side effects.

### Rust Runtime Scheduler

The Rust component owns execution and mutable machine state:

- CLI and daemon services;
- workspace and toolchain discovery;
- filesystem access and watching;
- process lifecycle and cancellation;
- asynchronous execution;
- CPU, memory, and I/O accounting;
- sandboxing;
- cache and content-addressed storage;
- local or remote executors; and
- diagnostics transport.

The planner should not be called after every completed action. Replanning is reserved for material changes such as failures, dynamic dependency discovery, resource-budget changes, or executor availability changes.

## 8. Resource-aware scheduling

Running Cargo and Nim independently can produce nested parallelism: each tool assumes it owns the machine and creates work according to the same CPU count. LAMINARIA's scheduler instead operates on the cross-language Action Graph.

Each action may declare or learn a resource profile:

```text
cpu demand
memory demand
I/O characteristics
estimated duration
cache-hit probability
executor requirements
```

Scheduling should optimize the wall-clock time to the requested artifact, not CPU utilization in isolation. Priority therefore depends on graph readiness and estimated remaining critical path, subject to live resource budgets.

Historical telemetry may improve duration and memory estimates, but learned values must remain explainable and must not change the semantic result of planning.

## 9. Cache and content identity

LAMINARIA separates three cache layers:

```text
compiler invocation cache
        ↓
action cache
        ↓
content-addressed artifact storage
```

An action or compiler-stage identity should include only semantically relevant inputs:

```text
operation kind
command and normalized arguments
relevant environment
input content digests
dependency artifact identities
toolchain identity
target and profile
backend selection
relevant configuration
```

Physical checkout paths should not define identity. LAMINARIA should normalize sources and toolchains into logical locations so equivalent work can be reused across Git worktrees, CI directories, repositories with the same source state, and machines with compatible toolchains.

## 10. Incremental compiler graph

Package-level invalidation is too coarse for the long-term goal. LAMINARIA studies an invalidation chain such as:

```text
changed source
  → affected semantic node
  → affected specialization
  → affected codegen unit
  → affected backend action
  → affected object
  → affected final artifact
```

Incrementality is treated as three related layers:

1. workspace-level affected analysis;
2. compiler-semantic and codegen invalidation; and
3. artifact and action-cache reuse.

The system must preserve correctness when a compiler exposes only coarser boundaries. Fine-grained integration is an optimization, not a prerequisite for a valid build.

## 11. Agent-oriented explainability

LAMINARIA is designed for both human and software-agent use. The graph and scheduler should be queryable directly rather than inferred from unstructured build logs.

Candidate interfaces include:

```text
laminaria dependency-graph
laminaria program-graph
laminaria action-graph
laminaria compiler-pipeline
laminaria codegen-units
laminaria critical-path
laminaria explain-dependency
laminaria explain-rebuild
laminaria explain-codegen
laminaria explain-backend-selection
laminaria explain-cache-miss
```

Every important decision should have a structured explanation: selected variant, dependency path, invalidation cause, cache-key difference, critical-path contribution, or rejected backend constraint.

## 12. Delivery strategy

LAMINARIA should descend into compiler internals gradually.

### Phase 1: Unified interface

Provide one CLI for build, run, test, check, formatting, linting, graph inspection, cache status, and toolchain diagnostics. Existing ecosystem tools remain the execution engines.

### Phase 2: Unified package and target graph

Normalize Cargo and Nimble metadata into a common workspace model. Add cross-language dependencies, affected analysis, and dependency explanations.

### Phase 3: Planning IR and Nim kernel

Stabilize `PlanningInput` and `ExecutionPlan`. Implement SCC analysis, constraint resolution, lazy variant expansion, pruning, and critical-path computation in Nim.

### Phase 4: Unified action scheduler

Represent generated native compilation, bindings, archives, and linking as actions. Enforce global CPU and memory budgets from the Rust runtime.

### Phase 5: Fine-grained compiler integration

Experiment with compiler-stage, codegen-unit, semantic-artifact, and backend boundaries where stable or maintainable integration is possible.

### Phase 6: Persistent and distributed execution

Add a daemon, durable graph state, remote cache, sandbox execution, and an abstract remote executor without inventing a new distributed protocol prematurely.

### Phase 7: Self-hosting

Use LAMINARIA to build its own Rust host and Nim planning kernel. Compare successive-stage plans and artifacts to continuously exercise the mixed-language architecture.

## 13. Evaluation plan

The research should use workloads that isolate different graph properties:

- Rust-heavy graphs with many crates, specializations, and codegen units;
- Nim-heavy graphs with large generated native-source sets;
- mixed graphs with simultaneous Rust and Nim native compilation;
- backend-variant workloads;
- FFI-heavy projects with several ABI boundaries;
- deep critical paths and wide independent action frontiers;
- variant-heavy workspaces;
- incremental semantic changes;
- repeated builds across Git worktrees; and
- local versus CI cache reuse.

Primary measurements include:

- graph construction cost and graph growth;
- explored versus pruned variant states;
- incremental invalidation precision;
- semantic, codegen, object, and final-artifact reuse;
- requested-artifact critical-path duration;
- CPU utilization and peak memory;
- cache hit rate across physical checkouts;
- FFI invalidation precision; and
- completeness and stability of explanations.

The important result is not only a faster full build. LAMINARIA must identify which computation was avoided, why a result was reused, and why the remaining work was necessary.

## 14. Research hypotheses

LAMINARIA will test the following hypotheses:

- **H1:** A package/task graph is too coarse for meaningful cross-language optimization; exposing compiler-pipeline work enables additional parallelism and reuse.
- **H2:** Backend selection can be represented as a constrained graph variant without making LLVM or any other backend the universal foundation.
- **H3:** Rust codegen units and Nim-generated native compilation can share one scheduler and reduce nested parallelism and global critical-path length.
- **H4:** Semantic and machine artifacts can be separated so `check`, `build`, and `test` become different artifact demands over one graph.
- **H5:** Treating FFI as a graph primitive produces more precise regeneration and invalidation than external build scripts.
- **H6:** Compiler-stage content identity enables reuse finer than package-level caching.
- **H7:** Demand-driven graph construction and state canonicalization can control variant explosion.
- **H8:** A structured compiler/build graph makes failures, rebuilds, backend choices, and cache misses directly explainable to software agents.

## 15. Non-goals and constraints

LAMINARIA is initially specific to Rust and Nim. It is not intended to become a fully generic replacement for Bazel, Buck2, Cargo, Nimble, `rustc`, Nim, LLVM, or native compilers.

The project should preserve existing manifests and lockfiles during adoption. It should reuse mature resolvers and compilers before attempting to replace their responsibilities. Fine-grained compiler integration must not make a correct coarse-grained build impossible.

Hermeticity and remote execution are staged capabilities, not initial requirements. Local development must remain useful with existing host toolchains.

## 16. Licensing direction

The project uses `MIT OR Apache-2.0` for both the Rust implementation and the Nim Planning Kernel. This keeps one contribution and reuse policy across the mixed-language codebase while retaining the explicit patent grant available under Apache-2.0.

The repository therefore carries:

```text
LICENSE-MIT
LICENSE-APACHE
THIRD_PARTY_LICENSES.md
```

Third-party compilers, backends, libraries, generated support code, and linked runtime components retain their own license obligations. If LAMINARIA later embeds runtime or startup code into user artifacts, that embedded portion must be reviewed separately rather than assuming the top-level tool license is automatically appropriate.

## 17. Open design questions

The next research decisions should focus on interfaces rather than implementation volume:

1. What is the smallest useful `PlanningInput` and `ExecutionPlan` schema?
2. Which artifact kinds require first-class identity in the initial implementation?
3. Which Rust and Nim compiler boundaries are stable enough for adapters?
4. How are dynamic dependencies incorporated without constant replanning?
5. Which constraints belong in the Nim planner, and which remain runtime admission rules?
6. How are toolchain identities normalized across machines?
7. What is the minimum structured explanation schema for agents?
8. How can self-hosting validate plan determinism and artifact reproducibility?

The highest-priority deliverable is the planning contract between the Nim kernel and Rust runtime. Once that contract is stable, graph resolution, scheduling, caching, diagnostics, and future executor implementations can evolve independently around it.



## 18. Prior scheduling evidence and reference benchmark

### 18.1 Cargo scheduler reconstruction

The August 31, 2026 study [*Could Cargo's scheduler be better?*](https://spirali.github.io/blog/cargo-scheduler/) reconstructs executable Rust build graphs from system-call traces of Cargo and its child processes. The study covers 17 Rust projects using debug builds and compares replayed Cargo schedules with alternative schedulers at parallelism levels of 4 and 16. Its author explicitly does not claim to describe Cargo's internal scheduling algorithm; the observed and replayed Cargo schedule is the baseline.

The most relevant observation for LAMINARIA is that one `rustc` invocation is not treated as one indivisible graph node. A dependent crate can begin once Rust metadata (`.rmeta`) is available, before the dependency's code generation and linking finish. The reconstructed graph therefore models:

```text
crate frontend
    → .rmeta available
    ├────────────→ dependent crate frontend
    → remaining compilation / code generation / link
```

The frontend and remaining compilation are a forced continuation of the same operating-system process, so the scheduler cannot place work between them on that worker. Nevertheless, the `.rmeta` production boundary exposes a real dependency edge earlier than whole-invocation completion.

This is concrete prior evidence for LAMINARIA's distinction between semantic artifacts and machine artifacts. It motivates studying metadata production, semantic analysis, specialization, codegen units, backend execution, object generation, and linking as producer-artifact-consumer relationships. It does not by itself establish that these boundaries are stable public compiler APIs; adapter feasibility remains a separate research question.

### 18.2 Critical-path-aware b-level scheduling

For each task `t`, the study computes a bottom level:

```text
b-level(t) = duration(t) + max(b-level(child))
```

When a worker becomes free, the ready task with the greatest b-level is selected. This greedy rule prioritizes work on or near the longest remaining dependency path instead of maximizing the number of immediately runnable tasks.

Across the 17 projects, b-level scheduling improved on the replayed Cargo baseline in 15 of 17 cases at 4 CPUs, with a median wall-clock reduction of about 8% and a best case of about 16%. At 16 CPUs it improved 14 of 17 cases, with a median reduction of about 2% and a best case of about 15%. Against the best schedule found by all evaluated heuristics and randomized searches—the study's pseudo-optimum—b-level was a median 1.3% slower at 4 CPUs, while Cargo was 9.6% slower. At 16 CPUs the corresponding medians were 0.4% and 2.3%.

These results support LAMINARIA's scheduler hypothesis: under constrained resources, shortening the global critical path can be more important than maximizing local parallelism.

### 18.3 Scheduling with imperfect cost information

The study also tests whether b-level requires precise execution-time prediction. With per-task duration estimates perturbed by Gaussian noise up to 60%, median schedules remained close to those produced with exact durations, although individual tail outcomes could degrade substantially.

A one-bit model—classifying tasks only as short or long—retained most of the benefit. It finished a median 1.5% above the pseudo-optimum at 4 CPUs and 0.5% above it at 16 CPUs, versus 1.3% and 0.4% for exact-duration b-level. A timing-blind graph-depth variant still beat the Cargo baseline on 16 of 17 projects at 4 CPUs and 12 of 17 at 16 CPUs, but showed materially worse tail cases. The one-bit signal therefore appears especially valuable as protection against pathological schedules rather than as a large median improvement.

LAMINARIA should evaluate scheduler sophistication incrementally:

1. graph-depth scheduling with uniform action cost;
2. binary-cost b-level scheduling;
3. historical-cost b-level scheduling;
4. resource-aware b-level scheduling; and
5. cross-language critical-path scheduling.

This sequence avoids making precise telemetry a prerequisite for an initially useful scheduler. Historical duration, cache-hit probability, memory demand, I/O behavior, backend characteristics, and executor constraints can be added as separately measurable refinements.

### 18.4 Cross-language extension

The published study concerns an externally reconstructed Rust/Cargo task graph. LAMINARIA extends the research question across compiler and language boundaries by placing the following work in one Action Graph:

- Rust metadata production and dependent frontends;
- Rust codegen units;
- LLVM, Cranelift, and GCC-family backend actions;
- Nim semantic processing and backend generation;
- Nim-generated C, C++, Objective-C, or JavaScript artifacts;
- native C and C++ compilation;
- FFI header and binding generation;
- archive creation and linking; and
- cache materialization and artifact availability.

The central question is whether the benefit of critical-path-aware scheduling observed inside Rust builds persists—or increases—when semantic stages, backend work, native compilation, FFI generation, and linking from Rust and Nim share one resource budget. Forced continuations must remain explicit constraints rather than being modeled as freely preemptible actions.

### 18.5 Reference benchmark protocol

LAMINARIA should first reproduce the published experiment on the same 17 projects, or a documented representative subset, before claiming a cross-language scheduling improvement. The comparison should hold the reconstructed graph and recorded task durations constant while varying only the scheduling policy:

```text
replayed Cargo baseline
    → LAMINARIA graph-depth
    → LAMINARIA binary-cost b-level
    → LAMINARIA historical-cost b-level
    → LAMINARIA resource-aware b-level
```

The initial reproduction should measure 4-CPU and 16-CPU configurations, median and tail behavior, distance from the best schedule found, and sensitivity to missing or noisy cost estimates. A second stage should apply the same protocol to Rust/Nim mixed workspaces and add peak memory, I/O pressure, cache availability, backend selection, and forced-continuation constraints.

This creates a falsifiable progression from a public Cargo scheduling baseline to LAMINARIA's cross-language scheduler. The project should report not only whether a schedule is faster, but which critical-path decisions changed, which estimates were used, and whether any gain came from graph decomposition, priority policy, resource admission, or cache availability.

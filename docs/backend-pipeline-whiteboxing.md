# Backend Pipeline White-boxing Research Direction

## Purpose

LAMINARIA must not decompose Rust and Nim frontends and codegen units only to treat the selected backend, linker, or WebAssembly toolchain as another opaque action.

This research track projects the internal work of LLVM, ThinLTO/DTLTO, LLD, WebAssembly code generation, `wasm-ld`, Binaryen, WIT and the Component Model into an observable and explainable nested computation graph, while materializing only those boundaries whose scheduling, caching, invalidation or reuse benefits exceed their cost.

The central rule is:

> White-boxing does not mean one process per compiler pass.

Making backend structure visible is different from turning every compiler pass into an external process or CAS artifact. LAMINARIA must preserve useful in-process locality and analysis state, measure the cost of serialization/reload and lost optimization context, and promote only economically useful boundaries into execution checkpoints.

## 1. Limitation of the current Backend Graph

The existing Backend Graph primarily models:

```text
Language representation
  → backend selection
  → backend lowering
  → backend optimization
  → machine artifact
```

This is sufficient to make LLVM, Cranelift, GCC-family routes and Nim backend families selectable variants, but it risks making all computation after backend selection opaque again.

LAMINARIA's research policy already prefers eliminating work, reusing valid artifacts, reducing invalidation, exposing parallelism and globally scheduling the remaining work at compiler-stage granularity. The same rule must apply inside backend, linker, post-link optimizer and componentization pipelines.

The Backend Graph is therefore split conceptually into:

```text
Backend Route Selection
  ↓
Backend Pipeline Expansion
```

Route selection solves backend/target/optimization/LTO/linker/artifact constraints. Pipeline expansion exposes the selected route's lowering, optimization, LTO, target codegen, linking and post-link work as a nested graph.

## 2. Backend Pipeline Provider

A backend should be modeled as a component that can expand a selected route into a LAMINARIA graph, not merely as an enum value.

Conceptually:

```text
expand_backend(
  input_artifacts,
  backend_route,
  target,
  optimization,
  lto_mode,
  debug_profile,
  toolchain_identity,
  constraints
) -> BackendPipelineGraph
```

The resulting graph may contain:

- logical stages;
- producer/artifact/consumer edges;
- observation points;
- candidate checkpoints;
- executable actions;
- dynamic graph-expansion points;
- invalidation dependencies;
- resource profiles;
- backend-specific typed metadata.

Backend-specific structure should not be flattened unnecessarily. LLVM pass-manager hierarchy, Binaryen pass runners, Cranelift stages and similar details may remain typed nested graphs.

## 3. Four boundary classes

### 3.1 Logical Stage

A semantically meaningful backend computation. It appears in explanations and analysis but is not necessarily an independent process.

Examples include inlining, loop optimization, vectorization, dead-code elimination and Binaryen function optimization.

### 3.2 Observation Boundary

A point where LAMINARIA can obtain timing, CPU/memory, IR or module size, optimization remarks, digests or transformation evidence.

Observation does not imply materializing a reusable artifact.

### 3.3 Checkpoint / Artifact Boundary

A materialized boundary that may be worth hashing, storing, reusing or transferring.

Candidate artifacts include:

- LLVM IR / bitcode;
- pre-link bitcode;
- ThinLTO module summaries;
- ThinLTO per-module indexes;
- ThinLTO backend outputs;
- native objects;
- relocatable WebAssembly objects;
- linked Core WebAssembly modules;
- optimized Core WebAssembly modules;
- WIT/component metadata;
- final WebAssembly Components.

### 3.4 Execution Boundary

A unit independently managed by the LAMINARIA scheduler with ready/running/blocked/completed state.

Logical and observable boundaries must not automatically become execution boundaries.

## 4. Checkpoint economics

LAMINARIA chooses backend checkpoints from measured economics rather than assuming that finer granularity is always better.

```text
checkpoint benefit =
  eliminated work
+ reusable work
+ reduced invalidation
+ scheduling gain
+ remote/distributed execution gain

checkpoint cost =
  serialization
+ deserialization/reload
+ hashing
+ process/IPC overhead
+ lost analysis state
+ lost cache locality
+ increased memory traffic
+ optimization-quality risk
```

LLVM's New Pass Manager is explicitly hierarchical across Module / CGSCC / Function / Loop and benefits from grouping work over related IR units. Therefore per-pass process splitting is not a default design goal.

## 5. LLVM Pipeline Graph

An LLVM route should expose at least the following conceptual layers:

```text
Language / Codegen Unit
  ↓
LLVM IR generation
  ↓
IR preparation / canonicalization
  ↓
Middle-end optimization pipeline
  ├─ Module
  ├─ CGSCC
  ├─ Function
  └─ Loop pass groups
  ↓
Pre-link optimization
  ↓
LTO strategy
  ├─ none
  ├─ ThinLTO
  └─ Full LTO
  ↓
Target-dependent code generation
  ↓
Object / relocatable target artifact
  ↓
Link
```

LAMINARIA should observe pass pipeline structure, execution order, timing, optimization remarks and selected analysis/invalidation behavior without requiring each pass to become an independent action.

### 5.1 LLVM observation surfaces

Research should evaluate:

- New Pass Manager Module / CGSCC / Function / Loop structure;
- `PassBuilder` default and customized pipelines;
- pass execution timing;
- optimization remarks (`Passed`, `Missed`, `Analysis`);
- IR/bitcode size and digest around selected pass groups;
- rustc codegen-unit, LTO and linker-plugin-LTO boundaries;
- a stable pipeline fingerprint for cache identity.

### 5.2 Pipeline identity

Checkpoint identity must consider more than input bitcode, including:

- LLVM version/build identity;
- target triple, data layout and target features;
- optimization level;
- pass-pipeline fingerprint;
- codegen options;
- LTO mode;
- PGO/profile inputs;
- debug configuration;
- plugin/external passes;
- relevant environment/toolchain inputs.

## 6. ThinLTO / DTLTO as an Action Graph

ThinLTO is a primary reference implementation for backend white-boxing.

```text
bitcode modules
  ↓
thin-link / combined summary analysis
  ↓
per-module summary index
  ↓
independent ThinLTO backend jobs
  ↓
native object outputs
  ↓
final link
```

DTLTO allows LLD to describe backend jobs using JSON containing compiler commands, inputs, outputs and per-module indexes and to delegate those jobs to an external distributor. This structure is unusually close to a LAMINARIA Action Graph.

LAMINARIA should compare three modes:

1. opaque in-process ThinLTO;
2. explicit thin-link/index-only plus per-module backend jobs;
3. DTLTO where the LAMINARIA scheduler consumes the distributor jobs.

Key questions include:

- whether link-discovered jobs can safely become a dynamic subgraph;
- whether per-module indexes are first-class dependency artifacts;
- whether backend jobs can share the global CPU/memory/I/O budget;
- how LLVM's native ThinLTO cache relates to LAMINARIA CAS/action cache;
- whether controlled edits rerun only necessary backend jobs;
- whether critical-path and wait causes remain explainable.

## 7. Convergence from Rust and Nim into LLVM

LLVM white-boxing is not Rust-only.

Candidate paths include:

```text
Rust
  → rustc_codegen_ssa / LLVM bitcode

Nim 2
  → nlvm → LLVM IR
  or
  → generated C → Clang → LLVM IR/bitcode

Nim 3 / Nimony
  → Leng / lengc → LLVM IR
```

LAMINARIA should measure how far these paths can participate in one LLVM IR/bitcode/LTO plan.

This does not assume language-level ABI compatibility. Target triple, data layout, symbol visibility, calling convention, runtime initialization, allocator ownership, panic/exception behavior, TLS and ownership must be tested separately.

## 8. WebAssembly is a Target Pipeline, not a backend family

WebAssembly should not be modeled as a peer of LLVM or Cranelift.

LAMINARIA should separate:

```text
Backend Engine
× Target ISA / Object Model
× Link Model
× Post-link Optimizer
× Composition Model
```

For example:

```text
backend = LLVM
target = wasm32
linker = wasm-ld
post_link = Binaryen/wasm-opt
composition = CoreModule | ComponentModel
```

## 9. WebAssembly Target Pipeline

A representative LLVM route is:

```text
Language IR
  ↓
LLVM IR
  ↓
LLVM optimization
  ↓
WebAssembly target codegen
  ↓
relocatable Wasm object
  ↓
wasm-ld
  ↓
Core WebAssembly Module
  ↓
Binaryen / wasm-opt
  ↓
Optimized Core Module
  ↓
WIT metadata / adapter processing
  ↓
componentization
  ↓
WebAssembly Component
```

`wasm-ld`, Binaryen and componentization must not be collapsed into a single `WASM backend` action.

### 9.1 wasm-ld

Treat the linker as an independent stage between relocatable WebAssembly objects and final core modules. Record linker inputs, symbol behavior, options and relevant GC/LTO effects.

### 9.2 Binaryen

`wasm-opt` has its own pass pipeline. LAMINARIA should expose pass execution, timing and module-change evidence while applying the same rule as LLVM: visibility does not imply one process per pass.

The initial checkpoint comparison should be linked Core Wasm versus post-link optimized Core Wasm.

### 9.3 WIT / Component Model

WIT metadata embedding, adapter selection and component encoding are candidate independent artifact/action boundaries.

`wasm-tools component embed` and `wasm-tools component new` provide concrete separable operations for testing invalidation between core-module generation and componentization.

A WIT-only edit may allow compile/link reuse when the core module contract remains compatible, but this must not be assumed when generated bindings, exports/imports or Canonical ABI requirements change.

## 10. Dynamic Graph Expansion

Some pipelines reveal their concrete child jobs only after an upstream action executes. DTLTO is the primary example.

Candidate model:

```text
GraphExpansionAction
  inputs: planning/link artifacts
  output: ExpansionManifest
  expands-to: child actions + artifact edges
```

Requirements include:

- content identity for the expansion result;
- deterministic child-graph generation or explicit non-deterministic inputs;
- ordinary scheduler/resource/cache/critical-path accounting for child actions;
- visible opaque fallback when expansion is unavailable;
- measured graph-expansion overhead.

## 11. Apply work elimination inside the backend

LAMINARIA's optimization hierarchy remains unchanged inside backend pipelines:

1. eliminate unnecessary backend stages;
2. reuse valid checkpoint artifacts;
3. reduce invalidation scope;
4. expose parallelism;
5. globally schedule the remaining work;
6. optimize individual remaining stages.

Experiments should test whether unchanged semantic/codegen artifacts can avoid IR regeneration, whether unaffected ThinLTO backend outputs are reused, whether unchanged Core Wasm avoids relinking or post-link work, and whether component-only changes can avoid recompilation when contracts permit.

## 12. Required metrics

Backend white-boxing evidence should include, as applicable:

- logical stage count;
- observation boundary count;
- materialized checkpoint count;
- executed/skipped backend stages;
- pass-group/backend-job wall and CPU time;
- queue/dependency/resource wait;
- peak/time-weighted memory;
- bytes serialized/deserialized/hashed/read/written;
- IR/bitcode/object/module/component sizes;
- optimization remarks and transformation evidence;
- observable analysis/cache reuse;
- ThinLTO backend job count and invalidation set;
- linker input/output and symbol inventory;
- Binaryen pass timing/module metrics;
- componentization/adaptation time and artifacts;
- graph-expansion overhead;
- reference-baseline ratio;
- checkpoint benefit versus checkpoint cost.

## 13. Completion criteria

This work is not complete merely because backend internals are visualized.

At minimum the research must demonstrate:

1. route selection and pipeline expansion as distinct concepts;
2. an LLVM pipeline classified into logical, observable, checkpoint and execution boundaries;
3. pass-level evidence without default per-pass process splitting;
4. a reproducible ThinLTO/DTLTO-to-LAMINARIA graph experiment;
5. WebAssembly modeled as a target pipeline through link/post-link/componentization;
6. at least one measured backend checkpoint benefit from work elimination or artifact reuse;
7. at least one measured case where an overly fine checkpoint is rejected because its cost is higher;
8. explicit reporting when opaque fallback is used.

## 14. Primary references

- LLVM New Pass Manager: https://llvm.org/docs/NewPassManager.html
- LLVM Optimization Remarks: https://llvm.org/docs/Remarks.html
- Clang ThinLTO: https://clang.llvm.org/docs/ThinLTO.html
- LLVM DTLTO: https://llvm.org/docs/DTLTO.html
- rustc codegen options: https://doc.rust-lang.org/rustc/codegen-options/
- LLD WebAssembly port: https://lld.llvm.org/WebAssembly.html
- Binaryen / wasm-opt: https://github.com/WebAssembly/binaryen
- wasm-tools: https://github.com/bytecodealliance/wasm-tools
- nlvm: https://github.com/arnetheduck/nlvm
- Nimony: https://github.com/nim-lang/nimony

These projects are reference implementations for observable pipeline boundaries, externalizable work, caching/scheduling and WebAssembly composition rather than merely implementation dependencies.

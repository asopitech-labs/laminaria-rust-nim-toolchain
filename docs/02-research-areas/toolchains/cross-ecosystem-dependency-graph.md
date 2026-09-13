# Cross-Ecosystem Dependency Graph Research

## Authority and scope

The [near-term research program](../../near-term-research-program.md) makes cross-ecosystem dependency resolution the current core research theme. This document defines that problem. It is not a proposal to wrap `cargo build`, `nimble build`, CMake, Meson, or a platform linker as four opaque actions.

The [Rust/Cargo C/C++ native-build model](rust-c-cpp-native-build-model_ja.md) records the current boundary. The [prior-art survey](cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md) compares package solvers, polyglot build graphs, incremental semantic systems, multi-level IR, and linker-integrated IR. *Package Managers à la Carte* is direct prior art for cross-ecosystem package resolution, so LAMINARIA's research claim is not a package solver alone; it is coupled incremental resolution through source semantics, multi-level IR, native ABI, symbols, and link closure.

## The non-substitutable problem

A successful Rust-to-Nim call proves a language or ABI path. It does not prove resolution of the graph injected by package ecosystems. The required closure may combine:

- Cargo packages, features, target predicates, build dependencies, proc-macro-like host work, and native-link metadata;
- Nimble packages, module search paths, compiler defines, tasks, generated sources, and `importc`/`importcpp` requirements;
- C headers, translation units, compile definitions, include paths, archives, shared libraries, `pkg-config`-style facts, and platform system libraries; and
- C++ templates, inline/header-only code, explicit instantiations/adapters, language/standard-library ABI, exceptions, RTTI, constructors, destructors, and link order.

These are not interchangeable package edges. LAMINARIA needs a typed graph whose nodes retain ecosystem identity while edges state what is required, produced, selected, rejected, and linked.

## Graph layers

```text
requested native executable
  -> package requirements and selected versions/features
  -> source/module/header/generated-unit relationships
  -> host/target toolchain and ABI constraints
  -> compile/adapter/archive artifacts
  -> symbol and link requirements
  -> final native executable
```

Resolution ends only when every demanded artifact has an identified producer or an accepted prebuilt identity and the final executable has a complete, ordered link closure. A package manager's decision is evidence/input, not permission to hide compilation or linking.

## Correctness invariants

- The graph distinguishes package, source, semantic, artifact, action, and physical-placement identities.
- Host work and target work cannot be merged merely because one package declared both.
- Features, versions, target predicates, toolchain capabilities, ABI, symbols, and link ordering remain explicit constraints.
- Unsupported build scripts, generators, macros, or native-library discovery fail with a structured gap; they do not silently execute an opaque fallback.
- A resolved graph is tested by producing and running the demanded native executable. A hand-maintained YAML graph and fixture-only validator are not verification authorities.

## Efficiency hypothesis

The candidate resolver should avoid constructing the Cartesian product of all packages, versions, features, targets, backends, toolchains, ABIs, and artifact kinds. The research compares eager expansion with demand-driven expansion plus constraint propagation, canonicalization, memoization, equivalent-state merging, dominance pruning, and SCC condensation.

Pruning applies beyond candidate states. Starting from the native executable, the system should avoid unreachable packages, sources/modules, semantic items, IR, objects/archive members, symbols/sections, and runtime artifacts as early as correctness permits. Late DCE and linker garbage collection reduce output size but cannot recover parse, type-check, monomorphization, or code-generation work already performed. The [cross-layer pruning contract](cross-layer-reachability-pruning_ja.md) defines roots, conservative retention, evidence, and measurements.

Graph completion is not merely enumeration of a reachable closure. Every package, source-semantic, language/intermediate-IR, ABI, symbol, and link obligation must become `Discharged` through transformation into the artifact, `Externalized` as an explicit runtime contract, or `Rejected` with a reason. The original Cargo/Nimble/C/C++ graph thereby remains provenance rather than a graph the artifact consumer must resolve again. The [dependency-discharge artifact contract](dependency-resolved-artifact-closure_ja.md) defines this property.

Correctness is mandatory. Among correct resolvers, compare:

- wall-clock resolution time;
- peak resident memory;
- states/nodes expanded, pruned, merged, and recomputed;
- invalidation after a no-op, leaf change, feature change, and target-condition change; and
- external compiler/linker actions avoided before and after resolution.

## Current boundary

The first experiment uses one Cargo crate, one Nimble package, one C library, and one C++ library, with one native executable as the demanded root. It includes one successful closure and one pre-compilation rejection. It does not claim complete ecosystem compatibility.

WebAssembly may later consume the same resolved graph as an optional target variant. It does not define this graph and is not an acceptance condition for the current milestone.

# Cross-Ecosystem Dependency Graph Research

## Authority and scope

The [near-term research program](../../near-term-research-program.md) makes cross-ecosystem dependency resolution the current core research theme. This document defines that problem. It is not a proposal to wrap `cargo build`, `nimble build`, CMake, Meson, or a platform linker as four opaque actions.

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

Correctness is mandatory. Among correct resolvers, compare:

- wall-clock resolution time;
- peak resident memory;
- states/nodes expanded, pruned, merged, and recomputed;
- invalidation after a no-op, leaf change, feature change, and target-condition change; and
- external compiler/linker actions avoided before and after resolution.

## Current boundary

The first experiment uses one Cargo crate, one Nimble package, one C library, and one C++ library, with one native executable as the demanded root. It includes one successful closure and one pre-compilation rejection. It does not claim complete ecosystem compatibility.

WebAssembly may later consume the same resolved graph as an optional target variant. It does not define this graph and is not an acceptance condition for the current milestone.

# LAMINARIA entity model: a measurement-derived ERD across Rust/Nim/foreign-native chains

## Purpose

[The compiler ownership contract](../../01-foundations/compiler-ownership-contract.md) requires LAMINARIA itself to hold "the entities, relations between entities, and state transitions/activities" it owns internally. This document records that model, confirmed against real measurement of the existing pipelines (Cargo/rustc/LLVM, `nim c`/`nim cpp`). It consolidates conclusions from issue #52 (Rust-side pipeline decomposition), issue #71 (entity model confirmation), issue #74 (build-flow integration), and issue #76 (Nim build-path measurement decomposition).

Each of these is a **reference/comparison experiment** against the existing pipeline (the Reference/baseline row of the [ownership contract](../../01-foundations/compiler-ownership-contract.md)) — none of it adopts the existing `nim c`/`cargo`/`rustc` as LAMINARIA's own implementation path.

## Rust-side entity chain (issue #52/#71)

Issue #52 decomposed Cargo/rustc/LLVM through real measurement; issue #71 redefined that as LAMINARIA's own entities.

### Confirmed entities (10, `Cgu` deliberately excluded)

`WorkspaceManifest`/`MemberManifest`, `PackageResolution`, `SourceUnit` (held as a feature-condition graph, [`SourceUnitGraph`](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/71#issuecomment-5708615948)), `SemanticFact`, `GenericDefinition`, `InstantiationKey` (a five-element key: declaration identifier, type arguments, const generic values, ABI/target, active feature set), `OptimizationDecision`, `LoweredModule`, `NativeObject`, `LinkSymbol`, `LinkedArtifact`.

All of these use the existing Rust MIR/LLVM IR representations as-is; none introduce a separate LAMINARIA-owned intermediate representation — what changes is only which unit a key is assigned to and where it is held (consistent with [project_laminaria_ir_priority]'s direction).

### Activities/state transitions (A0-A12)

- **A0**: `BuildStrategyContext` decision (most-likely strategy vs. environment-adaptive strategy, introduced in issue #52). An independent control entity decided from the build invocation context before A1. It affects only how target parameters are confirmed (apply a default immediately vs. detect), not A1-A8/A10-A12.
- **A1-A5**: Manifest construction (workspace hierarchy) → dependency resolution → feature-condition-graph construction → `SourceUnit` selection.
- **A6**: Semantic analysis (`SourceUnit` → `SemanticFact`).
- **A7**: Monomorphization (`GenericDefinition` → `InstantiationKey`). Shared as a globally unique key across the whole dependency closure, never duplicated — the core of boundary elimination.
- **A8**: Optimization decision (`InstantiationKey` → `OptimizationDecision`).
- **A9**: Code-generation preparation (`InstantiationKey` + target parameters → `LoweredModule`). **Corrected**: it was originally designed to "group multiple `InstantiationKey`s," but that reintroduced a `Cgu`-style post-hoc convergence point under a different name, so it was corrected to a **pure 1:1 function application**. How parallel-compilation execution units are cut is fully separated out as A10's scheduling concern.
- **A10-A12**: Code generation → symbol registration → linking. **The only convergence point is A12 (linking)** — information fans out only at A7 (monomorphization) and A11 (symbol registration).

## Nim-side entity chain (issue #76)

Issue #76 decomposed `nim c`/`nim cpp` through real measurement (Nim 2.2.10, real fixtures).

### Confirmed entities (12)

`NimblePackageManifest`, `NimBuildInvocation` (one `nim c` process invocation — a new concept with no direct Rust-side counterpart), `NimModuleUnit`, `ModuleImportEdge`, `StdlibModuleUnit`, `GeneratedCModule`, `GenericDeclaration`, `NimInstantiationKey`, `ExportedSymbol`, `CCompileCommand`, `CObjectFile`, `LinkCommand`/`NimExecutable`.

### Activities/state transitions (N1-N9)

N1 (manifest construction) → N2 (dependency resolution) → N3 (`NimBuildInvocation` startup) → N4 (module reachability resolution) → N5 (per-module semantic analysis + C generation, 1:1) → N6 (monomorphization) → N7 (C compilation) → N8 (linking, the convergence point) → N9 (`{.exportc.}` registration).

### Structural differences from the Rust side (confirmed by measurement)

1. **Semantic analysis and code generation are fused into N5** — the Rust side's separation of `SemanticFact` (A6) and `LoweredModule` (A9) has no counterpart on the Nim side.
2. **`NimInstantiationKey`'s multiplicity has two tiers**: 1:1 (deduplicated) within the same `NimBuildInvocation`, but 1:0..N (duplicated) across `NimBuildInvocation` boundaries. Measurement confirmed that duplicate compilation structurally identical to the Rust-side `Cgu` problem is real on the Nim side too — the same standard-library modules and monomorphized instances are fully regenerated between independent compiles.
3. **A pathology found by backtracking inspection**: the key `nim c` actually uses to identify a monomorphized instance mixes in the "module that first, syntactically, requested it" — information semantically unrelated to the declaration identifier, type arguments, or const generic values. Demonstrated concretely: for the identical program, merely swapping the order of two `import` statements changes the generated symbol name. This is worse than the Rust-side `Cgu` problem in that it depends on a more arbitrary factor (import order). LAMINARIA's own `NimInstantiationKey` design must use exactly the same three elements as the Rust side (declaration identifier, type arguments, const generic values) and deliberately exclude caller-dependence.
4. **The `{.exportc.}` boundary is stable and caller-independent** — unlike the instability of non-exportc internal symbols, FFI-boundary symbol names are never mangled and stay stable.

### Cost-scale judgment

The Nim-side duplicate-compilation problem is structurally real, but the measured cost (tens to hundreds of milliseconds per duplicated site) is two to three orders of magnitude smaller than the scale issue #52 confirmed on the Rust side (45 seconds out of 215 seconds, Phase-2 cost). Given [project_laminaria_goal_and_bottleneck] (LAMINARIA's primary goal is build-time speedup, and the main front is Rust-side LLVM), the structure should be designed correctly, but measurement gave no basis to treat it as an optimization target at the same priority as the Rust side.

## Integration of the Rust/Nim/foreign-native chains (issue #74)

The three chains were integrated into one flow diagram, with confirmed parallel-execution boundaries and merge points.

- **Rust-side A1-A5 and Nim-side N1-N4 are fully parallel** — independent front-end processing per language with no shared state.
- **The semantic-analysis stage (A6/N5-equivalent) is also fully parallel** — [an earlier design was corrected to "synchronize only between `SourceUnit`s that share an FFI boundary," then that correction itself was withdrawn](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/74#issuecomment-5710568243). Introducing type/ABI matching as a blocking synchronization point at the A6/N5 stage would itself have reintroduced the "post-hoc, artificial convergence point" that A9 had already eliminated.
- **The foreign-native chain** (`ForeignDecl` → `ForeignLibraryTarget` → `ResolvedForeignArtifact` → [`AdapterUnit`] → `ForeignCompileUnit` → `ForeignNativeObject`) **is an independent chain that only merges at A12**. It never becomes a semantic-analysis subject on either the Rust or Nim side (`SourceUnit`/`NimModuleUnit`) — this is a separation the ownership boundary itself requires, not a design freedom.
- **Type/ABI matching is an advisory downstream check between A11 and A12** (not a blocking synchronization point). Its execution granularity is per-`LinkSymbol` (confirmed by the fact that issue #75's `ReachabilityReport` implementation was already built at that granularity). Matching failure distinguishes `Unmatched` (folds into A12's existing definition) from `arity_mismatch` (a new category, planned as a separate `LinkGraphError` layer distinct from the existing `LoweringError`).
- **The convergence point remains A12 (linking) alone across all three chains** — this principle holds even after integrating all three chains.

## C/C++ foreign-native chain detail (issue #71/#73)

The concrete entities and granularity for implementing the requirements defined by [the Nim C/C++ library integration document](nim-c-cpp-library-integration.md).

### Entities (6)

`ForeignDecl` (generated by reference from the Rust/Nim-side `SemanticFact`; it never becomes a `SourceUnit`), `ForeignLibraryTarget`, `ResolvedForeignArtifact`, `AdapterUnit` (C++ only, conditional), `ForeignCompileUnit`, `ForeignNativeObject`.

### `AdapterUnit` granularity (confirmed in issue #73)

**One unsupported construct = one `AdapterUnit`** (`ForeignDecl`:`AdapterUnit` = 1:0..1). The existing `nim c`/`nim cpp` has no such automatic generation logic at all (measurement confirmed it only ever consumes a hand-written, fixed adapter file); this granularity was not derived from measured data but deduced from the A9-correction principle (never reintroduce a post-hoc, back-filled grouping into the entity model). If optimizing external-compiler invocation cost becomes necessary, the same "cache-key unit ≠ execution-scheduling unit" distinction used at A9 applies — as an execution-layer optimization, without changing the entity definition itself.

## Real-world verification of importc/importcpp/importobjc (issue #44)

The existing `nim c`/`nim cpp`/`nim objc` backends were verified against real invocations (Nim 2.2.10, Docker).

### importc (C FFI)

The header named by the `header` pragma is `#include`d directly, and the declared call is lowered transparently without any Nim mangling. However **it manages no explicit link input** — it relies on the system's implicit library resolution (gcc's default link behavior). Satisfying `compiler-ownership-contract.md`'s requirement that "foreign inputs, headers, flags, toolchain, outputs and link edges" be visible requires a design where LAMINARIA itself explicitly manages this as `ResolvedForeignArtifact`.

### importcpp (C++ templates/header-only APIs)

Verified against a real template class: **no standalone adapter/instantiation unit (a separate file) is generated at all**. Template instantiation is inlined directly into the `.cpp` file generated for the calling module, and left entirely to the C++ compiler's ordinary template mechanism. This confirms, through measurement, that the design decision in issue #73 (materializing `AdapterUnit` as an explicit, standalone compilation unit) is a **new LAMINARIA design choice**, not a reuse of `nim cpp`'s own behavior.

### importobjc (Objective-C FFI) — a pre-existing defect in Nim itself, not a LAMINARIA-side design problem

Web research confirmed Nim supports Objective-C as a third FFI language, but real-world verification found that **the code `{.importobjc.}` generates is syntactically invalid and fails to compile under a real Objective-C compiler (gobjc)**, even for basic usage — it reuses `importcpp`'s pattern-substitution syntax (`#`/`@`), but Objective-C's message-send syntax `[receiver selector: arg]` breaks C syntax when substituted into the name position of a C function-declaration macro. The generated code also violates Objective-C's own language semantics by statically (value-type) allocating an Objective-C object. The backend-branching mechanism itself (`when defined(objc)`) was separately confirmed to work correctly — this is independent of the FFI code-generation logic.

**Conclusion**: this is a pre-existing implementation defect in Nim itself, not a design problem LAMINARIA needs to solve. [The Nim C/C++ library integration document's](nim-c-cpp-library-integration.md) scope (C/C++ only) remains correct and unchanged. Should Objective-C support ever be considered in the future, this is recorded as the fact that the existing `nim objc` cannot serve as a reference implementation (because it does not work).

## Source issues (all closed)

- Issue #52: real-measurement decomposition of Cargo/rustc/LLVM, provisional ERD
- Issue #71: entity model confirmation (10 entities, multiplicities, A0-A12, Nim-side mapping, foreign-native chain)
- Issue #72: macro/template expansion support-scope confirmation (both Rust and Nim currently reject all such constructs; explicit detection added on the Nim side)
- Issue #73: `AdapterUnit` granularity confirmation (1:0..1)
- Issue #74: build-flow-diagram integration, advisory downgrade of type/ABI matching, matching granularity/failure-category confirmation
- Issue #75: type/ABI-level extension of FFI boundary matching (arity detection), new Rust<->Nim matching implementation
- Issue #76: real-measurement decomposition of the Nim build path, N1-N9 confirmation, backtracking inspection
- Issue #44: real-world verification of importc/importcpp/importobjc

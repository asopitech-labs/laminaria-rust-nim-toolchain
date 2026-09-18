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

- **A0**: `BuildStrategyContext` decision (most-likely strategy vs. environment-adaptive strategy, introduced in issue #52). Depends only on external input (the build invocation context: presence of CI config files, presence of prior build history) and requires no other activity's output. **Corrected (issue #83)**: this document previously said "A1 and N1 can start only after A0 completes," but that deduced a data dependency between A0 and A1/N1 that does not actually exist — A1's input (manifest change detection) and N1's input (`.nimble` change detection) neither one ever reads A0's output (the strategy decision). A0's output is consumed only at A9 (how target parameters are confirmed); it has no bearing on A1-A8/N1-N8 upstream of that. Issue #52's own text — "the strategy decision can be made at the same timing as stages 1-2 (manifest parsing, dependency resolution)" — meant they can be decided **in parallel**, not that an ordering is required. Following the existing demand-ready execution principle already established in this codebase (`crates/laminaria-run/src/compiler_work_executor.rs`'s `run_compiler_work_plan`: "an action starts as soon as every producer its declared inputs name has resolved" — meaning an action depending only on external input starts the moment its start condition is met), **A0, A1, and N1 are all independent actions depending only on external input, and all three can start simultaneously at build-invocation time**. The only constraint A0 must satisfy is completing before A9 starts — it is not a precondition for A1 or N1.
- **A1-A5**: Manifest construction (workspace hierarchy) → dependency resolution → feature-condition-graph construction → `SourceUnit` selection.
- **A6**: Semantic analysis (`SourceUnit` → `SemanticFact`).
- **A7**: Monomorphization (`GenericDefinition` → `InstantiationKey`). Shared as a globally unique key across the whole dependency closure, never duplicated — the core of boundary elimination.
- **A8**: Optimization decision (`InstantiationKey` → `OptimizationDecision`).
- **A9**: Code-generation preparation (`InstantiationKey` + target parameters → `LoweredModule`). **Corrected**: it was originally designed to "group multiple `InstantiationKey`s," but that reintroduced a `Cgu`-style post-hoc convergence point under a different name, so it was corrected to a **pure 1:1 function application**. How parallel-compilation execution units are cut is fully separated out as A10's scheduling concern.
- **A10-A12**: Code generation → symbol registration → linking. **The only convergence point is A12 (linking)** — information fans out only at A7 (monomorphization) and A11 (symbol registration). **A11's own guarantee**: it carries the same deduplication guarantee as A7 — registering the same `(identifier, linkage)` is idempotent, and when multiple A10 outputs produced in parallel arrive at once, insertion into the `LinkSymbol` store resolves to exactly one confirmed entry under that unique key (the final state is independent of insertion order; a later registration under an already-registered key never creates a duplicate). This closes the gap issue #82's verification found — A11 previously carried no documented idempotence guarantee — by extending A7's established design principle to A11.

## Nim-side entity chain (issue #76)

Issue #76 decomposed `nim c`/`nim cpp` through real measurement (Nim 2.2.10, real fixtures).

### Confirmed entities (12)

`NimblePackageManifest`, `NimBuildInvocation` (one `nim c` process invocation — a new concept with no direct Rust-side counterpart), `NimModuleUnit`, `ModuleImportEdge`, `StdlibModuleUnit`, `GeneratedCModule`, `GenericDeclaration`, `NimInstantiationKey`, `ExportedSymbol`, `CCompileCommand`, `CObjectFile`, `LinkCommand`/`NimExecutable`.

### Real measurement backing the entities

Real Nim 2.2.10 was run in a Docker container against real fixture source (`fixtures/rust-nim-c-abi-baseline/nim-lib/nimlib.nim` and the multi-module `fixtures/nim-heavy-workspace`).

**`GeneratedCModule` (1 real module = 1 generated `.c` file, including transitive imports).** Compiling a ~20-line module with five `{.exportc.}` procs produced six `.c` files, not one:

```
nimcache/@mnimlib.nim.c              -- the module itself (592 lines)
nimcache/@psystem.nim.c              -- Nim stdlib system (4886 lines)
nimcache/@psystem@sdollars.nim.c     -- system/dollars (89 lines)
nimcache/@psystem@sexceptions.nim.c  -- system/exceptions (443 lines)
nimcache/@pstd@sassertions.nim.c     -- std/assertions (154 lines)
nimcache/@pstd@sprivate@sdigitsutils.nim.c -- std/private/digitsutils (322 lines)
```

`fixtures/nim-heavy-workspace` (`fixture.nim` imports only `geometry.nim`, which in turn imports `primes.nim`) confirmed this 1:1 relationship holds through **transitive** imports too: all three of `@mfixture.nim.c`, `@mgeometry.nim.c`, and `@mprimes.nim.c` were generated, even though `fixture.nim` names only `geometry` directly.

**`CCompileCommand`/`LinkCommand` (structured compile/link data, from `nimcache/*.json`).** `nim c` records the exact `gcc` invocations it issued:

```json
"compile": [
  ["<path>/@psystem@sexceptions.nim.c", "gcc -c -w -fmax-errors=3 -fno-strict-aliasing -pthread -I<nimlib> -I<src> -o <obj> <c>"],
  ... one entry per generated .c file
],
"link": [ <one .o path per compile entry> ],
"linkcmd": "gcc -o <output> <all .o paths> -pthread -pthread -ldl"
```

**`ExportedSymbol` vs. internal symbols (real name vs. Nim's own mangling).** `{.exportc.}` procs keep their declared name verbatim; everything else is mangled with the declaring module name and a running counter:

```c
N_LIB_PRIVATE N_CDECL(int, laminaria_is_prime)(int n_p0);              // exportc: name preserved
N_LIB_PRIVATE N_NIMCALL(NIM_BOOL, isPrimeImpl__nimlib_u1)(NI n_p0);     // non-exportc: module+counter mangling
```

**`NimInstantiationKey` — deduplicates within a single `NimBuildInvocation`.** A synthetic fixture calling a generic `maxVal[T]` with the same type argument (`int`) twice and a different type argument (`float`) once produced exactly two instances, with the two same-type calls sharing one:

```c
N_LIB_PRIVATE N_NIMCALL(NI, maxVal__generics_u12)(NI a_p0, NI b_p1);   // int -- generated once
N_LIB_PRIVATE N_NIMCALL(NF, maxVal__generics_u23)(NF a_p0, NF b_p1);   // float -- separate instance
...
T2_ = maxVal__generics_u12(((NI)3), ((NI)5));   // call site 1
T3_ = maxVal__generics_u12(((NI)10), ((NI)2));  // call site 2 -- reuses the same instance
```

const generics behave the same way: `arraySum[T; N: static int]` called with `array[3, int]` and `array[5, int]` produced two distinct instances and two distinct array `typedef`s, one per const-generic value (`N`).

**`NimInstantiationKey` — duplicated across `NimBuildInvocation` boundaries.** The same `maxVal[int]` instance, called from two independent Nimble packages (`appone`/`apptwo`, each its own `nim c` process, both depending on a shared `libshared` package), was regenerated once per invocation, with the project name baked into the mangled name:

```c
// appone's own nimcache
N_LIB_PRIVATE N_NIMCALL(NI, maxVal__appone_u2)(NI a_p0, NI b_p1) { ... }

// apptwo's own nimcache -- an independent, byte-different copy of the same instance
N_LIB_PRIVATE N_NIMCALL(NI, maxVal__apptwo_u2)(NI a_p0, NI b_p1) { ... }
```

`diff`ing the two generated files showed the *only* difference was the mangled-name prefix — everything else, including the function body logic, was identical. The same duplication was confirmed for multiple `bin` entries inside a *single* Nimble package (`nimble build` launches one independent `nim c` process per `bin` entry, each with its own `nimcache`).

**Backtracking inspection found a deeper pathology: the instantiation key depends on caller order, not just declaration/type/const-generic value.** Within a single `NimBuildInvocation`, swapping the order of two `import` statements in an otherwise byte-identical program changed the generated symbol name for a shared generic instantiation:

```
import caller_a; import caller_b  →  maxVal__caller95a_u4
import caller_b; import caller_a  →  maxVal__caller95b_u4
```

This is a stronger instability than the Rust-side `Cgu` problem (which was at least stable per crate boundary) — Nim's own instance identity here depends on an arbitrary factor (source order) rather than any semantic property of the program.

### Activities/state transitions (N1-N9)

N1 (manifest construction) → N2 (dependency resolution) → N3 (`NimBuildInvocation` startup) → N4 (module reachability resolution) → N5 (per-module semantic analysis + C generation, 1:1) → N6 (monomorphization) → N7 (C compilation) → N8 (linking, the convergence point) → N9 (`{.exportc.}` registration).

**N1's start condition (corrected, issue #83)**: N1's input (`.nimble` change detection) never reads A0's output, so N1 does not need to wait for A0 to complete — at build-invocation time, A0, A1, and N1 all start simultaneously as independent actions depending only on external input. Both A1 and N1 simply follow the existing demand-ready execution principle: start work the moment it can start.

**N6's own guarantee**: within a single `NimBuildInvocation`, it carries the same deduplication guarantee as A7 — a given `NimInstantiationKey`'s instance is placed exactly once, and is handled idempotently even when requested simultaneously by multiple callers (confirmed by [Measurement 6](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/76#issuecomment-5713582004)). Deduplication across `NimBuildInvocation` boundaries is not guaranteed (see "Structural differences from the Rust side" above) — note this is a different-scoped guarantee from A11's `LinkSymbol` insertion guarantee (unique across the whole dependency closure, regardless of language or execution boundary).

| Activity | Start condition | End condition | Responsibility |
|---|---|---|---|
| N1 | `.nimble` file changed | Package declaration (`srcDir`, `bin`, `requires`) confirmed | Guarantees one manifest per package; does not resolve dependencies (N2's job) |
| N2 | `requires` declaration or a dependency's version changed | Each package's required external-package path is resolved | Guarantees single-solution consistency, Rust-side `PackageResolution`-equivalent; does not analyze modules (N3's job) |
| N3 | Once per `bin` entry, or per explicit `nim c <file>` invocation | Entry-point module confirmed, semantic-analysis phase entered | Guarantees each invocation owns an independent semantic-analysis context (confirmed by measurement: no compiler state is shared across invocations); does not decide reachable modules (N4's job) |
| N4 | `NimBuildInvocation` started | Reachable `NimModuleUnit` set confirmed (direct, transitive, and implicit stdlib imports) | Guarantees complete resolution of reachability including transitive imports; does not analyze individual modules (N5's job) |
| N5 | Per module found reachable by N4 | One `GeneratedCModule` confirmed | Guarantees 1 module = 1 generated `.c` file within a single `NimBuildInvocation`; does not itself guarantee monomorphization deduplication (N6's job, treated as a separate concern from semantic analysis) |
| N6 | A module uses a `GenericDeclaration` with concrete type arguments/const-generic values | `NimInstantiationKey` confirmed and its instance placed in the declaring module's `GeneratedCModule`; calling modules get an extern prototype only | Guarantees deduplication **only within the same `NimBuildInvocation`** (confirmed); does **not** guarantee deduplication across `NimBuildInvocation` boundaries (confirmed to be absent in existing `nim c` — this is exactly the boundary-elimination target LAMINARIA must resolve) |
| N7 | `GeneratedCModule` confirmed | `CCompileCommand` (`gcc -c`) run, `CObjectFile` produced | Guarantees a deterministic 1:1 transform; does not link (N8's job) |
| N8 | All `CObjectFile`s required by the same `NimBuildInvocation` are ready | `LinkCommand` run, `NimExecutable` produced | Guarantees it does not start until every part is ready (confirmed: `linkcmd` enumerates every `.o`); Rust-side A12-equivalent convergence point |
| N9 | N5 (semantic analysis) detects a `{.exportc.}` declaration | Unmangled real name registered in the symbol table | Guarantees an `exportc` declaration always keeps an unmangled real name; the type/arity cross-check against the corresponding Rust-side symbol is out of scope here (owned by the existing `compute_ffi_reachability_nim`, same advisory-downstream framework confirmed in issue #74/#75) |

### Multiplicity table (confirmed)

| Relation | Multiplicity | Basis |
|---|---|---|
| `NimblePackageManifest` : `NimBuildInvocation` | 1 : 0..N | N equals the number of `bin` entries (confirmed against a real multi-`bin` package: `nimble build` launches one independent `nim c` per entry) |
| `NimBuildInvocation` : `NimModuleUnit` (reachable set) | 1 : 1..N | One invocation semantically analyzes every module reachable from the entry point, including transitive imports |
| `NimModuleUnit` : `ModuleImportEdge` | 1 : 0..N | The number of direct `import` statements a module has |
| `NimBuildInvocation` + `NimModuleUnit` (reachable) : `GeneratedCModule` | 1 : 1 | Always 1:1 **within the same `NimBuildInvocation`** |
| `NimModuleUnit` (declaring) : `GenericDeclaration` | 1 : 0..N | The number of generic declarations a module has |
| `GenericDeclaration` : `NimInstantiationKey` | 1 : 0..N | The number of distinct type-argument/const-generic-value combinations |
| `NimInstantiationKey` : generated function instance (**within the same `NimBuildInvocation`**) | 1 : 1 | Confirmed: deduplicated even across module boundaries, with the instance placed in the declaring module and callers holding only an extern declaration |
| `NimInstantiationKey` : generated function instance (**across `NimBuildInvocation` boundaries**) | 1 : 0..N | Confirmed: N equals the number of independent `NimBuildInvocation`s that request the same key — **this is the Nim-side boundary-elimination target, structurally identical to the Rust-side `Cgu` problem** |
| `GeneratedCModule` : `CCompileCommand` | 1 : 1 | One `gcc -c` per generated `.c` file |
| `CCompileCommand` : `CObjectFile` | 1 : 1 | — |
| `CObjectFile` (set) : `LinkCommand` | 0..N : 1 | One link combines every `.o` |
| `LinkCommand` : `NimExecutable` | 1 : 1 | — |
| `NimModuleUnit` + declaration : `ExportedSymbol` | 1 : 0..N | The number of `{.exportc.}`-annotated declarations |

### Structural differences from the Rust side (confirmed by measurement)

1. **Semantic analysis and code generation are fused into N5** — the Rust side's separation of `SemanticFact` (A6) and `LoweredModule` (A9) has no counterpart on the Nim side.
2. **`NimInstantiationKey`'s multiplicity has two tiers**: 1:1 (deduplicated) within the same `NimBuildInvocation`, but 1:0..N (duplicated) across `NimBuildInvocation` boundaries. Measurement confirmed that duplicate compilation structurally identical to the Rust-side `Cgu` problem is real on the Nim side too — the same standard-library modules and monomorphized instances are fully regenerated between independent compiles.
3. **A pathology found by backtracking inspection**: the key `nim c` actually uses to identify a monomorphized instance mixes in the "module that first, syntactically, requested it" — information semantically unrelated to the declaration identifier, type arguments, or const generic values. Demonstrated concretely: for the identical program, merely swapping the order of two `import` statements changes the generated symbol name. This is worse than the Rust-side `Cgu` problem in that it depends on a more arbitrary factor (import order). LAMINARIA's own `NimInstantiationKey` design must use exactly the same three elements as the Rust side (declaration identifier, type arguments, const generic values) and deliberately exclude caller-dependence.
4. **The `{.exportc.}` boundary is stable and caller-independent** — unlike the instability of non-exportc internal symbols, FFI-boundary symbol names are never mangled and stay stable.

### Cost-scale judgment (corrected by issue #76 and issue #81)

Issue #76's initial measurement (a small fixture with one function and three stdlib imports) found the duplicate-compilation cost (tens to hundreds of milliseconds per duplicated site) two to three orders of magnitude smaller than the scale issue #52 confirmed on the Rust side (45 seconds out of 215 seconds, Phase-2 cost). Issue #81 re-measured against a fixture closer to real scale (a library using ten standard-library modules), and found the redundant cost non-negligible: **1.339 seconds for the `gcc -c` phase alone.**

**Correction**: the generalization "the duplication cost is small because the scale is small" is wrong — it is a scale-dependent judgment. That said, the 1.3 seconds measured here is still more than an order of magnitude below the Rust side's 45 seconds (issue #52), so [project_laminaria_goal_and_bottleneck]'s conclusion (LAMINARIA's primary goal is build-time speedup, and the main front is Rust-side LLVM; current priority is low) is not overturned. How far this duplicated cost actually accumulates at LAMINARIA's own real dependency-closure scale remains an open question requiring further measurement.

### The `when defined(...)` condition graph, measured (issue #78)

Confirmed by real measurement the property issue #71's Nim-side mapping evaluated ("`when defined(...)` is structurally the same as Rust's `cfg` attribute"). A fixture with 50 independent `when defined(featureN)` branches (each conditionally declaring one function) was compiled with all branches inactive and all branches active; compile time was nearly identical (0.278s vs. 0.287s) — **no exponential cost increase tied to the number of combinations (2^50); it scales linearly with the number of declarations (50)**. Issue #71's `SourceUnitGraph` design principle (the graph's node count is linear in declaration count, not exponential in feature combinations) holds on the Nim side too.

One difference from Rust's `cfg` attribute, however: whether a define is active changes what actually gets semantically analyzed. A declaration disabled by `when` is not merely "kept in the syntax tree but never selected" — **it never receives a mangled-name counter at all and is never a semantic-analysis subject in the first place** (confirmed: whether an earlier branch was active affects the mangled-name counter assigned to later declarations).

### Nimble's dependency resolution in practice (issue #77/#79)

Confirmed by real measurement the property issue #71 established for the Rust-side `PackageResolution` ("produces a single, consistent solution across the whole dependency graph"), on the Nim side. Against a real, published package (`zero_functional`), a deliberately contradictory constraint (`>= 1.0.0` and `< 1.0.0` simultaneously) was rejected outright as `Unsatisfiable dependencies`. A non-contradictory constraint (`>= 1.0.0`) resolved deterministically to a single version (`1.3.0`). **Nimble never allows multiple versions to coexist** — unlike Cargo, which can permit multi-version coexistence in some cases. Given this property, `NimInstantiationKey` was confirmed not to need version information (each package always resolves to exactly one version across the whole dependency closure).

The originally attempted verification path (issue #77: give a single local package two git-tagged versions and resolve directly) could not be carried out, due to structural constraints in Nimble (an unregistered package name cannot be resolved by name at all; the `file://` scheme does not support tag selection) — itself a finding: only packages registered in the official package index are eligible for name-based dependency resolution, unlike Cargo's `path = "../local-crate"` construct.

### Macro/template expansion in practice (issue #80)

Corrected, by real measurement, the insertion point issue #74 provisionally placed macro/template expansion at ("between N2 and N4"). Using a fixture combining a `template` declaration with a `when defined` branch:

- The template declaration itself is always semantically analyzed regardless of whether it is actually used (inside an active `when` branch) — an `XDeclaredButNotUsed` diagnostic fires even when unused.
- Template-call expansion happens **transparently inside N5 (semantic analysis + C generation), fused with optimizations such as constant folding** — a call to `double(21)` appeared in the generated C code fully folded to the constant `42`, with no trace of an independently held pre-expansion/post-expansion intermediate state.

This confirms, by measurement, that existing `nim c` uses a "transparent preprocessing" approach rather than "hold pre-expansion and post-expansion syntax trees as separate entities." Whether LAMINARIA's own macro/template expansion implementation should follow this existing fused approach is a separate design decision (Nim's own approach is not necessarily best for correctness or maintainability). Cache-invalidation conditions were also confirmed to follow N5's existing invalidation axis (a change to the content of the target `SourceUnit` itself) without needing a new axis.

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
- Issue #77: real-measurement of Nimble's dependency resolution (single-solution verification achieved via issue #79; found a structural constraint on package-name resolution)
- Issue #78: real-measurement confirmation of the `when defined(...)` condition graph (no combinatorial explosion; linear in declaration count)
- Issue #79: real-measurement of cross-package version-constraint conflicts in Nimble (fail-fast; no multi-version coexistence)
- Issue #80: confirmation of macro/template expansion's concrete insertion point (fused into N5; a transparent-preprocessing approach)
- Issue #81: scale-dependent real-measurement of Nim's duplicate-compilation cost (negligible at small scale but scale-dependent; reaches the low-seconds range at more realistic scale)

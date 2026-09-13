# First Cross-Ecosystem Dependency-Graph Experiment

## Decision being tested

Can LAMINARIA demand-resolve a typed dependency closure spanning Cargo, Nimble, C, and C++ and use that closure to build and run one ordinary native executable without hiding target compilation inside package-manager commands?

This is G1/G2 of the current [near-term research program](../../near-term-research-program.md). A single Rust/Nim call path and an owned WASM module do not satisfy this decision.

The experiment starts from the [prior-art survey](../../02-research-areas/toolchains/cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md). It must evaluate the Package Calculus as the package-level formal baseline and record any semantic extension or divergence instead of claiming cross-ecosystem package resolution itself as novel. Spack's ASP concretizer is the implementation baseline for version/variant/compiler/architecture solving; Bazel/Buck2 are action-graph baselines; rustc/Salsa/Pluto and MLIR/ThinLTO are incremental-semantic and IR-summary baselines.

## Fixed positive graph

Use one minimal project containing:

1. one Cargo package with an explicit feature and target condition;
2. one Nimble package consumed by the program;
3. one C library supplied as source or an identified archive;
4. one C++ library requiring one explicit adapter or instantiation unit; and
5. one native executable whose observable result depends on all four inputs.

Also include one deliberately unreachable package/provider candidate, one unused source/module or translation unit, one unused semantic/IR item, one unused archive member or symbol, and one apparently unreferenced item that must remain live because of an FFI export, constructor, linker directive, or runtime contract.

The graph must preserve package/version/feature identity, host-versus-target role, source/header inputs, generated adapter provenance, toolchain/ABI constraints, artifact producers, symbols, link order, and the final executable demand.

The graph must also contain a first-class `TestContract` for the exact production executable, plus any separately identified test executable, harness executable, test data, controls, observations, and target-execution requirements. Test-only dependencies must be distinguishable from production dependencies.

## Required executable evidence

Production resolver/planner/executor tests must directly establish that:

- only the demanded closure is expanded;
- every package, source-semantic, language/intermediate-IR, ABI, symbol, and link obligation reaches `Discharged`, `Externalized`, or `Rejected`, with the production operation that justifies the transition;
- every compile, adapter, archive, and link action has explicit inputs and outputs;
- the native executable is produced and run with the expected result;
- the recorded final-link inputs account for every required foreign artifact; and
- Cargo/Nimble metadata ingestion does not silently execute an opaque target build;
- no parse/lowering/compile action runs for items proven unreachable early;
- final symbol/section evidence excludes dead code while retaining FFI, constructor, dynamic, and runtime roots; and
- every retained or pruned node has a root path or a conservative-retention/pruning reason from the production graph.
- the exact production executable digest, not only an instrumented/test-profile variant, is executed by the harness;
- the harness exercises the Rust/Nim/C/C++ observable path and checks exit/output plus required ABI/symbol/runtime behavior;
- one missing or incompatible dependency is not recovered accidentally from the host environment; and
- test-only package, symbol, hook, and runtime dependencies do not enter the release artifact.

The resulting artifact must also be exercised in a clean runtime environment without Cargo, Nimble, Rust/Nim/C/C++ compilers, project sources, or build-only dependencies. This test is evidence that the original ecosystem obligations were discharged, not merely copied into a package-manager or store closure. A self-contained or relocatable-bundle profile must run using only its declared runtime artifacts. A system-integrated profile must enumerate and preflight its external ABI/symbol/runtime contracts.

Add one negative case that makes exactly one version, feature, target, ABI, symbol, or toolchain constraint incompatible. The resolver must return a structured explanation before compilation starts.

At least one case must require feedback across layers: a source-derived module/type/FFI fact or an IR-lowering result changes or rejects a provisional package/toolchain/artifact choice. A one-way `package resolve -> compile -> link` pipeline does not test the coupled-resolution hypothesis.

## Efficiency comparison

For the same graph, compare no pruning, linker GC only, compiler DCE plus linker GC, and cross-layer early pruning plus DCE/linker GC. Record wall-clock time, peak RSS, executable size, package/source/semantic/IR/artifact/symbol nodes retained and pruned, recomputed nodes, and external actions avoided for cold, no-op, leaf-change, root-set-change, and feature/target-change scenarios. Compare performance only when observable behavior is identical.

## Package Calculus comparison (fixed G1 workload)

This section records the required comparison against the Package Calculus
formal model [Gibb et al., "Package Managers à la Carte," ICFP 2026] for
the exact fixed workload `fixtures/cross-ecosystem-native-executable/`
uses (`app` [Cargo], `doubler` [Nimble], `cadd@{1,2}` [C], `cppmax` [C++]).
It is documentation evidence, not a machine-readable fixture and not a
test oracle -- no YAML catalog, fixture-only validator, or validator test
exists for it (see that fixture's own `README.md`).

### Package candidates and constraints as Package Calculus terms

| Ecosystem | Package term | Version/variant | Declared constraint (Package Calculus reading) |
| --- | --- | --- | --- |
| Cargo | `app` | `0.1.0` | feature `use_nim_double` (an optional-dependency/feature edge in the package term) |
| Nimble | `doubler` | `0.1.0` | `requires "nim >= 2.0.0"` (a version-range constraint edge) |
| C | `cadd` | `1.0.0` vs `2.0.0` | two candidate versions of the same logical package identity, otherwise unconstrained at the package layer |
| C++ | `cppmax` | `1.0.0` | unconstrained at the package layer |

A Package Calculus resolver over this input derives exactly one
uncontroversial result: `app`, `doubler`, and `cppmax` each resolve to
their single declared candidate, and `cadd` resolves to **both**
`1.0.0` and `2.0.0` as equally valid candidates, because nothing in the
package-level term (name, version range, feature flag) distinguishes
them -- the two `cadd` versions differ only in which C symbol their
*source* exports, a fact no package manifest in this fixture declares.

### What Package Calculus derives at the package layer

- A satisfying assignment exists for `app`/`doubler`/`cppmax` (trivial,
  single-candidate).
- For `cadd`, Package Calculus alone cannot narrow `{1.0.0, 2.0.0}` to
  one candidate: both satisfy every constraint expressed in package
  terms (name, version range). Nothing in the formal model as published
  reads a translation unit or a header.

### The exact semantic extension LAMINARIA adds

G1 adds one constraint kind Package Calculus's own term language does
not carry: a **source-derived required symbol** (`c_add`, discovered by
`laminaria_ir::foreign_discover` scanning `app/src/main.rs`'s real
`extern "C"` block) that a package candidate must satisfy via its own
**declared export** (`c_add`, discovered by
`laminaria_ir::c_header_discover` reading `c/cadd/v1/cadd.h`, versus
`c_add_v2` declared by `c/cadd/v2/cadd.h`). This single extension is
exactly what breaks the `cadd@{1.0.0, 2.0.0}` tie the package layer
alone leaves open, and exactly what the negative case exercises when
only `cadd@2.0.0` is offered: `resolve` rejects it
(`RejectionReason::MissingSymbol`, obligation `Symbol:c_add`) before G1
ever proposes a `RequiredAction`, i.e. before any compiler/archiver/
linker would run.

### The exact divergence: what Package Calculus does not model

Package Calculus, as published, resolves package-term satisfiability
and stops there. It does not itself model, and this experiment does not
claim it models:

- source-language semantics or an intermediate representation (Rust/Nim
  lowering, C/C++ translation units);
- ABI/target-triple compatibility at a foreign-function boundary;
- symbol-level export/import matching (the `c_add` vs. `c_add_v2`
  distinction above);
- archive/object production, or discharging a link-time obligation.

LAMINARIA's own `ObligationKind::{SourceModule, SemanticFfi, AbiTarget,
Symbol, ArtifactProduction, FinalLink, Runtime, Provenance}` in
`laminaria_plan::dependency_graph` exist precisely to cover this gap;
none of them is a Package Calculus concept, and none of this experiment's
findings claims otherwise.

### Non-claim

This experiment does not claim cross-ecosystem package resolution
itself (choosing one Cargo, one Nimble, one C, and one C++ candidate
together) is novel -- Package Calculus already formalizes exactly that
problem at the package layer, and Spack's ASP concretizer already
solves a considerably richer version/variant/compiler/architecture
instance of it. The candidate contribution under test here is narrower:
whether feeding a source-derived symbol fact back into an
otherwise-tied package-layer choice, before any build/compile/link work
starts, is both correct and load-bearing (i.e. changes the resolution
outcome, as the `cadd@1.0.0`/`cadd@2.0.0` case above demonstrates) for
at least one real cross-ecosystem workload.

## Stop condition

Stop after obtaining a correct positive closure, evidence that all obligations were discharged or externalized, a pre-compilation rejection, a harness-tested exact production native binary, and enough measurements to adopt, reject, or reformulate one graph representation or resolution algorithm. Do not extend the experiment to complete ecosystem coverage, WASM, distributed execution, or self-hosting.

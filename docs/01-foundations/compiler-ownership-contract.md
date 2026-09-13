# LAMINARIA compiler ownership contract

## Purpose and authority

LAMINARIA researches and develops **its own compiler, intermediate representations, and scheduler for Rust and Nim**. It is not a toolchain orchestrator whose compilation engine is Cargo/rustc, the Nim compiler, or an existing compiler backend. This contract corrects the ambiguity identified in the [2026-09-10 audit](../05-history/research-intent-audit-2026-09-10.md). It governs research objectives, issue dependencies, and acceptance criteria; it does not claim the current implementation already meets them.

The project itself is implemented in Rust and Nim and must ultimately compile itself through that same independent path. The implementation language split is not a split into Rust-toolchain and Nim-toolchain schedulers.

The current artifact goal is an ordinary native executable that the host OS can launch directly. WebAssembly is an optional target, not this contract's default target, the current milestone, or a substitute for the native path.

## Ownership of the target compilation path

The required direction is:

```text
requested native executable
  → typed dependency closure from Cargo / Nimble / C / C++ metadata
  → package / source / artifact / toolchain / ABI / symbol / link graph
Rust source / Nim source / both + resolved Rust/Nim dependency sources
  → LAMINARIA language processing and semantic analysis
  → LAMINARIA-owned IR(s), semantic facts and provenance
  → LAMINARIA analysis / transformations / partition decisions
  → LAMINARIA planning and resource-aware compiler-work scheduling
  → LAMINARIA target lowering and code generation
  → LAMINARIA-produced native objects
declared C/C++ dependencies
  → prebuilt native artifacts or explicit C/C++ compile/adapter actions
LAMINARIA-produced objects + foreign native artifacts
  → native executable through explicit runtime / assembly / link contracts
```

This is an ownership contract, not a fixed sequence of passes. Planning, analysis and execution may interact incrementally. The number/form of IRs, SSA/CFG use, partition granularity and backend boundaries remain research questions. The build Action Graph and `PlanningInput -> ExecutionPlan` alone are not a language IR.

LAMINARIA must derive semantic facts from supported source constructs, not require precompiled MIR, Nim-generated C or merged LLVM IR as its source of meaning. Compiler work units must be owned and executable by LAMINARIA, not merely labels around another compiler's internal work.

## Separate roles; no implicit transition

| Role | Permitted use of existing tools | What it proves |
| --- | --- | --- |
| Package metadata/candidate acquisition | Cargo/Nimble/C/C++ metadata, manifests, lockfiles, source acquisition, registry and system-library facts | Inputs to the LAMINARIA resolver |
| Cross-ecosystem dependency resolution and discharge | LAMINARIA jointly solves version, feature, target, host/target, source-semantic, language/intermediate-IR, ABI, symbol, artifact, and link constraints in one typed graph | The closure required by the native executable, each obligation's discharge/externalization, and explanations for selections/rejections |
| Lexical/syntactic parsing | A parsing library, a parser generator, or an existing compiler's own lexer/parser logic, used purely as a syntax-only component (tokens, concrete/abstract syntax tree, source positions) | A syntax tree, not meaning |
| Reference / baseline | Existing compilers, LLVM and build systems in explicitly selected comparison experiments | Behavior and costs under the tested reference contract |
| External bootstrap | Existing tools build the initial research executable | A starting executable, not independent compiler self-hosting |
| Delegated-build baseline | Current coarse Cargo/Nim project-build and self-build experiments | Planner/process integration and baseline measurements only |
| Declared foreign-native dependency | Compile an explicitly modeled C/C++ source/adapter unit, or consume an identified object/archive/shared library, as a dependency of LAMINARIA-produced target code | The foreign artifact and link input, not delegated Rust/Nim compilation |
| Independent compilation | LAMINARIA owns source semantics, IR, transformations, code generation and compiler-work scheduling | Candidate evidence for the project goal |

A package manager producing a lockfile inside one ecosystem is not the same as LAMINARIA discharging package-choice, source-semantic, language/intermediate-IR, ABI, symbol, and link obligations across Cargo, Nimble, C, and C++ into the final artifact. Nor is this merely copying the original graph into a deployment closure. Resolution must not silently run compilation through a build script, procedural macro, plugin or transitive tool invocation. Such work needs an explicit LAMINARIA-supported implementation contract; unsupported constructs/dependencies return a diagnostic. Dependency acquisition is not permission to invoke `cargo build`, `rustc`, `nim c`, `nim cpp`, nlvm or Nimony for Rust/Nim target compilation, nor to route LAMINARIA-owned Rust/Nim semantics through generated C/C++ or another existing compiler backend.

This restriction does **not** prohibit C/C++ compilation required by a declared foreign-native library dependency. LAMINARIA must preserve the distinction: an external C/C++ compiler may compile an identified foreign source or generated adapter unit, but it must not compile C/C++ emitted as the implementation of the Rust/Nim target unit. The foreign inputs, headers, flags, toolchain, outputs and link edges must be visible in the Program/Action Graph rather than hidden inside package resolution or an opaque outer build.

## C/C++ library reuse is a first-class requirement

Nim's practical ecosystem advantage includes direct use of C and C++ libraries. LAMINARIA must preserve that advantage even when its owned Nim path lowers source directly to LAMINARIA IR and skips Nim-generated C/C++.

LAMINARIA's own Rust/Nim implementation should prefer an established, suitable C/C++ library over reimplementing equivalent functionality when the dependency's correctness, portability, licensing, maintenance, security and measured cost satisfy the project requirements. This is a design decision subject to evidence, not a requirement to rewrite the library in Nim or Rust merely to keep all source in those languages.

For target projects, supported `importc`/`importcpp`-style declarations must lower to explicit foreign declarations and calls while retaining symbol identity, type/layout, calling convention, ownership/lifetime, exception/unwind and runtime obligations. The resulting LAMINARIA-produced object must be linkable with separately produced or prebuilt foreign objects, archives and shared libraries.

Some C++ facilities have no pre-existing linkable symbol. Templates, inline/header-only APIs, overload resolution, constructors/destructors and ABI-specific calls may require an explicit C++ instantiation or adapter unit. LAMINARIA may generate and compile that unit as a foreign dependency action. The generated source, reason, compiler/standard-library ABI, flags and output object remain inspectable; it is not an implicit fallback for Nim target compilation.

The canonical requirements, graph model and vertical-slice acceptance are defined in [Nim C/C++ library integration](../02-research-areas/compiler/nim-c-cpp-library-integration.md).

**Lexical/syntactic parsing reuse, precisely bounded.** Reuse limited to producing
tokens, a concrete/abstract syntax tree, and source positions is permitted —
including a parsing library (e.g. Rust's `syn`), a parser generator, or an
existing compiler's own lexer/parser logic studied and adapted as a syntax-only
component. This does not by itself establish compiler ownership and does not
extend to name resolution, type inference/checking, ownership/effect
interpretation, constant evaluation, owned-IR construction, analysis,
transformation, work partitioning, scheduling, or code generation — all of
which remain LAMINARIA's own responsibility. An already semantically-analyzed
representation (a typed AST, HIR, MIR, or equivalent) is not "just parsing" and
must not become a required input. Invoking `rustc`'s or Nim's own compiler
execution to obtain an AST is never a required entry point on the production
path — a reused parsing component must be one LAMINARIA controls directly
(in-process or a library dependency it invokes, not an opaque external
compiler process), including its execution unit, parallelism, and memory
lifetime. Macro expansion and compile-time execution are not covered by "just
parsing"; they need their own explicit implementation contract, and an
unsupported case returns a diagnostic rather than being silently skipped or
delegated. A reused parsing dependency is not exempted from LAMINARIA's own
eventual self-build target merely because it sits at the syntax layer.

LLVM/Cranelift/GCC projection may be studied as a **comparison experiment**. Making LLVM optional or invoking it as a library does not by itself establish compiler ownership. Existing-compiler orchestration, modified upstream compilers, and finer-grained invocation of their passes are not substitutes for the independent path. Reuse of algorithms or libraries is evaluated by the responsibilities actually retained; this contract neither mandates rewriting every utility nor grants an unexamined backend exception. Runtime, assembler and linker boundaries need explicit design and evidence and must not conceal delegated compilation.

Reference behavior is not an oracle for undefined behavior or an unsupported language contract. Record the semantic contract first and preserve discrepancies and uncertainty.

## Single-language projects

Rust-only, Nim-only and mixed projects all use the same LAMINARIA compiler/IR/planner/scheduler substrate. The requested language capabilities and transitive dependencies determine work; no dummy sources or unused-language compiler installation is required.

A delivered compiler must not need rustc to compile a supported Rust-only project or Nim's compiler to compile a supported Nim-only project. LAMINARIA's own Nim planner/runtime remains present for either input language. An external bootstrap needing both toolchains is a separate preparation operation.

Single-language speed/memory improvements are an evaluation opportunity, not a promise or a prerequisite for mixed-language research. This is not a rustc-only redevelopment project.

## Self-build milestones

Keep these milestones separate:

1. External tools produce stage0.
2. A delegated driver rebuilds the Rust host and Nim planner using external compilers: **bootstrap/delegated-driver evidence only**, even if repeated through stage2.
3. A LAMINARIA compiler processes a declared supported source subset through its own IR, transformations, scheduler and target generation without existing compilers.
4. That coverage grows to the actual Rust + Nim implementation and its dependency closure. Stage0's independent compiler produces stage1; stage1's own independent compiler produces stage2.

Only milestone 4 proves the intended compiler self-hosting. Verify isolated outputs, input/dependency/producer identities, actual compiler path, conformance and canonical plan/artifact comparisons. A copied binary, unused Nim component, or externally compiled generation cannot satisfy it. Unsupported dependencies remain an explicit coverage gap.

Current `self-build` and `build` command names do not change this classification. Renaming or gating the CLI requires a subsequent code change; documentation must not imply that has happened.

## Research order and acceptance

Center the current work on **#8 + #22 + #44**, connecting only the required parts of #3/#4/#5/#6/#7. Resolve a small closure containing a Cargo crate, Nimble package, C library, and C++ library; compile/link it through the production Nim planner/Rust runtime; and launch the resulting native executable. A single source call or hand-transcribed IR interpreter is useful earlier evidence but is not cross-ecosystem dependency resolution or binary delivery.

Use the necessary portions of #10/#11/#18–#21 for identity, semantics, path and resource evidence in parallel. #7/#12 add sound reuse and invalidation on this same path. #26 exposes Rust-only/Nim-only/mixed inputs without changing compiler ownership. #4 studies runtime/ABI integration; linking the Nim planner is not the prerequisite that replaces compiler research.

Full language coverage, distributed execution, WASM, every LLVM concept, and all profile matrices need not finish before this experiment. Conversely, the dependency graph must not collapse into opaque package-manager command sequences, and an ordinary runnable native binary belongs in the current vertical slice.

Evidence must distinguish source-derived IR from hand-authored IR, legal/rejected transformations, logical dependencies from runtime placement, and actual in-process compiler events from process-level observations. Negative tests prevent existing compiler invocation or silent fallback. Correctness and executed/non-executed work are checked independently of speedup.

## Hardware, persistence and heterogeneous nodes

Vertical integration and horizontal partitioning are joint design choices from the outset. Preserve analyses and intermediates in memory when useful; split or materialize work only when justified by correctness, reuse, locality, recovery and resource costs.

Research physical/logical cores, core classes, caches/NUMA, memory capacity/bandwidth, storage/network latency and bandwidth rather than scheduling by a single core count. Neither “always write to disk” nor “network is always faster” is a rule.

Windows, macOS and Linux/Raspberry Pi participation must distinguish execution host from compilation target, ISA/ABI, SDK/sysroot, runtime, target features and trust. Concurrent cross-compilation does not imply the producing node can run or validate the target artifact. These requirements shape the IR/partition and persistence contracts; see [horizontal distribution](../02-research-areas/execution/horizontal-distribution-research.md).

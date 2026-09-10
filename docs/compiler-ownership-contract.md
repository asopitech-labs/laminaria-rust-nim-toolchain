# LAMINARIA compiler ownership contract

## Purpose and authority

LAMINARIA researches and develops **its own compiler, intermediate representations, and scheduler for Rust and Nim**. It is not a toolchain orchestrator whose compilation engine is Cargo/rustc, the Nim compiler, or an existing compiler backend. This contract corrects the ambiguity identified in the [2026-09-10 audit](research-intent-audit-2026-09-10.md). It governs research objectives, issue dependencies, and acceptance criteria; it does not claim the current implementation already meets them.

The project itself is implemented in Rust and Nim and must ultimately compile itself through that same independent path. The implementation language split is not a split into Rust-toolchain and Nim-toolchain schedulers.

## Ownership of the target compilation path

The required direction is:

```text
Rust source / Nim source / both + resolved dependency sources
  → LAMINARIA language processing and semantic analysis
  → LAMINARIA-owned IR(s), semantic facts and provenance
  → LAMINARIA analysis / transformations / partition decisions
  → LAMINARIA planning and resource-aware compiler-work scheduling
  → LAMINARIA target lowering and code generation
  → target artifacts with explicit runtime / assembly / link contracts
```

This is an ownership contract, not a fixed sequence of passes. Planning, analysis and execution may interact incrementally. The number/form of IRs, SSA/CFG use, partition granularity and backend boundaries remain research questions. The build Action Graph and `PlanningInput -> ExecutionPlan` alone are not a language IR.

LAMINARIA must derive semantic facts from supported source constructs, not require precompiled MIR, Nim-generated C or merged LLVM IR as its source of meaning. Compiler work units must be owned and executable by LAMINARIA, not merely labels around another compiler's internal work.

## Separate roles; no implicit transition

| Role | Permitted use of existing tools | What it proves |
| --- | --- | --- |
| Package/dependency resolution | Cargo/Nim ecosystem metadata, manifests, lockfiles, source acquisition and resolution | Dependency inputs, not compilation |
| Reference / baseline | Existing compilers, LLVM and build systems in explicitly selected comparison experiments | Behavior and costs under the tested reference contract |
| External bootstrap | Existing tools build the initial research executable | A starting executable, not independent compiler self-hosting |
| Delegated-build baseline | Current coarse Cargo/Nim project-build and self-build experiments | Planner/process integration and baseline measurements only |
| Independent compilation | LAMINARIA owns source semantics, IR, transformations, code generation and compiler-work scheduling | Candidate evidence for the project goal |

Resolution must not silently run compilation through a build script, procedural macro, plugin or transitive tool invocation. Such work needs an explicit LAMINARIA-supported implementation contract; unsupported constructs/dependencies return a diagnostic. Dependency acquisition is not permission to invoke `cargo build`, `rustc`, `nim c`, `nim cpp`, nlvm, Nimony, or C/LLVM compilation on the target-production path.

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

Start **#25 + #3 + #6 + #8 together**: a small, declared Rust/Nim source subset, its independently derived semantic/IR contract, an executable transformation, and LAMINARIA-owned work scheduled by the production Nim planner/Rust runtime. Establish a narrow executable end-to-end slice; a hand-transcribed IR interpreter is useful earlier evidence but is not source compilation or target generation.

Use the necessary portions of #10/#11/#18–#21 for identity, semantics, path and resource evidence in parallel. #7/#12 add sound reuse and invalidation on this same path. #26 exposes Rust-only/Nim-only/mixed inputs without changing compiler ownership. #4 studies runtime/ABI integration; linking the Nim planner is not the prerequisite that replaces compiler research.

Full language coverage, distributed execution, every LLVM concept and all profile matrices need not finish before a small compiler experiment. Conversely, independent compilation must not be deferred as optional “deeper integration” while delegated builds become the product goal.

Evidence must distinguish source-derived IR from hand-authored IR, legal/rejected transformations, logical dependencies from runtime placement, and actual in-process compiler events from process-level observations. Negative tests prevent existing compiler invocation or silent fallback. Correctness and executed/non-executed work are checked independently of speedup.

## Hardware, persistence and heterogeneous nodes

Vertical integration and horizontal partitioning are joint design choices from the outset. Preserve analyses and intermediates in memory when useful; split or materialize work only when justified by correctness, reuse, locality, recovery and resource costs.

Research physical/logical cores, core classes, caches/NUMA, memory capacity/bandwidth, storage/network latency and bandwidth rather than scheduling by a single core count. Neither “always write to disk” nor “network is always faster” is a rule.

Windows, macOS and Linux/Raspberry Pi participation must distinguish execution host from compilation target, ISA/ABI, SDK/sysroot, runtime, target features and trust. Concurrent cross-compilation does not imply the producing node can run or validate the target artifact. These requirements shape the IR/partition and persistence contracts; see [horizontal distribution](horizontal-distribution-research.md).

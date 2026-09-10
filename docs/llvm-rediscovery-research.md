# LAMINARIA — LLVM Rediscovery Research

## Ownership correction (2026-09-10)

The [compiler ownership contract](compiler-ownership-contract.md) governs research objectives and acceptance.

Own compiler/IR/scheduler development is the main path, not optional later integration. Cargo/Nim ecosystem tools may resolve dependencies; existing compilation routes below are reference/observation or external-bootstrap baselines, not target-build alternatives. The Action Graph is not a substitute for a language IR.

## Position

LAMINARIA does not treat LLVM as a prerequisite foundation.

LLVM is one major solution produced by decades of compiler engineering and is therefore an important prior art, observation target, decomposition target, and comparison baseline. But LAMINARIA is not a project to connect Rust and Nim to LLVM, nor to transcribe LLVM's existing internal boundaries into an Action Graph.

LAMINARIA starts from a different question: if Rust and Nim are treated as one computational system, what compiler/backend architecture is actually required?

> Do not adopt LLVM concepts because they already exist. Rediscover why those concepts became necessary, then reconstruct only the forms justified by LAMINARIA's own experiments.

Arriving at an LLVM-like architecture is acceptable. Arriving at a different architecture is equally acceptable. Agreement with LLVM is not the success criterion; independent derivation is.

## Starting assumption

Concrete performance research using shape, alias, effect, iteration, reduction, specialization, and other facts before low-level lowering is defined in [Upstream Performance Optimization Research](upstream-performance-optimization-research.md).

Different compiler frontends are not assumed to produce the same IR, bitcode, machine code, or optimization opportunities from semantically corresponding programs or even from equivalent C ABI surfaces.

Rust/rustc, Nim 2, Nimony/Nim 3, Clang, and other compilers carry different language semantics, internal representations, lowering policies, runtime obligations, and attribute/metadata policies.

Therefore LAMINARIA does not begin with this convergence assumption:

```text
Rust ─┐
      ├→ LLVM IR → shared optimization → machine code
Nim  ─┘
```

Instead the research space is:

```text
Rust semantic facts ─┐
                     ├→ transformation / information loss / normalization
Nim semantic facts  ─┤
                     ├→ candidate shared computational representation
Other compiler facts ┘
                     ↓
             backend planning / optimization
                     ↓
                target artifacts
```

## Core research questions

1. Which semantic facts from Rust and Nim are actually required for cross-language planning?
2. At which compiler stages are those facts preserved, transformed, or lost?
3. Which properties of LLVM IR are fundamentally necessary, and which are LLVM-specific design choices?
4. Do SSA, CFG structure, type information, memory models, alias information, attributes, metadata, calling conventions, and data-layout contracts need the same representation in LAMINARIA?
5. Is a pass-based optimization architecture appropriate, and which analyses, transformations, and invalidation relations are genuinely independent?
6. Under what workloads do analysis preservation, pass ordering, fixpoints, interprocedural optimization, and LTO become necessary?
7. Where should the machine-independent / target-dependent boundary actually be drawn?
8. Why do object/link boundaries, LTO, and ThinLTO-like partitioning become useful, and do LAMINARIA's artifact/scheduler constraints lead to the same answer?
9. Can LAMINARIA produce a better common representation from upstream semantic facts instead of attempting to merge independently lowered frontend outputs?
10. If LAMINARIA chooses a different architecture from LLVM, can that choice be justified with correctness, compile latency, memory, I/O, generated-code quality, incrementality, and explainability evidence?

## How LLVM is used

### Prior art

Study the problems LLVM solves and the reasons behind its design, including SSA/IR, DataLayout, attributes and metadata, alias analysis, pass management, analysis preservation/invalidation, IPO, target lowering, code generation, LTO/ThinLTO, and instrumentation.

### Comparison target, not oracle

LLVM optimization remarks and pass instrumentation are evidence of what LLVM decided. They are not proof that LAMINARIA should make the same decision.

### Owned backend and comparison projections

Removing LLVM must not merely leave another existing compiler in charge. Implement owned IR, transformations, target lowering and code generation as the main path. LLVM/Cranelift/GCC projections remain comparison experiments; the independent backend is not an optional future variant.

## Rediscovery method

### Start from semantic workloads

Do not begin research from pre-existing LLVM IR. Define small semantically corresponding Rust/Nim workloads and determine what each compiler knows and how it transforms that information.

Workloads should cover arithmetic/overflow, branches/loops, aggregates, enums, pointers/aliasing, ownership/lifetime, allocation, callbacks, exceptions/panics/unwind, generics/specialization, vectorization, interprocedural optimization, dead-code reachability, and target-feature-sensitive operations.

### Trace differences upstream

A different final IR or machine artifact is not itself a research finding.

```text
observed difference
→ producing stage
→ originating semantic fact / compiler policy
→ transformation
→ information preserved / transformed / lost
```

### Re-test LLVM concepts one by one

For example, do not adopt function attributes merely because LLVM has them. Construct a workload where missing semantic facts break legality or optimization, identify the required fact, design how LAMINARIA should preserve it, and only then decide how it projects to LLVM or another backend.

The same applies to pass managers, analysis caches, LTO summaries, codegen partitioning, and target lowering.

## Candidate semantic/optimization substrate

The structure is intentionally not fixed yet.

```text
Language-specific semantic facts
  ↓
Preserved semantic facts + provenance
  ↓
Cross-language computational relations
  ↓
Optimization requirements / legality facts
  ↓
Backend-specific projection
  ↓
LAMINARIA-owned target lowering / code generation
(comparison only: LLVM | Cranelift | GCC)
```

LAMINARIA does not assume one universal IR is the correct answer. Multiple IRs, typed semantic facts, graph relations, and analysis databases may be more appropriate.

## Rediscovery is not LLVM reimplementation

The goal is not to reproduce LLVM APIs or passes.

Rediscovery means posing LLVM's solved problems again under LAMINARIA's constraints.

When studying ThinLTO, for example, the first question is not merely how to ingest DTLTO JSON. Ask why global summaries are required, what information is sufficient across modules, whether that summary should be identical for Rust and Nim, whether the backend-job partition is optimal for LAMINARIA scheduling, and whether upstream semantic information allows a different partition.

## Insufficient research results

The following are experimental infrastructure, not sufficient research outcomes by themselves:

- producing LLVM IR;
- linking bitcode;
- enabling LTO flags;
- enumerating LLVM passes;
- capturing optimization remarks;
- observing Rust/Nim IR differences;
- mapping LLVM boundaries into an Action Graph.

A research result should instead independently derive a necessary design condition, compare an LLVM alternative with a LAMINARIA-specific candidate, preserve semantic information to enable otherwise difficult cross-language optimization, establish a better LAMINARIA-specific artifact/execution boundary, show that an LLVM concept is unnecessary or differently representable, or independently confirm that an LLVM concept is necessary.

## Impact on existing tracks

Compiler Pipeline Decomposition must explain why a stage exists and what semantic information crosses it, not merely map existing compiler stages.

Native Linking must investigate where differing language semantics should be projected into a common contract, not stop at linkability.

Backend Graph must execute the LAMINARIA-owned optimization/target-generation path and classify existing-backend comparison routes separately.

LLVM White-boxing should study LLVM as prior art in order to rediscover analysis, transformation, summary, partitioning, and target contracts—not merely to control LLVM at finer granularity.

Cross-language LLVM/LTO convergence becomes a baseline experiment, not the final objective.

## Comparison dimensions

Compare at least semantic information retained/lost, optimization-legality information, compile latency, CPU/memory/I/O, intermediate representation size, analysis recomputation, invalidation precision, cross-language optimization opportunity, target portability, generated-code size/runtime, scheduling/distribution suitability, and explainability.

## Research principle

> Do not merely understand and use LLVM. Rediscover why LLVM became necessary.

> If LAMINARIA reaches the same answer as LLVM, that conclusion must come from LAMINARIA's evidence—not from treating LLVM as an axiom.

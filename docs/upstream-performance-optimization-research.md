# LAMINARIA — Upstream Performance Optimization Research

## Position

This document studies performance work performed **before lowering destroys high-level meaning**. The subject is not primarily target instruction selection, register allocation, or machine scheduling. It is the use of semantics to improve computational work, data movement, locality, parallelism, and specialization before target lowering.

The [compiler ownership contract](compiler-ownership-contract.md) governs the research goal and acceptance criteria. LLVM, MLIR, Polly, Pluto, Halide, ISPC, and existing C/C++ compilers are prior art, comparison baselines, and experimental projections. Delegating the production compilation path to them is not a LAMINARIA result.

This track makes the [LLVM rediscovery research](llvm-rediscovery-research.md) concrete around four questions:

- Which facts known by Rust and Nim frontends are lost in low-level IR?
- Can preserving those facts enable stronger transformations than merging separately lowered LLVM IR?
- Where should target-independent transformation end and target-, resource-, or workload-dependent schedule selection begin?
- How should transformation, analysis, search, and measurement become reusable and invalidatable computations in LAMINARIA's graph?

## Definition of “upstream of LLVM”

“Upstream” does not merely mean the early half of an LLVM pass pipeline.

```text
Rust source / Nim source / restricted kernel or domain description
  → language semantics and effects
  → preserved semantic facts and computational relations
  → legal high-level transformations
  → specialization and schedule search
  → progressively lowered representations
  → target-independent low-level IR
  → target lowering and machine code
```

The primary inputs include shape, rank, extent, stride, layout, aliasing, ownership, escape, effects, iteration domains, dependence, reductions, pipeline relations, stable values, profiles, parallel independence, and numerical contracts.

## Research goals

1. Demonstrate optimizations that become possible only when upstream facts are preserved from Rust and Nim source.
2. Explain legality from language semantics, effects, and dependence rather than successful backend execution or coincidentally equal output.
3. Separate algorithm, transformation, schedule, and target-lowering decisions.
4. Compare static cost models, profile feedback, and empirical autotuning.
5. Measure compile latency, memory, code size, search work, incrementality, and explainability as well as runtime.
6. Preserve reproducible evidence for rejected transformations and regressions, not only successes.

## Core hypotheses

- **H1 — Preserved semantics beat rediscovery.** Source-derived shape, alias, effect, and reduction facts enable more stable legal transformations than reconstruction from low-level loads, stores, and branches.
- **H2 — Progressive representations beat premature normalization.** Language facts, computational relations, structured compute forms, and low-level IR may need to coexist with provenance.
- **H3 — Separating algorithm from schedule enables safe search.** Order, granularity, layout, fusion, and parallelism can vary without redefining the computation.
- **H4 — Specialization requires values and context, not just types.** Shape, stride, alignment, ranges, effects, and hot call contexts are specialization dimensions.
- **H5 — A cost model alone is insufficient.** Static pruning should be combined with representative empirical measurement.
- **H6 — Instruction count is not performance.** Data movement, allocation, materialization, cache behavior, parallel overhead, code size, and startup must be separated.
- **H7 — Optimization search is incremental computation.** Changes to one fact, profile, or target constraint should not invalidate unrelated analysis and measurements.
- **H8 — Restricted semantics form an optimization contract.** Arbitrary pointers, effects, dispatch, exceptions, and external calls reduce what can be proven safely.

## Prior-art families and rediscovery questions

### Polyhedral compilation

Pluto, Polly, and MLIR Affine/Linalg demonstrate dependence-based loop interchange, fusion/fission, tiling, skewing, parallelization, and locality transformation over structured iteration spaces.

LAMINARIA must rediscover which source facts construct an iteration domain, how safe regions are isolated around non-affine control and effects, how fusion trades materialization against parallelism and working-set size, and whether tile size is a semantic transformation or a schedule parameter.

### Multi-level IR and progressive lowering

MLIR's important principle is to retain the abstraction required by a transformation until that transformation has run. LAMINARIA will compare early normalization into one low-level IR against coexisting language facts, relational representations, structured compute forms, and low-level IR. The comparison includes legal transformation count, consistency cost, serialization, invalidation, and diagnostic provenance.

### Algorithm/schedule separation and DSLs

Halide separates image and array algorithms from schedules. ISPC makes `uniform`, `varying`, and SPMD execution semantics explicit. LAMINARIA will compare automatic extraction from ordinary source, source-level contracts or annotations, and restricted embedded kernel/domain descriptions. Detection of incorrect contracts, debug visibility, forbidden fallback, and boundary data movement are first-class costs.

### Partial evaluation, staging, and specialization

Variants may specialize on compile-time values, runtime-stable values, shape, stride, alignment, value ranges, effects, and call context. Candidate experiments include fixed-shape loops, generated parsers or serializers, callback specialization, alignment/alias variants, algorithm thresholds, and ahead-of-time or JIT multiversioning. Code size, instruction-cache pressure, compile work, identity, and dispatch cost are part of the result.

### Equality saturation and exploratory rewriting

Greedy pass order can discard alternatives too early. LAMINARIA will test bounded equality-saturation-style exploration for pure expressions, address calculation, algebraic simplification, and selected fusion/fission spaces. It must account for overflow, numerical, and effect semantics, control graph growth, and retain an explanation of the extracted result.

### Profile-guided specialization and autotuning

Profiles may include input shape distributions, ranges, sparsity, branch coherence, allocation lifetime, call context, and measured schedule results—not only low-level branch counts. A hybrid of static rejection and representative empirical measurement is the primary candidate.

Profile environment, inputs, version, target, confidence, and freshness are part of identity. Holdout workloads detect overfitting and profile-driven regression.

### Source contracts

No-alias, alignment, purity, finality, ranges, and loop transformation directives principally make transformations legal. LAMINARIA contracts require source or analysis provenance, scope, verification, dependent transformations, invalidation conditions, and a diagnostic or runtime check for violations. Unsafe assumptions are not evidence of proven optimization.

### Algorithm and data representation choice

AoS/SoA, dense/sparse, eager/fused, scalar/batched, and computed/preindexed choices can dominate instruction-level optimization. Because they can change memory, ordering, stability, numerical behavior, and observable API behavior, they are explicit planning decisions with semantic and cost boundaries rather than peephole rewrites.

## Candidate architecture

This is an experimental candidate, not an adopted architecture.

```text
Rust/Nim source
  → syntax and source provenance
  → language-specific semantic facts
  → effects / alias / ownership / numeric contracts
  → computational relation graph
  → structured regions (loop / pipeline / reduction / kernel)
  → legality analysis
  → transformation space
  → static pruning
  → specialization and schedule variants
  → profile-guided / empirical selection
  → progressively lowered LAMINARIA IR(s)
  → LAMINARIA-owned target generation
```

Every transformation candidate should carry its input semantic identity, preconditions, legality evidence, affected relations, output variant identity, invalidated analyses, estimated cost delta, measured evidence, provenance, and rejection reason. A pass name alone is not an explanation.

## Cross-language research questions

1. How much Rust ownership and borrowing information can become cross-language alias, escape, and lifetime facts?
2. Which Nim effect, compile-time execution, and generic-instantiation facts can survive into a shared representation?
3. Can a strong contract from one language optimize a region called from the other without early C ABI lowering?
4. When are motion and fusion legal across panic, exception, unwind, destructor/finalizer, and GC safepoint behavior?
5. Can Rust and Nim iterators become a common pipeline relation that eliminates intermediate allocation and dispatch?
6. Can cross-language callback specialization be derived from source semantics rather than post-link guessing?
7. Should semantically equivalent but structurally different Rust and Nim implementations normalize or remain search variants?

## Initial workloads

- **W1 — Map/filter/reduce pipeline:** materialization elimination, producer/consumer fusion, reduction recognition, and parallel reduction over equivalent Rust and Nim iterators.
- **W2 — Affine stencil:** loop interchange, fusion, tiling, and parallelization, with Pluto/Polly/MLIR-style prior art as baselines.
- **W3 — Shape-specialized matrix kernel:** dynamic, profile-dominant, and fixed shapes; measure speedup, variant count, and code size.
- **W4 — AoS/SoA layout:** scan, filter, update, conversion, and FFI boundary cost for the same semantic record collection.
- **W5 — Alias-sensitive loop:** unknown aliasing, proven no-alias, source contract, and runtime overlap-check variants, including negative tests.
- **W6 — Context specialization:** specialize a generic or callback only for a hot context and measure cold-path and code-size regressions.
- **W7 — Irregular rejection case:** pointer chasing, observable effects, non-affine bounds, and panic/unwind must produce an explained rejection.
- **W8 — Autotuned schedule:** compare static, profile-selected, and empirically selected loop order, tile size, and fusion boundaries on holdout inputs.

Correctness includes ordering, overflow, numerical behavior, panic/effects, and concurrency contracts—not only final output equality.

## Comparison classes

| Class | Candidates | Purpose |
| --- | --- | --- |
| General compilers | Clang/LLVM, GCC, rustc, Nim compiler | conventional optimization baseline |
| Polyhedral | Pluto, Polly | dependence, tiling, fusion, parallelization |
| Multi-level IR | MLIR Affine/Linalg | semantic retention and progressive lowering |
| Schedule DSL | Halide | algorithm/schedule separation and autoscheduling |
| Parallel language | ISPC | explicit SPMD semantics |
| LAMINARIA | owned facts, IR, transforms, and search | production research path |

The result must attribute differences to information, transformations, cost models, runtime, or libraries rather than report only an aggregate winner.

## Measurement contract

- **Correctness:** semantic oracles, property-based comparison, overflow, rounding, NaN, ordering, effects, unwinding, races, determinism, and negative legality tests.
- **Runtime:** wall-time distribution, throughput/latency, allocation, peak memory, available hardware counters, and startup/warm/steady-state separation.
- **Compiler economics:** time and peak memory by stage, intermediate size, variant count, code size, total tuning CPU time, and invalidation/reuse after source, profile, or target changes.
- **Optimization quality:** candidates considered, apply/reject reasons, semantic facts and provenance, invalidated analyses, estimate-versus-measurement error, and transformations responsible for baseline differences.

Every measurement records toolchain, commit, target, flags, input identity, environment, and sample count. Results with different numerical or effect contracts are not directly comparable.

## Experimental phases

1. **Prior-art reproduction:** reproduce representative transformations and their input and failure conditions.
2. **Semantic fact inventory:** trace shape, alias, effect, iteration, reduction, and numerical facts from Rust and Nim source.
3. **One owned transformation slice:** execute source-derived facts, owned representation, legality, transformation, and target generation without upstream compiler output as a required input.
4. **Variant and empirical selection:** compare static and measured selection with distinct training and holdout inputs.
5. **Incremental and distributed search:** measure invalidation and reuse of analysis, candidate, and measurement artifacts.
6. **Cross-language transformation:** establish or evidence-backed reject at least one fusion, specialization, or allocation-elimination opportunity across a Rust/Nim boundary.

## Initial completion criteria

- Implement at least five of the eight workloads as source-derived Rust/Nim pairs.
- Reproduce representative transformations and failure conditions from at least three prior-art families.
- Show at least two kinds of high-level semantic fact that produce an optimization unavailable or unstable from low-level inference.
- Explain accepted and rejected transformations with LAMINARIA-owned legality evidence.
- Execute at least one owned transformation from source through target artifact generation.
- Compare at least one static schedule and empirically selected schedule on holdout inputs.
- Report compiler economics and code size as well as runtime.
- Measure invalidation and reuse for source, profile, and target changes.
- Establish one cross-language optimization or give reproducible semantic evidence that it is impossible.
- Produce human-traceable diagnostics for selection, rejection, and regression.

Enabling `-O3`, LTO, PGO, or Polly; generating MLIR/LLVM IR; calling Halide, ISPC, or a vendor library; observing one faster benchmark; or relying on an unchecked unsafe assumption does not satisfy these criteria.

## Non-goals

- Guarantee automatic algorithm improvement for arbitrary C/C++, Rust, or Nim programs.
- Reimplement LLVM, MLIR, or Halide APIs in the same form.
- Treat overfitting to one benchmark or CPU as product performance.
- Silently weaken floating-point, overflow, or effect semantics.
- Exclude exhaustive search cost from performance results.
- Count delegation to existing tools as evidence of compiler ownership.

## Primary references

- MLIR rationale, Affine dialect, and Linalg dialect: <https://mlir.llvm.org/docs/Rationale/Rationale/>, <https://mlir.llvm.org/docs/Dialects/Affine/>, <https://mlir.llvm.org/docs/Dialects/Linalg/>
- Lattner et al., “MLIR: Scaling Compiler Infrastructure for Domain Specific Computation”: <https://arxiv.org/abs/2002.11054>
- Pluto: <https://github.com/bondhugula/pluto>
- Polly: <https://polly.llvm.org/>
- Polyhedral compilation resources: <https://polyhedral.info/>
- Halide and its publications: <https://halide-lang.org/>
- Halide autoscheduler tutorial: <https://halide-lang.org/docs/tutorial/lesson_21_auto_scheduler_generate.html>
- Intel ISPC and its performance guide: <https://ispc.github.io/>, <https://ispc.github.io/perfguide.html>
- OpenMP specifications: <https://www.openmp.org/specifications/>
- Jones, Gomard, and Sestoft, “Partial Evaluation and Automatic Program Generation”: <https://studwww.itu.dk/people/sestoft/pebook/>
- Willsey et al., “egg: Fast and Extensible Equality Saturation”: <https://doi.org/10.1145/3434304>
- `egg` documentation and tutorials: <https://docs.rs/egg/latest/egg/>
- Clang PGO and ThinLTO: <https://clang.llvm.org/docs/UsersManual.html#profile-guided-optimization>, <https://clang.llvm.org/docs/ThinLTO.html>
- GCC optimization options: <https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html>

Claims and benchmark results in these materials are not evidence that they apply to LAMINARIA. Each claim must be independently tested under a fixed semantic contract, reproducible workload, measurements, and negative tests.

# Research Issue Prioritization and Minimal-Hypothesis Policy

## Purpose

LAMINARIA's current issues do not request finished product features or production-complete subsystems. Each issue is a finite experiment for deciding an architectural hypothesis.

Closing an issue means that enough evidence exists to support, reject, reformulate, or explicitly defer its hypothesis. It does not mean that the surrounding feature area is complete.

## Project-wide decision criteria

Evaluate candidate work in this order:

1. **Non-substitutability**: does it answer a LAMINARIA-specific question—such as resolving heterogeneous Cargo, Nimble, C, and C++ dependencies as one graph—that cannot be answered by merely composing existing compilers, build systems, schedulers, CAS products, remote execution, or OS facilities?
2. **Falsification power**: could failure change or reject a hypothesis about LAMINARIA's dependency graph, resolution algorithm, IR, semantic preservation, native target generation, or self-hosting?
3. **Architectural discrimination**: does it distinguish candidate architectures, rather than merely show that an implementation is possible?
4. **Minimality**: does it use the smallest source subset, workload, target, node count, and failure case needed for the decision?
5. **Leverage**: does it settle assumptions shared by multiple later issues?
6. **Engineering cost**: when the above are equal, prefer the shorter, reversible experiment that reuses existing assets.

## Whole-project optimization constraint

The unit of optimization is the requested artifact and the project progression
needed to produce, verify, and evolve it. A component, pass, cache, scheduler,
resolver, test harness, or measurement subsystem is never the objective by
itself.

Before adopting a locally improved result, evaluate it against the whole
decision boundary:

1. **Parent outcome**: identify the project outcome and active milestone that
   the change advances.
2. **End-to-end effect**: measure or reason about the requested artifact's
   correctness, completion latency, peak and retained resources, output
   quality, reproducibility, and operability—not only the edited component.
3. **Displaced cost**: include work, memory, I/O, complexity, failure risk, and
   maintenance transferred upstream, downstream, or into another research
   lane.
4. **Opportunity cost**: record which higher-leverage experiment is delayed and
   whether the decision narrows a later architecture or target without
   evidence.
5. **Trade-off position**: compare alternatives over the same boundary and
   state the accepted trade-off. Do not collapse incomparable correctness,
   time, memory, artifact quality, and maintainability effects into one scalar
   unless the project has explicitly adopted that utility function.

A better local metric is an observation, not a project conclusion. It may be
accepted when it improves the whole outcome, is neutral outside its boundary,
or makes a deliberate project-level trade-off whose costs are recorded. It is
rejected or reformulated when it merely moves cost elsewhere or consumes more
project effort than the uncertainty it resolves.

Measurement is subject to the same rule. Increase measurement precision only
until the evidence can distinguish the live architecture choices or falsify
the active hypothesis. Measurement infrastructure, benchmark coverage, and
telemetry detail have explicit implementation, runtime, storage, analysis, and
maintenance costs; optimizing them beyond the decision need is local
optimization of the evidence system.

## Priority classes

### P0 — LAMINARIA-specific hypotheses

- Jointly resolve Cargo/Nimble/C/C++ version, feature, target, source, and header constraints with source semantics, language/intermediate IR, ABIs, symbols, toolchains, and links in one demand-driven typed graph, so every dependency obligation is discharged, externalized, or rejected.
- Find the correct closure quickly, with low memory and incremental recomputation, without materializing the candidate Cartesian product; explain both selection and rejection.
- Produce an ordinary OS-runnable native executable from the resolved closure.
- Derive LAMINARIA-owned representations from Rust/Nim source semantics without requiring existing compiler IR as input.
- Preserve and compose semantic facts across the language boundary, explaining both legal transformations and rejections.
- Use semantic facts to partition or fuse compiler work and demonstrate a decision that is not merely a translation-unit, LLVM-module, or generic Action boundary.
- Feed composed/selected IR into LAMINARIA-owned target lowering and produce an executable artifact.
- Extend the same owned path to Rust-only, Nim-only, mixed inputs, and eventually LAMINARIA itself.

### P1 — Enablers required to decide P0

Identity, minimal measurement, planner/runtime connectivity, diagnostics, and runtime/ABI contracts only to the extent required to keep a P0 experiment valid. Completeness of the enabler is not the objective.

### P2 — Substitutable engineering and baselines

Generic thread pools, general CPU/RAM admission, CAS, atomic publication, remote transport, CLI polish, exhaustive platform support, and existing-toolchain matrices have prior-art feasibility. Implement only the minimum required by a P0/P1 experiment; do not independently pursue product completeness.

## Minimal decision contract for every issue

At the current research stage, each issue requires only:

1. **Minimal hypothesis**: one sentence that can be supported or rejected.
2. **Minimal experiment**: the smallest positive case and, when needed, one negative/counterexample.
3. **Observation**: only the correctness and comparison evidence needed for the decision.
4. **Stop condition**: enough evidence to adopt, reject, reformulate, or defer the hypothesis.
5. **Non-goals**: full feature coverage, all targets/OSes/failures, optimal performance, production operations, and finished-product quality.

Long acceptance lists, extensions, and future requirements already present in issue bodies remain a research backlog and evidence menu. They are not a requirement to satisfy every item in one pass. Items selected for the next experiment must be named in its minimal decision contract before implementation starts.

## Review rule

A review finding blocks the current experiment only when:

- the evidence does not actually decide the hypothesis;
- a correctness, identity, provenance, or comparison defect could change the conclusion;
- the work violates research ownership, such as hidden fallback to an existing compiler; or
- success is vacuous because the required counterexample is absent.
- a reported local improvement transfers material cost or risk elsewhere in
  the project and the end-to-end trade-off has not been evaluated.

Generality, completeness, extensibility, tuning, API polish, and additional platform support are non-blocking unless the current hypothesis requires them. Record them as later candidates instead of silently adding them to the issue.

## Current project-wide priority

The canonical statement of current evidence, the near-term stopping condition, and later phases is [LAMINARIA Project Progression and Near-Term Research Goal](../near-term-research-program.md). This policy defines how work is selected; that roadmap records which decision is currently selected.

1. Jointly resolve Cargo/Nimble/C/C++ package constraints, source semantics, language/intermediate IR, artifacts, toolchains, ABIs, symbols, and links in one typed graph that records dependency-obligation discharge state.
2. Demand-expand only the closure of a requested native executable; show one consistent result and one pre-compilation rejection.
3. Execute the resolved closure through the production planner/runtime and build and run an ordinary native binary.
4. Compare eager and demand-driven resolution by wall time, peak memory, explored states, and recomputation.
5. Add semantic IR, fusion/partition, identity, persistence, and heterogeneous placement only where counterexamples to that graph require them.
6. Expand the surviving path to LAMINARIA's own transitive dependency closure and self-hosting.

WebAssembly may be compared later as an optional target, but it is neither the goal nor a required gate in this ordering.

This is not issue-number order or unchecked-box order. Update it when new evidence changes the architecture's uncertainty.

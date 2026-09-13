# Research Issue Prioritization and Minimal-Hypothesis Policy

## Purpose

LAMINARIA's current issues do not request finished product features or production-complete subsystems. Each issue is a finite experiment for deciding an architectural hypothesis.

Closing an issue means that enough evidence exists to support, reject, reformulate, or explicitly defer its hypothesis. It does not mean that the surrounding feature area is complete.

## Project-wide decision criteria

Evaluate candidate work in this order:

1. **Non-substitutability**: does it answer a LAMINARIA-specific question that cannot be answered by composing existing compilers, build systems, schedulers, CAS products, remote execution, or OS facilities?
2. **Falsification power**: could failure change or reject a hypothesis about LAMINARIA's IR, semantic preservation, transformations, partition/fusion, target generation, or self-hosting?
3. **Architectural discrimination**: does it distinguish candidate architectures, rather than merely show that an implementation is possible?
4. **Minimality**: does it use the smallest source subset, workload, target, node count, and failure case needed for the decision?
5. **Leverage**: does it settle assumptions shared by multiple later issues?
6. **Engineering cost**: when the above are equal, prefer the shorter, reversible experiment that reuses existing assets.

## Priority classes

### P0 — LAMINARIA-specific hypotheses

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

Generality, completeness, extensibility, tuning, API polish, and additional platform support are non-blocking unless the current hypothesis requires them. Record them as later candidates instead of silently adding them to the issue.

## Current project-wide priority

The canonical statement of current evidence, the near-term stopping condition, and later phases is [LAMINARIA Project Progression and Near-Term Research Goal](near-term-research-program.md). This policy defines how work is selected; that roadmap records which decision is currently selected.

1. Compose source-derived Rust and Nim IR as one semantic workload.
2. Apply one owned cross-language transformation and demonstrate both legality and rejection.
3. Generate and execute owned WebAssembly from the transformed result without delegated compilation.
4. Compare fused and split boundaries for the same workload and obtain one LAMINARIA-specific partition decision.
5. Add resource accounting, identity, persistence, and heterogeneous placement only where that decision requires them.
6. Expand the validated subset to Rust-only, Nim-only, mixed projects, and ultimately LAMINARIA itself.

This is not issue-number order or unchecked-box order. Update it when new evidence changes the architecture's uncertainty.

# Metrics-First Research Policy

LAMINARIA treats performance and execution-path evidence as part of correctness for research claims about scheduling, compiler integration, caching and linking.

## Rule

A change is not accepted merely because tests pass. Where an issue claims improved scheduling, compiler decomposition, cache reuse, native linking, backend selection or reduced work, the implementation must prove that the intended path ran and that resource behavior is consistent with the claim.

## Required observations

As applicable, record:

- wall-clock duration;
- CPU time/utilization;
- peak and time-weighted memory;
- disk/network I/O and wait;
- graph/action counts;
- critical-path length;
- cache hits/misses and their reasons;
- generated artifacts and their sizes;
- compiler/backend/linker path actually selected;
- fallback/delegation use;
- reference-project baseline and ratio.

## Failure semantics

If an implementation is functionally correct but materially slower, more resource-hungry, or structurally inconsistent with the intended execution model, the implementation remains incomplete.

If the intended optimized or direct path is not selected and the system silently falls back to an opaque tool invocation, the research claim is not satisfied even when outputs are correct.

Benchmarks must not be weakened after a regression simply to make the implementation pass. Unexpected results should change the implementation or the hypothesis.

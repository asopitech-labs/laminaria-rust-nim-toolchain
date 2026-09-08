# Metrics-First Research Policy

LAMINARIA treats performance and execution-path evidence as part of correctness for research claims about scheduling, compiler integration, caching and linking.

## Rule

A change is not accepted merely because tests pass. Where an issue claims improved scheduling, compiler decomposition, cache reuse, native linking, backend selection or reduced work, the implementation must prove that the intended path ran and that resource behavior is consistent with the claim.

Functional correctness and execution correctness are separate requirements. Producing the correct final artifact does not prove that the intended graph, invalidation set, cache path, backend, scheduler or direct-link path was used.

## Optimization hierarchy

Prefer eliminating work over making unnecessary work faster:

1. eliminate unnecessary actions or compiler stages;
2. reuse already-valid artifacts;
3. reduce invalidation scope;
4. expose useful parallelism;
5. schedule globally under resource constraints;
6. optimize individual actions.

Parallelizing work that should not have run is not considered a sufficient optimization.

## No-op invariant

When source content, relevant configuration, toolchain identity and compatible environment inputs are unchanged, the requested build should converge to a true no-op for execution actions.

No-op evaluation must measure more than cache-hit percentage. As applicable, record:

- no-op wall time and CPU time;
- graph nodes inspected;
- filesystem metadata operations;
- bytes hashed/read;
- process launches;
- compiler/codegen/link actions executed;
- reasons for any non-zero execution work.

A nominal 100% cache-hit result that still performs substantial hashing, I/O, process startup or redundant graph work is not a satisfactory no-op result.

## Required observations

As applicable, record:

- wall-clock duration;
- CPU time/utilization;
- peak and time-weighted memory;
- disk/network I/O and wait;
- graph/action counts;
- critical-path length;
- per-action queue wait, dependency wait and execution time;
- cache hits/misses and their reasons;
- generated artifacts and their sizes;
- compiler/backend/linker path actually selected;
- invalidation set and work avoided;
- fallback/delegation use;
- reference-project baseline and ratio.

## Execution-correctness evidence

For changes to incremental execution, caching, scheduling or compiler decomposition, tests should assert the expected execution set where feasible. Examples include:

- which compiler stages must rerun after a controlled edit;
- which unaffected stages must not rerun;
- which artifacts must be reused;
- whether an action waited because of dependencies, scheduler policy or resource saturation;
- whether a direct/native/backend-specific path was actually selected.

A test that rebuilds everything and merely verifies the final binary is insufficient evidence for an incremental or scheduling claim.

## Failure semantics

If an implementation is functionally correct but materially slower, more resource-hungry, performs materially more work, or is structurally inconsistent with the intended execution model, the implementation remains incomplete.

If the intended optimized or direct path is not selected and the system silently falls back to an opaque tool invocation, the research claim is not satisfied even when outputs are correct.

Benchmarks must not be weakened after a regression simply to make the implementation pass. Unexpected results should change the implementation or the hypothesis.

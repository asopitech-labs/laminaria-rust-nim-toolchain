# Metrics-First Research Policy

LAMINARIA treats performance and execution-path evidence as part of correctness for research claims about scheduling, compiler/backend integration, caching and linking.

## Rule

A change is not accepted merely because tests pass. Where an issue claims improved scheduling, compiler/backend decomposition, cache reuse, native linking, backend selection, backend white-boxing, WASM target-pipeline decomposition or reduced work, the implementation must prove that the intended path ran and that resource behavior is consistent with the claim.

Functional correctness and execution correctness are separate requirements. Producing the correct final artifact does not prove that the intended graph, invalidation set, cache path, backend route, backend pipeline, scheduler, linker/post-link path or direct-link path was used.

## Optimization hierarchy

Prefer eliminating work over making unnecessary work faster:

1. eliminate unnecessary compiler/backend actions or stages;
2. reuse already-valid artifacts/checkpoints;
3. reduce invalidation scope;
4. expose useful parallelism;
5. schedule globally under resource constraints;
6. optimize individual remaining actions.

Parallelizing work that should not have run is not considered a sufficient optimization.

## Backend checkpoint economics

Backend white-boxing does not imply that every logical pass boundary becomes a process or cache artifact.

For every proposed checkpoint, measure both sides:

```text
benefit =
  eliminated work
+ reusable work
+ reduced invalidation
+ scheduling/distribution gain

cost =
  serialization/deserialization
+ hashing and I/O
+ process/IPC overhead
+ lost compiler analysis state
+ cache-locality degradation
+ increased memory traffic
+ generated-code-quality risk
```

A finer graph is not a success criterion by itself. A checkpoint must be rejected from the default path when measured cost exceeds the demonstrated benefit.

## No-op invariant

When source content, relevant configuration, toolchain identity and compatible environment inputs are unchanged, the requested build should converge to a true no-op for execution actions.

No-op evaluation must measure more than cache-hit percentage. As applicable, record:

- no-op wall time and CPU time;
- graph nodes inspected;
- dynamic graph expansions performed;
- filesystem metadata operations;
- bytes hashed/read/written/serialized/deserialized;
- process launches;
- compiler/codegen/backend/link/post-link actions executed;
- reasons for any non-zero execution work.

A nominal 100% cache-hit result that still performs substantial hashing, I/O, process startup, graph expansion or redundant graph work is not a satisfactory no-op result.

## Required observations

As applicable, record:

- wall-clock duration;
- CPU time/utilization;
- peak and time-weighted memory;
- disk/network I/O and wait;
- graph/action counts;
- logical/observation/checkpoint/execution boundary counts;
- dynamic graph-expansion time and child-action counts;
- critical-path length;
- per-action queue wait, dependency wait and execution time;
- cache hits/misses and their reasons;
- generated IR/bitcode/object/module/component artifacts and sizes;
- compiler/backend/linker/post-link path actually selected;
- LLVM pass-group timing and optimization remarks where available;
- ThinLTO backend-job count/index/invalidation evidence;
- Binaryen pass timing/module metrics where available;
- WIT/adaptation/componentization timing and artifacts;
- invalidation set and work avoided;
- fallback/delegation use;
- reference-project baseline and ratio;
- checkpoint benefit versus checkpoint cost.

## Execution-correctness evidence

For changes to incremental execution, caching, scheduling or compiler/backend decomposition, tests should assert the expected execution set where feasible. Examples include:

- which compiler/backend stages must rerun after a controlled edit;
- which unaffected stages must not rerun;
- which artifacts/checkpoints must be reused;
- which ThinLTO backend jobs should or should not run;
- whether `wasm-ld`, Binaryen or componentization should rerun;
- whether an action waited because of dependencies, scheduler policy or resource saturation;
- whether a direct/native/backend-specific path was actually selected;
- whether dynamic child actions actually entered LAMINARIA scheduling rather than a hidden nested executor.

A test that rebuilds everything and merely verifies the final binary/module is insufficient evidence for an incremental or scheduling claim.

## Generated-code quality

Compile-time improvements that change backend pipeline structure must also check generated-code quality where relevant. Depending on the experiment, record runtime performance, code size, optimization remarks, LTO/import evidence or other target-appropriate quality signals.

LAMINARIA must not accept a compile-time optimization that silently damages the generated artifact beyond the defined experiment tolerance without explicitly recording the trade-off.

## Failure semantics

If an implementation is functionally correct but materially slower, more resource-hungry, performs materially more work, damages generated-code quality, or is structurally inconsistent with the intended execution model, the implementation remains incomplete.

If the intended optimized/direct/white-box path is not selected and the system silently falls back to an opaque tool invocation or hidden nested scheduler, the research claim is not satisfied even when outputs are correct.

Benchmarks must not be weakened after a regression simply to make the implementation pass. Unexpected results should change the implementation or the hypothesis.

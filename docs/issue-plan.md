# Research Issue Plan

This file maps the initial GitHub issue set to the research program. The GitHub issues are the execution tracker; this document preserves the intended research decomposition.

## Initial tracks

1. Compiler pipeline decomposition
2. Rust–Nim native linking without mandatory C ABI boundary
3. Backend Graph and backend variants
4. Unified Action Graph and resource-aware scheduling
5. Artifact identity, incremental invalidation and CAS
6. Variant-space control in the Nim Planning Kernel
7. WASM mixed-language integration
8. Agent-oriented explainability and evidence schema
9. Reference workload and metrics harness
10. Work elimination, execution correctness and no-op build invariants

## Optimization order

LAMINARIA should prefer:

1. eliminating unnecessary actions/compiler stages;
2. reusing valid artifacts;
3. reducing invalidation scope;
4. exposing parallelism;
5. globally scheduling the remaining work;
6. optimizing individual actions.

The research program must distinguish these effects in evidence. Parallelizing work that should not have executed is not equivalent to eliminating it.

Each issue must provide reproducible evidence. Passing functional tests alone is not sufficient for architecture, performance, scheduling, cache, compiler-boundary, reduced-work or linking claims. Controlled incremental tests should validate the expected execution set as well as the final artifact.

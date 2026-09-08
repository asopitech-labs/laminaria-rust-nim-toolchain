# Research Issue Plan

This file maps the GitHub issue set to the research program. The GitHub issues are the execution tracker; this document preserves the intended research decomposition.

## Core tracks

1. Compiler pipeline decomposition — #3
2. Rust–Nim native linking without mandatory C ABI boundary — #4
3. Backend route selection and capability constraints — #5
4. Unified Action Graph and resource-aware scheduling — #6
5. Artifact identity, incremental invalidation and CAS — #7
6. Variant-space control in the Nim Planning Kernel — #8
7. WASM mixed-language integration/topology — #9
8. Agent-oriented explainability and evidence schema — #10
9. Reference workload and metrics harness — #11
10. Work elimination, execution correctness and no-op build invariants — #12

## Backend pipeline white-boxing expansion

11. Expand backend routes into nested observable/checkpoint/execution graphs and define checkpoint economics — #13
12. White-box LLVM pass/codegen/LTO pipeline boundaries without pass-per-process decomposition — #14
13. Map ThinLTO/DTLTO dynamic backend jobs into the LAMINARIA scheduler and cache graph — #15
14. Decompose the WebAssembly target pipeline through `wasm-ld`, Binaryen, WIT and componentization — #16
15. Evaluate shared LLVM IR/LTO convergence across Rust, Nim 2 and Nimony routes — #17

The detailed architecture is documented in:

- `backend-pipeline-whiteboxing.md`
- `backend-pipeline-whiteboxing_ja.md`

## Responsibility boundaries

### #5 versus #13

#5 answers **which backend route is valid and selected**. #13 answers **how the selected backend expands into internal computation and which boundaries become observable/checkpoint/execution nodes**.

### #9 versus #16

#9 compares mixed-language WebAssembly integration topologies and boundary costs. #16 white-boxes the target production pipeline itself: relocatable Wasm, `wasm-ld`, Core Wasm, Binaryen, WIT/adapters and componentization.

### #4 versus #17

#4 tests direct Rust–Nim native object/link contracts without a mandatory C ABI boundary. #17 tests whether Rust/Nim-origin LLVM artifacts can participate in a shared LLVM/LTO plan and explicitly separates backend artifact compatibility from language/runtime ABI compatibility.

### #6/#7/#12 versus #15

#15 is not a separate scheduler/cache architecture. It is the ThinLTO/DTLTO stress case that must use #6 scheduling, #7 identity/CAS and #12 work-elimination semantics. DTLTO's externally described backend jobs are used to test dynamic graph expansion rather than adding a hidden nested scheduler.

## Optimization order

LAMINARIA should prefer:

1. eliminating unnecessary compiler/backend actions or stages;
2. reusing valid artifacts/checkpoints;
3. reducing invalidation scope;
4. exposing parallelism;
5. globally scheduling the remaining work;
6. optimizing individual remaining actions.

The research program must distinguish these effects in evidence. Parallelizing work that should not have executed is not equivalent to eliminating it.

## Boundary rule

Backend white-boxing must not equate graph visibility with process granularity. Every backend boundary is classified independently as:

1. logical stage;
2. observation boundary;
3. checkpoint/artifact boundary;
4. execution boundary;
5. dynamic graph-expansion point.

A compiler pass may be visible and measured without being separately serialized or scheduled.

## Evidence rule

Each issue must provide reproducible evidence. Passing functional tests alone is not sufficient for architecture, performance, scheduling, cache, compiler/backend-boundary, reduced-work or linking claims. Controlled incremental tests should validate the expected execution set as well as the final artifact.

Backend checkpoint work must measure both benefit and cost. At least one overly fine checkpoint candidate must be allowed to fail the economics test; increasing graph granularity is not itself a success criterion.

# Issue #50 R1 progress: generic demand feedback slice

## Result

The R0-locked `rust-heavy-workspace` now has a source-fact-to-planner path for
one generic function instance. For a `fixture-bin` executable request, the
planner retains `sum_generic<i64>` and omits the `#[cfg(test)]`-only
`sum_generic<i32>` from its demand-relative generic work set. For a
`fixture-core` test request, that selection reverses: `i32` remains and `i64`
is omitted. This is graph-level planning evidence only; no compiler work is
claimed to have been executed or avoided by a production executor.

## Checkpoint contract

- **Consumes:** The checked-in Rust source for `fixture-core` and `fixture-bin`
  in `fixtures/rust-heavy-workspace`, the generic-function declarations found
  in the provider source, and an explicitly selected production or test demand
  root.
- **Must preserve:** The existing package-level `rust_cross_layer` planner and
  its `many-unrequested-targets` integration test; R0's Cargo behavior oracle;
  and the rule that discovery uncertainty must not silently remove required
  work.
- **Evidence:** `laminaria-ir::rust_generic_demand` discovers the fixture's
  `sum_generic<i64>` call and test-only `sum_generic<i32>` call. The generic
  work planner requires requested instances to be a subset of the eager
  inventory, then reports eager, feedback, and pruned monomorphize/codegen
  work identities. The integration test checks both the executable and core
  test request directions. Unsupported generic inference and unknown macros
  are structured discovery errors.
- **Enables:** A later owned executor can consume a tested specialization work
  set and produce evidence that the omitted specialization never reached
  compiler work. It does not itself enable a claim of R1 completion.

## Current boundary and remaining R1 result

The source collector is a narrow `syn`-based recognizer, not Rust type
checking. It currently handles the locked fixture's direct generic call,
annotated `Vec<T>` argument, unsuffixed integer array literal, and its known
expression macros. It accepts functions with exactly one type parameter and
fails closed for other generic signatures, unrecognized macros, and unsupported
argument inference. Its `include_test_cfg` behavior is validated against the
fixture's inline `#[cfg(test)]` module; it is not a general implementation of
Rust cfg evaluation.

The planner creates demand-relative `Monomorphize` and `Codegen` identities.
It is not yet connected to LAMINARIA-owned Rust parsing/type/IR lowering,
symbol/link liveness, a native target executor, or process/action provenance.
Therefore this slice does not establish the full requested-artifact → unit →
semantic/IR → symbol/liveness → unit feedback loop, a positive artifact from
the owned path, a compile-before-reject run, or an actual difference in
executed work. Cargo remains the R0 reference oracle only. No resource or
performance claim is made.

R1 remains open until the missing loop is implemented and evidence shows a
positive owned artifact (or structured pre-compiler rejection) and distinct
eager/feedback execution sets. R2 remains responsible for matched raw resource
evidence and causal attribution.

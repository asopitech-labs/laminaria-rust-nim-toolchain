# Issue #50 R1 progress: generic demand feedback slice

## Result

The R0-locked `rust-heavy-workspace` now has a source-fact-to-planner path for
one generic function instance. For a `fixture-bin` executable request, the
planner retains `sum_generic<i64>` and omits the `#[cfg(test)]`-only
`sum_generic<i32>` from its demand-relative generic work set. For a
`fixture-core` test request, that selection reverses: `i32` remains and `i64`
is omitted. An artifact feedback plan now composes that specialization demand
with the package selection from the same artifact request and rejects generic
providers that are not candidates or are not selected. This is graph-level
planning evidence only; no compiler work is claimed to have been executed or
avoided by a production executor.

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
  `sum_generic<i64>` call and test-only `sum_generic<i32>` call. Package
  candidates and direct dependencies come from the existing
  `cargo_metadata 0.18.1` crate; the source collector resolves the fixture's
  imported type/function roots and avoids interpreting `Cluster::...` as a
  package. The artifact feedback planner requires requested instances to be
  in the eager inventory, verifies canonical Cargo package identities across
  package/generic layers, and reports eager, feedback, and pruned
  monomorphize/codegen work identities. The integration test checks both the
  executable and core-test request directions. Unsupported generic inference
  and unknown macros are structured discovery errors.
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

The planner creates demand-relative semantic-analysis, IR-lowering,
monomorphization, codegen, and symbol/liveness identities for each selected
generic instance. These are planning identities, not evidence that any stage
ran. It is not yet connected to LAMINARIA-owned Rust parsing/type/IR lowering,
symbol/link liveness execution, a native target executor, or process/action
provenance.

The artifact feedback plan exposes a combined eager/feedback/pruned execution
identity set containing both package-stage and generic-instance work. The
executor bridge now maps its typed `RustExecutionWork` values to the concrete
action IDs supplied by the action builder; it never guesses from an ID prefix
or from ordering. The selected IDs are closure-checked before dispatch, and
the executor records only successfully completed IDs. The focused regression
therefore proves a real six-action eager run, a three-action feedback run, and
structured rejection when a producer is omitted.

This still does not establish the full requested-artifact → unit →
semantic/IR → symbol/liveness → unit feedback loop, a positive compiler
artifact from the owned path, or raw resource/performance attribution. Cargo
remains the R0 reference oracle. R2 remains responsible for matched raw
resource evidence and causal attribution.

R1's current result is the first executable package feedback slice: typed
artifact demand selects the action set, dependency closure is enforced, and
dispatch evidence is observable. The remaining R1 work is to connect the same
selection contract to the real artifact-producing path and its independent
consumer evidence.

The executor now publishes a typed `CompilerWorkExecutionReceipt` from the
actual `ArtifactStore` evidence, including selected actions, successful
dispatch identities, and produced evidence artifact identities. A separate
integration-test consumer validates the receipt against the immutable plan and
requested evidence artifact, and rejects unknown or tampered action identities.
This consumer accepts both eager and feedback-produced positive evidence; it
does not rely on the mutable `laminaria-bootstrap` tag or on producer-side
assertions alone.

## Checkpoint failure report — 2026-10-08

The R0-locked positive subject is the actual `fixture-bin` executable, with
the four output lines recorded in `issue-50-r0-oracle-lock.md`. The existing
feedback executor's positive evidence is an interpreted owned-IR result, not
that executable. Its direct test
`the_r0_generic_fixture_is_rejected_before_owned_program_publication` confirms
that the current owned frontend rejects the real `sum_generic` declaration
before publishing even a candidate program. The locked binary also uses
`Vec`, iterators, structs, `Option`, and formatting outside the current owned
IR. `laminaria-ir` has an owned WASM generator but no owned native object
generator; G2's fixed native fixture currently calls external `rustc` and Nim
and cannot serve as the owned Rust producer for this R1 subject.

Thus the current action-selection and receipt checkpoints remain valid, but
they do not enable R1's positive native-artifact checkpoint. Completing that
checkpoint while preserving the locked R0 subject entails a substantially
larger source-semantics, runtime, and native-generation path. Narrowing the
subject would change R0's acceptance contract. Under the goal-driven work
instruction policy, the instruction author must decide whether to preserve
the locked subject and add a causal sequence of owned-compiler checkpoints, or
revise the R1 subject and its oracle explicitly. Neither a delegated Cargo
binary nor an interpreted evidence artifact is an equivalent completion.

# First Cross-Ecosystem Dependency-Graph Experiment

## Decision being tested

Can LAMINARIA demand-resolve a typed dependency closure spanning Cargo, Nimble, C, and C++ and use that closure to build and run one ordinary native executable without hiding target compilation inside package-manager commands?

This is G1/G2 of the current [near-term research program](../../near-term-research-program.md). A single Rust/Nim call path and an owned WASM module do not satisfy this decision.

## Fixed positive graph

Use one minimal project containing:

1. one Cargo package with an explicit feature and target condition;
2. one Nimble package consumed by the program;
3. one C library supplied as source or an identified archive;
4. one C++ library requiring one explicit adapter or instantiation unit; and
5. one native executable whose observable result depends on all four inputs.

The graph must preserve package/version/feature identity, host-versus-target role, source/header inputs, generated adapter provenance, toolchain/ABI constraints, artifact producers, symbols, link order, and the final executable demand.

## Required executable evidence

Production resolver/planner/executor tests must directly establish that:

- only the demanded closure is expanded;
- every compile, adapter, archive, and link action has explicit inputs and outputs;
- the native executable is produced and run with the expected result;
- the recorded final-link inputs account for every required foreign artifact; and
- Cargo/Nimble metadata ingestion does not silently execute an opaque target build.

Add one negative case that makes exactly one version, feature, target, ABI, symbol, or toolchain constraint incompatible. The resolver must return a structured explanation before compilation starts.

## Efficiency comparison

For the same graph, compare an eager-enumeration baseline with the demand-driven candidate. Record wall-clock time, peak RSS, expanded/pruned/merged states, recomputed nodes, and external actions avoided for cold, no-op, leaf-change, and feature/target-change scenarios.

## Stop condition

Stop after obtaining a correct positive closure, a pre-compilation rejection, a runnable native binary, and enough measurements to adopt, reject, or reformulate one graph representation or resolution algorithm. Do not extend the experiment to complete ecosystem coverage, WASM, distributed execution, or self-hosting.

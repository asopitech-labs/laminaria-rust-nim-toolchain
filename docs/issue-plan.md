# Research Issue Plan

This file maps the GitHub issue set to the research program. The GitHub issues are the execution tracker; this document preserves the intended research decomposition and dependency order.

## Measurement foundation — execute first

LAMINARIA must measure the ordinary toolchain path before replacing or optimizing it. The permanent measurement spine is therefore an architectural dependency of the later compiler/backend/scheduler experiments.

1. Measurement spine, reference workloads and metrics harness — #11
   - multi-version native measurement environments and exact toolchain fingerprints — #18
   - versioned end-to-end Run envelope and process/resource tracer — #19
   - artifact deltas and compiler-native telemetry — #20
   - baseline scenarios, repetition/noise policy and regression comparison — #21

Recommended order:

```text
#18 Environment / Multi-version Toolchain Identity
  ├→ #22 Toolchain Version Variant / Compatibility Model
  ↓
#19 Run + Process/Resource Trace
  ↓
#20 Artifact + Version-aware Compiler Telemetry
  ↓
#21 Scenario + Comparison Discipline
  ↓
#23 Validated Profiles / Progressive Configuration
  ↓
#3/#7/#13–#17 and scheduler/cache research consume the same evidence spine
```

The detailed designs are documented in:

- `measurement-foundation.md`
- `measurement-foundation_ja.md`
- `multi-version-toolchains.md`
- `multi-version-toolchains_ja.md`
- `validated-toolchain-profiles.md`
- `validated-toolchain-profiles_ja.md`

## Core tracks

2. Compiler pipeline decomposition across supported toolchain versions — #3
3. Rust–Nim native linking without mandatory C ABI boundary — #4
4. Backend route selection and capability constraints — #5
5. Unified Action Graph and resource-aware scheduling — #6
6. Artifact identity, incremental invalidation and CAS — #7
7. Variant-space control in the Nim Planning Kernel — #8
8. WASM mixed-language integration/topology — #9
9. Agent-oriented explainability and evidence schema — #10
10. Work elimination, execution correctness and no-op build invariants — #12
11. Multi-version Rust/Nim toolchain selection and artifact compatibility — #22
12. Validated toolchain profiles and progressive configuration — #23

## Backend pipeline white-boxing expansion

13. Expand backend routes into nested observable/checkpoint/execution graphs and define checkpoint economics — #13
14. White-box LLVM pass/codegen/LTO pipeline boundaries without pass-per-process decomposition — #14
15. Map ThinLTO/DTLTO dynamic backend jobs into the LAMINARIA scheduler and cache graph — #15
16. Decompose the WebAssembly target pipeline through `wasm-ld`, Binaryen, WIT and componentization — #16
17. Evaluate shared LLVM IR/LTO convergence across Rust, Nim 2 and Nimony routes — #17

The detailed architecture is documented in:

- `backend-pipeline-whiteboxing.md`
- `backend-pipeline-whiteboxing_ja.md`

## Responsibility boundaries

### #18 versus #22

#18 owns installation/discovery, named toolchain sets, exact resolution and ToolchainFingerprint generation. #22 owns how compiler/toolchain version participates in Variant Graph resolution, capability constraints, artifact compatibility, and cross-version reuse policy.

### #22 versus #23

#22 owns the broad internal version/compatibility search space. #23 owns the narrower user-facing qualification layer: `recommended`, `latest-validated`, `long-term`, `preview`, progressive presets and advanced overrides.

A combination accepted by #22 is only a candidate for #23. Static compatibility does not imply recommendation.

### #23 versus #11/#18–#21

#23 does not invent a separate trust system. Profile qualification must consume the common Measurement Spine evidence from #11/#18–#21. Profile aliases resolve to exact ToolchainFingerprint bundles and immutable profile revisions before execution.

### #22 versus #3/#7/#20

#22 defines the common multi-version model. #3 maps actual compiler-pipeline boundaries per toolchain capability, #7 applies the exact toolchain identity to cache/artifact compatibility, and #20 adapts version-specific native telemetry into the common Run schema.

### #11/#18–#21 versus later research

#11 and #18–#21 own the common evidence model: environment/toolchain identity, Run/process/resource trace, artifact/telemetry records, scenario/repetition/comparison semantics and observer-overhead measurement.

Later research may extend these schemas with backend-specific data, but must not create incompatible benchmark/evidence stores.

### #5 versus #13

#5 answers **which backend route is valid and selected**. #13 answers **how the selected backend expands into internal computation and which boundaries become observable/checkpoint/execution nodes**.

### #9 versus #16

#9 compares mixed-language WebAssembly integration topologies and boundary costs. #16 white-boxes the target production pipeline itself: relocatable Wasm, `wasm-ld`, Core Wasm, Binaryen, WIT/adapters and componentization.

### #4 versus #17

#4 tests direct Rust–Nim native object/link contracts without a mandatory C ABI boundary. #17 tests whether Rust/Nim-origin LLVM artifacts can participate in a shared LLVM/LTO plan and explicitly separates backend artifact compatibility from language/runtime ABI compatibility.

### #6/#7/#12 versus #15

#15 is not a separate scheduler/cache architecture. It is the ThinLTO/DTLTO stress case that must use #6 scheduling, #7 identity/CAS and #12 work-elimination semantics. DTLTO's externally described backend jobs are used to test dynamic graph expansion rather than adding a hidden nested scheduler.

## Toolchain profile rule

LAMINARIA intentionally has two different surfaces:

```text
Internal: broad candidate variant space
  ↓ constraint compatibility
User-facing: narrow evidence-backed profiles
```

Default user-facing profiles should include at least `recommended`, `latest-validated`, `long-term`, `preview`, and `custom`.

Upstream freshness/support and LAMINARIA qualification are separate dimensions. In particular, a newest stable release is not automatically recommended until qualified.

Rust's normal upstream model is stable/beta/nightly; LAMINARIA must not imply an upstream Rust LTS channel. A `long-term` Rust-containing profile is a LAMINARIA-maintained bundle with its own support policy.

Configuration is progressively disclosed:

1. profile only;
2. intent preset;
3. advanced overrides;
4. expert graph constraints.

Any override must re-evaluate qualification status rather than inheriting the base profile's validation badge.

## Multi-version toolchain rule

Compiler version is a graph dimension, not an ambient machine setting.

- Rust and Nim toolchains are resolved from selectors to exact ToolchainFingerprint values;
- Cargo `rust-version`, Rust edition, selected rustc, Cargo resolver behavior and nightly capability are separate constraints;
- one connected normal Cargo/Rust crate graph normally resolves to one Rust toolchain;
- cross-version `rmeta`/`rlib`/internal compiler artifacts are not assumed compatible;
- lower-level object/archive/Wasm/native boundaries may be studied separately with explicit compatibility evidence;
- Nim 2/Nim 3 and multiple Rust versions use the same high-level toolchain-version abstraction while preserving language-specific metadata.

## Environment rule

Reproducibility and canonical performance isolation are separate concerns.

- container/Nix-like environments may reproduce bootstrap/correctness;
- canonical performance baselines normally execute natively on the measured environment;
- WSL, native Linux, macOS and other host classes have distinct EnvironmentFingerprint values;
- different fingerprints are non-comparable by default unless an explicit cross-environment study says otherwise;
- resolved tool executables/revisions are recorded rather than only requested version labels.

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

Single wall-clock samples are insufficient for architecture decisions. The measurement spine stores raw samples, characterizes environment noise, records explicit cold/warm/no-op/cache state, and measures observer overhead.

Backend checkpoint work must measure both benefit and cost. At least one overly fine checkpoint candidate must be allowed to fail the economics test; increasing graph granularity is not itself a success criterion.

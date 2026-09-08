# Measurement Foundation Research Direction

## Purpose

Before LAMINARIA optimizes compiler pipelines, backends, LTO, linkers, or WebAssembly target pipelines, it needs a permanent measurement spine that can reproduce the same input under the same toolchain and execution state, then observe the whole path in terms of time, resources, artifacts, and compiler-native telemetry.

This is not a disposable benchmark script. It is the first implementation of the observation contract that future LAMINARIA Action Graph, scheduler, cache/CAS, and explainability features will consume.

Central rule:

> Measure the whole path before optimizing a hidden part of it.

A Run should cover the real path from workspace preparation through frontend, codegen, backend, link, post-link, and final requested artifact instead of beginning with isolated LLVM microbenchmarks.

## 1. First things to stabilize

The repository is currently research-document centric and has no LAMINARIA runtime, CI harness, toolchain lock, or benchmark collector. This makes it possible to make measurement infrastructure the first permanent code.

Stabilize first:

1. environment/toolchain identity;
2. run/scenario identity;
3. process/resource tracing;
4. compiler-native telemetry adapters;
5. artifact inventory;
6. baseline/repetition/comparison policy;
7. measurement overhead itself.

LLVM/ThinLTO/WASM white-boxing (#13–#17) should extend the common Run/Trace/Artifact schema rather than invent separate evidence formats.

## 2. Separate reproducibility from performance isolation

A reproducible environment and a representative performance environment are not the same problem.

Containers, Nix, or equivalent mechanisms may be used for bootstrap and correctness reproduction. Canonical performance baselines should normally execute natively on the host being measured so VM filesystem bridges and container-host effects are not silently attributed to the compiler.

WSL is a distinct environment class, not interchangeable with native Linux. Runs with different EnvironmentFingerprint values are not directly comparable for performance by default.

## 3. Repository-owned toolchain lock

Define a repository-owned toolchain manifest such as:

```text
toolchains.lock.toml
```

It should pin or identify:

- Rust stable/nightly and required components;
- Nim 2 and Nimble;
- Nimony/Nim 3 source revision/build identity;
- nlvm revision where required;
- LLVM/Clang/LLD/opt/llc;
- `wasm-ld`;
- Binaryen / `wasm-opt`;
- `wasm-tools`;
- target sysroot/SDK/WASI SDK identities.

Ecosystem-native manifests such as `rust-toolchain.toml` may coexist, but measurement records must fingerprint the executable that actually ran: path, reported version, source/release revision, binary digest where practical, and host/target information.

Do not use `latest` as measurement identity.

## 4. EnvironmentFingerprint

Before each Run, record a machine-readable fingerprint including:

```text
schema version
OS/version
kernel
architecture
CPU model and topology
memory capacity
filesystem type for source/build/cache paths
virtualization/container/WSL status
relevant process/resource limits
repository commit and dirty state
toolchain fingerprints
target triple/features
sysroot/SDK identities
measurement harness version
```

Where relevant, also record CPU governor/turbo/power mode, cgroup/CPU quota, memory limits, and swap.

Environment capture must use an allow-list and must not store secrets.

## 5. Run envelope

All measurements use one versioned Run model:

```text
Run
  run_id
  schema_version
  workload_id
  scenario_id
  requested_artifact
  environment_fingerprint
  toolchain_fingerprint
  preparation_record
  cache_state
  root_command
  monotonic start/end timestamps
  result
  process_trace
  compiler_telemetry
  artifact_delta
  measurement_overhead
```

Candidate storage layout:

```text
runs/<run-id>/
  run.json
  environment.json
  processes.jsonl
  compiler-events.jsonl
  artifacts.jsonl
  stdout.log
  stderr.log
  summary.json
```

Raw run directories are normally ignored by Git. Schemas, scenarios, fixtures, and selected reference results are versioned.

## 6. Global clock and process tracing

Use one monotonic Run clock. Observe the root command and child process tree.

Capture where available:

```text
pid / parent relation
executable identity
normalized argv
cwd
start/end timestamp
exit status
user/system CPU
peak RSS
read/write I/O
major/minor faults
context switches
```

Use layered probes:

- Level 0: portable lifecycle and wall time;
- Level 1: OS resource/process-tree data;
- Level 2: compiler-native telemetry;
- Level 3: platform profiler such as `perf`, tracing, or optional eBPF.

Privileged profiling is not a baseline requirement.

## 7. Compiler-native telemetry adapters

Process traces cannot explain compiler internals, so normalize tool-native telemetry onto the same Run clock.

### Rust / Cargo

- Cargo `--timings` may be retained as supplementary human evidence; its stable timing report is not the canonical machine-readable source.
- rustc nightly `-Z self-profile` / measureme is a candidate for compiler-query and stage observation.
- Cargo JSON messages may assist artifact/process relationships.

### LLVM

Capture pass timing, optimization remarks, time trace/statistics where applicable, selected IR/bitcode/codegen markers, and ThinLTO/DTLTO manifests (#14/#15).

### Nim / Nimony

Investigate stage timing/artifact diagnostics separately for Nim 2 and Nimony. Use instrumented compiler builds or wrappers when native telemetry is insufficient.

### WebAssembly

Integrate `wasm-ld`, Binaryen post-link data, and WIT/embed/adapter/componentization evidence (#16).

If native telemetry is unavailable, mark the stage opaque instead of assigning the outer process time to an invented internal stage.

## 8. Artifact inventory

Record artifact changes before and after the Run for declared observation roots.

Examples include Rust metadata/rlib, emitted MIR/LLVM IR/bitcode, Nim-generated C/C++, objects, archives, ThinLTO indexes, native outputs, relocatable Wasm objects, Core Wasm modules, optimized Wasm modules, WIT/component metadata, adapters, and final Components.

Artifact records include logical path, type, size, content digest, producer identity when proven, and create/change/delete state.

Do not blindly rehash every file on every no-op measurement. Measure metadata-scan, changed-candidate detection, hashing, and I/O cost so artifact detection itself can be optimized.

## 9. Scenario state machine

A benchmark scenario is defined by **pre-state + change + requested artifact**, not just a command.

Initial classes:

```text
clean/cold build
warm rebuild
true no-op
Rust implementation-only edit
Nim implementation-only edit
backend/config-only change
link-only change
worktree relocation with identical content
```

Later scenarios include ThinLTO partial edits, Wasm link/post-link changes, and WIT/adapter/component-only changes.

Preparation is excluded from the timed command but is recorded explicitly.

## 10. Explicit cache state

Do not use `warm` as the only cache description. Record Cargo/target state, compiler incremental state, sccache-like state where present, future LAMINARIA CAS/action cache, ThinLTO cache, page-cache policy where controlled, and artifact-directory preparation.

Cache-clear operations are part of Run preparation evidence.

## 11. Baseline, repetition, and noise

Architecture decisions must not depend on one wall-clock sample.

Store every raw sample and repeat within one EnvironmentFingerprint. Reports should be able to expose sample count, min, median/p50, p90 where meaningful, mean, variance/standard deviation, and relative difference.

Use more stable counters such as CPU time, instructions, or cycles where available rather than replacing wall time with them.

`rustc-perf` is a reference for collector/benchmark/comparison separation. LLVM test-suite/LNT is a reference for machine-readable compile/runtime metrics and result comparison.

Regression thresholds should reflect the measured noise floor rather than one universal percentage.

## 12. Measure measurement overhead

Observer cost is part of the design.

Compare modes such as:

```text
minimal wrapper
process tracing
compiler telemetry
artifact hashing
platform profiler
```

Record the wall/CPU/I/O/memory overhead introduced by each layer. A finer trace that materially slows compilation is itself an architectural cost.

## 13. Visualization and export

The canonical store is the versioned LAMINARIA schema, not a UI format.

Exports may include Perfetto/Chrome Trace, JSON summaries, CSV/Parquet, and human-readable comparisons.

The timeline should eventually correlate processes, compiler stages, artifact production, waits, and resource use on one clock.

## 14. Responsibility of the first permanent code

The first implementation should remain useful when the full LAMINARIA planner/scheduler exists.

Responsibilities:

```text
bootstrap / doctor
resolve and fingerprint tools
execute scenario
own root process lifecycle
collect process/resource data
normalize telemetry
record artifact deltas
write versioned Run results
compare runs
```

Ordinary Cargo/rustc/Nim/LLVM execution can be wrapped and measured before LAMINARIA replaces any scheduling responsibility.

## 15. Success conditions

The foundation is established when at least:

1. a fresh environment can reproduce the pinned toolchains;
2. doctor output explains environment/toolchain differences;
3. one Rust and one Nim workload can be captured in the Run schema;
4. process tree and major wall/CPU/memory/I/O metrics are correlated;
5. artifact deltas are recorded;
6. cold/warm/no-op are reproducible distinct scenarios;
7. raw samples can regenerate comparison reports;
8. tracing/hashing overhead is measured;
9. different EnvironmentFingerprint values are not silently compared as the same baseline;
10. #14–#17 can attach additional telemetry to the same schema.

## 16. Reference projects

- `rust-lang/rustc-perf` — compiler performance collector, corpus, continuous comparison;
- Rust Compiler Development Guide — `-Z self-profile`, `perf`, Cargo timing entry points;
- LLVM test-suite / LNT — machine-readable compile/runtime metrics and comparisons;
- LLVM ThinLTO / DTLTO — later dynamic backend-job integration.

The first goal is not a dashboard. It is to ensure that the computation LAMINARIA intends to improve is already observable before LAMINARIA starts changing it.

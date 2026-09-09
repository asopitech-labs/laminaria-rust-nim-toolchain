# LAMINARIA — Horizontal Distribution and Distributed Compiler Research

## Position

LAMINARIA treats horizontal distribution as a research subject, not as a deployment feature to be added after the compiler graph is designed.

The central question is not simply whether compiler commands can be sent to other machines. It is:

> Starting from Rust and Nim semantic facts, what computation can be partitioned across machines while preserving correctness, optimization legality, incrementality, resource accounting, and explanation?

This document is a cross-cutting research charter for #6, #7, #13–#17 and #25. It does not make Kbuild, LLVM DTLTO, Bazel, distcc, or icecream the LAMINARIA architecture.

## 1. Three different kinds of distribution

The following must not be conflated.

### 1.1 Source/build-graph distribution

Independent source files or generated translation units are compiled on different workers.

```text
source/header/config graph
        ↓
translation units
        ↓
object files × N
        ↓
archives / final link
```

This is the ordinary Linux Kbuild and distcc/icecream pattern. It scales well when translation units are sufficiently independent, but it does not preserve a rich cross-language semantic model beyond the compiler invocation boundary.

### 1.2 Backend distribution after a global analysis

A global or cross-module analysis first produces summaries or indexes. Independent backend jobs then compile partitions using those results.

```text
lowered modules
        ↓
global summary / thin-link
        ↓
partition indexes
        ↓
backend jobs × N
        ↓
native objects / final link
```

LLVM ThinLTO and DTLTO are the primary prior-art examples. The global barrier and the partition contract are as important as the worker parallelism.

### 1.3 Action-level remote execution

A build system constructs an action graph and sends declared actions to a remote execution service.

```text
declared action graph
        ↓
CAS / action cache
        ↓
remote workers × N
        ↓
action results
```

Bazel Remote Execution is the representative prior art. In this model the compiler is normally a tool invoked by an action, not the owner of the distributed semantic model.

## 2. Prior-art comparison

| System | Distributed unit | Global information | Main boundary | What it does not answer |
| --- | --- | --- | --- | --- |
| Linux Kbuild | source/object compilation | `.config`, generated headers, dependency files | object, archive, final link | cross-language semantic partitioning |
| distcc | compiler invocation | command-line/environment contract | preprocessed source or compiler input | optimization legality and semantic provenance |
| icecream | compiler invocation with central scheduling | compiler environment package and worker load | compiler invocation | common Rust/Nim computational representation |
| LLVM ThinLTO | per-module backend compilation | combined summary and per-module indexes | thin-link, backend object, final link | whether LLVM's summary/partition is right for LAMINARIA |
| LLVM DTLTO | ThinLTO backend jobs described by JSON | LLD-generated job manifest | external distributor and final link | upstream semantic information lost before bitcode |
| Bazel Remote Execution | declared build/test Action | action inputs, command, environment, platform | action/CAS/result | compiler-internal boundaries and language semantics |

The closest existing design to compiler-level horizontal distribution is LLVM DTLTO. The closest design to a globally distributed heterogeneous build graph is Bazel Remote Execution. Neither is a substitute for the LAMINARIA substrate question.

## 3. Linux kernel as a distribution case study

Linux is not normally built as one globally optimized program representation. Kbuild reads `.config`, descends through configuration-selected directories, compiles `obj-y` and `obj-m` entries, combines built-in objects into directory-level `built-in.a` archives, and eventually links `vmlinux` and modules.

The conceptual graph is:

```text
.config + generated headers
        ↓
Kbuild directory/object selection
        ↓
C / assembly / Rust translation units
        ↓
object files (parallel)
        ↓
directory built-in.a archives
        ↓
vmlinux / modules
        ↓
architecture-specific post-processing
```

`make -jN` exposes parallelism between independent build actions. The kernel jobserver also exists to pass the available parallelism budget to helper programs instead of allowing nested invocations to exceed the parent limit.

This design has two important research observations:

1. object compilation is a productive coarse execution boundary;
2. archive membership and link order remain semantic or operational constraints, so arbitrary worker completion order cannot define the final artifact.

Kbuild can select Clang and LLVM utilities with `make LLVM=1`. The current Linux source tree also contains an experimental `LTO_CLANG_THIN_DIST` mode in which ThinLTO index generation and backend compilation are made explicit, producing native objects after the ThinLTO backend phase. This creates a useful comparison between ordinary Kbuild distribution and explicit distributed ThinLTO.

Linux therefore demonstrates a layered design rather than one universal distributed compiler:

```text
Kbuild source DAG
    + optional compiler invocation distribution
    + optional ThinLTO global-summary/backend distribution
    + final architecture-specific link and post-processing
```

## 4. LLVM ThinLTO and DTLTO as prior art

LLVM ThinLTO separates a global thin-link from parallel backend compilation. DTLTO integrates this with the link step and asks LLD to generate a JSON description of backend jobs. Each job includes a module bitcode input, an individual index and output paths; common compiler arguments and inputs are represented separately.

The important property is not the JSON format. It is the explicit contract:

```text
global analysis
  → explicit partition/index
  → independently executable backend job
  → native object
  → final linker integration
```

DTLTO deliberately keeps distribution-system details outside LLVM. It also requires matching compiler/linker versions and a distributor that understands the version-specific schema. These are direct reminders that a remote job is not identified by its source file alone.

For LAMINARIA, the DTLTO model is a baseline to measure, not a canonical answer. We must ask:

- which facts made the global summary necessary;
- which summary facts are required for correctness and which only improve profitability;
- whether the module is the right partition unit for Rust and Nim;
- whether language-specific semantic facts should cross the distribution boundary;
- what is invalidated after a semantic, toolchain, target or profile change;
- whether the final link can consume results without hiding a nested scheduler;
- whether another partition is better for incrementality or resource locality.

## 5. Candidate LAMINARIA distributed model

LAMINARIA must not begin with merged LLVM IR as its canonical distributed input.

```text
Rust semantic facts ─┐
                     ├→ preserved facts + provenance
Nim semantic facts  ─┘
                              ↓
                    global requirements / summaries
                              ↓
                    candidate partition manifest
                              ↓
                 remote semantic/backend actions × N
                              ↓
                    backend-specific projection
                              ↓
                     objects / archives / final link
```

A candidate partition manifest should identify, at minimum:

- semantic workload and demanded artifact;
- partition identity and provenance;
- required cross-partition facts;
- legality assumptions and unresolved obligations;
- exact language/compiler/backend/toolchain identities;
- target, data layout, ABI and feature contract;
- declared input artifacts and output artifacts;
- resource requirements and expected cost;
- cache/invalidation identity;
- scheduler placement constraints;
- explanation of why this partition is valid.

The representation may be one IR, multiple IRs, typed fact sets, graph relations, analysis databases, or a hybrid. The experiment must decide this from workload evidence.

## 6. Persistence and materialization are part of the partition decision

Do not model persistence as “write every intermediate to the local disk.” A logical artifact and its physical replicas are different objects. Candidate storage tiers include:

- in-memory or process-local state;
- local SSD/NVMe;
- peer or worker-local cache;
- remote CAS/object storage;
- durable archival storage.

The scheduler should choose whether to keep, materialize, replicate, transfer or recompute an intermediate. The decision depends on reuse probability, recovery value, data locality, capacity, serialization/hash cost, transfer bandwidth/latency, storage bandwidth/latency, consistency/commit cost and failure risk. A network path may beat an older local HDD for large transfers, but small I/O latency, availability and coordination can reverse that result. Locality is therefore a measured variable, not a preference rule.

The persistence record must separate:

- logical artifact identity, semantic/provenance identity and compatibility;
- physical replica locations and worker/storage capabilities;
- replica lineage, completeness, commit state, retention/GC and recovery status;
- bytes serialized, hashed, written, read, uploaded, downloaded and recomputed;
- the explanation for choosing persistence, transfer or recomputation.

## 7. Research questions

1. What is the smallest global fact set that permits independent Rust/Nim backend work?
2. Can a partition be defined before lowering semantic information into a backend-specific IR?
3. Which semantic facts must be replicated to workers, and which can be summarized?
4. Does module-level partitioning minimize total time, or does it create poor load balance and invalidation scope?
5. When does communication cost exceed the benefit of remote execution?
6. Can worker results be reused across source edits, worktrees, machines and compiler versions?
7. How should failed, unavailable or heterogeneous workers affect correctness and reproducibility?
8. Can LAMINARIA prevent nested compiler/backend schedulers from competing with the global scheduler?
9. Which boundaries are logical, observation, checkpoint, execution or dynamic-expansion boundaries?
10. Which LLVM/Kbuild/Bazel design choices are independently necessary for LAMINARIA, and which are historical or backend-specific choices?
11. When is remote persistence cheaper or more useful than local persistence or recomputation?
12. Which intermediate artifacts should remain vertically integrated and ephemeral, and which deserve horizontal materialization?

## 8. Falsifiable hypotheses

### H1 — Coarse translation-unit distribution is a useful baseline

For sufficiently independent Rust, Nim-generated-native, or C translation units, object-level distribution reduces wall time without changing the final artifact. The claim fails if transfer, environment preparation, or load imbalance dominates the saved local CPU time.

### H2 — Global analysis is a real barrier, not an implementation detail

For cross-module optimization, a worker cannot safely make all decisions from local inputs alone. Removing or weakening the required global facts must cause either a missed optimization, an unsafe transformation, or an explicit conservative fallback.

### H3 — Backend partitioning is not automatically the semantic partition

The partition that LLVM uses after lowering may not preserve the facts LAMINARIA needs for cross-language planning or precise invalidation. This hypothesis is supported if a source/semantic partition retains useful information or invalidates less work than a bitcode-module partition.

### H4 — Remote execution requires a stronger identity than source content

A reusable remote result must include producer, compiler/backend version, target/data layout/features, relevant flags, semantic provenance and environment contract. Reusing a result across a changed identity must either be rejected or proven equivalent.

### H5 — Finer distribution is not monotonically better

Splitting by LLVM pass, small semantic relation or tiny generated artifact eventually loses locality, increases serialization and scheduling overhead, or changes optimization quality. At least one over-fine candidate must be measured and rejected.

## 9. Workloads and experiments

### Experiment 0 — Kbuild-shaped object DAG

Use a configuration-selected, many-translation-unit workload to measure the baseline:

- graph construction and dependency discovery;
- object compile parallelism;
- archive and link barriers;
- load balance;
- generated-header and configuration invalidation;
- local versus remote transfer cost.

### Experiment 1 — LLVM ThinLTO/DTLTO baseline

Capture thin-link, index, backend-job and final-link evidence. Record the exact job manifest, compiler/linker identity, cache behavior, job durations, input/output sizes and final artifact.

### Experiment 2 — Rust/Nim semantic partition candidate

Define paired Rust/Nim workloads from source contracts, not from merged LLVM IR. Build a candidate preserved-fact and provenance representation, derive a partition manifest, project at least one partition to LLVM, and keep the representation open to another backend.

### Experiment 3 — Partition comparison

Compare source/object, LLVM module, ThinLTO backend, and candidate semantic partitions on the same workload. A successful build is not enough; the comparison must include retained information, legality evidence and invalidation scope.

### Experiment 4 — Incremental and no-op distribution

Repeat after:

- a local semantic edit;
- a change in a Rust/Nim boundary;
- a generated-header/configuration change;
- a target or feature change;
- a compiler/backend change;
- an unchanged rebuild.

Measure which remote jobs are avoided, which are reused, which are invalidated and how much metadata/transfer work is required to determine that result.

### Experiment 5 — Failure and heterogeneity

Remove workers, vary worker speed, change compiler identity, inject a missing input, and interrupt a backend job. The result must fail closed or use an explicit documented fallback; silent local rebuilding is not equivalent to successful distributed execution.

## 10. Required evidence

- exact source revisions and semantic workload contracts;
- exact Rust/Nim/frontend/backend/compiler/linker/toolchain identities;
- machine and worker EnvironmentFingerprints;
- graph and partition manifests;
- declared inputs, outputs and content digests;
- global-summary/index provenance;
- scheduler placement, queue wait, execution time and resource use;
- transfer, serialization, hashing and CAS costs;
- cache hits, misses and invalidation reasons;
- expected versus actual execution set;
- final artifact correctness and runtime checks;
- semantic information retained, transformed and lost;
- negative results, fallbacks and unresolved obligations.

## 11. Completion criteria

This research is not complete when a Linux kernel or LLVM ThinLTO build merely runs on multiple machines.

Completion requires:

1. at least one Kbuild-shaped or equivalent object-DAG distribution baseline;
2. at least one ThinLTO/DTLTO-style global-summary-to-backend-job experiment;
3. at least one candidate partition beginning from preserved Rust/Nim semantic facts rather than merged LLVM IR;
4. a comparison of at least two partition granularities with measured communication and locality costs;
5. a controlled incremental experiment proving the avoided and executed job sets;
6. an identity and failure policy that prevents unsafe cross-toolchain reuse;
7. at least one LLVM/Kbuild design choice independently supported by LAMINARIA evidence;
8. at least one distribution boundary rejected, reformulated or left unresolved based on evidence;
9. evidence that the global scheduler, not a hidden nested compiler scheduler, accounts for worker resource use;
10. reproducible commands, fixtures and committed machine-readable evidence.

## 12. Relation to existing research tracks

- **#6 Scheduler:** owns global resource-aware placement and execution accounting.
- **#7 CAS/invalidation:** owns identity and reuse policy for remote inputs and outputs.
- **#13 Backend graph:** owns logical/observation/checkpoint/execution boundary economics.
- **#14 LLVM white-boxing:** supplies LLVM pass, analysis and backend evidence, but does not define the LAMINARIA partition.
- **#15 ThinLTO/DTLTO:** supplies the global-summary and dynamic backend-job baseline.
- **#17 Rust/Nim LLVM convergence:** supplies lowered-artifact compatibility evidence, not the final common substrate.
- **#25 LLVM rediscovery:** decides whether the distributed partition should be LLVM-derived, semantic-fact-derived, or a hybrid.

## 13. Non-goals

- claiming that every compiler pass should become a remote process;
- treating remote execution speedup as proof of a common semantic substrate;
- treating successful linking as proof of cross-language optimization compatibility;
- hiding environment, toolchain or target differences behind a generic worker label;
- adopting LLVM's module partition or Bazel's Action shape without independently testing its necessity;
- creating a separate evidence store incompatible with the measurement spine.

## References

- [Linux Kernel Makefiles](https://www.kernel.org/doc/html/latest/kbuild/makefiles.html)
- [Building Linux with Clang/LLVM](https://kernel.org/doc/html/next/kbuild/llvm.html)
- [Linux kernel jobserver module](https://www.kernel.org/doc/html/latest/tools/jobserver.html)
- [Linux kernel `arch/Kconfig` — LTO and distributed ThinLTO options](https://github.com/torvalds/linux/blob/master/arch/Kconfig)
- [LLVM DTLTO](https://www.llvm.org/docs/DTLTO.html)
- [Clang ThinLTO](https://clang.llvm.org/docs/ThinLTO.html)
- [Bazel Remote Execution](https://bazel.build/remote/rbe)
- [Bazel Remote Caching](https://bazel.build/remote/caching)
- [distcc](https://github.com/distcc/distcc)
- [icecream](https://github.com/icecc/icecream)

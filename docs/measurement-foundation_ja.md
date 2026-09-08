# Measurement Foundation / 計測基盤 研究方針

## 目的

LAMINARIAがcompiler pipeline、backend、LTO、linker、WebAssembly target pipelineを改善する前に、同じ入力を同じ環境・同じtoolchain・同じ実行状態で再現し、処理フロー全体を時系列・resource・artifact・compiler-native telemetryの観点から観測できる恒久的な「計測の背骨」を構築する。

この基盤は一時的なbenchmark scriptではない。将来のLAMINARIA Action Graph、scheduler、cache/CAS、explainabilityが利用する観測契約の初期実装とする。

中心原則:

> Measure the whole path before optimizing a hidden part of it.

最初からLLVM pass単体のmicrobenchmarkだけを見るのではなく、workspace preparationからfrontend、codegen、backend、link、post-link、final artifactまで、requested artifactへ到達する実際の処理を一つのRunとして記録する。

## 1. 現状と優先順位

現時点のrepositoryは研究文書を中心とし、LAMINARIA runtime、CI、toolchain lock、benchmark harnessはまだ存在しない。この段階では、最初の恒久コードをtoolchain本体より先にmeasurement foundationへ置ける。

最初に固定するもの:

1. environment/toolchain identity;
2. run/scenario identity;
3. process/resource trace;
4. compiler-native telemetry adapter;
5. artifact inventory;
6. baseline/repetition/comparison policy;
7. measurement overhead itself.

LLVM/ThinLTO/WASM white-boxing (#13–#17)は、この基盤が出す共通Run/Trace/Artifact schemaへデータを追加する形にする。

## 2. Reproducibility と Performance Isolation を分離する

再現可能な環境と、性能を正しく測る環境は同じ問題ではない。

### Bootstrap / correctness environment

container、Nix等の再現可能なpackage/environment mechanismを利用してよい。目的はtoolchainを構築し、同じfunctional fixtureを再現すること。

### Canonical performance environment

性能baselineは原則として対象host上でnative executionする。Docker Desktop等のVM、host filesystem bridge、共有volumeのI/O特性をcompiler性能として混ぜない。

WSLはLinux nativeと同一baselineにまとめず、WSL/kernel/filesystem/virtualization情報を含む独立environment classとして扱う。

異なるEnvironmentFingerprintのRunはdefaultでは直接performance regression比較しない。

## 3. Toolchain Lock

tool manager固有のlockだけをsource of truthにせず、repository-ownedなtoolchain manifestを定義する。

候補:

```text
toolchains.lock.toml
```

最低限記録する対象:

- Rust stable/nightly channelまたはexact version/date;
- rustc/cargo/rustup component set;
- Nim 2 compiler + Nimble;
- Nimony/Nim 3 source revision/build identity;
- nlvm revision where experiments require it;
- LLVM/Clang/LLD/opt/llc versions/build identity;
- `wasm-ld`;
- Binaryen / `wasm-opt`;
- `wasm-tools`;
- target sysroot/SDK/WASI SDK identity where applicable.

Rustについては`rust-toolchain.toml`等、ecosystem-native manifestを併用してよい。ただしmeasurement recordは最終的に**実際に解決されたexecutable**を記録する。

各toolは可能な範囲で以下をfingerprintする。

```text
logical tool name
absolute executable path
reported version
source/release revision
binary digest
host/target information
relevant plugin/component identities
```

`latest`をmeasurement identityとして使用しない。

## 4. EnvironmentFingerprint

Run開始前にmachine/environmentをmachine-readableに記録する。

最低限:

```text
schema version
OS / version
kernel
architecture
CPU model
physical/logical CPU topology
memory capacity
filesystem type for source/build/cache paths
virtualization/container/WSL status
relevant process/resource limits
source repository commit + dirty state
toolchain fingerprints
target triple / target features
relevant sysroot/SDK identities
measurement harness version
```

性能に影響し得る場合はCPU governor、turbo/power mode、cgroup/CPU quota、memory limit、swap等も取得する。

環境変数はallow-list方式で保存し、token/password等のsecretをcaptureしない。

## 5. Run Envelope

すべての測定を共通Runとして保存する。

概念schema:

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
  start/end monotonic timestamps
  result
  process_trace
  compiler_telemetry
  artifact_delta
  measurement_overhead
```

保存形の候補:

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

`runs/`自体は通常git管理せず、fixture/scenario/schemaと選定されたreference resultsだけをversion管理する。

## 6. Global clock と Process Trace

Run全体に一つのmonotonic clockを持つ。

root commandだけでなくchild process treeを観測し、最低限以下を記録する。

```text
pid / parent relation
executable identity
normalized argv
cwd
start/end timestamp
exit status
user/system CPU time
peak RSS where available
read/write I/O where available
major/minor faults where available
context switches where available
```

portableなcore collectorを最初に持ち、platform-specific probeを追加できる構造にする。

### Probe levels

- **Level 0:** portable process lifecycle + wall time;
- **Level 1:** OS resource usage / process tree;
- **Level 2:** compiler-native telemetry;
- **Level 3:** platform profiler (`perf`, tracing, optional eBPF等)。

privileged profilerをbaseline実行の必須条件にしない。

## 7. Compiler-native telemetry adapters

process traceだけではcompiler内部を説明できないため、各toolが提供するnative telemetryを同じRun clockへ正規化する。

### Rust / Cargo

- Cargo `--timings`はcrate concurrencyのhuman-readable evidenceとして補助利用する。ただし現在のstable Cargoのtiming reportはhuman consumption向けでmachine-readable source of truthにはしない。
- rustc nightlyの`-Z self-profile` / measuremeをcompiler query/stage observationの候補とする。
- Cargo JSON messageはartifact/process relationshipの補助に使う。

### LLVM

- pass timing;
- optimization remarks;
- time trace / statistics where applicable;
- IR/bitcode/codegen stage markers;
- ThinLTO/DTLTO job manifest (#14/#15)。

### Nim / Nimony

compilerが直接提供するstage timing/artifact diagnosticsを調査し、足りない境界はinstrumented buildまたはwrapperで補う。Nim 2とNimonyは同じadapterと仮定しない。

### WebAssembly

- `wasm-ld` link stage;
- Binaryen pass/post-link data;
- WIT/embed/adapter/componentization stage (#16)。

native telemetryが取れない場合は`opaque`として明示し、外側のprocess wall timeを内部stage時間と偽らない。

## 8. Artifact Inventory

Run前後で観測対象rootのartifact変化を記録する。

対象例:

```text
Rust metadata / rlib
MIR/LLVM IR/bitcode where emitted
Nim generated C/C++
object
archive
ThinLTO index/output
linked executable/library
relocatable Wasm object
Core Wasm module
optimized Core Wasm module
WIT/component metadata
adapter
WebAssembly Component
```

artifact recordにはlogical path、type、size、content digest、producer identity where proven、creation/change/delete stateを含める。

全fileを無条件に再hashしてno-op測定を破壊しない。metadata scan、changed-candidate detection、content hashのコスト自体を測定し、より安い判定方式へ改善できるようにする。

## 9. Scenario State Machine

benchmarkはcommand名ではなく**実行前状態 + change + requested artifact**で定義する。

初期scenario class:

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

後続:

```text
ThinLTO single-module implementation edit
ThinLTO interface/import-affecting edit
Wasm link-only change
Binaryen option-only change
WIT/adapter/component-only change
```

Scenario preparationはtimed commandから分離するが、何をdelete/modify/seedしたかはRunに記録する。

## 10. Cache State を明示する

`warm`という語だけで状態を表現しない。

最低限:

- Cargo target/build cache state;
- incremental compiler state;
- sccache等のcompiler cache state when used;
- LAMINARIA CAS/action cache state when introduced;
- ThinLTO cache state;
- filesystem/page-cache policy where controlled;
- artifact directory preparation state.

cacheをclearした操作もRun preparationへ記録する。

## 11. Baseline / Repetition / Noise Policy

単発wall-clock値をarchitecture判断へ使わない。

すべてのraw sampleを保持し、同一EnvironmentFingerprint内でrepeat可能にする。

reportは少なくとも以下を出せるようにする。

```text
sample count
min
median / p50
p90 where sample count permits
mean
variance / standard deviation
relative difference
```

compile benchmarkではwall timeだけでなくCPU time、instruction/cycle等のより安定したmetricが取れる環境では併用する。

reference implementationとしてRustの`rustc-perf`はcollectorと継続比較の分離、LLVM test-suiteはJSON result、compile_time/code size、複数result比較の考え方を参考にする。

regression thresholdは固定の万能値にせず、baseline repeatからnoise floorを推定し、measurement qualityと共に判定する。

## 12. Measurement Overhead を測る

observer effectを隠さない。

比較mode:

```text
measurement disabled / minimal wrapper
process tracing enabled
compiler telemetry enabled
artifact hashing enabled
platform profiler enabled
```

各層がwall/CPU/I/O/memoryへ与えるoverheadを記録する。

traceを細かくした結果compilerが遅くなった場合、そのコストもLAMINARIAの設計コストである。

## 13. Visualization / Export

canonical storeは独自versioned schemaとし、可視化形式をidentityにしない。

export候補:

- Chrome Trace / Perfetto timeline;
- JSON summary;
- CSV/Parquet等の分析用出力;
- human-readable comparison report.

timelineではprocess、compiler stage、artifact production、wait、resource usageを同一時間軸に表示できることを目標とする。

## 14. 最初の恒久コードの責務

最初のimplementationは単なるshell benchmark collectionではなく、将来LAMINARIA runtimeへ残るmeasurement/runtime infrastructureとする。

責務:

```text
bootstrap / doctor
resolve and fingerprint tools
execute scenario
own root process lifecycle
collect process/resource data
normalize telemetry
record artifact deltas
write versioned Run result
compare runs
```

toolchain自体のplanning/scheduler実装前でも、ordinary Cargo/rustc/Nim/LLVMをこのwrapper経由で実行してbaselineを蓄積できる。

## 15. 成功条件

足場が成立したと言えるのは、少なくとも次を満たした時である。

1. fresh machine/environmentでpinned toolchainを再現できる;
2. `doctor`相当の出力からenvironment/toolchain差を説明できる;
3.一つのRust workloadと一つのNim workloadをRun schemaで計測できる;
4. process tree、wall/CPU/memory/I/Oの主要値を関連付けられる;
5. generated artifactの前後差を記録できる;
6. cold / warm / no-opを明確に異なるscenarioとして再現できる;
7. raw samplesとcomparison reportを再生成できる;
8. tracing/hashing自身のoverheadを測定できる;
9.異なるEnvironmentFingerprintを誤って同一baselineとして比較しない;
10. #14–#17がこのschemaへ追加telemetryを接続できる。

## 16. 参考プロジェクト

- `rust-lang/rustc-perf` — compiler performance collector、benchmark corpus、継続比較
- Rust Compiler Development Guide — `-Z self-profile`、`perf`、Cargo timings等のprofiling入口
- LLVM test-suite / LNT — reproducible compile/runtime metrics、JSON output、comparison
- LLVM ThinLTO / DTLTO — dynamic backend job manifestを伴う後続integration対象

この基盤の目的はbenchmark dashboardを先に作ることではない。**LAMINARIAが改善しようとしている計算を、改善前から正しく観測し続けられること**である。

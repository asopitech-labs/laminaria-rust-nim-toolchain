# Measurement Foundation / 計測基盤 研究方針

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

共通の証拠基盤を使いつつ、独自コンパイル・比較・bootstrapの結果を分離する。以下の外部process/native telemetry実験はbaselineとして保持する。独自経路にはsource→IRの由来、IR/変換revision、in-process compiler計算・解析無効化・scheduler所有権の証拠が別途必要であり、process traceだけで証明しない。必要最小の計測を#25/#3/#6/#8と並行し、計測全体の完了を直列の前提にしない。

## 目的

LAMINARIAがcompiler pipeline、backend、LTO、linker、WebAssembly target pipelineを改善する前に、同じ入力を同じ環境・同じtoolchain・同じ実行状態で再現し、処理フロー全体を時系列・resource・artifact・compiler-native telemetryの観点から観測できる恒久的な「計測の背骨」を構築する。

この基盤は一時的なbenchmark scriptではない。将来のLAMINARIA Action Graph、scheduler、cache/CAS、explainabilityが利用する観測契約の初期実装とする。

中心原則:

> Measure the whole path before optimizing a hidden part of it.

最初からLLVM pass単体のmicrobenchmarkだけを見るのではなく、workspace preparationからfrontend、codegen、backend、link、post-link、final artifactまで、requested artifactへ到達する実際の処理を一つのRunとして記録する。

## 1. 現状と優先順位

当初は研究文書中心であり計測基盤を最初のコードとして計画した。現在はlock、fingerprint、Run、CI、plannerと外部委譲build baselineが存在する。以下は計測trackの責務であり、独自compilerの実装より先に全計測を完成させるという順序ではない。

最初に固定するもの:

1. environment/toolchain identity;
2. run/scenario identity;
3. process/resource trace;
4. compiler-native telemetry adapter;
5. artifact inventory;
6. baseline/repetition/comparison policy;
7. measurement overhead itself.

LLVM/ThinLTO/WASM white-boxing (#13–#17)は、この基盤が出す共通Run/Trace/Artifact schemaへデータを追加する形にする。

複数compiler versionの扱いは `multi-version-toolchains_ja.md` をcanonical policyとし、#18/#22で実装・検証する。

## 2. Reproducibility と Performance Isolation を分離する

再現可能な環境と、性能を正しく測る環境は同じ問題ではない。

### Bootstrap / correctness environment

container、Nix等の再現可能なpackage/environment mechanismを利用してよい。目的はtoolchainを構築し、同じfunctional fixtureを再現すること。

### Canonical performance environment

性能baselineは原則として対象host上でnative executionする。Docker Desktop等のVM、host filesystem bridge、共有volumeのI/O特性をcompiler性能として混ぜない。

WSLはLinux nativeと同一baselineにまとめず、WSL/kernel/filesystem/virtualization情報を含む独立environment classとして扱う。

異なるEnvironmentFingerprintのRunはdefaultでは直接performance regression比較しない。

## 3. Multi-version Toolchain Lock

tool manager固有のlockだけをsource of truthにせず、repository-ownedなmulti-toolchain manifestを定義する。

候補:

```text
toolchains.lock.toml
```

このmanifestは単一のRust stable + nightlyを表すのではなく、複数のnamed compiler/toolchain setを同時に記述できる必要がある。

最低限記録する対象:

- 複数のRust exact stable/beta/nightly/source revision + required components;
- rustc/Cargo/sysroot/standard-library identity;
- Rust toolchainにbundled/selectedされたLLVM/codegen backend identity;
- Nim 2 exact compiler version/revision + Nimble;
- Nimony/Nim 3 source revision/build identity;
- nlvm revision where experiments require it;
- LLVM/Clang/LLD/opt/llc versions/build identity;
- `wasm-ld`;
- Binaryen / `wasm-opt`;
- `wasm-tools`;
- target sysroot/SDK/WASI SDK identity where applicable.

Rustについては`rust-toolchain.toml`等、ecosystem-native manifestを併用してよい。ただしそれはproject/user selectorであり、measurement recordは最終的に**実際に解決されたexact executable/toolchain**を記録する。

`stable`、`nightly`等のmoving selectorを受け付けてもよいが、artifact/cache/Run identityにはそのRunでresolvedされたexact version/revision/buildを使う。`latest`をmeasurement identityとして使用しない。

各toolchainは可能な範囲で以下をfingerprintする。

```text
logical toolchain name / requested selector
compiler family
exact resolved compiler version/revision/build
absolute executable path
binary digest
Cargo/Nimble identity
component set
sysroot / standard-library identity
bundled or selected LLVM/backend identity
host/target information
telemetry capability set
LAMINARIA adapter identity
```

RustではCargo `rust-version`、Rust edition、selected rustc、Cargo resolver behavior、stable/nightly capabilityを別constraintとして保持し、一つの`rust_version`へ潰さない。

詳細は `multi-version-toolchains_ja.md` を参照する。

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
selected/resolved toolchain fingerprints
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
  requested_toolchain_selector
  resolved_toolchain_fingerprint
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

telemetry supportは言語全体の固定属性ではなく、**resolved compiler/toolchainごとのcapability**として扱う。exact toolchain、adapter version、native event schema、coverage、unsupported/opaque regionを記録する。

### Rust / Cargo

- Cargo `--timings`はcrate concurrencyのhuman-readable evidenceとして補助利用する。ただしstable Cargoのtiming reportをmachine-readable source of truthにはしない。
- Cargo JSON messageはartifact/process relationshipの補助に使う。
- rustc `-Z self-profile` / measuremeは、選択されたexact toolchainが対応する場合のみcompiler query/stage observationに利用する。
- nightly/internal telemetryが存在しないRust versionでも共通Run schemaは成立させ、内部はopaque/coarse-grainedとして明示する。
- rustc version間でquery/stage名、telemetry schema、LLVM backendが異なる場合、その差を正規化で消さない。

### LLVM

- pass timing;
- optimization remarks;
- time trace / statistics where applicable;
- IR/bitcode/codegen stage markers;
- ThinLTO/DTLTO job manifest (#14/#15)。

LLVM telemetryには、そのLLVMがstandalone toolchainなのか、特定rustc/nlvm/Nimony routeにbundled/selectedされたものなのかを含むexact backend identityを付与する。

### Nim / Nimony

compilerが直接提供するstage timing/artifact diagnosticsをexact Nim 2/Nimony revisionごとに調査し、足りない境界はinstrumented buildまたはwrapperで補う。Nim 2とNimonyは同じadapter/capabilityと仮定しない。

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

artifact recordにはlogical path、type、size、content digest、exact producing ToolchainFingerprint、producer identity where proven、creation/change/delete stateを含める。

全fileを無条件に再hashしてno-op測定を破壊しない。metadata scan、changed-candidate detection、content hashのコスト自体を測定し、より安い判定方式へ改善できるようにする。

compiler-semantic artifactのcross-version reuseはdefaultで禁止し、artifact-kind-specific compatibility evidenceを要求する。詳細は #7/#22を参照する。

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
compiler/toolchain version-only change
```

後続:

```text
ThinLTO single-module implementation edit
ThinLTO interface/import-affecting edit
Wasm link-only change
Binaryen option-only change
WIT/adapter/component-only change
```

Scenario preparationはtimed commandから分離するが、何をdelete/modify/seed/selectしたかはRunに記録する。

## 10. Cache State を明示する

`warm`という語だけで状態を表現しない。

最低限:

- Cargo target/build cache state;
- incremental compiler state;
- sccache等のcompiler cache state when used;
- selected compiler/toolchain identity;
- LAMINARIA CAS/action cache state when introduced;
- ThinLTO cache state;
- filesystem/page-cache policy where controlled;
- artifact directory preparation state.

cacheをclearした操作もRun preparationへ記録する。

## 11. Baseline / Repetition / Noise Policy

単発wall-clock値をarchitecture判断へ使わない。

すべてのraw sampleを保持し、同一EnvironmentFingerprint内でrepeat可能にする。

compiler-version比較ではEnvironmentFingerprint、scenario、cache stateを原則固定し、selected ToolchainFingerprintだけを意図した変数として変更する。

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
resolve named toolchain selector
fingerprint exact resolved tools
execute scenario
own root process lifecycle
collect process/resource data
normalize version-aware telemetry
record artifact deltas
write versioned Run result
compare runs/toolchain versions
```

toolchain自体のplanning/scheduler実装前でも、ordinary Cargo/rustc/Nim/LLVMをこのwrapper経由で実行してbaselineを蓄積できる。

## 15. 成功条件

足場が成立したと言えるのは、少なくとも次を満たした時である。

1. fresh machine/environmentで複数のexact Rust toolchainとNim 2/Nim 3系toolchainを再現できる;
2. `doctor`相当の出力からenvironment/toolchain差を説明できる;
3.同一Rust workloadを複数exact Rust toolchainでRun schemaにより計測できる;
4.一つのNim 2 workloadとNimony/Nim 3 workloadを共通schemaへ接続できる;
5. process tree、wall/CPU/memory/I/Oの主要値を関連付けられる;
6. generated artifactの前後差とproducing ToolchainFingerprintを記録できる;
7. cold / warm / no-op / toolchain-version-only changeを明確に異なるscenarioとして再現できる;
8. raw samplesとcomparison reportを再生成できる;
9. tracing/hashing自身のoverheadを測定できる;
10.異なるEnvironmentFingerprintを誤って同一baselineとして比較しない;
11. cross-version compiler-semantic artifact reuseをcompatibility evidenceなしに行わない;
12. #14–#17/#22がこのschemaへ追加telemetry/compatibility evidenceを接続できる。

## 16. 参考プロジェクト

- `rust-lang/rustc-perf` — compiler performance collector、benchmark corpus、継続比較
- Rust Compiler Development Guide — `-Z self-profile`、`perf`、Cargo timings等のprofiling入口
- Cargo `rust-version` — package MSRVとtoolchain selection constraint
- rustc metadata (`rmeta`) — compiler version/metadata compatibilityの参考
- LLVM test-suite / LNT — reproducible compile/runtime metrics、JSON output、comparison
- LLVM ThinLTO / DTLTO — dynamic backend job manifestを伴う後続integration対象

この基盤の目的はbenchmark dashboardを先に作ることではない。**LAMINARIAが改善しようとしている計算を、複数compiler versionを含めて改善前から正しく観測し続けられること**である。

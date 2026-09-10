# LAMINARIA

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

独自コンパイラ・IR・スケジューラが本経路であり、任意の後続統合ではない。Cargo/Nim ecosystemは依存解決に利用できるが、以下に登場する既存コンパイル経路は比較・観測または外部bootstrapのbaselineであり、本ビルドの選択肢ではない。Action Graphは言語IRの代わりにならない。

## Rust Nim Unified Toolchain

### RustとNimのcompiler pipeline、依存関係、生成物、実行計画を単一の計算グラフとして扱う統合ツールチェーンの研究開発

---

## 1. プロジェクト概要

LAMINARIAは、RustとNimによるソフトウェア開発を、言語ごとに分離されたbuild systemの集合ではなく、一つの統合された計算システムとして扱うための研究開発プロジェクトである。

研究対象はpackage managementやbuild commandの統合だけではない。RustとNimのcompiler pipelineそのものを分解し、次の計算過程をdependency、artifact、constraint、actionによって構成される共通グラフとして扱う。

```text
Source
↓
Semantic Analysis
↓
Language IR
↓
Specialization / Transformation
↓
Code Generation
↓
Backend
↓
Machine Artifact
↓
Archive / Link
```

LAMINARIAは、**Package Graph → Program Graph → Variant Graph → Artifact Graph → Action Graph** という複数レベルのモデルを構築し、その上で依存解決、組合せ探索、incremental computation、cache identity、critical path解析、resource-aware schedulingを行う。

LAMINARIA自身もRustとNimで実装する。RustはCLI、application logic、OS interaction、process execution、cache、storage、runtime schedulingを担当し、Nimはgraph resolution、constraint propagation、variant exploration、graph transformation、critical-path analysis、組合せ最適化などのplanning kernelを担当する。LAMINARIA自身を、研究対象となるRust+Nim統合compiler/build architectureのreference implementationとする。

## 2. 背景

RustとNimを同一プロジェクトで利用する場合、実際には複数の独立した計算系が存在する。

```text
Cargo dependency resolution Nimble dependency resolution
Rust compiler pipeline Nim compiler pipeline
Rust codegen backend Nim backend generation
C / C++ compiler and linker FFI and binding generation
compiler cache / build cache CI scheduler
```

一般的なbuild orchestrationは、これらを `cargo build`、`nimble build`、`nim c`、`clang`、`link` のようなopaqueなcommandとして接続する。しかし各command内部には、さらに依存関係と並列性がある。

Rustには概念的に、Parsing / expansion、HIR、type analysis、MIR、MIR analysis / optimization、monomorphization、codegen units、codegen backend、object files、archive / linkというpipelineがある。Nimにも、semantic processing、backend generation、C / C++ / Objective-C / JavaScript、native compilation、object files、archive / linkという変換系がある。

言語ごとのbuild commandを実行単位とすると、この内部並列性とcross-language dependencyをLAMINARIA側から利用できない。LAMINARIAではcompiler自体が持つ内部境界まで含めて統合対象とする。

## 3. 中心問題

LAMINARIAの中心的な問いは、RustとNimがそれぞれ独立して持つdependency semantics、compiler pipeline、backend、artifact generationを、意味情報を失わず一つの計算グラフへ再構成できるか、である。

さらに、そのグラフを十分に細粒度化することで、言語境界やcompiler境界を越えたincremental computation、caching、schedulingを成立させられるかを検証する。`cargo build` と `nim c` は最終的な計算単位ではなく、より細かいgraphを発見するための入口となる。

## 4. 研究ゴール

### 4.1 Unified Program Graph と独自IR

LAMINARIA自身がRust/Nimソースを処理し、型、値、制御・データ依存、所有権、効果、overflow、runtime要件と由来を保持するIR群を研究・実装する。各既存compilerの出力を共通planning metadataへ写像するだけではない。packageやartifactの関係と、プログラムの意味を混同しない。

### 4.2 Compiler Pipeline Decomposition

既存rustc/Nimのstage mapは比較・情報損失の観測に使う。本経路は言語の意味から必要な解析・変換・無効化境界を独立に導出し、LAMINARIA自身で実装する。MIR、CGU、Nim-generated Cの境界を既定にしない。

```text
Rust source / Nim source / both
  → LAMINARIA source processing + semantic facts
  → LAMINARIA-owned IR(s) + provenance
  → legal analysis / transformation / specialization
  → demand-driven partition and resource plan
  → LAMINARIA target lowering / code generation
  → target artifacts + explicit runtime/link contract
```

### 4.3 Backend-Agnostic Compilation Model

LAMINARIA自身のtarget loweringとコード生成を研究する。LLVM/Cranelift/GCCやNimのC/JS経路は比較用であり、それらの選択を本コンパイラの代替にしない。target・ABI・runtime・最適化契約を表現し、実装と証拠で採否を決める。

### 4.4 Unified Action Graph

独自の意味解析、解析依存の更新、合法性判定、変換、specialization、target生成、artifactの保持・転送・再計算を表現する。Actionは外部processである必要はなく、言語名ごとにqueueを分けない。順序付きcommand列だけをcompiler IRと呼ばない。

### 4.5 Compiler-work Scheduling

独自IR上の計算をどこまで同一process・memory内で統合し、どこから並列・別nodeへ分割するかを研究する。意味依存、解析状態、critical path、core/cache/NUMA、memory帯域・容量、I/O/network費用を同時に扱う。既存Rust CGUとNim C unitのscheduleは比較baselineである。

### 4.6 Combinatorial Graph Resolution

`Package × Target × Profile × Feature × Generic Instance × Host/Target × Backend × Native Compiler × Artifact Type × FFI Configuration` という状態空間を扱う。全状態をデカルト積として生成せず、lazy expansion、constraint propagation、canonicalization、memoization、equivalent-state merging、dominance pruning、SCC condensation、demand-driven artifact resolution、incremental recomputationによって必要なgraphだけを生成する。このplanning kernelをNimで実装する。

### 4.7 Artifact-Oriented Dependency Model

dependencyをpackage同士のedgeだけでなく、次の形で扱う。

```text
Producer Action → Artifact → Consumer Action
```

本経路のartifactはLAMINARIAの意味・解析・変換・target表現とそのidentityを含む。比較経路ではRust metadata/MIR、LLVM bitcode、Nim-generated C等も記録するが、独自IRと互換とは仮定しない。

### 4.8 Semantic Build / Check Separation

compiler pipelineを分解し、semantic correctnessとmachine artifact generationを分離する。`check`系operationではdependency resolution、semantic analysis、type checking、FFI compatibility analysisまでを計算し、machine code generationを要求しないexecution graphを構成する。build / check / testは別々のcommand implementationではなく、異なるartifact demandとして表現する。

### 4.9 FFI as a Graph Primitive

Rust/Nim間のFFIを外部build scriptの副作用ではなく、Artifact Graph上の第一級の関係として扱う。

```text
Rust semantic representation → C ABI surface → header / binding representation → Nim consumer
Nim exported representation → C ABI surface → header / binding representation → Rust consumer
```

ABI invalidation、binding regeneration、rebuild propagation、compatibility check、cache invalidationを通常のgraph operationへ統合する。

### 4.10 Cross-Language Critical Path Scheduling

schedulerの目的を最大CPU利用率ではなく、**requested artifactが完成するまでのwall-clock time最小化**とする。graph dependency、estimated action duration、CPU / memory requirement、IO characteristics、backend cost、cache hit probability、critical path、artifact availabilityをplanningに利用する。Nim planning kernelがglobal graphを解析し、Rust runtime schedulerがmachine上の実際のresource状態を用いてexecutionを行う。

### 4.11 Incremental Compiler Graph

file変更時にpackage全体をinvalidateするのではなく、次の伝播を追跡できるモデルを研究する。

```text
Changed source → Affected semantic node → Affected specialization
→ Affected codegen unit → Affected backend action → Affected object → Affected final artifact
```

incrementalityをworkspace incrementality、compiler incrementality、artifact incrementalityの三層として扱う。

### 4.12 Unified Cache Identity

Action単位だけでなくcompiler pipelineの各stageにcontent identityを与える。

```text
Identity = operation + semantic inputs + relevant configuration
+ toolchain identity + dependency artifacts
```

物理的なworkspace pathやworktree pathとは独立したidentityにより、repository、branch、worktree、CI checkout、machineを越えたartifact reuseを研究する。

### 4.13 Agent-Oriented Compiler Toolchain

AI coding agentがcompiler/build system内部の状態を直接問い合わせられることを研究対象とする。

```text
laminaria dependency-graph laminaria program-graph
laminaria action-graph laminaria compiler-pipeline
laminaria codegen-units laminaria critical-path
laminaria explain-dependency laminaria explain-rebuild
laminaria explain-codegen laminaria explain-backend-selection
laminaria explain-cache-miss
```

agentがcompiler outputを推測するのではなく、compiler/build graphそのものを観測可能にする。

## 5. RustとNimの役割

### Nim Planning Kernel

graph construction、normalization、variant resolution、constraint solving、artifact demand propagation、SCC decomposition、lazy expansion、state merging、pruning、critical-path computation、planning optimizationを担当する。Nim関連処理に閉じず、Rust compiler graphを含めたLAMINARIA全体の計算問題を扱う。

### Rust Runtime

CLI、OS interaction、resource accounting、storage、IPC、診断と独自コンパイラ計算の実行をRust runtimeが担う。比較・bootstrapの外部process実行は別の役割であり、本経路のcompile engineではない。

## 6. LAMINARIA Compiler Topology

```text
Rust source / Nim source / both
  → LAMINARIA source processing + semantic facts
  → LAMINARIA-owned IR(s) + provenance
  → legal analysis / transformation / specialization
  → demand-driven partition and resource plan
  → LAMINARIA target lowering / code generation
  → target artifacts + explicit runtime/link contract
```

この経路のコンパイラ計算をNim plannerとRust runtime schedulerが扱う。独自source/IR処理なしのbuild graphはbootstrap・比較用であり、Rust-only/Nim-onlyでも本経路の所有権を変えない。

## 7. 比較対象

| 対象 | 比較する観点 |
| --- | --- |
| Bun | unified developer interface、package/build/test/runのtoolchain ownership |
| Cargo / rustc | dependency semantics、feature/target resolution、unit graph、query model、MIR、monomorphization、codegen units、backend abstraction、metadata / rlib |
| `rustc_codegen_ssa` / LLVM / Cranelift / GCC | backend abstraction、MIR lowering、codegen interface、backend-specific optimization、machine artifact generation |
| Nim compiler | semantic pipeline、C/C++/Objective-C/JavaScript backend、generated source、native compiler integration、nimcache、compile/link boundary |
| Buck2 | Action Graph、critical path、action digest、CAS、local/remote execution、incremental daemon architecture |
| Bazel | explicit action semantics、hermetic execution、remote execution、content-addressed artifacts |
| Pants | dependency inference、fine-grained invalidation、source-level graph |
| Nx | project graph、task graph、affected analysis |
| sccache | compiler invocation cache、Rust/C/C++ reuse |

LAMINARIAの特徴は、build systemの上位からcompilerを操作するだけでなく、compiler内部のsemantic/codegen boundaryをbuild graph側へ引き上げることにある。

## 8. 研究仮説

- **仮説A:** package/task graphでは保持できない意味依存を独自IRで表し、解析・変換・再利用と並列性を導出できる。
- **仮説B:** LLVMをRust専用backendとして固定せず、backend selectionをgraph上のvariantとして扱うことでcompiler/toolchain architectureを一般化できる。
- **仮説C:** 独自コンパイラ計算の垂直統合と水平分割を資源に応じて選び、既存compilerのnested scheduleより短いcritical pathまたは少ない資源使用を得られるかを検証する。
- **仮説D:** semantic artifactとmachine artifactを分離することで、check、build、test等を異なるartifact demandとして統一できる。
- **仮説E:** FFIをgraph primitiveとして扱うことで、言語境界のincremental invalidationを通常のdependency propagationへ統合できる。
- **仮説F:** compiler stage単位のcontent identityにより、crate/package単位より細かいartifact reuseが成立する。
- **仮説G:** 組合せ状態を事前生成せずdemand-drivenに探索することで、variant explosionを制御できる。
- **仮説H:** compiler graphをstructured interfaceとして公開することで、AI agentがbuild failure、cache miss、backend selection、critical pathを直接分析できる。

## 9. 評価ワークロード

- **Rust-heavy Compiler Graph:** 多数crate、generic specialization、複数codegen unitを持つ構成。
- **Nim-heavy Backend Graph:** 多数Nim moduleと大量のgenerated C/C++を持つ構成。
- **Mixed Codegen Graph:** Rust codegen unitとNim-generated native compilationが同時に存在する構成。
- **Backend Variant Workload:** LLVM / Cranelift等のbackend差異を含む構成。
- **FFI-heavy Graph:** Rust/Nim間に複数のABI boundaryを持つ構成。
- **Deep Critical Path / Wide Compiler Graph:** 長いartifact chainまたは多数の独立codegen actionを持つ構成。
- **Variant-heavy Graph:** feature、target、backend、artifact type等の組合せが多い構成。
- **Incremental Semantic Change:** 最終machine artifactへ影響しないsemantic変更を含む構成。
- **Git Worktree Workload:** 同一source historyを共有する複数worktreeでのbuild。

## 10. 評価指標

- graph construction cost、graph node / edge growth、variant exploration count
- incremental invalidation range、semantic / codegen / object cache reuse
- critical-path duration、CPU utilization、peak memory、backend switching cost
- FFI invalidation precision、worktree cache reuse、explanation completeness

単純なfull-build benchmarkだけではなく、**どの計算を省略できたか**を主要指標とする。

## 11. 研究上の位置付け

LAMINARIAは、Bunのunified toolchain、Cargo / rustcのlanguage-aware compiler semantics、Nimのexplicit multi-backend compilation pipeline、Buck2のAction Graph / execution、Bazelのartifact/action identity、Pantsのdependency inference、Nxのaffected graph analysis、sccacheのcompiler cacheを、Rust + Nim compiler pipelineという具体的な対象に接続する。

## 12. 最終研究目標

LAMINARIAが目指すのは、Rust build、Nim build、C compilation、LLVM codegen、linking、FFI generationという独立した工程の集合ではない。これらを **Input → Transformation → Artifact → Dependency** の組み合わせとして一つのgraphへ落とす。

最終的にLAMINARIAが、何を計算すべきか、何が既に存在するか、どの意味情報が変化したか、どのspecializationが影響を受けるか、どのbackend workが必要か、何を並行実行できるか、要求artifactを何が阻んでいるか、なぜActionが実行されたかを、一つの計算モデルから回答できる状態を研究する。

## Project Statement

**LAMINARIA — Rust Nim Unified Toolchain**

LAMINARIA researches and implements its own compiler, semantic IRs, transformations, target generation and resource-aware scheduler for Rust and Nim, ultimately compiling its own Rust + Nim implementation. Existing toolchains are separate reference/bootstrap tools, not the target compilation engines.

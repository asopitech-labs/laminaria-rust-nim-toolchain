# LAMINARIA

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
Cargo dependency resolution       Nimble dependency resolution
Rust compiler pipeline            Nim compiler pipeline
Rust codegen backend              Nim backend generation
C / C++ compiler and linker       FFI and binding generation
compiler cache / build cache      CI scheduler
```

一般的なbuild orchestrationは、これらを `cargo build`、`nimble build`、`nim c`、`clang`、`link` のようなopaqueなcommandとして接続する。しかし各command内部には、さらに依存関係と並列性がある。

Rustには概念的に、Parsing / expansion、HIR、type analysis、MIR、MIR analysis / optimization、monomorphization、codegen units、codegen backend、object files、archive / linkというpipelineがある。Nimにも、semantic processing、backend generation、C / C++ / Objective-C / JavaScript、native compilation、object files、archive / linkという変換系がある。

言語ごとのbuild commandを実行単位とすると、この内部並列性とcross-language dependencyをLAMINARIA側から利用できない。LAMINARIAではcompiler自体が持つ内部境界まで含めて統合対象とする。

## 3. 中心問題

LAMINARIAの中心的な問いは、RustとNimがそれぞれ独立して持つdependency semantics、compiler pipeline、backend、artifact generationを、意味情報を失わず一つの計算グラフへ再構成できるか、である。

さらに、そのグラフを十分に細粒度化することで、言語境界やcompiler境界を越えたincremental computation、caching、schedulingを成立させられるかを検証する。`cargo build` と `nim c` は最終的な計算単位ではなく、より細かいgraphを発見するための入口となる。

## 4. 研究ゴール

### 4.1 Unified Program Graph

package dependencyだけでなく、compilerが認識するprogram structureを共通グラフへ統合する。対象にはpackage、crate、Nim module、target、feature、generic instance、generated source、codegen unit、FFI artifact、metadata、object、archiveを含む。RustとNimの内部表現を直接同一化するのではなく、各compilerからLAMINARIAの共通semantic modelへ写像する。

### 4.2 Compiler Pipeline Decomposition

compiler invocationを一つのActionとして扱うのではなく、その内部pipelineを複数の計算段階としてモデル化する。

```text
Rust: Frontend → HIR / semantic representation → MIR → MIR transformation
      → monomorphization collection → codegen units → backend lowering
      → backend optimization → object generation → link

Nim:  Frontend → semantic representation → backend transformation
      → generated C / C++ / Objective-C / JavaScript → native compiler
      → object generation → link
```

両者を共通Action Graph上へ展開する。

### 4.3 Backend-Agnostic Compilation Model

LLVMをRust専用の固定backendとは扱わず、backendを独立したgraph primitiveとしてモデル化する。

```text
Language IR → Backend Action → Backend IR / Machine Artifact
```

RustではMIR / monomorphized programからLLVM、Cranelift、GCC backendを、Nimではsemantic programからC、C++、Objective-C、JavaScriptのbackend familyを扱う。backend selectionをvariant resolutionの対象とし、`language × target × optimization × backend × native compiler × linker` を共通のconstraint problemとして扱う。

### 4.4 Unified Action Graph

共通Actionには次を表現できる。

```text
RustFrontend / RustAnalysis / RustMIR / RustMonomorphization / RustCodegenUnit
NimFrontend / NimAnalysis / NimBackendGeneration
BackendLowering / BackendOptimization
CCompile / CppCompile / ObjectGeneration
BindingGeneration / Archive / Link / Test / CodeGeneration
```

言語名はscheduler queueを分割する属性ではなく、そのActionの意味を説明するmetadataとなる。

### 4.5 Codegen Unit Scheduling

Rust compiler内部のcodegen unitと、Nimが生成したnative source compilationを同じscheduling problemとして扱えるかを研究する。

```text
Rust CGU A ─── LLVM ─────→ A.o
Rust CGU B ─── Cranelift → B.o
Nim C unit C ─ clang ────→ C.o
Nim C unit D ─ clang ────→ D.o
```

目的は別々の並列性を最大化することではなく、最終artifactまでのcritical pathを最小化する並列実行である。

### 4.6 Combinatorial Graph Resolution

`Package × Target × Profile × Feature × Generic Instance × Host/Target × Backend × Native Compiler × Artifact Type × FFI Configuration` という状態空間を扱う。全状態をデカルト積として生成せず、lazy expansion、constraint propagation、canonicalization、memoization、equivalent-state merging、dominance pruning、SCC condensation、demand-driven artifact resolution、incremental recomputationによって必要なgraphだけを生成する。このplanning kernelをNimで実装する。

### 4.7 Artifact-Oriented Dependency Model

dependencyをpackage同士のedgeだけでなく、次の形で扱う。

```text
Producer Action → Artifact → Consumer Action
```

ArtifactにはRust metadata、MIR-related compiler metadata、object file、LLVM bitcode、generated C / C++、C header、static archive、dynamic library、executableなどがある。dependencyを理解するために必要なartifactと、machine codeを生成するために必要なartifactを区別する。

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
laminaria dependency-graph       laminaria program-graph
laminaria action-graph           laminaria compiler-pipeline
laminaria codegen-units          laminaria critical-path
laminaria explain-dependency     laminaria explain-rebuild
laminaria explain-codegen        laminaria explain-backend-selection
laminaria explain-cache-miss
```

agentがcompiler outputを推測するのではなく、compiler/build graphそのものを観測可能にする。

## 5. RustとNimの役割

### Nim Planning Kernel

graph construction、normalization、variant resolution、constraint solving、artifact demand propagation、SCC decomposition、lazy expansion、state merging、pruning、critical-path computation、planning optimizationを担当する。Nim関連処理に閉じず、Rust compiler graphを含めたLAMINARIA全体の計算問題を扱う。

### Rust Runtime

CLI、application logic、compiler/tool discovery、filesystem、process lifecycle、async execution、resource accounting、cache / CAS、daemon、IPC、sandbox execution、native process scheduling、diagnostics transportを担当する。Rust関連処理に閉じず、Nim compilerやLLVM等を含めたexecution infrastructureを管理する。

## 6. LAMINARIA Compiler Topology

```text
Source Graph
 ├─ Rust Frontend → Rust Semantic IR → MIR / Specialization ┐
 └─ Nim Frontend  → Nim Semantic IR  → Transformation       ├→ Backend Graph
                                                            │   ├─ LLVM
                                                            │   ├─ Cranelift
                                                            │   ├─ GCC
                                                            │   ├─ C / C++
                                                            │   └─ JS
                                                            ↓
                                                       Artifact Graph
                                                            ↓
                                                Objects / Metadata / Archive / Link
                                                            ↓
                                                      Final Artifact
```

このgraph全体を一つのplannerとschedulerから扱う。

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

- **仮説A:** package/task graphだけではcross-language optimizationには粗すぎ、compiler pipeline内部をAction Graphへ露出することで新しい並列性とcache reuseが得られる。
- **仮説B:** LLVMをRust専用backendとして固定せず、backend selectionをgraph上のvariantとして扱うことでcompiler/toolchain architectureを一般化できる。
- **仮説C:** Rust codegen unitとNim generated-native-source compileを同じschedulerへ載せることで、nested parallelismより短いglobal critical pathを実現できる。
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

LAMINARIA researches and implements a unified computational model for Rust and Nim toolchains, decomposing language frontends, semantic stages, code generation, compiler backends, artifacts, caching, and execution into a single explainable action graph for cross-language planning and resource-aware scheduling.

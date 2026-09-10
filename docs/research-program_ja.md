# LAMINARIA 研究プログラム

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

独自コンパイラ・IR・スケジューラが本経路であり、任意の後続統合ではない。Cargo/Nim ecosystemは依存解決に利用できるが、以下に登場する既存コンパイル経路は比較・観測または外部bootstrapのbaselineであり、本ビルドの選択肢ではない。Action Graphは言語IRの代わりにならない。

## 目的

LAMINARIAは、RustとNimのコンパイル、依存解決、コード生成、成果物、リンク、キャッシュ、実行を一つの計算システムとして扱う研究開発プロジェクトである。

この文書は、その方向性を再現可能で検証可能な研究課題へ落とし込むための実行方針を定義する。`research-foundations.md` がアーキテクチャ仮説を扱うのに対し、本書は研究トラック、必要な証拠、完了条件を定義する。backend内部のwhite-boxingについては `backend-pipeline-whiteboxing_ja.md` を詳細設計とする。

## 研究ポリシー

LAMINARIAでは、単にビルドが通る、テストが通る、期待値が返るだけでは研究課題の完了とみなさない。

機能的正しさと実行経路の正しさは別の要件である。正しい最終成果物が得られても、不要な処理をすべて再実行していた、誤ったbackend/scheduler経路を通った、opaqueな外部buildやbackendへ黙って委譲した、といった場合はincremental、scheduling、native linking、backend white-boxing、work eliminationの成立を証明しない。

アーキテクチャ、スケジューリング、性能、キャッシュ、コンパイラ境界、backend pipeline、リンク方式を扱う課題では、実際にどの実行経路が選択されたか、どの成果物が生成されたか、CPU・メモリ・I/Oを含む資源挙動がどうだったか、どのActionが実行され、どのActionが正しく省略されたかを証拠として残す。

参考プロジェクトは単なる目標文言ではなく、実測比較と設計検証の基準として扱う。参考実装から性能・資源消費・実行構造・実行仕事量が大きく乖離した場合は、ベンチマーク条件を弱めるのではなく実装そのものを再検討する。

LAMINARIAでは最適化を原則として次の順で優先する。

1. 不要なActionまたはcompiler/backend stageを除去する
2. 既に有効なartifactを再利用する
3. invalidation範囲を狭める
4. 有効なparallelismを露出する
5. 全体resource制約の下でglobal schedulingする
6. 個々のAction自体を高速化する

本来実行不要な処理を並列化して高速化しても、work eliminationの代替とはみなさない。

## 研究中核 — 独自IR・コンパイラ・scheduler（#25/#3/#6/#8）

対応範囲を宣言したRust/Nimソースを、独自の意味解析・IR・合法性を持つ変換・target生成まで通す。production Nim planner/Rust runtimeからそのコンパイラ計算を実行し、既存compilerが本経路で動かないことを否定テストで検証する。単一言語と混成は同じ基盤を使う。手書きIR、外部compiler trace、リンク成功は、それぞれ限定した実験証拠である。

#3の以下のstage inventoryは比較・情報損失調査として残すが、研究中核を後回しにする依存順序ではない。#4の統合や#18–#24の計測・UXは必要部分を並行して支える。

## 研究トラックA — Compiler Pipeline Decomposition

RustとNimのcompiler pipelineのどこまでを、安定した入力・出力・invalidation関係を持つgraph nodeとして外部から扱えるかを調査する。

Rustではfrontend、HIR、type analysis、MIR、monomorphization、codegen unit、backend handoff、object、archive、linkを対象とする。

Nimではfrontend、semantic processing、backend generation、generated C/C++等、native compile、backend handoff、object、archive、linkを対象とする。

各境界を次のいずれかに分類する。

- public/stable
- observable but internal
- experimental integration possible
- opaque

既存compilerの粗い経路は比較用としてのみ保持する。本経路で未対応の意味処理は診断して停止し、既存compilerで代替しない。

## 研究トラックB — C ABIを必須境界としないRust/Nim Native Linking

RustとNimの生成物を、一度C ABI surfaceへ落とすことを必須とせず、同一native artifact/link planへ参加させられるかを研究する。

これは「任意のRust型とNim型を直接交換できる」という仮定ではない。object/link互換性とlanguage semantics互換性を分離し、adapterが本当に必要な境界だけを明示する。

詳細は `rust-nim-native-linking.md` に定義する。

## 研究トラックC — Backend Route / Backend Pipeline Graph

本経路は独自IRの最適化・target生成を実行する。以下のLLVM/Nim既存backend variantとpipeline投影は比較モデルであり、本コンパイラの選択肢として代替しない。

backendを一つの抽象概念で済ませず、次の二段階を分離する。

```text
Backend Route Selection
  ↓
Backend Pipeline Expansion
```

### C1 — Backend Route Selection

LLVM、Cranelift、GCC系Rust backend、およびNimのC/C++/Objective-C/JavaScript等を固定前提ではなくgraph variantとして扱う。

backendだけでなくtarget、optimization、debug info、ABI、native compiler、LTO mode、linker、artifact kindの互換性をconstraintとして扱う。

### C2 — Backend Pipeline White-boxing

選択されたbackendを再びopaqueな単一Actionへ畳み込まない。lowering、optimization、LTO、target codegen、link、post-link等をbackend固有のnested graphとしてLAMINARIAへ投影する。

ただしwhite-boxingを「全passを独立processにすること」と同一視しない。境界を以下に分類する。

1. Logical Stage
2. Observation Boundary
3. Checkpoint / Artifact Boundary
4. Execution Boundary
5. Dynamic Graph Expansion Point

LLVM New Pass Managerのように、pass groupingがcache localityやoptimization qualityへ影響する実装では、内部を観測可能にしながら実行分割は限定する。

checkpointの採否は次の利益とコストを実測して決定する。

```text
benefit = work elimination + reuse + invalidation reduction + scheduling/distribution gain
cost = serialization + reload + hashing/I/O + process/IPC + lost analysis/locality + optimization risk
```

詳細は `backend-pipeline-whiteboxing_ja.md`、実行Issueは #13、#14、#15を参照する。

## 研究トラックD — Unified Action Graph / Scheduler

LAMINARIA自身のsource/IR解析・変換・target生成・artifact計算を一つの資源認識schedulerで統合・分割する。既存compiler/backend jobは別に測定する比較対象であり、本経路の計算単位を決めるものではない。

Cargo、Nim、LLVM/LTO等がそれぞれ独立にCPUを使い切るnested parallelismではなく、LAMINARIAが全体のCPU、memory、I/O budgetとcritical pathを見て実行順序を決める。

critical pathの分析では、Actionごとに少なくともqueue wait、dependency/resource wait、execution timeを分離し、「処理自体が遅い」のか「開始が遅れた」のかを区別する。

水平分散はscheduler、cache、backend、LLVM再発見trackを横断するfirst-class research subjectである。Kbuild型のobject分散、LLVM ThinLTO/DTLTO backend分散、Action-level remote executionを比較するが、どのpartition単位もLAMINARIAのcanonical semantic partitionとは仮定しない。詳細は `horizontal-distribution-research.md` / `_ja.md` を参照する。

異種nodeの参加は、この研究の別軸である。execution hostとcompilation targetを分離する。Windows、macOS、Raspberry Pi nodeは、自身向けcompile、別targetへのcross-compile、test実行、measurement evidenceの提供のいずれも担いうる。actionへの参加可否は、OS、ISA、ABI、target triple、sysroot/SDK、linker、compiler/toolchain、target feature、runtime、trust/qualification constraintを満たす場合だけvalidとする。Rustのtarget support tierとCargoの明示的target選択はbaselineであり、任意の混在node actionのvalidityを保証するものではない。

schedulerはhost/target pairとtarget-specific artifact boundaryを明示的に扱う必要がある。toolchain、sysroot、input、target contractが独立していれば、cross-compilation actionは異種node上で同時実行できる。一方、native execution、target-specific linking、performance measurement、runtime testは対応するnodeを要求することがある。CPU architecture、endianness、pointer width、libc/ABI、object format、SDK availability、enabled CPU featureをidentity、placement、invalidation、explanationへ含める。

DTLTO等、上流Actionの実行後にchild backend jobsが判明する場合は、hidden nested schedulerではなくDynamic Graph Expansionとして扱えるかを研究する。

## 研究トラックE — Artifact Identity / Incremental / CAS

semantic artifact、generated source、backend IR/bitcode、LTO index、backend output、object、Core Wasm、optimized Wasm、component等に対して、物理checkout pathへ不必要に依存しないidentityを定義する。

worktree、CI checkout、互換machine間で同一計算を再利用できるかを検証する。

cache hitだけでなく、なぜ再利用できたか／できなかったかを説明可能にする。また、artifact reuseとwork elimination/no-opは別の効果として測定する。

永続化とmaterializationは固定されたlocal disk実装ではなく、schedulingの判断である。logical artifact identityとphysical replicaを分離し、memory上のephemeral state、local NVMe、peer cache、remote CAS/object storage、durable archiveの間で、どこにいつ保存するかを決める。intermediateは、reuse、recovery、locality、parallelismの期待利益が、recomputation、serialization、hashing、transfer、storage、consistencyのコストを上回る場合だけmaterializeまたはreplicateする。placement、replica lineage、retention/GC、commit状態、failure recovery、residency constraintをevidenceに記録する。

backend checkpoint identityでは、入力artifactだけでなくtoolchain version、target/data layout/features、optimization/pass pipeline、LTO、profile input、debug、plugin等のsemantically relevantな設定を含める。

## 研究トラックF — Variant Explosion Control

以下の直積をeagerに生成しない。

`target × profile × features × host/target role × backend route × backend pipeline mode × compiler × linker × post-link optimizer × composition model × artifact kind × cross-language boundary`

constraint propagation、canonicalization、memoization、equivalent-state merging、SCC、demand propagation、pruningで必要な状態だけを展開する。

## 研究トラックG — WebAssembly Target Pipeline

WebAssemblyをLLVM/Cranelift等と同列のbackend familyとして扱わない。

少なくとも次のdimensionを分離する。

```text
Backend Engine
× Target ISA / Object Model
× Link Model
× Post-link Optimizer
× Composition Model
```

代表的なLLVM routeでは、以下を独立stage/artifact候補として扱う。

```text
LLVM IR
→ LLVM optimization
→ WebAssembly target codegen
→ relocatable Wasm object
→ wasm-ld
→ Core Wasm module
→ Binaryen / wasm-opt
→ optimized Core Wasm module
→ WIT metadata / adapters
→ componentization
→ WebAssembly Component
```

single module、multi-module/component、adapter generation、runtime duplication、code size、boundary cost、cache/invalidationの違いを実測する。

`wasm-ld`、Binaryen、WIT embedding、adapter、componentizationを一つの`WASM backend`へ隠さない。Binaryenについてもpass-level observationとprocess/checkpoint分割を分離する。

「WASM向けcompile flagが存在する」だけでは対応とはみなさず、実際にartifactを生成・実行して検証する。詳細は #9、#16を参照する。

## 研究トラックH — Explainability

LAMINARIAは少なくとも以下を構造化して説明できる必要がある。

- なぜそのdependency/variantを選んだか
- なぜrebuildしたか
- なぜcacheをhit/missしたか
- なぜAction/backend stageをskip/eliminateできたか
- なぜbackend/linker/post-link combinationを採用・拒否したか
- どのbackend routeがどのnested pipelineへ展開されたか
- どの境界がlogical/observable/checkpoint/executionなのか
- dynamic graph expansionで何のchild Actionが生成されたか
- critical pathは何か
- critical path上の遅延がqueue/dependency/resource/executionのどれによるか
- どのcompiler/backend stageがopaqueでcoarse-grained executionになったか

## 研究トラックI — Work Elimination / No-op Invariant

LAMINARIAは、既存の計算をcacheしたり並列化したりする前に、requested artifactの生成に本当に必要な計算だけを実行できるかを研究する。

主な対象は次の通り。

- compiler/backend-stage単位のdemand-driven execution
- cache reuseとは別のwork eliminationモデル
- 中間stageを通さずartifactを直接consumerへ渡せる経路
- unchanged buildでのtrue no-op invariant
- no-op判定そのものに必要なmetadata check、hash、read、process launchのコスト
- controlled editに対するexpected executed/non-executed Action setの検証
- LLVM/ThinLTO/backend checkpointの再利用・省略
- wasm-ld/Binaryen/componentizationの部分的省略
- elimination、reuse、parallelization、individual action optimizationの効果分離

### No-op invariant

source content、関連config、toolchain identity、互換environment inputが不変なら、明示的にenvironment-sensitiveと定義されたActionを除き、compiler/codegen/backend/link/post-link execution Actionは0であることを目標とする。

`cache hit = 100%`だけでは十分ではない。no-op判定のために大量のhash計算、I/O、graph traversal、process startupを行っている場合は、そのコストを別途測定する。

## 研究トラックJ — Cross-language LLVM / LTO Convergence

Rust、Nim 2、Nim 3/NimonyのLLVM系経路を「Nim/Rust→LLVM」と一括りにせず、frontend/backend由来ごとに互換性を検証する。

候補:

```text
Rust → rustc LLVM bitcode
Nim 2 → nlvm → LLVM IR
Nim 2 → generated C → Clang → LLVM IR/bitcode
Nim 3/Nimony → Leng/lengc → LLVM IR
```

同一LTO/ThinLTO planへ参加できるかを、target triple、data layout、symbol visibility、calling convention、runtime initialization、allocator、panic/exception、TLS、ownership等と分離して検証する。

LLVM artifactがlink可能であることと、language-level ABI互換やcross-language inliningが成立することを同一視しない。詳細は #17を参照する。

## 評価ワークロード

最低限、次を用意する。

1. Rust-heavy workspace
2. Nim-heavy generated native source workspace
3. mixed Rust/Nim native executable
4. direct native-link workload
5. conventional C ABI baseline
6. backend-route variant workload
7. backend checkpoint economics workload
8. LLVM pass/pipeline observation workload
9. ThinLTO/DTLTO dynamic backend-job workload
10. wide parallel graph
11. deep critical-path graph
12. boundary-heavy graph
13. incremental semantic edit
14. unchanged/no-op workload
15. worktree reuse
16. compiler/backend-work-elimination fixture
17. mixed-language WebAssembly
18. Wasm link/post-link/component invalidation workload
19. Rust/Nim 2/Nimony shared LLVM/LTO compatibility workload

## 必須メトリクス

- graph construction time
- node/edge count
- logical/observation/checkpoint/execution boundary count
- dynamic graph expansion time and child-action count
- explored/pruned/merged variants
- critical-path duration
- action wall time
- per-action queue wait / dependency-resource wait / execution time
- CPU time/utilization
- peak/time-weighted memory
- I/O volume/wait
- serialized/deserialized/hashed/read/written bytes
- generated source/IR/bitcode/object/archive/module/component size
- semantic/codegen/backend/object/final artifact reuse
- compiler/backend stage別のexecuted/skipped action count
- LLVM pass group timing and optimization remarks where available
- ThinLTO backend job count / index / invalidation set
- linker inputs/symbols
- Binaryen pass timing/module metrics where available
- WIT/adaptation/componentization artifact and timing
- no-op時のmetadata/hash/read/process-launch overhead
- cache hit/miss reason
- invalidation set size
- fallback/delegation path usage
- reference baseline ratio
- checkpoint benefit versus checkpoint cost

## 完了条件

研究Issueは、committed code、commands、fixtures、measurement evidenceから第三者が主張を再現できる場合にのみ完了とする。

controlled incremental testでは最終成果物だけでなくexpected execution setも検証する。すべてをrebuildして正しいbinaryを得ただけではincremental executionの正しさを証明しない。

backend white-boxingでは、内部stageを表示できただけでは完了としない。少なくとも一つのcheckpointでwork elimination/reuseの利益を実測し、同時に一つ以上の過剰な細粒度化が不利益になるケースも測定し、実行境界の選択へ反映する。

仮説が外れた場合は、その失敗理由を記録して設計を更新する。元の主張を守るためにbenchmark条件を弱めたり、fallback経路を隠したりしない。

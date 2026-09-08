# LAMINARIA 研究プログラム

## 目的

LAMINARIAは、RustとNimのコンパイル、依存解決、コード生成、成果物、リンク、キャッシュ、実行を一つの計算システムとして扱う研究開発プロジェクトである。

この文書は、その方向性を再現可能で検証可能な研究課題へ落とし込むための実行方針を定義する。`research-foundations.md` がアーキテクチャ仮説を扱うのに対し、本書は研究トラック、必要な証拠、完了条件を定義する。

## 研究ポリシー

LAMINARIAでは、単にビルドが通る、テストが通る、期待値が返るだけでは研究課題の完了とみなさない。

機能的正しさと実行経路の正しさは別の要件である。正しい最終成果物が得られても、不要な処理をすべて再実行していた、誤ったbackend/scheduler経路を通った、opaqueな外部buildへ黙って委譲した、といった場合はincremental、scheduling、native linking、work eliminationの成立を証明しない。

アーキテクチャ、スケジューリング、性能、キャッシュ、コンパイラ境界、リンク方式を扱う課題では、実際にどの実行経路が選択されたか、どの成果物が生成されたか、CPU・メモリ・I/Oを含む資源挙動がどうだったか、どのActionが実行され、どのActionが正しく省略されたかを証拠として残す。

参考プロジェクトは単なる目標文言ではなく、実測比較と設計検証の基準として扱う。参考実装から性能・資源消費・実行構造・実行仕事量が大きく乖離した場合は、ベンチマーク条件を弱めるのではなく実装そのものを再検討する。

LAMINARIAでは最適化を原則として次の順で優先する。

1. 不要なActionまたはcompiler stageを除去する
2. 既に有効なartifactを再利用する
3. invalidation範囲を狭める
4. 有効なparallelismを露出する
5. 全体resource制約の下でglobal schedulingする
6. 個々のAction自体を高速化する

本来実行不要な処理を並列化して高速化しても、work eliminationの代替とはみなさない。

## 研究トラックA — Compiler Pipeline Decomposition

RustとNimのcompiler pipelineのどこまでを、安定した入力・出力・invalidation関係を持つgraph nodeとして外部から扱えるかを調査する。

Rustではfrontend、HIR、type analysis、MIR、monomorphization、codegen unit、backend、object、archive、linkを対象とする。

Nimではfrontend、semantic processing、backend generation、generated C/C++等、native compile、object、archive、linkを対象とする。

各境界を次のいずれかに分類する。

- public/stable
- observable but internal
- experimental integration possible
- opaque

fine-grained integrationができない場合でも、正しく動くcoarse-grained実行経路は常に残す。

## 研究トラックB — C ABIを必須境界としないRust/Nim Native Linking

RustとNimの生成物を、一度C ABI surfaceへ落とすことを必須とせず、同一native artifact/link planへ参加させられるかを研究する。

これは「任意のRust型とNim型を直接交換できる」という仮定ではない。object/link互換性とlanguage semantics互換性を分離し、adapterが本当に必要な境界だけを明示する。

詳細は `rust-nim-native-linking.md` に定義する。

## 研究トラックC — Backend Graph

LLVM、Cranelift、GCC系Rust backend、およびNimのC/C++/Objective-C/JavaScript backendを固定前提ではなくgraph variantとして扱えるかを検証する。

backendだけでなくtarget、optimization、debug info、ABI、native compiler、linker、artifact kindの互換性をconstraintとして扱う。

## 研究トラックD — Unified Action Graph / Scheduler

Rust codegen work、Nim generated C/C++ compilation、binding/shim generation、object generation、archive、linkを同一schedulerへ載せる。

CargoとNimがそれぞれ独立にCPUを使い切るnested parallelismではなく、LAMINARIAが全体のCPU、memory、I/O budgetとcritical pathを見て実行順序を決める。

critical pathの分析では、Actionごとに少なくともqueue wait、dependency/resource wait、execution timeを分離し、「処理自体が遅い」のか「開始が遅れた」のかを区別する。

## 研究トラックE — Artifact Identity / Incremental / CAS

semantic artifact、generated source、backend artifact、object、final artifactに対して、物理checkout pathへ不必要に依存しないidentityを定義する。

worktree、CI checkout、互換machine間で同一計算を再利用できるかを検証する。

cache hitだけでなく、なぜ再利用できたか／できなかったかを説明可能にする。また、artifact reuseとwork elimination/no-opは別の効果として測定する。

## 研究トラックF — Variant Explosion Control

以下の直積をeagerに生成しない。

`target × profile × features × host/target role × backend × compiler × linker × artifact kind × cross-language boundary`

constraint propagation、canonicalization、memoization、equivalent-state merging、SCC、demand propagation、pruningで必要な状態だけを展開する。

## 研究トラックG — WASM

RustとNimを単に別moduleとして動かすだけでなく、compiler/backend/link graphとして統合した場合に何が可能になるかを評価する。

single module、multi-module/component、adapter generation、runtime duplication、code size、boundary cost、cache/invalidationの違いを実測する。

「WASM向けcompile flagが存在する」だけでは対応とはみなさず、実際にartifactを生成・実行して検証する。

## 研究トラックH — Explainability

LAMINARIAは少なくとも以下を構造化して説明できる必要がある。

- なぜそのdependency/variantを選んだか
- なぜrebuildしたか
- なぜcacheをhit/missしたか
- なぜActionをskip/eliminateできたか
- なぜbackend/linker combinationを採用・拒否したか
- critical pathは何か
- critical path上の遅延がqueue/dependency/resource/executionのどれによるか
- どのcompiler stageがopaqueでcoarse-grained executionになったか

## 研究トラックI — Work Elimination / No-op Invariant

LAMINARIAは、既存の計算をcacheしたり並列化したりする前に、requested artifactの生成に本当に必要な計算だけを実行できるかを研究する。

主な対象は次の通り。

- compiler-stage単位のdemand-driven execution
- cache reuseとは別のwork eliminationモデル
- 中間stageを通さずartifactを直接consumerへ渡せる経路
- unchanged buildでのtrue no-op invariant
- no-op判定そのものに必要なmetadata check、hash、read、process launchのコスト
- controlled editに対するexpected executed/non-executed Action setの検証
- elimination、reuse、parallelization、individual action optimizationの効果分離

### No-op invariant

source content、関連config、toolchain identity、互換environment inputが不変なら、明示的にenvironment-sensitiveと定義されたActionを除き、compiler/codegen/link execution Actionは0であることを目標とする。

`cache hit = 100%`だけでは十分ではない。no-op判定のために大量のhash計算、I/O、graph traversal、process startupを行っている場合は、そのコストを別途測定する。

## 評価ワークロード

最低限、次を用意する。

1. Rust-heavy workspace
2. Nim-heavy generated native source workspace
3. mixed Rust/Nim native executable
4. direct native-link workload
5. conventional C ABI baseline
6. backend-variant workload
7. wide parallel graph
8. deep critical-path graph
9. boundary-heavy graph
10. incremental semantic edit
11. unchanged/no-op workload
12. worktree reuse
13. compiler-work-elimination fixture
14. mixed-language WASM

## 必須メトリクス

- graph construction time
- node/edge count
- explored/pruned/merged variants
- critical-path duration
- action wall time
- per-action queue wait / dependency-resource wait / execution time
- CPU time/utilization
- peak/time-weighted memory
- I/O volume/wait
- generated source/object/archive/module size
- semantic/codegen/object/final artifact reuse
- compiler stage別のexecuted/skipped action count
- no-op時のmetadata/hash/read/process-launch overhead
- cache hit/miss reason
- invalidation set size
- linker inputs/symbols
- fallback/delegation path usage
- reference baseline ratio

## 完了条件

研究Issueは、committed code、commands、fixtures、measurement evidenceから第三者が主張を再現できる場合にのみ完了とする。

controlled incremental testでは最終成果物だけでなくexpected execution setも検証する。すべてをrebuildして正しいbinaryを得ただけではincremental executionの正しさを証明しない。

仮説が外れた場合は、その失敗理由を記録して設計を更新する。元の主張を守るためにbenchmark条件を弱めたり、fallback経路を隠したりしない。

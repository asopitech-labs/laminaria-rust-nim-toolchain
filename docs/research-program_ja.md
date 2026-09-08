# LAMINARIA 研究プログラム

## 目的

LAMINARIAは、RustとNimのコンパイル、依存解決、コード生成、成果物、リンク、キャッシュ、実行を一つの計算システムとして扱う研究開発プロジェクトである。

この文書は、その方向性を再現可能で検証可能な研究課題へ落とし込むための実行方針を定義する。`research-foundations.md` がアーキテクチャ仮説を扱うのに対し、本書は研究トラック、必要な証拠、完了条件を定義する。

## 研究ポリシー

LAMINARIAでは、単にビルドが通る、テストが通る、期待値が返るだけでは研究課題の完了とみなさない。

アーキテクチャ、スケジューリング、性能、キャッシュ、コンパイラ境界、リンク方式を扱う課題では、実際にどの実行経路が選択されたか、どの成果物が生成されたか、CPU・メモリ・I/Oを含む資源挙動がどうだったかを証拠として残す。

高速に見えても意図した経路を通っていないfallback実装、stub、test専用経路、外側から既存ツールへ処理を丸投げするだけの実装は、研究対象の成立を証明しない。

参考プロジェクトは単なる目標文言ではなく、実測比較と設計検証の基準として扱う。参考実装から性能・資源消費・実行構造が大きく乖離した場合は、ベンチマーク条件を弱めるのではなく実装そのものを再検討する。

## 研究トラックA — Compiler Pipeline Decomposition

RustとNimのcompiler pipelineのどこまでを、安定した入力・出力・invalidaton関係を持つgraph nodeとして外部から扱えるかを調査する。

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

## 研究トラックE — Artifact Identity / Incremental / CAS

semantic artifact、generated source、backend artifact、object、final artifactに対して、物理checkout pathへ不必要に依存しないidentityを定義する。

worktree、CI checkout、互換machine間で同一計算を再利用できるかを検証する。

cache hitだけでなく、なぜ再利用できたか／できなかったかを説明可能にする。

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
- なぜbackend/linker combinationを採用・拒否したか
- critical pathは何か
- どのcompiler stageがopaqueでcoarse-grained executionになったか

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
11. worktree reuse
12. mixed-language WASM

## 必須メトリクス

- graph construction time
- node/edge count
- explored/pruned/merged variants
- critical-path duration
- action wall time
- CPU time/utilization
- peak/time-weighted memory
- I/O volume/wait
- generated source/object/archive/module size
- semantic/codegen/object/final artifact reuse
- cache hit/miss reason
- invalidation set size
- linker inputs/symbols
- fallback/delegation path usage
- reference baseline ratio

## 完了条件

研究Issueは、committed code、commands、fixtures、measurement evidenceから第三者が主張を再現できる場合にのみ完了とする。

仮説が外れた場合は、その失敗理由を記録して設計を更新する。元の主張を守るためにbenchmark条件を弱めたり、fallback経路を隠したりしない。

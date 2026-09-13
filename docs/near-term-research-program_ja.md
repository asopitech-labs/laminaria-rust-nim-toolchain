# LAMINARIA 全体進行と当面の研究ゴール

本書をLAMINARIA文書群とタスク選択の正準な起点とする。[文書マップ](README.md)では、他の全文書を本書から下流の基盤、研究領域、作業項目、手順、履歴として整理する。

## 長期ゴール

LAMINARIAの長期ゴールは、Cargo、Nimble、C、C++の各ecosystemが持つpackage、source、生成物、toolchain、ABI、linkの関係を、一つの説明可能な依存関係グラフとして解決し、その解決結果からRust/Nimを含むprojectの**通常実行できるnative binary**を効率的、高速、省メモリに生成することにある。最終的にはLAMINARIA自身とその推移的依存を同じ経路でbuildする。

LAMINARIAは対応するRust/Nim source semantics、IR、変換、work分割／融合、scheduler、native target生成を所有する。CargoやNimble、C/C++側のpackage metadata、manifest、lockfile、source取得、system library情報は入力として利用できるが、package managerが暗黙に起動するbuild script、compiler、linkerを依存解決そのものと混同しない。

## 成果物とtargetの優先順位

現在のprimary targetは、一般的なOSで直接起動できるnative executableである。object、archive、shared library、runtime、system libraryを明示的なgraph node/edgeとして解決し、最終linkまで到達して初めてvertical sliceが通ったと数える。

WebAssemblyは将来比較できる任意targetの一つであり、現在のゴール、必須milestone、またはarchitectureの既定値ではない。WASM生成や実行の既存証拠はtarget-generationの限定実験として保持するが、native binary生成やcross-ecosystem dependency resolutionの代わりにはしない。

## 現在地と中心的な未解決点

現在までに、限定されたsource-derived IR、owned validation/interpretation/transformation、Nim planning kernelとRust executor、incremental discovery、demand-driven execution、identity、measurement、native/LLVM/WASM経路について個別の証拠がある。

一方、固定したsource間の単一callが通ることは、実際のproject dependency graphを解決できる証拠ではない。package managerから得るfeatures、versions、target条件、build dependencies、generated source、native library、header、link order、ABI、toolchain制約を含む推移的closureは、単一のsemantic call compositionとは別の問題である。

現在もっとも代替不可能な未解決点は、**Cargo圏、Nimble圏、C圏、C++圏をまたぐ異種の依存関係を、ecosystem固有の意味を失わず一つの需要駆動graphへ正規化し、候補爆発を抑えながら時間・ピークメモリ・再計算量を最小化して、native executableに必要なclosureを解けるか**である。

## 当面のゴール

> Cargo、Nimble、C、C++の各ecosystemから注入される外部依存を含む固定projectについて、LAMINARIAがpackage/source/artifact/toolchain/ABI/link関係を一つの型付き依存グラフとして解決し、選択理由と拒否理由を説明し、そのclosureから通常実行できるnative binaryを生成する。さらに、解決時間、ピークメモリ、展開・枝刈り・再計算した状態数を計測し、naiveな全候補展開より優れた解法を一つ判断する。

これは全Cargo/Nimble semantics、全C/C++ build system、全platform、最速compiler、production package manager、WASM対応、分散build、またはself-hostingの完成を意味しない。成功条件は、固定した現実的なmixed dependency workloadについて、正しいclosure、実行可能native binary、negative case、資源測定、architecture判断を得ることである。

## 当面の研究プログラム

### G1 — cross-ecosystem dependency graph

最小だが非自明なfixtureを固定する。少なくともCargo crate、Nimble package、C library、C++ libraryを各一つ含め、version/feature/target条件、生成またはadapter action、native link edgeを明示する。各ecosystemのidentityを保持したまま共通graphへ正規化し、要求artifactから必要closureだけを展開する。

- 中心Issue: #8、#22、#44
- 関連Issue: #3、#4、#5、#7、#18
- positive case: 唯一の整合するclosureと、その選択理由を得る
- negative case: version、feature、ABI、symbol、toolchainのいずれか一つが両立しないclosureを、compile開始前に構造化診断で拒否する
- 停止条件: package/source/artifact/toolchain/linkの各edgeが追跡可能で、opaqueなpackage-manager buildへ逃げずにclosureを確定または拒否できる

### G2 — native executable vertical slice

G1で解決したclosureをproduction Nim planner / Rust runtimeへ渡し、必要なcompile、adapter、archive、link actionを実行して、対象OSで直接起動できるnative executableを生成する。単一callの成立ではなく、推移的外部依存を含むgraph全体が成果物へ到達することを検証する。

- 中心Issue: #6、#4、#5、#44
- 関連Issue: #3、#10、#12、#20
- 停止条件: binaryの起動結果、全入力とproducerのidentity、実行／省略action、最終link入力が直接的な実行可能テストで確認できる

### G3 — 解決効率と増分性の比較

同じdependency workloadで、全候補を先に展開するbaselineと、lazy expansion、constraint propagation、canonicalization、memoization、equivalent-state merging、dominance pruning、SCC condensationを用いるcandidateを比較する。cold resolution、no-op、leaf dependency変更、feature/target条件変更を測る。

- 中心Issue: #8、#7、#11、#12
- 関連Issue: #6、#19〜#24
- 判断指標: wall-clock、peak RSS、展開／枝刈り／mergeしたstate数、再計算node数、実行を回避した外部tool数
- 停止条件: correctnessを維持したうえで、少なくとも一つのgraph algorithmまたは表現について採用・棄却・再定式化を判断できる

G1〜G3を当面の研究マイルストーンとする。意味IRの融合／分割やWASM target pipelineは有用な別研究だが、このmilestoneの直列gateにしない。

## その後のプロジェクト進行

1. Cargo/Nimble/C/C++ semanticsとnative platform coverageを反例駆動で拡張する。
2. 実在する解決済みgraphからcompiler workの分割／融合、incremental invalidation、resource-aware schedulingを再導出する。
3. 同じ論理graphについてmemory、local disk、peer、remote durable storeへの配置を比較する。
4. native executable経路のsource/dependency coverageをLAMINARIA自身のstage0→stage1→stage2へ広げる。
5. WASM、shared library、その他のtargetを、native経路と同じtyped graphを消費する任意target variantとして評価する。

## supporting trackの扱い

semantic IR、target pipeline、measurement、identity、diagnostic、toolchain profile、CI、platform compatibility、UX、baseline比較は、G1〜G3の判断を信頼可能にするため必要な範囲で起動する。任意targetの完成や単一compiler pathの疎通を、dependency resolution milestoneの代わりにしない。

## タスク選択規則

1. 要求するnative artifactと対象host/targetを固定する。
2. そのartifactのclosureへ関与するecosystemとconstraintを列挙する。
3. 最大の未知を反証する最小のpositive/negative graphを固定する。
4. correctnessと同時に、時間、peak memory、探索state、再計算量を直接測る。
5. 結果を得た時点で停止し、graph model、algorithm、次のgoalを更新する。

したがって現在の次のタスクは、G1のcross-ecosystem dependency graph実験である。

## 現在の判断に必要な文書

- 制約: [独自コンパイラ責務契約](01-foundations/compiler-ownership-contract_ja.md)と[研究優先順位ポリシー](01-foundations/research-prioritization-policy_ja.md)
- 中心研究: [Cross-ecosystem dependency graph research](02-research-areas/toolchains/cross-ecosystem-dependency-graph.md)
- 実行順序: [Research Issue Plan](03-work-items/issue-plan.md)
- 現在の実験: [最初のcross-ecosystem dependency graph実験](03-work-items/design/cross-ecosystem-dependency-graph-first-experiment.md)
- supporting trackと履歴を含む全体索引: [文書マップ](README.md)

# LAMINARIA 全体進行と当面の研究ゴール

本書をLAMINARIA文書群とタスク選択の正準な起点とする。[文書マップ](README.md)では、他の全文書を本書から下流の基盤、研究領域、作業項目、手順、履歴として整理する。

## 長期ゴール

LAMINARIAの長期ゴールは、Cargo、Nimble、C、C++の各ecosystemが持つpackage、source、生成物、toolchain、ABI、linkの関係を、一つの説明可能な依存関係グラフとして解決し、その解決結果からRust/Nimを含むprojectの**通常実行できるnative binary**を効率的、高速、省メモリに生成することにある。最終的にはLAMINARIA自身とその推移的依存を同じ経路でbuildする。

LAMINARIAは対応するRust/Nim source semantics、IR、変換、work分割／融合、scheduler、native target生成を所有する。CargoやNimble、C/C++側のpackage metadata、manifest、lockfile、source取得、system library情報は入力として利用できるが、package managerが暗黙に起動するbuild script、compiler、linkerを依存解決そのものと混同しない。

## 成果物とtargetの優先順位

現在のprimary targetは、一般的なOSで直接起動できるnative executableである。object、archive、shared library、runtime、system libraryを明示的なgraph node/edgeとして解決し、最終linkまで到達して初めてvertical sliceが通ったと数える。

WebAssemblyは将来比較できる任意targetの一つであり、現在のゴール、必須milestone、またはarchitectureの既定値ではない。WASM生成や実行の既存証拠はtarget-generationの限定実験として保持するが、native binary生成やcross-ecosystem dependency resolutionの代わりにはしない。

## 成果物が与える価値

LAMINARIAはCargo、Nimble、C、C++のpackage選択だけを先に解いて結果を配るものではない。packageからsource/module/type/FFI、language IRからintermediate IRへのlowering、artifact/toolchain/ABI/symbol/linkまでを協調して解き、各依存義務をspecialization、lowering、code generation、static link、embedding、または明示的externalizationによってbuild時に**discharge（充足して消込）**する。その利用者は元のpackage manager、compiler、header、feature、ABI、link-order graphを再構築せず、生成されたnative artifactを実行できる。

元のdependency graphは導出・再現・監査のprovenanceとして保存するが、利用者が再resolutionする実行時topologyにはしない。OS、kernel、driver、dynamic library等への物理的依存が必ずゼロになるという意味ではなく、残るものは明示・検証可能なruntime contractとしてexternalizeする。枝刈りは不要な義務を`ProvenIrrelevant`と証明して解決・生成workを減らす重要な最適化だが、この価値の本体ではない。

さらに、testできないbinaryは完成artifactとみなさない。exact production artifact identityにtest contract、harness、target environment、control／observation、raw evidenceを結び付ける。test-profileやinstrumented binaryだけの成功でproduction binaryを認定せず、test-only dependencyをrelease artifactへ漏らさない。

## 現在地と中心的な未解決点

現在までに、限定されたsource-derived IR、owned validation/interpretation/transformation、Nim planning kernelとRust executor、incremental discovery、demand-driven execution、identity、measurement、native/LLVM/WASM経路について個別の証拠がある。

一方、固定したsource間の単一callが通ることは、実際のproject dependency graphを解決できる証拠ではない。package managerから得るfeatures、versions、target条件、build dependencies、generated source、native library、header、link order、ABI、toolchain制約を含む推移的closureは、単一のsemantic call compositionとは別の問題である。

ecosystem横断のpackage resolution自体には、*Package Managers à la Carte*という直接的な形式先行研究がある。したがって現在もっとも代替不可能な未解決点は、**Cargo、Nimble、C、C++のpackage選択を、sourceから発見されるmodule・type・FFI事実、multi-level IR lowering、native artifact・ABI・symbol・link orderと一つの需要駆動typed graph上で増分協調解決し、候補爆発を抑えながら時間・ピークメモリ・再計算量を最小化できるか**である。

## 当面のゴール

> Cargo、Nimble、C、C++の各ecosystemから注入される外部依存を含む固定projectについて、LAMINARIAがpackage選択、source/semantic事実、language-to-intermediate IR lowering、artifact/toolchain/ABI/symbol/link関係を一つの型付き依存グラフ上で増分協調解決する。各依存義務をdischarge、externalize、または理由付きでrejectし、利用者が元のecosystem graphを再解決せず直接実行できるnative binaryを生成する。到達不能なworkの早期枝刈りを含む解法について、解決時間、ピークメモリ、output size、展開・枝刈り・再計算した状態数を測り、naiveな全候補・code展開より優れた方式を一つ判断する。

これは全Cargo/Nimble semantics、全C/C++ build system、全platform、最速compiler、production package manager、WASM対応、分散build、またはself-hostingの完成を意味しない。成功条件は、固定した現実的なmixed dependency workloadについて、正しいclosure、実行可能native binary、negative case、資源測定、architecture判断を得ることである。

## 3レーンの研究プログラム

研究を、成果物の意味と完全性を担う**Lane A — Semantic and Artifact Closure**、compiler計算の効率と物理実行を担う**Lane B — Efficient Compiler Computation**、exact artifactの制御・観測・反証・認定を担う**Lane C — Executable Verification and Testability**に分ける。三者は別graphを持たず、同じpackage/source/semantic/IR/artifact/ABI/symbol/link/test node、identity、provenanceを読む。source semanticsとIRは三laneを接続する共有substrateである。

### Lane A — Semantic and Artifact Closure

#### A1 / G1 — cross-ecosystem dependency graph

最小だが非自明なfixtureを固定する。少なくともCargo crate、Nimble package、C library、C++ libraryを各一つ含め、version/feature/target条件、生成またはadapter action、native link edgeを明示する。package層はPackage Calculusへ写すか、写せない正確なsemantic divergenceを記録する。その結果をsource/semanticとnative-artifact constraintへ接続し、各ecosystemのidentityを保持したまま要求artifactから必要closureだけを展開する。

- 中心Issue: #8、#22、#44
- 関連Issue: #3、#4、#5、#7、#18
- positive case: 唯一の整合するclosureと、その選択理由を得る
- negative case: version、feature、ABI、symbol、toolchainのいずれか一つが両立しないclosureを、compile開始前に構造化診断で拒否する
- 停止条件: package/source/artifact/toolchain/linkの各edgeが追跡可能で、opaqueなpackage-manager buildへ逃げずにclosureを確定または拒否できる

#### A2 / G2 — native executable vertical slice

G1で解決したclosureをproduction Nim planner / Rust runtimeへ渡し、必要なsemantic validation、lowering、compile、adapter、archive、link actionを実行して、対象OSで直接起動できるnative executableを生成する。単一callの成立ではなく、推移的外部依存に由来するpackage/source/IR/ABI/link義務がそれぞれ`Discharged`、`Externalized`、または`Rejected`へ到達することを検証する。不要codeはearly pruning、compiler DCE、linker GCで除去し、FFI export、constructor、dynamic-retention root、runtime supportは保守的に保持する。

- 中心Issue: #6、#4、#5、#44
- 関連Issue: #3、#10、#12、#20
- 停止条件: binaryの起動結果、各依存義務のdischarge/externalization evidence、全入力とproducerのidentity、実行／省略action、最終link入力、保持／枝刈りしたsymbol/sectionが直接的な実行可能テストで確認できる

### Lane B — Efficient Compiler Computation

#### B1 / G3 — 解決効率、枝刈り、増分性の比較

同じdependency workloadで、全候補・codeを先に展開するbaselineと、lazy expansion、cross-layer reachability pruning、constraint propagation、canonicalization、memoization、equivalent-state merging、dominance pruning、SCC condensationを用いるcandidateを比較する。cold resolution、no-op、leaf dependency変更、root-set変更、feature/target条件変更を測る。

- 中心Issue: #8、#7、#11、#12
- 関連Issue: #6、#19〜#24
- 判断指標: wall-clock、peak RSS、output size、展開／枝刈り／mergeしたstate数、保持／枝刈りしたpackage/source/IR/artifact/symbol数、再計算node数、実行を回避した外部tool数
- 停止条件: correctnessを維持したうえで、少なくとも一つのgraph algorithmまたは表現について採用・棄却・再定式化を判断できる

### Lane C — Executable Verification and Testability

#### C1 — exact production artifact harness（#49）

exact production binaryをtest subjectとして、clean target environment、明示input／control、exit／signal／stdout／stderr／ABI／symbol／runtime observation、negative dependency、raw evidenceを一つの`TestContract`で実行する。instrumented/test-profile artifactは別identityとし、test-only dependencyをrelease artifactへ漏らさない。source／IR／artifact変更からretest集合を導出する研究はC3へ拡張する。

### 共有milestone gate

- **M0 — contract lock:** Lane Aのobligation/artifact contract、Lane Bのevent/identity/measurement contract、Lane Cのtest subject/control/observation/oracle contractを同じ固定graphについて確定する。
- **M1 — first tested dependency-discharged native artifact（現在）:** A1/G1とA2/G2がexact production native artifactを成立させ、B1/G3が同じgraph上でcorrectness-equivalentな効率比較と一つのalgorithm判断を得て、C1/C2がexact binary、cross-language path、ABI/runtime、negative dependency、pruning equivalenceを直接testする。
- **M2 — representative tested native projects:** ecosystem coverageと、増分性・memory・publication correctness、test selection、environment coverageを実projectへ広げる。
- **M3 — tested self-hosted native toolchain:** stage0→stage1→stage2の依存義務とproducer lineageを、説明可能でresource-boundedかつconformance-testedなowned computationとして成立させる。
- **M4 — qualified resilient release:** package/update/rollback契約と、対象profileに必要なlocalまたはdistributed recovery契約を満たす。

意味IRの融合／分割やWASM target pipelineは有用な別研究だが、M1のnative evidenceの代わりにしない。詳細な3レーンportfolioは[Project Work Portfolio](03-work-items/project-portfolio.md)に定める。

## その後のプロジェクト進行

1. Cargo/Nimble/C/C++ semanticsとnative platform coverageを反例駆動で拡張する。
2. 実在する解決済みgraphからcompiler workの分割／融合、incremental invalidation、resource-aware schedulingを再導出する。
3. 同じ論理graphについてmemory、local disk、peer、remote durable storeへの配置を比較する。
4. native executable経路のsource/dependency coverageをLAMINARIA自身のstage0→stage1→stage2へ広げる。
5. WASM、shared library、その他のtargetを、native経路と同じtyped graphを消費する任意target variantとして評価する。

## supporting trackの扱い

semantic IR、target pipeline、measurement、identity、diagnostic、toolchain profile、CI、platform compatibility、UX、baseline比較は、G1〜G3の判断を信頼可能にするため必要な範囲で起動する。任意targetの完成や単一compiler pathの疎通を、dependency resolution milestoneの代わりにしない。

## タスク選択規則

1. 要求するnative artifact、保持すべきexport/dynamic root、対象host/targetを固定する。
2. そのartifactのclosureへ関与するecosystemとconstraintを列挙する。
3. 最大の未知を反証する最小のpositive/negative graphを固定する。
4. correctnessと同時に、時間、peak memory、探索state、再計算量を直接測る。
5. 結果を得た時点で停止し、graph model、algorithm、次のgoalを更新する。

したがって現在の次のタスクは、G1のcross-ecosystem dependency graph実験である。

## 現在の判断に必要な文書

- 制約: [独自コンパイラ責務契約](01-foundations/compiler-ownership-contract_ja.md)と[研究優先順位ポリシー](01-foundations/research-prioritization-policy_ja.md)
- 3レーンの基礎研究: [Lane A — Semantic and Artifact Closure](02-research-areas/toolchains/lane-a-semantic-artifact-closure-foundations_ja.md)、[Lane B — Efficient Compiler Computation](02-research-areas/execution/lane-b-efficient-compiler-computation-foundations_ja.md)、[Lane C — Executable Verification and Testability](02-research-areas/toolchains/lane-c-executable-verification-foundations_ja.md)
- 中心研究: [Cross-ecosystem dependency graph research](02-research-areas/toolchains/cross-ecosystem-dependency-graph.md)
- 先行研究と差分: [複数ecosystem依存とcompiler IRを結合して解く先行研究調査](02-research-areas/toolchains/cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md)
- 枝刈りのcorrectnessと計測: [Native executableをrootとするcross-layer枝刈り](02-research-areas/toolchains/cross-layer-reachability-pruning_ja.md)
- 依存義務を成果物へ変換する契約: [異種依存義務をbuild時にdischargeするartifact contract](02-research-areas/toolchains/dependency-resolved-artifact-closure_ja.md)
- 成果物のtestability: [Testable Native Artifactと第一級Test Harness](02-research-areas/toolchains/testable-native-artifact-harness_ja.md)
- 全projectの成果・能力・未Issue化gap: [Project Work Portfolio](03-work-items/project-portfolio.md)
- 実行順序: [Research Issue Plan](03-work-items/issue-plan.md)
- 現在の実験: [最初のcross-ecosystem dependency graph実験](03-work-items/design/cross-ecosystem-dependency-graph-first-experiment.md)
- supporting trackと履歴を含む全体索引: [文書マップ](README.md)

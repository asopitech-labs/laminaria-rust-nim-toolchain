# LAMINARIA 全体進行と当面の研究ゴール

本書をLAMINARIA文書群とタスク選択の正準な起点とする。[文書マップ](README.md)では、他の全文書を本書から下流の基盤、研究領域、作業項目、手順、履歴として整理する。

## 長期ゴール

LAMINARIAの長期ゴールは、RustとNimのsource semanticsをLAMINARIA自身が解析し、独自のIR、変換、work分割／融合、scheduler、target生成を通して、最終的にLAMINARIA自身をbuildできることにある。

CargoやNimbleをpackage管理や依存解決の入力として利用することはあっても、`rustc`、Nim compiler、LLVMなどへ対象sourceの意味解析、IR変換、code generationを委譲した経路は、この長期ゴールの達成経路とは数えない。

## 現在地

現在までに、次の能力について個別の成立証拠がある。

- 限定されたRust/Nim source subsetからのLAMINARIA-owned IR生成
- owned IR上の検証、解釈、限定された変換
- 限定されたowned WebAssembly生成と実行
- Nim planning kernelとRust executorによるplanning/execution
- dynamic discovery、incremental session、demand-driven executionの限定実験
- identity、measurement、native/LLVM経路のbaseline実験

これらは完成したsubsystemではない。また、個別に成立しただけでは長期ゴールの中核仮説を支持しない。現在もっとも大きい未解決点は、**異なる言語のsource semanticsから得た表現を、既存compiler/backend境界より前で一つの計算として合成し、LAMINARIA自身の判断で変換・分割・生成できるか**である。

## 当面のゴール

当面の研究ゴールを次の一つに固定する。

> Rust sourceのcallerとNim sourceのcalleeから得たsemantic IRをLAMINARIAが一つの計算として合成し、言語境界を越えるowned transformationを適用し、owned target artifactを実行したうえで、同じ計算を融合する場合と分割する場合のどちらを選ぶべきかを、保持できるsemantic factsに基づいて一つ決定する。

このゴールはmixed-language compilerの完成、最速化、全言語機能、全OS対応、分散build、またはself-hostingの完了を意味しない。成功条件は、固定した一つのworkloadについてarchitecture判断を得ることである。候補IRで成立しないことや、早期境界化が必要だと分かることも有効な結果である。

## 当面の研究プログラム

### G1 — semantic compositionとowned vertical slice

固定したRust caller / Nim calleeをsourceから別々に解析し、宣言と定義を解決して一つのowned IRへ合成する。言語境界を越える変換を一つ行い、変換前後のowned interpreterとowned WebAssemblyの結果を一致させる。signature mismatchを変換・生成前に拒否する。

- 主Issue: #31
- 根拠となるIssue: #3、#25、#5
- 判断: 現在のsemantic representationで言語横断の計算を保持・変換・生成できるか
- 停止条件: 成立、欠落semantic factの特定、または早期境界化が必要という判断のいずれか

G1が終わるまで、一般的なscheduler改善、resource accounting、永続化、remote execution、追加platform、CLI完成度は、G1の証拠を成立させるために不可欠な場合だけ扱う。

### G2 — 融合／分割境界の比較

G1と同じsource workloadを、少なくとも次の二つの候補で比較する。

1. 合成IR内でcross-language callを変換・融合してからtarget生成する。
2. call境界を残し、明示的なcontract/artifact境界として分割する。

性能競争を主目的にしない。比較するのは、必要なsemantic facts、変換合法性、provenance、再計算・無効化単位、生成artifact、説明可能性である。

- 主Issue: #25
- 関連Issue: #13、#29、必要な範囲だけ#4/#7
- 判断: LAMINARIA固有の最初のsemantic partition/fusion ruleを採用、棄却、または再定式化する
- 停止条件: 固定workloadについて一つの境界判断と、その判断に必要だった事実を記録する

### G3 — 同一owned pathの入力形態による反証

G1/G2で選んだ同じowned pathを、Rust-only、Nim-only、mixedの各最小fixtureへ適用する。単一言語時に既存compilerへ退行しないこと、mixedだけを特別扱いしたarchitectureでないことを確認する。

- 主Issue: #26
- 関連Issue: #28、#3、#5
- 判断: 選択したpathが入力言語数ではなくsemantic workloadを単位にできるか
- 停止条件: 三形態で成立する、またはいずれかでarchitecture差が必要な理由を特定する

G1〜G3を当面の研究マイルストーンとする。G1の結果が現在のIR仮説を棄却した場合は、G2/G3へ惰性で進まず、#25で表現仮説を再定式化する。

## その後のプロジェクト進行

### Phase 2 — semantic pressureの拡張

制御フロー、再帰、ownership/aliasing、effect、runtime依存、foreign dependencyなど、現在の判断を壊し得るworkloadを一つずつ追加する。#32、#33、#34、#37、#42、#44を、網羅実装ではなく反例探索として使用する。

### Phase 3 — work decompositionと実行architecture

実在するowned compiler workから、Action境界、incremental invalidation、resource model、pull schedulingを再導出する。#6、#7、#8、#12、#13、#27、#36を使う。既に成立したgeneric scheduling機能を磨くことではなく、semantic partitionが実行計画をどう変えるかを判定する。

### Phase 4 — 物理配置、永続化、異種node

同じ論理workを、memory、local disk、peer、remote durable storeへどう配置するか、またWindows/macOS/Linux/Raspberry Piなど異なるhost/targetへどう配置するかを比較する。#38〜#41を中心に、#6/#7へ必要な制約だけ戻す。networkやstorageの一般的実装可能性ではなく、LAMINARIAのsemantic/invalidation境界が配置判断を変えるかを問う。

### Phase 5 — coverage拡張とself-hosting

前段で棄却されなかったowned pathのsource subsetとdependency形態を段階的に広げ、LAMINARIA自身のcrate/moduleをstage0→stage1→stage2で置き換える。#2を長期統合Issueとして使う。既存toolchainによるbootstrap buildは比較・移行手段であり、達成判定ではない。

## supporting trackの扱い

measurement、identity、diagnostic、toolchain profile、CI、platform compatibility、UX、baseline比較は、上記の判断を信頼可能にするためのsupporting trackである。#10、#11、#14〜#24、#30などは独立した完成品ロードマップとして進めず、P0実験の結論が変わり得る欠陥、または次の判断に必要な証拠があるときだけ起動する。

## タスク選択規則

次のタスクは常に次の順で決める。

1. 現在の研究ゴールと判断ゲートを明記する。
2. その判断を妨げる最大の未知を一つ選ぶ。
3. 未知を反証できる最小のpositive/negative experimentを固定する。
4. 既存技術で代替可能な工学は、実験成立に必要な最小量へ制限する。
5. 結果を得た時点で停止し、次のゴールまたはarchitectureを更新する。

したがって、現在の次のタスクはG1の最初の実験である。これは「次に空いているIssue」だからではなく、当面のゴール全体で最大の未知を直接判定するから選択される。

## 現在の判断に必要な文書

- 制約: [独自コンパイラ責務契約](01-foundations/compiler-ownership-contract_ja.md)と[研究優先順位ポリシー](01-foundations/research-prioritization-policy_ja.md)
- 実行順序: [Research Issue Plan](03-work-items/issue-plan.md)
- 現在の実験: [Issue #31 — 最初の最小言語横断owned-transformation実験](03-work-items/design/issue-31-first-minimal-experiment.md)
- supporting trackと履歴を含む全体索引: [文書マップ](README.md)

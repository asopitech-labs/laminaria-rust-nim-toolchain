# 研究Issueの優先順位・最小仮説検証ポリシー

## 目的

LAMINARIAの現段階のIssueは、完成品の機能やproduction品質のsubsystemを納品するためのものではない。各Issueは、プロジェクト全体のarchitectureを決めるために、反証可能な研究仮説を最小の実験で判定する単位である。

Issueのcloseは、その領域の実装が完成したことを意味しない。仮説を支持、棄却、または未解決として次へ送るための証拠と判断が揃ったことを意味する。

## 全Issueに優先する判断基準

作業候補は次の順で評価する。

1. **代替不可能性**: Cargo、Nimble、C、C++の異種依存を一つのgraphとして解くことなど、既存compiler、build system、scheduler、CAS、remote execution、OS機構を並べるだけでは答えられないLAMINARIA固有の問いか。
2. **反証力**: 失敗した場合に、LAMINARIAの依存graph、解決algorithm、IR、意味保持、変換、native target生成またはself-hostingの仮説を棄却・変更できるか。
3. **architecture識別力**: 複数のcandidate architectureから一つを選ぶ証拠になるか。単に「実装できた」だけでは研究上の優先度は低い。
4. **最小性**: 判断に必要な最小のsource subset、workload、target、node数、failure caseであるか。
5. **波及性**: 後続の複数Issueの前提を決めるか。
6. **工学コスト**: 上記が同等なら、短く、可逆で、既存資産を再利用できる実験を先にする。

## プロジェクト全体最適の拘束条件

最適化の単位は、要求されたartifactと、それを生成・検証・発展させるproject progressionである。
個別component、pass、cache、scheduler、resolver、test harness、measurement subsystemを
それ自体で目的にしない。

局所的に改善した結果を採用する前に、次の全体decision boundaryで評価する。

1. **Parent outcome**: その変更が進めるproject outcomeとactive milestoneを特定する。
2. **End-to-end effect**: 編集したcomponentだけでなく、requested artifactのcorrectness、
   completion latency、peak／retained resource、output quality、reproducibility、operabilityへの
   影響を測定または説明する。
3. **Displaced cost**: upstream、downstream、または別research laneへ移されたwork、memory、I/O、
   complexity、failure risk、maintenanceを含める。
4. **Opportunity cost**: 遅延する高leverageな実験と、証拠なしに狭める将来architecture／targetの
   選択肢を記録する。
5. **Trade-off position**: 同じboundaryでalternativeを比較し、採用するtrade-offを明示する。
   correctness、time、memory、artifact quality、maintainabilityの比較不能な影響を、projectが
   utility functionを明示していない状態で一つのscalarへ潰さない。

局所metricの改善は観測であってproject上の結論ではない。全体outcomeを改善する、境界外に
中立である、またはcostを記録したproject-level trade-offを成立させる場合に採用できる。
costを別の場所へ移しただけの場合や、解消する不確実性より多くのproject effortを消費する場合は、
棄却または再定式化する。

Measurementにも同じ規則を適用する。計測精度は、現在生きているarchitecture候補を識別するか、
active hypothesisを反証できる地点まで高めればよい。measurement infrastructure、benchmark coverage、
telemetry detailにはimplementation、runtime、storage、analysis、maintenance costがある。判断に不要な
精緻化は、evidence system自体の局所最適化として扱う。

## 優先度

### P0 — LAMINARIA固有仮説

- Cargo/Nimble/C/C++から得るversion、feature、target、source、header制約をsource semantics、language/intermediate IR、ABI、symbol、toolchain、linkと需要駆動で協調解決し、全依存義務をdischarge／externalize／rejectできるか。
- 候補の直積を生成せず、正しいclosureを高速・省メモリ・増分的に求め、その選択／拒否理由を説明できるか。
- 解決済みclosureから、一般的なOSが直接起動できるnative executableを生成できるか。
- Rust/Nimのsource semanticsから、既存compiler IRを入口にせずLAMINARIA-owned representationを導けるか。
- 言語境界を越えてsemantic factsを保持・合成し、合法な変換と拒否を説明できるか。
- semantic factsに基づいてcompiler workを分割／融合し、既存のtranslation-unit、LLVM module、generic Action境界とは異なる判断を示せるか。
- composed/selected IRをLAMINARIA-owned target loweringへ渡し、実行可能artifactを生成できるか。
- 同じowned pathをRust-only、Nim-only、mixed、最終的にはLAMINARIA自身へ拡張できるか。

### P1 — P0を判定するためのenabler

identity、最小計測、planner/runtime接続、diagnostic、runtime/ABI contractなど、P0実験の正当性を保つために必要な範囲。enabler単独の網羅性やproduction化を目的にしない。

### P2 — 代替可能な工学・baseline

汎用thread pool、一般的なCPU/RAM admission、CAS、atomic publication、remote transport、CLI polish、全OS対応、既存toolchain matrixなど。技術的feasibilityはprior artで確認できるため、P0/P1実験が要求する最小範囲だけ実装する。独立して完成品を目指さない。

## Issueの最小判定契約

各Issueは、現段階では次の5項目だけを必須契約とする。

1. **最小仮説**: 一文で支持または棄却できる問い。
2. **最小実験**: 仮説を判定する最小のpositive caseと、必要なら一つのnegative/counterexample。
3. **観測**: correctnessと判断に直接必要な差分だけ。汎用telemetryの完成を要求しない。
4. **停止条件**: 結果から採用、棄却、再定式化、保留のいずれかを決定できた時点。
5. **非ゴール**: 全機能、全target、全OS、全failure、最適性能、production運用、完成品品質。

既存Issue本文の長いAcceptance criteria、extension、将来要件は研究backlog／候補証拠として保持するが、現段階で全項目を一括達成するclose条件とは解釈しない。次の実験に採用する項目は、着手前に最小判定契約へ明示する。

## レビュー規則

レビュー指摘が現在の実験を止めるのは、次のいずれかの場合だけである。

- 証拠が仮説を実際には判定していない。
- correctness、identity、provenance、比較条件の欠陥により結論が変わり得る。
- 既存compilerへのhidden fallbackなど、研究責務を破っている。
- negative caseがなく、成功がvacuousである。
- 報告された局所改善がproject内の別領域へ重大なcostまたはriskを移しているのに、end-to-endの
  trade-offが評価されていない。

汎用性、完全性、拡張性、性能調整、API polish、追加platform対応は、現在の仮説判定に必要でなければblockerにしない。後続候補として記録し、同じIssueへ無断で追加しない。

## プロジェクト全体の現在の優先順

具体的な現在地、当面の終了条件、後続フェーズは [LAMINARIA 全体進行と当面の研究ゴール](../near-term-research-program_ja.md) をcanonical roadmapとする。このポリシーは選び方を定め、roadmapは現在どの判断を選んだかを定める。

1. Cargo/Nimble/C/C++のpackage制約、source semantics、language/intermediate IR、artifact、toolchain、ABI、symbol、link関係を一つの型付きgraphで協調解決し、依存義務のdischarge状態を表す。
2. 要求native executableから必要closureだけを展開し、整合する一例とcompile前に拒否する一例を示す。
3. 解決済みclosureをproduction planner/runtimeで実行し、通常実行できるnative binaryを生成・起動する。
4. eager baselineと需要駆動candidateを、wall-clock、peak memory、探索state、再計算量で比較する。
5. そのgraphを壊す反例に必要な範囲でsemantic IR、融合／分割、identity、persistence、heterogeneous placementを追加する。
6. 有効だった範囲をLAMINARIA自身の推移的dependency closureとself-hostingへ拡張する。

WebAssemblyは任意targetとして後続比較できるが、この優先順のgoalまたは必須gateではない。

この順序はIssue番号順でも、未完了チェックボックス数順でもない。新しい証拠でarchitecture上の不確実性が変われば更新する。

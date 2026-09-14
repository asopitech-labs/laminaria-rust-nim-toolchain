# ゴール駆動の作業指示ポリシー

## 1. 目的

LAMINARIAの作業指示は、現在のproject stateから観測可能なproject outcomeへ至る
有限の経路を定義する。禁止する実装の列挙を主構造にしない。禁止事項の集合は開いており、
列挙されていない別のshortcutがgoalを満たすことまでは保証できない。

指示者はgoal、semantic decision、acceptance relation、checkpoint sequenceに責任を持つ。
実装者は、固定されたcheckpoint間を移動するengineering choiceに責任を持つ。新しいproduct、
research、semantics、evidence、authorityの判断が必要になった場合、実装者は停止し、その判断を
指示者へ返す。

本ポリシーは[当面の研究プログラム](../near-term-research-program_ja.md)、
[project portfolio](../03-work-items/project-portfolio.md)、
[fixture policy](fixture-policy_ja.md)と併せて適用する。

## 2. 指示の順序

実質的な作業指示は、必ず次の順序で書く。

1. **Goal** — parent project outcomeにとって意味のある、外部から観測可能な到達状態。
2. **Starting state** — 現在再利用できる能力と既知のgap。現在実装を仕様へ昇格させない。
3. **Checkpoint A、B、...** — goalへ必然的につながる、短く順序付けた中間結果。
4. **Verification gate** — completionを反証できる直接観測と独立relation。
5. **Handoff** — 次のmilestoneがそのまま消費する出力と、close／方向変更の決定者。

Non-goalやsafety constraintは、このpositive pathの後にguardrailとして置ける。ただし、
それらを指示全体の構造にしない。

## 3. Goalの要件

Goalには次をすべて含める。

- 要求するsubjectまたはartifact。
- 外部から観測できるbehavior。
- そのbehaviorが成立すべきdependency、environment、identity境界。
- 進めるparent capabilityまたはresearch decision。
- 結果を誤りと判定できるevidence。

「要求したnative artifactがclean environmentで起動し、保持された全dependency obligationに
production evidenceがある」のように結果で書く。「enumを追加する」「testを7本書く」
「特定commandを呼ぶ」のような活動をgoalの代わりにしない。

implementation名、command、module、test名を固定するのは、それらが既にpublic contractまたは
cross-component contractである場合だけとする。それ以外はproject outcomeではなく実現手段である。

## 4. Checkpoint contract

各checkpointは、次の5項目だけで構成する。

| 項目 | 必須の意味 |
| --- | --- |
| Result | checkpoint完了時に存在する観測可能な状態 |
| Consumes | 正本となる入力と、直前checkpointの出力 |
| Must preserve | semantics、identity、side effect、ownershipの不変条件 |
| Evidence | 結果を反証できる直接観測または独立relation |
| Enables | 状態を再構築・推測せずに開始できる次checkpoint |

Checkpoint resultは、想定した悪い実装の列挙より確実に経路を束縛する。たとえば、
「negative workloadがstructured rejectionを返し、process traceとfilesystem diffから
production actionが開始されていないと観測できる」はresultである。compiler commandの
禁止表現を増やし続けることは、これと同じcontractではない。

sequenceは因果的に完全でなければならない。最終acceptance claimは必ずいずれかのcheckpointが
生成し、各checkpoint outputは後続checkpointまたはfinal handoffが消費する。次に判断・実行
できることを変えない儀礼的checkpointは削除する。

指示者はcompletion relationを次の形で説明できなければならない。

```text
Checkpoint Aのresult
  必要な正本inputを伴ってCheckpoint Bを開始可能にする
Checkpoint Bのresult
  要求されたsubjectに対するVerification gateを開始可能にする
Verification gateのresult
  明示された境界内でGoalが成立したことを示す
```

割り当てたactivityの集合を完了することではなく、このrelationがwork contractである。

## 5. 判断の所有者

指示者は、assignment前に次を決定する。

- project goalとparent outcome。
- semantic meaningとlifecycle state。
- artifact／subject identity。
- authority境界と許容side effect。
- checkpoint順序とhandoff shape。
- acceptance oracleとcompletionを宣言できる者。

実装者は、その境界内で次を決定できる。

- 内部の分割とlocal data structure。
- contractではない命名。
- checkpoint到達に必要なrefactoring。
- 効率的な実装手法。
- 指定されたverification relationを強化する追加test。

実装者は、予期しない挙動を「十分近い」と独自判断したり、checkpointを再解釈したり、oracleを
弱めたり、production resultを代替物へ置き換えたりしない。その場合はcheckpoint failureとして、
観測状態、evidence、影響するcheckpoint、指示者が決めるべき事項を返す。

## 6. 指示者のpreflight

作業を渡す前に、指示者はgoalからcheckpointまでをpreflightする。

1. Goalを当面の研究プログラムとportfolio capabilityへ接続する。
2. 各checkpointが単なるcode changeやCI greenではなくgoalを進めることを確認する。
3. toolやside effectに関する不安定な仮定を実測する。`dump`、`metadata`、`check`等の
   command名はread-onlyの証拠ではない。
4. 各checkpointについて、弱いtestを通る誤結果を一つ想定し、それが失敗するようresultまたは
   oracleを強化する。
5. final handoffだけで次checkpointを開始でき、記録外で生成したstateへ依存しないことを確認する。
6. contract維持に必要でないimplementation prescriptionを削除する。

完全なpositive checkpoint pathを書けない場合、その作業はassignment可能ではない。先に不足する
decisionを作るか解決する。

## 7. Verification gate

Verificationはcode形状のchecklistではなく、checkpoint pathに沿って行う。

各checkpointについて次を記録する。

- 実行したproduction entry point。
- inputとsubject identity。
- 観測したresult。
- 独立relationまたはoracle。
- そのresultによって開始可能になったdownstream checkpoint。

CI成功、test件数、source-text検索、enumの存在、graph nodeの存在、既知command一つの不在は
補助観測にすぎない。checkpoint resultそのものを直接構成する場合を除き、completion evidenceには
ならない。

reviewerは因果chainを検査する。存在は到達性ではない。plan済みactionは実行evidenceではない。
生成予定pathはartifactではない。test variantはproduction subjectではない。implementationが
serializeした現在値は独立oracleではない。

## 8. Deviationとrevision

Checkpointへ記述どおり到達できない場合は次の順序で扱う。

1. checkpointの意味を変更したり代替resultへ置き換えたりする前に停止する。
2. 指示の仮定を反証した最小の観測を報告する。
3. blocked checkpointと、必要なowner decisionを特定する。
4. 指示者がgoal pathを改訂するか、goal pathを維持して別の実現経路を選ぶ。
5. resultが有効な最後のcheckpointから再開する。

Issue bodyの改訂だけがauthoritative pathを変更する。progress commentやcompletion reportはevidenceを
提供するが、goal／checkpoint contractを上書きしない。

## 9. Completion責任

実装者はcheckpoint evidenceとreadinessを報告する。issueが明示的に別の者へ割り当てない限り、
reviewerがcompletion decisionを持つ。

提出物がgoalを満たさない場合、まず指示をreviewする。

- Goalは外部から観測可能だったか。
- Checkpoint chainからgoalが必然的に導かれたか。
- 弱い代替物でもevidenceを満たせなかったか。
- 必須resultではなく誤った手段を指示していなかったか。
- 指示者が持つべきdecisionを実装者へ渡していなかったか。

再作業を求める前に指示の欠陥を修正する。曖昧または誤った指示から誠実に生成された提出物は、
instruction contractが失敗したevidenceである。

## 10. 必須のissue形状

implementation／experiment issueは、次のpositive structureを持つ場合だけassignment可能とする。

```text
Goal
  観測可能なproject resultとparent outcome

Starting state
  再利用可能な能力と既知gap

Checkpoint A
  Result / Consumes / Must preserve / Evidence / Enables

Checkpoint B
  Result / Consumes / Must preserve / Evidence / Enables

Verification gate
  end-to-end observationと独立oracle

Handoff
  次のconsumer、closure authority、stop condition
```

編集するfile一覧、避けるcommand一覧、追加するtest一覧だけでは、このstructureを代替できない。

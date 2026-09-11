# #36 T0 — 増分planning契約の確定（設計提案）

対象issue: #36（「Implement incremental Nim/Rust planning for dynamically
discovered dependencies」、parent #28、related #6/#8/#10/#19/#37）。本書は
#36をT0（契約確定、本書）とT1（実装、後続）に分割したうちのT0であり、
コード実装は一切含まない。ゴール設定担当（Claude）が候補比較・契約案を
提出し、指示者が採否と仕様revisionを記録した上でT1を発行する。

基準commit: `09a997c`。本書のすべての事実主張は、下記の各行番号のファイル
を直接読んで確認したものであり、想像で補っていない。

---

## §0. issue #36自身の完了条件との対応

issue #36本文の受け入れ基準10項目のうち、T0が担うのは最初の1項目
「増分契約のschema、version、snapshot/generation、event identityが明示
される」の確定である。残り9項目（実装・実行を要求するもの）はT1以降の
対象であり、本書はそれらの**契約**（入力event列・期待graph・期待trace・
診断reason）を固定するに留め、実コード・実行結果は一切含まない。

---

## §1. 既存契約の棚卸し（本書のすべての設計判断の根拠）

### 1.1 現行`PlanningInput`/`PlanOutcome`は完全に一回限り、かつプロセス寿命一回限り

`crates/laminaria-plan/src/types.rs:93-102`:
```rust
pub struct PlanningInput {
    pub schema_version: String,
    pub demanded_artifacts: Vec<String>,
    pub actions: Vec<Action>,
}
```
`nim-planner/src/planning_kernel.nim:120`の`plan*(input: PlanningInput): PlanOutcome`は、
1回の完全な`PlanningInput`を受け取り、1回の完全な`ExecutionPlan`/`PlanRejection`を
返す純粋関数であり、呼出間で一切状態を保持しない（モジュールレベルの
可変状態はゼロ）。さらに`nim-planner/src/laminaria_planner.nim:26-77`の
`main()`は、stdinを1回読み(27-32)、`plan()`を1回呼び(69)、結果を書いて
`quit(0)`(75)で**プロセスごと終了する**。つまり現行契約は「アルゴリズム
として一回限り」なだけでなく「プロセスの寿命として一回限り」であり、
Rust側が2回目の増分要求を送れる生きたNimプロセスは存在しない。

### 1.2 `RejectionReasonKind`(5種、Rust/Nim完全一致)

`crates/laminaria-plan/src/types.rs:137-145`(Rust)と
`nim-planner/src/contract.nim:144-149`(Nim)は完全に一致する5variant:
`Cycle`/`UnsupportedInput`/`MissingProducer`/`DuplicateProducer`/
`InvalidContractVersion`(wire値はsnake_case)。`PlanRejection`
(`types.rs:147-158`)は`{schema_version, reason_kind, reason_detail,
cycle_path: Vec<String>}`——`cycle_path`は`reason_kind == Cycle`の
ときのみ埋まる。本書はこの5種を破壊的に変更せず、新規diagnostic
（後述§6.4）を**追加**する形で設計する。

### 1.3 `ArtifactRef`は2variantのみ、`Source`は「producer不要」を既に表現できる

`types.rs:40-45`: `ArtifactRef::Source{path}` / `ArtifactRef::Declared{artifact_id}`
の2つのみ。`Source`はplanning_kernelのアルゴリズム上「producerを要求
されない」入力を表す既存の一次プリミティブである。

**本書の設計判断（重要）**: 増分planningで「前generationで既に完成した
artifact」を表すために`ArtifactRef::Source`を流用**しない**。`Source`の
`path`フィールドはファイルシステム上のソースファイルを指す意味が
既に固定されており、「計算済みのcompiler-work成果物」を指すのに転用
すると意味の混同を生む。代わりに§3で新規フィールド
`known_satisfied_artifacts: Vec<String>`を`PlanningInput`(v2)へ追加する。

### 1.4 CPU budget dispatchは「待機＝slot占有」が現行の構造そのもの

`crates/laminaria-run/src/compiler_work_executor.rs:752-758`:
```rust
pub fn run_compiler_work_plan_with_concurrency_trace(
    plan: &ExecutionPlan,
    store: &mut ArtifactStore,
    cpu_budget: NonZeroUsize,
) -> (Result<(), CompilerWorkExecutionError>, usize)
```
`run_compiler_work_plan_inner`(760-810)は`cpu_budget.get()`個のOSスレッドを
`std::thread::scope`で起動し(784-797)、各スレッドは`worker_loop`(622-701)を
**planの生存期間全体にわたって**実行する。readyなactionがなければ
`ready_or_done.wait(state)`(630-641)でそのスレッド自身がcondvar上で
ブロックする——つまり「まだ準備できていない」状態と「budgetの1枠を
占有している」状態は、現行実装では**構造的に同一**である(スレッドは
待機中もbudgetの1枠を握ったまま解放しない)。`ComputeConcurrencyProbe`
(446-465)は実計算区間のみを数えるため待機中のスレッドを検出できない。

**これが#36の「依存待ちはCPU実行slotを占有しない」という受け入れ基準が
現行実装に対して要求する、唯一かつ本質的な変更点である**——ロジカルな
action状態(`blocked_dependency`)とOSスレッド/実行枠占有を分離する必要が
あり、現行コードには流用できる分離機構が存在しない(本書はこの分離の
**観測可能な契約**を固定するに留め、実装方式(thread pool再設計・
async化等)はT1の選択に委ねる——観測可能な契約そのものは§9の
「dependency wait」計測境界と§10 case1で固定する)。

### 1.5 「generation」という語は既に別概念で使われている——本書は`planning_generation`と呼ぶ

`crates/laminaria-run/src/self_build.rs:1-27`(モジュール冒頭)および
`docs/self-build.md`が定義する「generation」は、**自己ビルドの世代**
(stage0がstage1を、stage1がstage2を生成する)を指す。
`ARTIFACT_GENERATION_ROOT`(`self_build.rs:44`)、`generation_root: &Path`・
`generation_label: &str`(`self_build.rs:203-210`)はいずれも**呼び出し側が
渡すディレクトリパス・人間可読ラベル**であり、単調カウンタではない。

本書が§4で新規定義する「増分planningの世代」(#36自身の受け入れ基準が
使う語)は、**これとは完全に別の概念**であり、混同を避けるため本書全体で
一貫して**`planning_generation`**と呼ぶ(自己ビルドのgenerationとは
コード上も無関係、両者が同時に登場する文脈は存在しない)。

### 1.6 「同一semantic keyへの合流」は issue #7/#12 の既存identity機構を再利用する

`crates/laminaria-run/src/reuse.rs:56-62`:
```rust
pub struct ArtifactIdentityKey {
    pub schema_version: String,
    pub source_digest_sha256: String,
    pub toolchain_digest_sha256: Option<String>,
    pub command_identity: String,
    pub target_identity: Option<String>,
}
```
issue #7/#12が既に確立した「2つのRunが同一artifactを指すか」の判定機構
(`decide_reuse`, `reuse.rs:296-`)は、`command_identity`+`source_digest_sha256`
+`toolchain_digest_sha256`+`target_identity`の完全一致で判定する。本書は
このshapeを**再利用**し、§4.4の`semantic_key`をこの5フィールドから
決定的に導出する(新しい並行した識別方式を発明しない)。ただし
`compute_identity_key`(`reuse.rs:249-264`)自体は完了済み`Run`からしか
呼べない(実行後の情報を要求する)ため、本書はplanning時点で同じ
5フィールドを`CompilerWorkDescriptor`から構成する**新しい**導出手順を
定義する(§4.4)——「semantic key」という語自体はリポジトリ全体で
本書が初出であり(既存コード・docsに前例なし)、
`CompilerWorkDescriptor.semantic_input_artifact_ids`(`compiler_work.rs:147`、
1つのactionが**読む**artifact id集合)とは別概念であることを明示する。

### 1.7 `docs/self-build.md`の既存のpull型scheduling拒否への応答

`docs/self-build.md:66-78`は、Buck2/BazelのSkyframe型(依存未解決なら
`null`を返し後で再起動する)demand駆動schedulingを**明示的に採用しない**
と記録している。理由は2点: (a)当時cacheが存在しなかった、(b)Nim
plannerが「1回の完全な文書を返す」one-shot subprocess呼出だった。

**本書の設計は、この2つの前提のいずれも破らない**:
- (a)は issue #7/#12(`reuse.rs`)により部分的に解消済みだが、本書自体は
  cacheの実装を要求しない。
- (b)は本書が最も重視する制約であり、§2の設計原則「Nim plannerは
  純粋・無状態・one-shotのまま変更しない」が直接この文言を維持する。
  増分性は**Rust runtime側が`plan()`を複数回、更新された入力で呼び直す**
  ことで実現し、Nim plannerをSkyframe型の「保留して後で再開される
  関数」に変えることは一切行わない。`plan()`は依然として「1回の完全な
  文書を返す」ままである。

`docs/research-foundations.md:268`は既にこの方向性を支持している:
> "The planner should not be called after every completed action.
> Replanning is reserved for material changes such as failures, dynamic
> dependency discovery, resource-budget changes, or executor
> availability changes."

同`:447`(§17 未解決事項の4番目)は「動的依存をconstant replanningなしに
どう取り込むか」を未解決事項として明記しており、本書はこの問いに直接
答える。

---

## §2. 設計原則

1. **Nim plannerの`plan*`/`planFromJson*`/`decodePlanningInputOrReject*`の
   シグネチャ・挙動は変更しない。** 増分性はRust runtime側の責務であり、
   Nim側には「新しいwire fieldを1つ追加で解釈する」以外の変更を要求しない
   (§3.1)。
2. **Nim plannerはpull-based/Skyframe型の「保留して再開される関数」には
   ならない。** 毎回、完全でself-containedな`PlanningInput`(v2)を受け取り、
   完全な`PlanOutcome`を1回で返す。
3. **「全体再計画」ではなく「累積グラフの再計画」。** 既に`completed`の
   actionは次回呼出で`known_satisfied_artifacts`として渡され、producer
   探索の対象から除外される(§3.1)——これにより`docs/research-foundations.md:268`
   の「material changesのみreplan」原則と、#36自身の境界条件「全action
   完了ごとの全体再計画を標準経路にしない」の両方を満たす。再計画の
   トリガーは新規依存の発見（`DependencyDiscovered`event）のみであり、
   action完了そのものではreplanしない。
4. **CPU実行枠の占有とロジカルな依存待ちを分離する契約を固定する
   （実装方式はT1が選ぶ）。**
5. **`semantic_key`はissue #7/#12の`ArtifactIdentityKey`shapeを再利用する。**

---

## §3. Request/Eventスキーマ（schema version・全enum・必須field）

### 3.1 `IncrementalPlanningInput`（`PlanningInput`のv2拡張）

schema version: `"incremental-planning-input/0.1.0"`(独立したschema。
既存`PlanningInput.schema_version`の`"0.2.0"`とは別の識別子——後方互換の
主張をしない、新規契約であることを明示する)。

```yaml
IncrementalPlanningInput:
  schema_version: string          # 固定値 "incremental-planning-input/0.1.0"
  planning_generation: u64        # このsessionでの世代番号（§4.2）
  demanded_artifacts: [string]    # 既存PlanningInputと同一意味
  actions: [Action]               # 既存Action型そのまま（型変更なし）
  known_satisfied_artifacts: [string]  # NEW: 前世代までに完成済みのartifact id。
                                        # planning_kernel.plan*はこの集合に
                                        # 含まれるDeclared artifactについて
                                        # MissingProducerを要求しない
                                        # （Sourceと同じ扱いだが別概念、§1.3）。
```

`known_satisfied_artifacts`が唯一の新規wire fieldである。Nim側の変更点は
`planning_kernel.nim:162-188`の「missing producer」判定において、
`neededIds`内のDeclared artifactが`knownSatisfiedArtifacts`に含まれる場合は
producer探索をスキップする、という1箇所の追加のみ(既存の`plan*`の入出力
シグネチャ・`ordered_actions`の意味・`ExecutionPlan`の形は無変更)。

### 3.2 `PlanningEvent`（Rust runtime内部、Nim plannerへは送らない）

Nim plannerへの入力は依然として§3.1の`IncrementalPlanningInput`のみ
(1.1の制約を守るため)。`PlanningEvent`はRust runtime**内部**の状態機械を
駆動する契約であり、Nim plannerとのIPC境界を越えない。

schema version: `"incremental-planning-event/0.1.0"`。

```yaml
PlanningEvent:
  event_id: string        # 一意識別子。既存generate_run_id()と同じ規約
                           # ({unix_ns}-{pid}-{counter}、laminaria-experimentの
                           # unique_run_id()と同型) を踏襲、新規モノトニック
                           # counterはこのeventスキーマ専用
  sequence_number: u64     # 発行側(action単位)が単調増加させる番号。
                           # 順序入替検出に使う（event_id自体は順序を
                           # 保証しない）
  planning_generation: u64 # このeventが有効とみなされていたgeneration
  emitted_at_unix_ns: u128
  kind: PlanningEventKind
```

`PlanningEventKind`(enum、全variant固定、これ以上の追加はT1判断に
委ねず本書で確定する):

```yaml
PlanningEventKind:
  - DependencyDiscovered:
      action_id: string
      newly_required_artifact_ids: [string]  # このactionが新たに要求する
                                              # producer未確定のartifact
  - ProducerCompleted:
      artifact_id: string
      produced_by_action_id: string
  - ProducerFailed:
      artifact_id: string
      produced_by_action_id: string
      failure_reason: string
  - DemandRequested:
      semantic_key: string       # §4.4
      requested_by: string       # 論理的な要求元識別子（consumer id）
  - DemandCancelled:
      semantic_key: string
      requested_by: string       # DemandRequestedと対応するrequested_byの取消
```

必須field(全variant共通): `event_id`, `sequence_number`,
`planning_generation`, `emitted_at_unix_ns`, `kind`。省略可能なfieldは
存在しない——#35 D0の教訓(「D1実装者が再選択できる余地を残さない」)を
踏襲し、任意fieldをゼロにする。

---

## §4. Identity

### 4.1 snapshot identity（既存を再利用、変更なし）

`source_snapshot_id`は`crates/laminaria-plan/src/compiler_work.rs:64-70`の
既存定義をそのまま使う: 1つのsource fileの内容を指す文字列identity
(実装は`compute_source_snapshot_id`、`compiler_work_executor.rs:239-244`の
SHA-256)。本書はこれを変更しない。増分planningにおいても、同一
`source_snapshot_id`を持つactionの再送出は§7の冪等規則の対象になる。

### 4.2 `planning_generation`identity（新規、自己ビルドgenerationと別概念、§1.5）

- 型: `u64`、単調増加、**Rust runtime側が所有・発行**(§5)。
- 初期値: 1つの増分planning sessionの開始時に`0`。
- 増分規則: `DependencyDiscovered`eventを受理してRust runtimeが
  `IncrementalPlanningInput`をNim plannerへ再送出するたびに、
  `planning_generation`を1増やす。**それ以外のいかなるイベント
  (ProducerCompleted/Failed、Demand*)でも増やさない**(§2原則3の
  「再計画は新規依存発見のみがトリガー」を`planning_generation`の
  増分規則として固定する)。
- 「古いevent」の判定規則: あるeventの`planning_generation`が、Rust
  runtimeが現在保持している最新の`planning_generation`より**小さく**、
  かつそのeventの内容が既に最新世代のplanで反映済み(該当actionが既に
  最新世代のplanに存在し、そのeventが伝える情報が既知)である場合、
  そのeventは`stale_generation`として拒否する(§6.4の診断ケース)。
  「小さいが、まだ未反映の情報を含む」eventは stale として拒否せず
  通常処理する(単に到着が遅れただけで、情報自体は依然有効なため)——
  この区別が§7の冪等規則の核心である。

### 4.3 event identity（冪等性の基礎）

`event_id`は`crates/laminaria-run/src/lib.rs:379-385`の既存
`generate_run_id()`(`{unix_ns}-{pid}`)と同じ文字列規約を踏襲し、
D1-aで追加した`laminaria-experiment::unique_run_id()`と同型の単調
counterを付加する: `{unix_ns}-{pid}-{counter}`。Rust runtimeは処理済み
`event_id`の集合を保持し、既知の`event_id`を持つeventは（内容が同一か
異なるかに関わらず）即座に無視する(§7.1)。

### 4.4 `semantic_key`（新規、issue #7/#12の`ArtifactIdentityKey`shapeを再利用、§1.6）

`ArtifactIdentityKey`(`reuse.rs:56-62`)と同じ5フィールド
(`schema_version, source_digest_sha256, toolchain_digest_sha256,
command_identity, target_identity`)を、**planning時点で**
`CompilerWorkDescriptor`から構成する:

```
semantic_key = SHA-256(canonical_json({
  schema_version: "0.1.0",              # REUSE_SCHEMA_VERSION と同一値を踏襲
  source_digest_sha256: <source_provenance.source_snapshot_id>,
  toolchain_digest_sha256: null,        # planning時点では未解決(執行前)、
                                         # 常にnull固定 — 執行後の実
                                         # toolchain identityとは異なる
                                         # 弱いkeyであることを明示する
  command_identity: <operation_version + ":" + sorted(semantic_input_artifact_ids).join(",")
                     + ":" + sorted(requested_functions).join(",")>,
  target_identity: <language>,          # "rust" | "nim"、CompilerWorkDescriptor.language
}))
```

`toolchain_digest_sha256`を常に`null`固定するのは、正直さのため
(`decide_reuse`の既存規則「未解決は差異として扱う」をそのまま流用すると
毎回不一致になってしまうため、semantic_keyの用途ではtoolchain identityを
比較対象に含めない、という設計判断を明示する——執行後の実
reuse判定(`decide_reuse`)とsemantic_key合流(§8)は**別の判定**であり、
前者を後者の代用にしない)。

---

## §5. Nim plannerとRust runtimeの所有状態・責務

| 項目 | 所有者 | 根拠 |
|---|---|---|
| `plan*`の純粋計算(1世代分の閉包計算・cycle検出・topo sort) | Nim planner | §1.1、変更なし |
| `known_satisfied_artifacts`の解釈(producer探索スキップ) | Nim planner | §3.1、唯一の新規Nim側変更 |
| `planning_generation`カウンタの発行・増分 | Rust runtime | §4.2、Nim側は世代を発行せず受け取るのみ |
| 累積グラフ状態(discovered/blocked_dependency/readyの全action) | Rust runtime | §1.1(Nimは無状態のまま) |
| `PlanningEvent`のキュー・順序・冪等判定 | Rust runtime | §3.2、Nim plannerへは送らない内部契約 |
| CPU budget・実行枠の割当・`blocked_dependency`の非占有化 | Rust runtime | §1.4 |
| `semantic_key`の計算・合流判定 | Rust runtime | §4.4(Nim側はsemantic_keyを一切知らない) |
| 共有producerの参照カウント・取消判定 | Rust runtime | §8 |
| 診断reason(§6.4)の最終決定(`stale_generation`/`cancelled_result`はRust側のみが判定可能な情報。`missing_producer`/`cycle`はNim plannerの`plan*`が既存の`RejectionReasonKind`としてそのまま報告し、Rust側はそれを転記するのみ) | Rust runtime | §1.2 |

---

## §6. 状態遷移

### 6.1 状態(6種、#36本文の指定通り)

`discovered` → `blocked_dependency` → `ready` → `running` →
`completed` | `failed` | `cancelled`

### 6.2 遷移表

| from | to | 条件 | 一度だけ性 |
|---|---|---|---|
| (初期) | `discovered` | actionが`IncrementalPlanningInput.actions`または`DependencyDiscovered`eventにより初めて識別される | — |
| `discovered` | `ready` | 全ての宣言済みinput artifactが既に`known_satisfied_artifacts`または`completed`済み | — |
| `discovered` | `blocked_dependency` | 宣言済みinputのうち少なくとも1つが未完成 | — |
| `blocked_dependency` | `blocked_dependency` | 追加の`DependencyDiscovered`eventにより、blocking producer集合が**増加**する(既存の待機理由に追加、リセットしない) | 冪等(§7.3) |
| `blocked_dependency` | `ready` | blocking producer集合が空になる(最後の1つが`ProducerCompleted`) | **一度だけ**(#36 case2の要求。複数producerが同時に完了しても、blocking集合が空になった瞬間の1回のみ遷移する) |
| `ready` | `running` | Rust runtimeがCPU budgetの空き枠をこのactionに割り当てる(スケジューラ判断、eventではない) | — |
| `running` | `completed` | 実行成功 | — |
| `running` | `failed` | 実行失敗 | — |
| `running` | `completed`だが公開しない(`cancelled_result`診断、§6.4) | 実行完了時点で、このactionへの参照カウント(§8)が既に0 | §6.3参照 |
| (discovered\|blocked_dependency\|ready) | `cancelled` | 参照カウントが0に達する(§8) | — |
| いずれか | `cancelled`(直接) | 全ての`DemandRequested`が対応する`DemandCancelled`で相殺され、かつ`running`未満の状態 | — |

### 6.3 `running`中の取消は非プリーム型

現行`compiler_work_executor.rs`にプリエンプション機構は存在しない
(§1.4で確認済み、worker_loopは`dispatch_action`呼出を中断しない)。
したがって`running`状態のactionへの取消要求は、実行を中断させず
「取消要求済み」フラグを記録するのみ。実行が`completed`/`failed`で
終わった時点で、参照カウントを再確認する:
- 参照カウント > 0(他のconsumerがまだ必要としている): 通常通り
  `completed`/`failed`として結果を公開する(共有producer生存規則、
  §8、case5)。
- 参照カウント == 0: 結果は`ArtifactStore`へは記録するが(証拠を
  失わない、このプロジェクト全体の一貫した方針)、どのconsumerへも
  公開しない。この終端状態を`cancelled_result`診断(§6.4)として
  明示的に区別する——「黙って捨てる」ことも「黙って公開する」ことも
  しない。

### 6.4 診断reason(新規、既存`RejectionReasonKind`5種に追加する形)

既存の`missing_producer`/`cycle`は、増分planning下でも**Nim planner自身が
`plan*`の呼出結果として直接報告する**(§5表)。以下3種はRust runtime側が
新規に判定・報告する(既存enumを拡張するのではなく、Rust runtime内部の
別のdiagnostic分類として追加する——既存`RejectionReasonKind`は
plan()呼出1回に対する拒否理由であり、増分session全体の診断とは
scopeが異なるため、意図的に別のenumとする):

```yaml
IncrementalDiagnosticReason:
  - stale_generation       # §4.2、既に上書きされたgeneration由来のevent
  - cancelled_result        # §6.3、取消後に完了した結果
  - dependency_failed       # blocking producerがfailedで終わった場合、
                             # blocked_dependency中のactionが取るべき終端状態
```

---

## §7. 冪等規則（重複・遅延・順序入替event）

### 7.1 重複(duplicate)

同一`event_id`を持つeventの2回目以降の到着は無視する(§4.3)。

### 7.2 遅延(delayed)

`planning_generation`が現在の最新世代より古いeventのうち、その内容が
既に最新世代の状態に反映済みのものは`stale_generation`として拒否する
(§4.2)。反映済みでない情報を含む遅延eventは、通常のeventとして処理する
(生成が古いこと自体は拒否理由にならない、情報の新しさで判断する)。

### 7.3 順序入替(reordered)

`sequence_number`は発行側(action単位)が単調増加させるため、同一
actionに対する2つの`ProducerCompleted`/`DependencyDiscovered`eventが
逆順で到着しても、Rust runtimeは`sequence_number`の低い方を「既に
上書きされた」と判定できる。ただし状態遷移そのもの(§6.2)は**内容ベース**
で冪等になるよう設計している(例: `blocked_dependency → blocked_dependency`
は「blocking集合への追加」という集合演算であり、event到着順序に依存
しない——2つの`DependencyDiscovered`eventがどちらの順で来ても最終的な
blocking集合は同じになる)。これが#36 case4「eventの重複・遅延・順序
入替で最終graphと公開結果が変化しない」の根拠である。

---

## §8. semantic key合流・共有producer取消規則

- 同一`semantic_key`(§4.4)を持つ2つ以上の`DemandRequested`eventは、
  **1つの**action/実行へ合流する。2番目以降の`DemandRequested`は、
  既に`discovered`以降の状態にあるactionへの参照カウント+1として
  扱われ、新しいactionを作らない。
- 参照カウントは`DemandRequested`ごとに+1、対応する`DemandCancelled`
  ごとに-1。カウントが0に達したときのみ§6.2の取消遷移が発生する
  (case5「一consumerの取消後も共有producerが生存する」の根拠——
  取消はconsumer単位の要求撤回であり、producer自体の即時破棄ではない)。
- `blocked_dependency`中のactionが取消された場合(参照カウント0):
  そのactionが依存していたproducerへの参照カウントも連動して-1する
  (推移的取消)。ただしそのproducerが他のconsumerからも要求されていれば
  (参照カウント>0のまま)生存する。

---

## §9. 計測境界

| 指標 | 開始 | 終了 | 備考 |
|---|---|---|---|
| `dynamic_expansion_count` | session開始(`planning_generation=0`) | session終了 | `DependencyDiscovered`eventを受理し`planning_generation`を実際に増分した回数(§4.2)。eventを受理したが増分しなかった場合(内容が既存グラフに変化なし)はカウントしない |
| IPC時間 | Rust runtimeが`IncrementalPlanningInput`をNim plannerへ送出する直前 | `PlanOutcome`を受領した直後 | 既存`M8-many-unrequested-nim-planner`の`kernel_nanos`(`planning_kernel.plan`単体)とは別に、往復全体を計測する。既存の測定境界(issue#35 D0 M8)の区別をそのまま踏襲——kernel時間とround-trip時間を混同しない |
| dependency wait | action が`blocked_dependency`へ遷移した時刻 | 同actionが`ready`へ遷移した時刻 | action単位で記録。§1.4で確認した「現行はwait=slot占有」という既存の制約からの脱却を証明する直接指標 |
| time-to-first-useful-work | session開始 | 最初のactionが`running`へ遷移した時刻 | 「別枝の依存発見が継続中でも先にready枝が実行開始する」(case1)ことを裏付ける単一指標 |

---

## §10. 固定する最低限6種類→10 case

「診断を個別に」という指定(#36指示、本タスク指示の項目6)に従い、
`stale_generation`/`cancelled_result`/`missing_producer`/
`dependency_failed`/`cycle`を**5つの独立したcase**として分離する。
合計10 case、詳細な初期状態・入力event列・期待graph・期待trace・
診断reasonは機械可読な`docs/design/issue-36-t0-cases.yaml`に定義する
(本書はcase一覧と各caseの目的のみを示す):

1. `T0-incremental-ready-branch-proceeds-during-discovery` — 別枝の依存
   発見が継続中に、依存が閉じたready枝が実compiler workを開始する
   (§9 time-to-first-useful-work、§6.2 discovered→ready)。
2. `T0-incremental-late-dependency-single-resume` — 後からproducer依存が
   追加され、一度だけ待機・再開する(§6.2 blocked_dependency→ready
   「一度だけ」性)。
3. `T0-incremental-semantic-key-merge` — 同一semantic keyへの同時要求が
   一計算へ合流する(§8)。
4. `T0-incremental-event-reorder-duplicate-delay-stable-result` —
   eventの重複・遅延・順序入替で最終graphと公開結果が変化しない(§7)。
5. `T0-incremental-cancel-survives-shared-producer` — 一consumerの取消後も
   共有producerが生存する(§8参照カウント)。
6. `T0-incremental-diagnostic-stale-generation` — 古いgenerationのevent
   拒否(§4.2、§7.2)。
7. `T0-incremental-diagnostic-cancelled-result` — 取消後に完了した結果の
   診断(§6.3)。
8. `T0-incremental-diagnostic-missing-producer` — 増分session内での
   missing producer(既存`RejectionReasonKind::MissingProducer`が
   増分文脈でも正しく報告されることの確認)。
9. `T0-incremental-diagnostic-dependency-failed` — blocking producerの
   失敗によるblocked_dependency actionの終端診断。
10. `T0-incremental-diagnostic-cycle` — 新規発見された依存が既存グラフと
    cycleを構成する場合の診断(既存`RejectionReasonKind::Cycle`+
    `cycle_path`が増分文脈でも正しく報告されることの確認)。

全caseは、issue #36の境界「sleepだけの疑似workではなく、
source-derived owned compiler workで実行を証明する」に従い、
`crates/laminaria-ir`由来の実IR評価(`LowerSource`→`ValidateIr`→
`EvaluateEvidence`、既存`compiler_work_executor.rs`のcompiler-work
actionKind)を使う固定caseとしてcases.yamlに定義する。

---

## §11. 明示的除外事項（本タスク指示・issue #36境界の反映）

- sleep-onlyの疑似workload。
- Rust側代替plannerへのfallback(境界条件として既に明記、否定試験を
  §10 case群のいずれにも混入させない——#36自身の受け入れ基準最後の項目
  「外部source compilerまたはRust側plannerへのfallbackを否定試験する」は
  T1が実装する試験であり、T0は契約上「fallbackしない」と明記するのみ)。
- 全action完了ごとの全体再計画(§2原則3で明示的に拒否)。
- D3の資源最適化。
- WASM生成(#5 T0/T1の対象、本書と混在させない)。
- 速度向上判定(D4の対象)。
- T0自体はコードを実装しない。

---

## §12. 採否欄

- 指示者: (未記入)
- 採否日: (未記入)
- 判定: (未記入)
- 判定revision: (未記入)
- 備考: (未記入)

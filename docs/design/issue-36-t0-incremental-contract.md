# #36 T0 — 増分planning契約の確定（設計提案、改訂版）

対象issue: #36（「Implement incremental Nim/Rust planning for dynamically
discovered dependencies」、parent #28、related #6/#8/#10/#19/#37）。本書は
#36をT0（契約確定、本書）とT1（実装、後続）に分割したうちのT0であり、
コード実装は一切含まない。

**本書は第2版改訂である。** 初版（基準commit `e3b528c`）は5件のP1で不採択
となり、`1a2e9af`で改訂したが、その改訂版も以下7件の欠陥で再度不採択と
なった:

1. `DependencyDiscovered`契約は`new_actions`を1件以上と定めるが、旧
   改訂版のcase2/6/10はこれを空で送っていた(契約とcaseの矛盾)。
2. stale-generation試験(旧case6)は、現在世代・event世代がともに`0`で、
   staleを一度も再現していなかった。
3. `extends_inputs`(既存Actionの`inputs`だけを書き換え、`id`を維持する)
   は、既存実装の`Action.id`が`CompilerWorkDescriptor`(semantic inputs
   を含む)から再計算される、という既存契約(§1.3)に反していた。
4. 旧case5は`demanded_artifacts`が1件しかないのに、初期状態として
   参照カウント2を要求しており、その参照元(consumer-a/b)を表現する
   手段が契約になかった。
5. 旧case7/8/11は、診断対象となる不正状態(取消後の実行完了、
   missing producer)を`StartSession`自体の初期状態として持ち込んで
   いた——診断対象はすべて`ApplyDelta`で初めて導入されなければならない。
6. 旧case4の「重複」2 eventは実際には異なる`event_id`を持っており、
   `event_id`による重複判定(§7.1)では検出できないものだった。
   「完全重複」(同一id・同一payloadの再送)と「別idを持つ意味的重複」
   (状態遷移そのものの内容的冪等性、§7.3)は別の現象であり、別caseに
   分離する必要がある。
7. `cases.yaml`のActionは`id`/`kind`/`inputs`/`outputs`のみで、契約が
   要求する`command_identity`/`compiler_work`を欠いており、artifact id
   も具体的な値を示していなかった。

**本書はこれら7件すべてに対応する形で§3・§6・§8・§10を再改訂した。**
指示された範囲(Actionのimmutable化+replacement方式、`StartSession.
initial_demands`+需要移送・取消規則、完全なAction/CompilerWorkDescriptor
catalog+具体的artifact ID、対象caseの修正、YAML検証スクリプトのCI組込)
に限定し、他issueの作業は混在させていない。旧版からの変更点は各節末尾に
「**改訂**:」として明示する。

基準commit: `1a2e9af`。本書のすべての事実主張は、下記の各行番号のファイル
を直接読んで確認したものであり、想像で補っていない。artifact idは
実際の`compute_artifact_id`(FNV-1a、`compiler_work.rs:251-263`)を
Pythonで再実装し、実引数から計算した具体的な値である
(`docs/design/issue-36-t0-cases.yaml`の`artifact_id_definitions`参照)。

---

## §0. issue #36自身の完了条件との対応

issue #36本文の受け入れ基準10項目のうち、T0が担うのは最初の1項目
「増分契約のschema、version、snapshot/generation、event identityが明示
される」の確定である。残り9項目（実装・実行を要求するもの）はT1以降の
対象であり、本書はそれらの**契約**（wire protocol・入力event列・期待
graph・期待trace・診断reason）を固定するに留め、実コード・実行結果は
一切含まない。

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

**改訂: この事実認識自体は変えない（現行コードの記述として正確）が、
初版が§2でこの事実から導いた結論（「Nim plannerは変更しない」）を撤回
する。** 指示により、T1は`laminaria_planner.nim`の`main()`を1回読み・
1回書き・終了する構造から、**session単位で起動され、複数commandを
順に読み・複数responseを順に書き、`CloseSession`で終了する**構造へ
書き換える。これは現行コードへの破壊的変更であり、「1箇所の追加のみ」
という初版の主張は誤りだったと認める。詳細は§2・§3。

### 1.2 `RejectionReasonKind`(5種、Rust/Nim完全一致)——変更なし

`crates/laminaria-plan/src/types.rs:137-145`(Rust)と
`nim-planner/src/contract.nim:144-149`(Nim)は完全に一致する5variant:
`Cycle`/`UnsupportedInput`/`MissingProducer`/`DuplicateProducer`/
`InvalidContractVersion`(wire値はsnake_case)。`PlanRejection`
(`types.rs:147-158`)は`{schema_version, reason_kind, reason_detail,
cycle_path: Vec<String>}`——`cycle_path`は`reason_kind == Cycle`の
ときのみ埋まる。本書はこの5種を破壊的に変更せず、新規diagnostic
（後述§6.4）を既存enumとは**別の**enumとして追加する。

### 1.3 `Action`のidentityは`inputs`/`outputs`に依存しない——増分性の鍵となる既存事実

`crates/laminaria-plan/src/types.rs:77-91`:
```rust
pub struct Action {
    pub id: String,
    pub kind: ActionKind,
    pub command_identity: String,
    pub inputs: Vec<ArtifactRef>,
    pub outputs: Vec<ArtifactRef>,
    pub compiler_work: Option<CompilerWorkDescriptor>,
}
```
依存関係は`inputs`の`ArtifactRef::Declared{artifact_id}`が他actionの
`outputs`と一致することから**構造的に導出**される(手書きの`depends_on`
リストはない)。一方`crates/laminaria-plan/src/compiler_work.rs:265-284`の
`lower_source_artifact_id(operation_version, language, source_snapshot_id,
requested_functions, subset_version)`が示す通り、**`Action.id`自体は
`inputs`/`outputs`に一切依存せず、`CompilerWorkDescriptor`の中身
(source_snapshot_id・requested_functions等)のみから決定的に計算される**
(同ファイル冒頭コメント: 「`Action.id`自体がこの作業のidentity
(`work_id`)を兼ねる」)。

**この事実が§1.8の設計判断(discovery未解決の間はaction自体を作らない)
の直接の根拠である**: あるactionの`inputs`に後から新しいDeclared
artifact idを追加しても、そのaction自身の`id`は変化しない(`inputs`は
`*_artifact_id`系関数のいずれの引数にも現れない)。しかし`requested_functions`
のような**descriptor自体の内容**が後から変わる場合は、そのactionの`id`
は必然的に変化する——「既存actionをin-placeで書き換える」ことと
「新しいidを持つ別actionが生まれる」ことは、変わるfieldの種類によって
結果が異なる。本書はこの区別を曖昧にしない(§1.8)。

**改訂(第2版): 「`inputs`だけを書き換える」という逃げ道は、この
domainには実在しない。** `crates/laminaria-run/src/compiler_work_executor.rs:276-338`
の実際のAction構築を直接確認すると、
```rust
let validate_action = Action {
    id: validate_id.clone(),
    inputs: vec![ArtifactRef::declared(&lower_id)],
    outputs: vec![ArtifactRef::declared(&validate_id)],
    compiler_work: Some(CompilerWorkDescriptor {
        semantic_input_artifact_ids: vec![lower_id.clone()],
        ...
```
`inputs`は常に`semantic_input_artifact_ids`(+`LowerSource`の場合の
`ArtifactRef::Source`)の**宣言形そのもの**であり、両者が独立に動く
「意味的ではない、順序だけのedge」はこのcompiler-workの4kindには
存在しない。また`outputs`は常に`[Declared(self.id)]`——つまり
**あるactionの出力artifact idは、そのaction自身のidと同一の文字列**
である。したがって「`inputs`だけ書き換えてidは維持する」という操作は、
この4 kindのどれについても意味を持たない: 意味的入力が変わる箇所は
必ず`semantic_input_artifact_ids`(または`LowerSource`の
`requested_functions`)を通じて`id`自体を変化させる。初版・第1改訂の
`extends_inputs`は、この事実を確認せずに導入された、存在しない逃げ道
だった(第2版改訂の核心的な自己訂正、§3.4)。

### 1.4 CPU budget dispatchは「待機＝slot占有」が現行の構造そのもの——変更なし

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
ブロックする——「まだ準備できていない」状態と「budgetの1枠を占有している」
状態は、現行実装では構造的に同一である。`ComputeConcurrencyProbe`
(446-465)は実計算区間のみを数えるため待機中のスレッドを検出できない。

**これが#36の「依存待ちはCPU実行slotを占有しない」という受け入れ基準が
現行実装に対して要求する、唯一かつ本質的な変更点である**——実装方式
(thread pool再設計・async化等)はT1の選択に委ねるが、観測可能な契約
そのものは§9で`cpu_slot_acquired`/`cpu_slot_released`という直接の
trace事象として固定する(改訂: 初版は「dependency wait」という間接指標
のみだったが、指示により直接のslot占有事象を追加する)。

### 1.5 「generation」という語は既に別概念で使われている——本書は`planning_generation`と呼ぶ（変更なし）

`crates/laminaria-run/src/self_build.rs:1-27`と`docs/self-build.md`が定義する
「generation」は自己ビルドの世代を指し、`planning_generation`(本書§4.2)
とは無関係である。

### 1.6 【初版の誤り】`semantic_key`という新規識別スキームは撤回する

初版§1.6/§4.4は、issue #7/#12の`ArtifactIdentityKey`(`reuse.rs:56-62`)
の**shapeだけ**を借りた新しい`semantic_key`ハッシュを提案していた。
レビューはこれを2点で誤りと指摘した:

1. **参照先そのものが誤っている。** `ArtifactIdentityKey`/`decide_reuse`
   (`reuse.rs:249-264`,`296-`)は**実行完了後のRun**から構成する
   post-execution reuse判定の機構であり、planning時点(実行前)には
   そもそも呼べない。本書が真に必要としていたのは「planning時点で
   計算済み・検証済みの識別子」であり、それは`reuse.rs`にはなく
   `crates/laminaria-plan/src/compiler_work.rs`の
   `lower_source_artifact_id`/`validate_ir_artifact_id`/
   `transform_function_artifact_id`/`evaluate_evidence_artifact_id`
   (`compiler_work.rs:265-359`)である。これらは**まさに`Action.id`
   そのもの**を計算する関数で、既に`source_snapshot_id`・
   `requested_functions`・`test_inputs`・`transform`パラメータ・
   `language`(`lower_source_artifact_id`の`language`引数)を**すべて**
   引数に取る——`toolchain_digest_sha256`を含まない代わりに、
   これらtoolchain非依存の入力だけで既に完全である。
2. **独自ハッシュを新設する必要が最初からなかった。** 本書は§4.4を
   全面撤回し、「planning時点の合流キーは既存の`*_artifact_id`関数の
   戻り値そのもの、または(未解決なら)`PlanningInput.demanded_artifacts`
   が既に使っている呼び出し元指定の文字列のいずれかであり、新しい
   識別スキームを一切発明しない」と改める(§4.4改訂・§8改訂)。

### 1.7 `docs/self-build.md`の既存のpull型scheduling拒否への応答——設計の変化に合わせて再論証する

`docs/self-build.md:66-78`は、Buck2/BazelのSkyframe型(依存未解決なら
`null`を返し後で再起動する、**個々のnode関数**が保留・再開される)
demand駆動schedulingを明示的に採用しないと記録している。

初版はこの拒否を「Nim plannerをone-shotのまま変更しない」ことで
維持すると論じたが、**本改訂はその前提(b)を指示により変更する**
(§2)。したがって初版の論証はもはや成立せず、**再論証が必要**である:

**本書がSkyframe型と区別する点は「one-shotプロセスかどうか」ではなく
「個々のnodeが保留・再開されるかどうか」である。** 本書のsession設計は:
- 1つの`ApplyDelta`呼出は、その場で**完全に**処理され、完全な
  `PlanDelta`または`Rejected`を返す——「まだ準備できていないので後で
  もう一度呼んでほしい」という保留応答は存在しない(Skyframeの
  `null`→再起動パターンはこの契約に一切現れない)。
- Nim plannerは「1つのnode関数の評価を途中で中断し、別の入力で
  再開する」ことをしない。1回の`ApplyDelta`は、そのeventが持つ情報
  すべてをその場で累積graphへ反映し切る、1回で完結する処理である。
  違いは「何回呼ばれるか」(1回→複数回)であって、「1回の呼出の中身が
  保留・再開可能か」ではない。
- (a)cacheの不在という前提はissue #7/#12(`reuse.rs`)により部分的に
  解消済みであり、本書はこれを維持する。

`docs/research-foundations.md:268`の「material changesのみreplan」原則、
および同`:447`の未解決事項4番目への応答という位置づけも変更しない
——「replan」の単位が「document全体の再送」から「1 eventの適用」に
変わっただけで、「actionの完了そのものではreplanしない」という原則
(新規依存発見のみがトリガー)は不変である。

### 1.8 【新設】固定fixtureの前提修正——`lower_rust_source`は未解決の呼び出しを許容しない

レビュー指摘の核心的事実誤認: `crates/laminaria-ir/src/rust_frontend.rs:716-752`
の`Expr::Call`処理を直接確認すると、
```rust
if !ctx.declared_functions.contains(&name) {
    return Err(vec![unsupported_shape(
        format!("call to '{name}', which is not in this lowering request"),
        c.span(),
    )]);
}
```
(747-752行)——`add_or_double`を`requested_functions = ["add_or_double"]`
だけで`lower_rust_source`へ渡すと、`double`への呼出に到達した時点で
**即座に`Diagnostic`を返す**(部分的なProgramやIRは一切生成されない)。
したがって初版が想定した「`add_or_double`だけを先にlowerし、`double`
未解決のまま何らかの中間状態を得る」という前提は成立しない。また
`lower_rust_source`は複数回に分けて呼んだProgramを後から結合する手段を
一切持たない(1回の呼出が1つの完結したProgramを返すのみ)。

**本書の修正: `LowerSource`より前段に、独立した`DiscoverSourceDependencies`
という新しいowned compiler-work operationを置く。** これは`lower_rust_source`
を一切呼ばない、より浅い操作——`syn::parse_file`で構文木を得た後、
既知の`requested_functions`から到達可能な`Expr::Call`の呼び出し先識別子
を(`rust_frontend.rs:716-752`と**同じ**識別子解決規則: 非identifier呼出
除外・qualified path除外・local/parameter shadowing除外を再利用して)
収集するだけの、副作用のない構文走査である。`lower_rust_source`の
前提(`requested_functions`が呼び出し閉包として完結していること)を
一度も破らずに閉包を求められる。

T1は`crates/laminaria-plan/src/types.rs::ActionKind`へ5番目のvariant
`DiscoverSourceDependencies`を追加し、`compiler_work.rs`へ既存の
`lower_source_artifact_id`(265-284行)と同じ引数shapeを持つ姉妹関数を
1つ追加する:

```rust
pub fn discover_source_dependencies_artifact_id(
    operation_version: &str,
    language: &str,
    source_snapshot_id: &str,
    requested_functions_known_so_far: &[&str],
    subset_version: &str,
) -> String
```

この操作の`outputs`は1つのDeclared artifact(内容: `requested_functions_known_so_far`
から到達可能で、かつまだその集合に含まれていない関数名の、ソート済み
リスト)であり、`semantic_input_artifact_ids`は空(`LowerSource`と同様、
入力はsourceそのもの)。

この操作が完了して初めて、Rust runtimeは**閉じた**`requested_functions`
集合(`["add_or_double", "double"]`)を1回の`LowerSource`呼出へ渡す
——`double`を別Programとしてlowerして後から結合する、という初版の
設計は採用しない(§1.3の事実により、`double`を含むLowerSourceの`Action.id`
は`requested_functions`集合全体から決まる1つのidであり、「2つのProgramの
結合」という概念自体が存在しない: 1回のlowering呼出が1つのProgramを
返し、その中に`add_or_double`と`double`の両方の`FnFact`が入る)。

これは§1.3の帰結でもある: `DiscoverSourceDependencies`が完了して
`requested_functions`の閉包が確定するまで、`LowerSource`/`ValidateIr`/
`EvaluateEvidence`のいずれの`Action.id`も**計算不能**である(閉包が
`Action.id`計算の入力そのものだから)。したがって「未解決のまま
placeholder actionを作り、後でin-placeで書き換える」という設計は
採らない——**閉包が確定するまで、これら下流3 actionはgraphに一切
存在しない**。閉包確定と同時に、正しいidを持つ3 actionがまとめて
1回の`DependencyDiscovered`で挿入される(§3.2・§10 case1)。

---

## §2. 設計原則（全面改訂）

1. **Nim plannerはsession-scopedプロセスになる。** `laminaria_planner.nim`の
   `main()`は、1つのsessionの生存期間中、`StartSession`→`ApplyDelta`\*
   →`CloseSession`という一連のcommandをstdinから1行1JSONで順に読み、
   対応するresponseをstdoutへ1行1JSON書き出す。`CloseSession`を受理して
   初めてプロセスは`quit(0)`する。(初版の「Nim側は変更しない」という
   principleは撤回——§1.1・§1.7)
2. **wire protocolは`StartSession`/`ApplyDelta`/`CloseSession`
   (Rust→Nim)と、対応する`PlanDelta`/`Rejected`(Nim→Rust)に固定する。**
   `SessionClosed`は`CloseSession`への応答としてのみ存在する(§3.1)。
3. **初期graphだけを全送信し、以後はdeltaのみを送る。** `StartSession`は
   既存`PlanningInput`(v0.2.0、無変更)を1回だけ運ぶ。それ以降、通常経路
   では全graphを再送しない——`ApplyDelta`は常に1個のeventのみを運ぶ
   (§3.1・§3.2)。
4. **Nimはgraph・需要閉包・cycle検出・ready frontierを所有する。Rustは
   compiler work実行・artifact・CPU/resource状態を所有する。**
   (§5改訂)。「保留・再開されるnode関数」を持たない、という区別で
   Skyframe型pull schedulingとは異なると論証する(§1.7)。
5. **`DiscoverSourceDependencies`を独立したowned compiler-work operation
   として契約に加える。** `lower_rust_source`/`lower_nim_source`の前提
   (閉じた`requested_functions`)を一度も破らない(§1.8)。
6. **合流キーは既存の`*_artifact_id`関数の戻り値、または既存の
   `demanded_artifacts`文字列のいずれかであり、新しい識別スキームを
   発明しない。** (§1.6・§4.4・§8)
7. **CPU実行枠の占有とロジカルな依存待ちの分離を、`cpu_slot_acquired`/
   `cpu_slot_released`という直接のtrace事象で固定する。** (§9)
8. **全体再計画ではなく1 eventずつの累積適用。** actionの完了そのものは
   再計画のトリガーにならない(§1.7、変更なし)。

---

## §3. Wire Protocol（全面改訂）

### 3.1 Command(Rust→Nim、session確立後はstdinへ1行1JSON)

schema version: `"incremental-planner-protocol/0.1.0"`。

```yaml
IncrementalPlannerCommand:
  schema_version: string      # 固定値 "incremental-planner-protocol/0.1.0"
  session_id: string           # このsessionを識別する。Rust runtimeが発行
  command_index: u64            # 0から単調増加。StartSession=0
  kind: IncrementalPlannerCommandKind
```

```yaml
IncrementalPlannerCommandKind:
  - StartSession:
      initial_graph: PlanningInput   # 既存 crates/laminaria-plan/src/types.rs:93-102
                                     # の schema_version "0.2.0" をそのまま使う。
                                     # 新しいinput schemaは作らない(初版の
                                     # IncrementalPlanningInput/known_satisfied_artifacts
                                     # は撤回——session自体が累積状態を持つため
                                     # 不要になった)。session中ちょうど1回のみ送る。
      initial_demands: [DemandReference]   # NEW(第2版改訂)。§3.4のDemandReference
                                            # と同型。`initial_graph.demanded_artifacts`
                                            # (既存field、文字列のみ)を補足し、
                                            # どの`requested_by`が最初から要求して
                                            # いるかを明示する(§4.4改訂・§8改訂)。
                                            # 省略時、Nimは`demanded_artifacts`の
                                            # 各entryに対して匿名の要求者1件
                                            # (参照カウント1)を暗黙に仮定する
                                            # (既存PlanningInputとの後方互換)。
                                            # 同一artifact_idが複数の
                                            # DemandReferenceで指定される場合、
                                            # 参照カウントはその個数になる
                                            # (§8 case5)。**重要(fix#5)**:
                                            # `initial_graph`+`initial_demands`
                                            # が表す開始状態そのものは、常に
                                            # 単独で有効(閉包・cycleなし、
                                            # 未解決のproducer参照なし)でなければ
                                            # ならない——診断対象となる不正状態は
                                            # 必ず後続の`ApplyDelta`が導入する
                                            # (§10各case)。
  - ApplyDelta:
      event: PlanningEvent          # §3.2。ちょうど1個のeventを運ぶ。
  - CloseSession: {}
```

**改訂: 初版の`IncrementalPlanningInput`(v2)と`known_satisfied_artifacts`
フィールドは完全に撤回する。** これらは「Rustが一回限りのNim呼出を
複数回、累積状態を毎回全部渡し直しながら繰り返す」設計のための機構
だった。session自体がNim側に累積graphを保持させる以上、この機構は
不要であり、残しておくと死んだ複雑さになる。

### 3.2 Response(Nim→Rust、stdoutへ1行1JSON)

```yaml
IncrementalPlannerResponse:
  schema_version: string
  session_id: string
  in_reply_to_command_index: u64
  kind: IncrementalPlannerResponseKind
```

```yaml
IncrementalPlannerResponseKind:
  - PlanDelta:
      planning_generation: u64
      changed_actions: [ActionStateChange]   # このeventの結果、実際に状態が
                                              # 変化した(または新規追加された)
                                              # actionのみ。無変化なら空配列。
      diagnostic: IncrementalDiagnosticReason?  # stale_generation |
                                                 # cancelled_result |
                                                 # dependency_failed | null
                                                 # (§6.4)。診断があっても
                                                 # PlanDelta自体は返る
                                                 # (Rejectedとは別、§3.3)。
  - Rejected:
      reason_kind: RejectionReasonKind   # 既存5variant、無変更(§1.2)
      reason_detail: string
      cycle_path: [string]               # reason_kind==cycleのときのみ
  - SessionClosed: {}                     # CloseSessionへの応答。この直後
                                           # プロセスはquit(0)する。
```

```yaml
ActionStateChange:
  action_id: string
  from_state: ActionState?     # null は「このactionがこのdeltaで初めて
                                # graphに現れた」ことを意味する
  to_state: ActionState?       # Nimが追跡する6種、discovered|
                                # blocked_dependency|ready|completed|
                                # failed|cancelled。`running`はRust側の
                                # 実行status overlayでありNimはこれを
                                # 報告しない(§6.1)。**null は、この
                                # actionがsupersede(§3.4・§1.3改訂)により
                                # 退役したことを意味する(その場合
                                # `retired_because_superseded_by`が
                                # 必ず埋まる、第2版改訂・fix#3)。**
  retired_because_superseded_by: string?  # NEW(第2版改訂)。non-null は
                                # `to_state == null`のときのみで、この
                                # actionを置き換えた新しいAction idを
                                # 指す。
  new_action: Action?          # non-null は「graphにまだ存在しなかった
                                # Actionが挿入された」ことを意味し、その
                                # 場合は必ずfull Action(id/kind/
                                # command_identity/inputs/outputs/
                                # compiler_work)を運ぶ——artifact idの
                                # 文字列だけを運ぶことはない
                                # (初版P1「eventがartifact IDしか運ばない」
                                # への直接の修正)。
```

### 3.3 `PlanDelta`の診断つき応答と`Rejected`の使い分け

`Rejected`は既存`RejectionReasonKind`(cycle / missing_producer / 等)の
ときのみ使う——このeventはgraphへ一切反映されず、直前の有効なgeneration
状態がそのまま保持される。一方`stale_generation`/`cancelled_result`/
`dependency_failed`(§6.4、新設3種)は、eventの**受理自体は成功したが
観測可能な変化がない、または既に決まっている終端結果を確定させただけ**
の場合であり、`PlanDelta`(空の`changed_actions`または実際の終端遷移を
含む)+`diagnostic`として返す。両者を同じ`Rejected`にまとめない
——前者は「このgeneration全体を採用しない」、後者は「1つのactionに
ついて特定の理由を記録する」という異なる操作だからである。

### 3.4 `PlanningEvent`(Nim IPC境界を**越える**——初版からの重要な訂正)

**改訂: 初版§3.2は「`PlanningEvent`はRust runtime内部の契約であり、
Nim plannerへは送らない」と明記していたが、これはレビューが指摘した
P1「増分協調が実質的にRust内部へ閉じている」の直接の原因だった。
本書はこれを撤回し、`PlanningEvent`は`ApplyDelta`のペイロードとして
Nim plannerへ直接送られる、と改める。**

**改訂(第2版): `extends_inputs`/`InputsExtension`を完全に撤回し、
`supersessions`/`Supersession`に置き換える。** §1.3改訂が示す通り、
この4 compiler-work kindでは`inputs`と`semantic_input_artifact_ids`が
常に連動しており、「`inputs`だけ書き換えてidは維持する」という操作は
存在しない。あるactionが意味的に新しい入力を必要とするようになった
場合、既存actionは一切変更せず、新しい(再計算された)idを持つ
replacement actionを`new_actions`へ追加し、`supersessions`でどの既存
actionを置き換えるかを明示する。

schema version: `"incremental-planning-event/0.1.0"`。

```yaml
PlanningEvent:
  event_id: string        # {unix_ns}-{pid}-{counter}、既存generate_run_id()
                           # (crates/laminaria-run/src/lib.rs:379-385)と
                           # D1-aのunique_run_id()と同型の規約を踏襲
  sequence_number: u64     # 発行側(action単位)が単調増加させる番号
  planning_generation: u64 # 発行側(Rust)がこのeventを組み立てた時点で
                            # 最新と認識していたgeneration。Nim側の
                            # staleness判定入力(§4.2)
  emitted_at_unix_ns: u128
  kind: PlanningEventKind
```

```yaml
PlanningEventKind:
  - DependencyDiscovered:
      discovering_action_id: string   # 実際にこの発見を行ったaction
                                       # (例: DiscoverSourceDependencies)
      new_actions: [Action]           # **常に1個以上**(第2版改訂・fix#1:
                                       # 契約違反していた旧case2/6/10を
                                       # すべて修正)。このdeltaが
                                       # 主張する、full Action(id/kind/
                                       # command_identity/inputs/outputs/
                                       # compiler_work、§1.3・§1.8)。
                                       # 既にgraphへ同一idのActionが
                                       # 存在する場合、Nimはこのentryを
                                       # 無害な再主張として扱い、無視する
                                       # (§7.1改訂: 「同一新規Action idの
                                       # 再送」を duplicate discoveryの
                                       # 正規表現とする、§10 case2)。
      supersessions: [Supersession]   # NEW(第2版改訂、extends_inputsを
                                       # 置き換える、fix#3)。既存の
                                       # activeなAction(このdeltaより
                                       # 前からgraphに存在し、まだ
                                       # supersedeされていないもの)を
                                       # `new_actions`中の1つで置き換える
                                       # 場合のpairを列挙する。空配列は
                                       # 「new_actionsは全て新規の葉で
                                       # あり、既存Actionを置き換えない」
                                       # ことを意味する。
      new_demands: [DemandReference]  # new_actionsが存在することで初めて
                                       # 表現可能になった外部からの需要。
                                       # (§1.8のadd_or_double例: consumer-b
                                       # の本来の要求はここで初めて
                                       # 具体的なartifact_idを得る)
  - ProducerCompleted:
      artifact_id: string
      produced_by_action_id: string
  - ProducerFailed:
      artifact_id: string
      produced_by_action_id: string
      failure_reason: string
  - DemandRequested:
      artifact_id: string    # 既存の *_artifact_id 系関数の戻り値、または
                              # 既存PlanningInput.demanded_artifactsが既に
                              # 使っている呼び出し元指定文字列のいずれか
                              # (§1.6・§4.4——新しい識別スキームではない)
      requested_by: string
  - DemandCancelled:
      artifact_id: string
      requested_by: string
```

```yaml
DemandReference:
  artifact_id: string
  requested_by: string

Supersession:
  old_action_id: string   # 既にgraphに存在し、まだsupersedeされていない
                           # activeなAction。存在しない/既にsuperseded
                           # 済みのidを指す場合、このdelta全体を
                           # `Rejected{reason_kind: unsupported_input}`
                           # とする(既存5variantのうち、「入力そのものが
                           # このplanning_kernelの前提を満たさない」を
                           # 表す既存の分類に素直に収まるため、新規
                           # variantは追加しない)。
  new_action_id: string   # 同じevent中の new_actions[*].id のいずれか
```

必須field(全variant共通): `event_id`, `sequence_number`,
`planning_generation`, `emitted_at_unix_ns`, `kind`。省略可能なfieldは
存在しない(#35 D0の教訓を踏襲)。

**supersession適用時の副作用(第2版改訂、fix#3・fix#4)**:
1. `old_action_id`は`retired_because_superseded_by: new_action_id`と
   して`PlanDelta.changed_actions`へ報告され、以後のいかなるevent
   (`ProducerCompleted`/`ProducerFailed`/`DemandRequested`/
   `DemandCancelled`)からも`superseded`として扱われる——Rust runtimeは
   supersessionをPlanDeltaから知った時点で、以後`old_action_id`を
   一切実行・参照しない(実行対象を`new_action_id`へ切り替える)ことを
   前提とする契約であり、Nimはそれでも古いidを参照するeventが届いた
   場合、`stale_generation`診断(§6.4)として扱う(そのidはもはや
   このgenerationの有効な対象ではないため、新しい診断enumを追加しない)。
2. `old_action_id`に対する既存の参照カウント(§8)は、そのまま
   `new_action_id`へ**全数移送**される——supersessionは要求元
   (`requested_by`)から見て「同じものを指す名前が変わった」だけであり、
   需要そのものが消長するわけではない。
3. `old_action_id`を`inputs`で参照していた**他の既存action**がある
   場合、そのactionもまた意味的入力(=`old_action_id`という文字列)が
   変わることになり、fix#3の同じ規則により**それ自身もreplacement
   actionとして再導入されなければならない**(cascadeする)。本書の
   固定caseは、この cascade を要求する構成を意図的に避けている
   (§10 case12の注記)——T1が実装する際にcascade自体を検出・拒否する
   か、連鎖的に解決するかは、この後の反復で決定する未解決事項として
   明記する(D2以降で拡張する余地として残す、T0が今すぐ決め切る
   必要はない事項)。

---

## §4. Identity（全面改訂）

### 4.1 snapshot identity——変更なし

`source_snapshot_id`は`crates/laminaria-plan/src/compiler_work.rs:64-70`の
既存定義(`compute_source_snapshot_id`、SHA-256)をそのまま使う。

### 4.2 `planning_generation`identity——**所有者をNim plannerに変更**

- 型: `u64`、単調増加。**改訂: Nim planner(session状態)が所有・発行する。**
  初版はRust runtime側所有としていたが、session設計ではgraph状態その
  ものがNim側にあり、「あるeventが既に反映済みか」の判定にはgraph状態
  へのアクセスが必要なため、生成カウンタと停滞判定を同じ場所(Nim)に
  置くのが自然である。Rust runtimeはeventを組み立てる時点で自分が
  最後に観測した`planning_generation`をeventへ記録する(§3.4)のみで、
  カウンタの真の所有者ではない。
- 初期値: `StartSession`受理時に`0`。
- 増分規則: `ApplyDelta`が`DependencyDiscovered`を運び、かつそれが
  実際に新しい情報を含む(stale/duplicateでない)場合にのみ、Nimは
  `planning_generation`を1増やす。それ以外のeventでは増やさない
  (§2原則8)。
- 「古いevent」の判定: あるeventの`planning_generation`がNimの現在の
  generationより小さく、かつそのeventの内容が既に最新generationの
  graphへ反映済みである場合、Nimはそのeventを`stale_generation`
  診断つきの`PlanDelta`(`changed_actions: []`)として返す(§6.4)。
  反映済みでない情報を含む遅延eventは通常処理する(§7.2)。

### 4.3 event identity(冪等性の基礎)——変更なし、判定主体をNimに変更

`event_id`の文字列規約は変更しない(§3.4)。**改訂: 既知の`event_id`
集合の保持と重複判定はNim planner(session状態)が行う**——Rust runtime
内部にとどまらず、Nim自身がgraphの正本を持つので、重複適用を防ぐ
最終防衛線もそこに置く。

### 4.4 【初版の`semantic_key`を撤回】planning時点の合流キーは既存の`*_artifact_id`

**改訂: 初版のハッシュ式(`semantic_key = SHA-256(...)`、
`toolchain_digest_sha256`を常にnull固定するもの)を全文撤回する。**

`DemandRequested`/`DemandCancelled`の`artifact_id`(§3.4)は、次のいずれか
でなければならない:

1. **既に閉包が確定しているケース**(branch_readyのように、要求された
   関数の呼び出し閉包が最初から既知): `crates/laminaria-plan/src/
   compiler_work.rs`の対応する`*_artifact_id`関数
   (`lower_source_artifact_id`/`validate_ir_artifact_id`/
   `transform_function_artifact_id`/`evaluate_evidence_artifact_id`、
   265-359行)を実際の`operation_version`/`language`/
   `source_snapshot_id`/`requested_functions`/`test_inputs`等**すべて**
   から計算した、既にgraphに存在するかこれから存在するActionの`id`
   そのもの。
2. **閉包が未確定のケース**(branch_discoveringのように、発見が完了する
   まで最終artifactのidを計算できない、§1.8): その閉包が
   `DiscoverSourceDependencies`によって確定した**後**に、Rust runtime
   が(1)と同じ関数群を使って計算し、`DependencyDiscovered`の
   `new_demands`(§3.4)として初めて`DemandRequested`相当の要求を
   表明する。閉包確定前にこの要求を`artifact_id`として先に表明する
   ことはしない(表明しようにも値が計算できない)。

いずれの場合も、**新しいハッシュ関数・新しい識別子schemeを本書は
一切定義しない**——既存の、既に検証済みの関数の戻り値だけを合流キーに
使う。

---

## §5. Nim plannerとRust runtimeの所有状態・責務（全面改訂）

| 項目 | 所有者 | 根拠 |
|---|---|---|
| session全体の生存期間(`StartSession`〜`CloseSession`) | Nim planner(プロセスとして) | §2・§3 |
| 累積graph状態(discovered/blocked_dependency/ready/running/completed/failed/cancelledの全action、`inputs`/`outputs`) | Nim planner | §2原則4(初版から反転) |
| 需要閉包計算・cycle検出・ready frontier判定(`plan*`由来の既存アルゴリズムをsession向けに再利用) | Nim planner | §1.1のアルゴリズム自体は流用、呼ばれ方のみ変わる |
| `planning_generation`カウンタの発行・増分・staleness判定 | Nim planner | §4.2(初版から反転) |
| `event_id`重複集合の保持 | Nim planner | §4.3(初版から反転) |
| compiler work(`LowerSource`/`ValidateIr`/`TransformFunction`/`EvaluateEvidence`/`DiscoverSourceDependencies`)の実行 | Rust runtime | §1.8、変更なし |
| `DiscoverSourceDependencies`の実行結果から`new_actions`/`supersessions`/`new_demands`を構成し、`DependencyDiscovered`として送出する | Rust runtime | §1.8・§3.4(P1「producerを生成できない」への修正) |
| CPU budget・実行枠の割当・`cpu_slot_acquired`/`cpu_slot_released`の発行 | Rust runtime | §1.4・§9 |
| `ArtifactStore`(実行結果の実体) | Rust runtime | 変更なし |
| 合流キー(`*_artifact_id`)の計算 | Rust runtime(計算)、Nim(合流判定の実行) | §4.4——計算はRustの既存関数、合流(参照カウント判定)はNimのgraph操作 |
| 診断reason(§6.4)の最終決定。`stale_generation`はNimがgraph状態から判定(§4.2)。`cancelled_result`/`dependency_failed`はNimがgraph状態(参照カウント・blocked_on)から判定。既存`missing_producer`/`cycle`はNimの既存閉包アルゴリズムがそのまま報告 | Nim planner | §1.2・§6.4(初版から反転: すべてNim側判定に統一) |
| 外部compiler(`rustc`/`nim`)・Rust側代替plannerへのfallback | **存在しない**(§10 case11) | §11 |

**改訂の要点**: 初版はgraph状態・generation・event冪等性をすべてRust
runtime所有としていたが、これは指示が求める「Nimが増分協調の主体」
という設計と正面から矛盾していた。本書はこれらすべての所有をNim側へ
反転させ、Rust runtimeの責務を「compiler workを実際に実行すること」
「CPU/資源状態を管理すること」「実行結果からeventを構成しNimへ送る
こと」に純化した。

---

## §6. 状態遷移

### 6.1 状態(7種、#36本文の6状態+`cancelled`)——変更なし、ただし**観測点を分離する(新設)**

`discovered` → `blocked_dependency` → `ready` → `running` →
`completed` | `failed` | `cancelled`

**新設の明確化**: Nim planner自身がgraph操作として追跡する状態は
`discovered`/`blocked_dependency`/`ready`/`completed`/`failed`/`cancelled`
の6種のみである——需要閉包・cycle検出・ready frontier判定(§5)は
「あるactionが実行中かどうか」に一切依存しないため、Nimにとって
`running`は意味を持たない。`running`は**Rust runtimeが`ready`な
actionを実際に実行している間だけ付与する、Rust側の実行statusの
overlay**であり(§5「CPU/resource状態」の一部)、`ApplyDelta`/`PlanDelta`
のいずれの`ActionStateChange.to_state`にも現れない。`cases.yaml`の
`expected_graph`はNimのgraph状態とRustの実行status overlayを合成した、
テスト観測可能な全体像として記述する(§10)。`ready→running`
(Rustのスケジューラ判断)と`running→completed`/`failed`(Rust自身の
実行結果、続けて`ProducerCompleted`/`ProducerFailed`をNimへ送出する
契機)は、したがってNimへのeventそのものではなく、Rust内部の実行
statusの変化として起こる——Nimへ伝わるのはその**結果**
(`ProducerCompleted`/`ProducerFailed`)だけである。

### 6.2 遷移表(所有者の反転を反映、遷移条件自体は初版から実質不変)

| from | to | 条件 | 一度だけ性 |
|---|---|---|---|
| (初期) | `discovered` | `StartSession.initial_graph`または`DependencyDiscovered.new_actions`により、Nimのgraphへ初めて挿入される | — |
| `discovered` | `ready` | 宣言済み`inputs`の全Declared artifactが既に`completed`(またはSourceで解決済み) | — |
| `discovered` | `blocked_dependency` | 宣言済み`inputs`のうち少なくとも1つが未完成 | — |
| `blocked_dependency` | `ready` | blocking集合が空になる(最後の1つが`ProducerCompleted`) | **一度だけ**(#36 case2)。複数producerが同時に完了しても、blocking集合が空になった瞬間の1回のみ遷移する |
| (discovered\|blocked_dependency\|ready) | (状態は変えず) | 同一idの`new_actions`エントリが重複して主張される(§7.1・§10 case2) | 冪等(無視) |
| いずれか(active) | `superseded`(=`to_state: null`、§3.2) | `supersessions`で`old_action_id`として名指しされる(§3.4・§10 case12) | 一度だけ。参照カウントは`new_action_id`へ全数移送(§8) |
| `ready` | `running` | Rust runtimeがCPU budgetの空き枠をこのactionに割り当て、`cpu_slot_acquired`を発行する(§9) | — |
| `running` | `completed` | 実行成功。Rust runtimeが`ProducerCompleted`を送出し、同時に`cpu_slot_released`を発行する | — |
| `running` | `failed` | 実行失敗。`ProducerFailed`送出+`cpu_slot_released` | — |
| `running` | `completed`だが公開しない(`cancelled_result`診断、§6.4) | 実行完了時点で、このactionへの参照カウント(§8)が既に0(§10 case7: 参照カウント0への到達自体は、readyの間に届いた`DemandCancelled`で先に`cancelled`へ遷移させ、その後に届く遅延`ProducerCompleted`をこの行で扱う) | §6.3参照 |
| (discovered\|blocked_dependency\|ready) | `cancelled` | 参照カウントが0に達する(§8) | — |
| いずれか | `cancelled`(直接) | 全ての`DemandRequested`が対応する`DemandCancelled`で相殺され、かつ`running`未満の状態 | — |

`blocked_dependency`中は`running`ではないため、その間`cpu_slot_acquired`
は一度も発行されない——これが§1.4の「待機＝slot占有」からの脱却を
`running`状態とslot occupancyの1:1対応として直接保証する(§9の
不変条件)。

### 6.3 `running`中の取消は非プリエンプション型——変更なし

現行`compiler_work_executor.rs`にプリエンプション機構は存在しない
(§1.4)。`running`状態のactionへの取消要求は実行を中断させず、完了時点で
参照カウントを再確認する:
- 参照カウント > 0: 通常通り`completed`/`failed`として公開する(§8 case5)。
- 参照カウント == 0: `ArtifactStore`へは記録するが公開しない
  (`cancelled_result`診断、§6.4)。

### 6.4 診断reason(既存`RejectionReasonKind`5種とは別enum、判定主体はすべてNim)

```yaml
IncrementalDiagnosticReason:
  - stale_generation       # §4.2、Nimが判定
  - cancelled_result        # §6.3、Nimが参照カウントとgraph状態から判定
  - dependency_failed       # blocking producerがfailedで終わった場合、
                             # blocked_dependency中のactionが取るべき終端状態、
                             # Nimが判定
```

既存の`missing_producer`/`cycle`は、session下でもNim planner自身が
`ApplyDelta`処理の一部として`Rejected`で直接報告する(§5表)。

---

## §7. 冪等規則（重複・遅延・順序入替event、判定主体はすべてNim）

### 7.1 重複(duplicate)——**2つの異なる形を区別する(第2版改訂、fix#6)**

初版・第1改訂は「重複」を1種類として扱っていたが、実際には性質の
異なる2つの現象であり、本改訂は明示的に分離する:

1. **完全重複(同一`event_id`の再送)**: 同一`event_id`を持つeventの
   2回目以降の到着は、Nimが保持する既知`event_id`集合と照合し、無視
   する(`PlanDelta`、`changed_actions: []`、`diagnostic: null`——診断
   なしの無害な無視。§4.3)。at-least-once配送で同じevent(payloadも
   同一)が物理的に2回届くケース(§10 case4)。
2. **内容的な重複(別`event_id`・同一の意味内容)**: `event_id`は
   異なる(例: 独立した2回のretryがそれぞれ新しい`event_id`を発行した)
   が、結果として同じ状態遷移を主張するevent。この場合`event_id`集合
   での重複検出には引っかからないため、**状態遷移そのものが内容ベース
   で冪等でなければならない**(§7.3、§10 case13)——例えば既に
   `completed`のactionへ2回目の`ProducerCompleted`が(別`event_id`で)
   届いても、状態は`completed`のまま変化せず、エラーにもならない。
   `DependencyDiscovered`についても、既にgraphに存在するidと同じ
   `new_actions`エントリの再主張は無害に無視する(§6.2改訂の新しい行、
   §10 case2)。

この2つを同じ「重複」として扱い、`event_id`ベースの検出だけで
両方カバーできると誤って想定したことが、第1改訂のcase4の欠陥だった。

### 7.2 遅延(delayed)

`planning_generation`が現在の最新世代より古いeventのうち、その内容が
既に最新世代の状態に反映済みのものを`stale_generation`として返す
(§4.2)。反映済みでない情報を含む遅延eventは通常のeventとして処理する。

### 7.3 順序入替(reordered)

`sequence_number`は発行側(action単位)が単調増加させるため、同一
actionに対する2つの`ProducerCompleted`/`DependencyDiscovered`eventが
逆順で到着しても、Nimは`sequence_number`の低い方を「既に上書きされた」
と判定できる。状態遷移そのもの(§6.2)は内容ベースで冪等になるよう
設計している(`blocked_dependency → blocked_dependency`はblocking
集合への追加という集合演算であり、到着順序に依存しない)。

---

## §8. 合流・共有producer取消規則（`semantic_key`を撤回し既存identityへ統一）

- 同一`artifact_id`(§4.4——既存`*_artifact_id`関数の戻り値、または
  閉包確定後にRustが計算する値)を持つ2つ以上の`DemandRequested`は、
  **1つの**action/実行へ合流する。2番目以降は、既に`discovered`以降の
  状態にあるactionへの参照カウント+1として扱われ、新しいactionを
  作らない。
- 参照カウントは`DemandRequested`ごとに+1、対応する`DemandCancelled`
  ごとに-1。0に達したときのみ§6.2の取消遷移が発生する(case5)。
- `blocked_dependency`中のactionが取消された場合、そのactionが依存
  していたproducerへの参照カウントも連動して-1する(推移的取消)。
  他のconsumerから要求されていれば(参照カウント>0)生存する。
- **NEW(第2版改訂、fix#4)**: session開始時点の初期需要は
  `StartSession.initial_demands`(§3.1)から構成する——`demanded_artifacts`
  (既存field、文字列のみ)だけでは「誰が」要求しているかを表現でき
  ず、複数consumerによる初期参照カウントを正当化できなかった
  (第1改訂case5の欠陥)。
- **NEW(第2版改訂、fix#3)**: `supersessions`(§3.4)により
  `old_action_id`が退役するとき、`old_action_id`に対して保持されて
  いた参照カウントは**全数**`new_action_id`へ移送される——
  supersessionは要求元から見て「指す名前が変わっただけ」であり、
  需要そのものの消長ではない。

---

## §9. 計測境界（`cpu_slot_acquired`/`cpu_slot_released`を追加）

| 指標 | 開始 | 終了 | 備考 |
|---|---|---|---|
| `dynamic_expansion_count` | session開始(`planning_generation=0`) | `CloseSession` | `DependencyDiscovered`を受理し`planning_generation`を実際に増分した回数。stale/duplicateはカウントしない |
| IPC時間 | `ApplyDelta`をNimへ送出する直前 | 対応する`PlanDelta`/`Rejected`を受領した直後 | 1 event単位で計測する。初版の「1回のround-trip全体」という定義は、単位が「document全体」から「1 event」に変わった以外は同じ |
| dependency wait | actionが`blocked_dependency`へ遷移した時刻 | 同actionが`ready`へ遷移した時刻 | action単位 |
| time-to-first-useful-work | session開始 | 最初のactionが`running`へ遷移した時刻 | case1の根拠 |
| **`cpu_slot_acquired`**(新設) | Rust runtimeがCPU budgetの1枠をあるactionに割り当てた瞬間 | — | `running`遷移と同時に発行(§6.2) |
| **`cpu_slot_released`**(新設) | — | 同actionの実行が終わり(`completed`/`failed`/`cancelled_result`確定)、slotが解放された瞬間 | `running`脱出と同時に発行 |

**不変条件(新設、全caseで検証可能な形で固定する)**: 任意の時刻`t`について
`held_slots(t) == |{a : state(a, t) == running}|`が常に成立する——
`blocked_dependency`状態のactionは`held_slots`に一切寄与しない
(§1.4からの脱却を、間接指標ではなく直接のtrace事象の集計として証明
する、指示への対応)。

---

## §10. 固定する最低限6種類→13 case（fixture修正・schema完全準拠・否定case+supersession+内容的冪等caseを追加）

### 10.1 共有fixture(修正版)

```yaml
fixtures:
  branch_ready:
    rust_source: "fn f(x: i32) -> i32 { x }"
    nim_source: "proc g(x: int32): int32 =\n  x\n"
    expected_rust_result: 5
    expected_nim_result: 5
    test_input: 5
    # 閉包は最初から既知("f"はどの関数も呼ばない) -- discoveryを経ない。
  branch_discovering:
    rust_source_file: "fixtures/laminaria-semantic-substrate-prototype/rust-src/add_or_double.rs"
    initially_requested_functions: ["add_or_double"]   # 閉包未確定のまま開始(§1.8)
    discovered_closure: ["add_or_double", "double"]     # DiscoverSourceDependenciesが
                                                          # 確定させる、1回のLowerSourceに
                                                          # 渡す閉じた集合(§1.8で修正)
    test_input: { a: 3, b: 4, use_double: 1 }
    expected_result: 6   # D0確定済み(issue #35)、crates/laminaria-ir/src/rust_frontend.rs:830-835
```

**改訂: `branch_discovering`は「`add_or_double`だけを先にlowerする」
という前提を廃し、`DiscoverSourceDependencies`が完了して初めて
`add_or_double`/`double`両方を含む1回の`LowerSource`が作られる、という
§1.8の設計に合わせて書き直した。**

**新設(第2版改訂、case12用)**: `branch_ready`の`f`を編集し、
`f_helper`という新規関数を呼ぶようになった版
(`fixtures.branch_ready_edited_for_case12`、`docs/design/issue-36-t0-
cases.yaml`)を、source-edit-triggered supersessionの実演(§3.4)専用に
追加した。

### 10.2 case一覧(第2版改訂: 全13case、`StartSession`単独有効性を全case
で満たし、診断対象の不正状態は必ず`ApplyDelta`で導入する、fix#5)

1. `T0-incremental-ready-branch-proceeds-during-discovery` — 別枝の依存
   発見が継続中に、依存が閉じたready枝が実compiler workを開始する。
   `StartSession`は`AID_LOWER_F`(ready)と`AID_DISCOVER_ADD`(ready)の
   みを持つ、単独で有効な graph。
2. `T0-incremental-duplicate-discovery-single-resume`(第2版改訂で改称。
   旧`late-dependency-single-resume`) — 既に`new_actions`として存在
   するActionと同一idの再主張(=duplicate discovery、fix#1・fix#6)が
   状態を変化させず、その後の本物の`ProducerCompleted`が
   blocked→readyを一度だけ起こす。
3. `T0-incremental-semantic-key-merge` — 同一artifact_id(既存
   `lower_source_artifact_id`の戻り値)への同時要求が一計算へ合流する。
   (caseの識別名は据え置くが、内容は`semantic_key`ではなく既存
   `*_artifact_id`を使う、§4.4改訂)
4. `T0-incremental-event-reorder-duplicate-delay-stable-result` —
   **完全重複(同一event_id・同一payloadの再送)**と順序入替が最終
   graphと公開結果を変化させない(第2版改訂: 真の重複に修正、fix#6。
   別event_idを持つ内容的重複はcase13へ分離)。
5. `T0-incremental-cancel-survives-shared-producer` — 一consumerの
   取消後も共有producerが生存する。`StartSession.initial_demands`で
   consumer-a/consumer-bの2件を明示し、そこから参照カウント2を
   導出する(第2版改訂、fix#4)。
6. `T0-incremental-diagnostic-stale-generation` — 古いgenerationの
   eventをNimが拒否する。`StartSession`は空に近い有効な graph、
   `ApplyDelta`でまず本物の発見によりgeneration 0→1へ正当に進め、
   その後にgeneration 0を名乗る(既に反映済みの内容の)eventを送って
   初めてstaleを再現する(第2版改訂: 実際にstaleにした、fix#2)。
7. `T0-incremental-diagnostic-cancelled-result` — 取消後に完了した
   結果の診断。`StartSession`は有効なdemand付きready状態から開始し、
   `DemandCancelled`(→Nim視点でready→cancelled)の**後**に遅れて届く
   `ProducerCompleted`が診断対象を発生させる(第2版改訂: 診断対象を
   `ApplyDelta`側へ移した、fix#5)。
8. `T0-incremental-diagnostic-missing-producer` — 増分session内での
   missing producer。`StartSession`は`AID_DISCOVER_ADD`のみの有効な
   graphから開始し、`ApplyDelta`のdiscoveryが自己矛盾した
   (存在しないproducerに依存する)actionを持ち込む(第2版改訂、fix#5)。
9. `T0-incremental-diagnostic-dependency-failed` — blocking producer
   の失敗によるblocked_dependency actionの終端診断。`branch_ready`
   (`AID_LOWER_F`/`AID_VALIDATE_F`)を使い、discovery未解決の閉包に
   依存しない、単独で有効な初期状態に統一(第2版改訂の整合性強化)。
10. `T0-incremental-diagnostic-cycle` — discoveryが導入する**2つの
    新規action同士が互いに依存し合う**ことで、既存graphに触れずに
    自己完結したcycleを構成する場合の診断(第2版改訂: 既存action自体
    を書き換えてcycleを作る、というextends_inputs時代の設計をやめ、
    supersessionの複雑なcascadeを要求しない構成にした)。abstract
    action(実compiler-work kindに紐付かない、Nim自身のcycle検出
    アルゴリズムを単体で検証するための構造的case)であることを明示
    する。
11. `T0-incremental-no-fallback-on-rejection` — `Rejected`
    (missing_producer)を受けた後、Rust runtimeが外部compiler
    (`rustc`/`nim`)を一切起動せず、Rust側の代替planningロジックにも
    フォールバックせず、graphは直前の有効なgeneration状態のまま
    変化しないことを固定する(§11)。`StartSession`はcase8と同型の
    単独有効なgraphから開始する(第2版改訂、fix#5)。
12. **`T0-incremental-supersession-replaces-superseded-action`(新設、
    fix#3の直接の実例)** — `branch_ready`の`AID_LOWER_F`がまだ
    `ready`(何もconsumeしていない)段階で、discoveryにより`f`が実は
    別関数`f_helper`も呼ぶと判明し、閉じた集合`["f","f_helper"]`を
    持つ`AID_LOWER_F_V2`が`supersedes_action_id: AID_LOWER_F`として
    導入される。`AID_LOWER_F`は`retired_because_superseded_by:
    AID_LOWER_F_V2`として退役し、`AID_LOWER_F`に対する参照カウント
    (もしあれば)は全数`AID_LOWER_F_V2`へ移送される。既存consumerに
    よるcascade(§3.4 supersession適用時の副作用3)は本caseの対象外
    ——意図的に、まだ何もconsumeしていない段階で置換することで
    cascadeを要求しない最小構成にしている(§3.4に明記した未解決事項)。
13. **`T0-incremental-content-idempotent-different-event-id`(新設、
    fix#6で「別サブcaseに分離」と指示された内容的重複)** — 独立した
    2回のretryがそれぞれ**別の`event_id`**を発行して同じ
    `ProducerCompleted`を主張しても(`event_id`ベースの重複検出には
    引っかからない)、既に`completed`のactionへの2回目の適用は状態を
    変化させない——冪等性は`event_id`の一致にではなく、状態遷移
    そのものの内容(§7.3)に宿ることを示す。

全caseは`crates/laminaria-ir`由来の実IR評価(`LowerSource`→
`ValidateIr`→`EvaluateEvidence`、および新設`DiscoverSourceDependencies`)
を使う固定caseとして`docs/design/issue-36-t0-cases.yaml`に完全記述する
(event_id・sequence_number・planning_generation・emitted_at_unix_ns
を含む宣言schema完全準拠、`pass_criteria.t1`表記、`command_identity`/
`compiler_work`を含む完全なActionと、実際の`compute_artifact_id`
(FNV-1a)から計算した具体的なartifact idを持つcatalog、fix#7)。
case10のみ、Nim自身のcycle検出アルゴリズムを単体で検証する目的から
abstract actionを使う——それ以外の全caseは実際のcompiler-work kind
(`LowerSource`/`ValidateIr`/`EvaluateEvidence`/`DiscoverSourceDependencies`)
を使う。

---

## §11. 明示的除外事項

- sleep-onlyの疑似workload。
- 全action完了ごとの全体再計画(§2原則8で明示的に拒否)。
- D3の資源最適化。
- WASM生成(#5 T0/T1の対象、本書と混在させない)。
- 速度向上判定(D4の対象)。
- T0自体はコードを実装しない。
- **外部source compilerまたはRust側代替plannerへのfallback**——§10
  case11で明示的に否定試験する(初版は「境界条件として明記するのみ」
  だったが、指示によりT0自身が固定case化する)。

---

## §12. 採否欄

- 指示者: (未記入)
- 採否日: (未記入)
- 判定: (未記入)
- 判定revision: (未記入)
- 備考: (未記入)

# セルフビルド: stage0 → stage1 (issue #8・#6・#4 の最初のスライス)

本ドキュメントは、`docs/research-foundations.md` 第7節および Phase 3/Phase 7
のロードマップ項目(「`PlanningInput` と `ExecutionPlan` を安定化させる」
「LAMINARIA自身でRustホストとNimプランニングカーネルをビルドする」)で
名指しされていた `plan(PlanningInput) -> ExecutionPlan` 契約の、具体的かつ
実装済みのプロトコルを記述する。この作業以前、両者はコードとして一切
存在しなかった。本スライスは issue #8(Nim Planning Kernel)・issue #6
(Rust Action Graph/スケジューラ)・issue #4(Rust↔Nim統合境界)が共同で
最初に実装する対象である。

## 本スライスのスコープ

- fixtures外の本番Nimパッケージ(`nim-planner/`)による
  `plan(PlanningInput) -> ExecutionPlan` の実装。
- Rustホストが計画を得る唯一の経路である本番Rustクレート
  (`crates/laminaria-plan`)。本コードベースのどこにもRust側で代替計画を
  作る経路は存在しない。
- その計画を使って、LAMINARIA自身のRustホストとNimプランナーをソースから
  実際にビルドし、世代(generation)ルートへ組み立てる `laminaria
  self-build`。
- ローカル・逐次実行のみ。キャッシュ再利用(issue #7/#12)なし、分散実行
  なし、stage1→stage2の自己再ビルドなし — いずれも今後の作業として
  明示的に除外する。

## 発明ではなく実証済みの先行事例に基づく設計

`docs/project-proposal.md` 第7節/第11節は、まさにこのAction
Graph/プランナー問題に対する4つの参照プロジェクトを名指ししている:
**Buck2**・**Bazel**・**Pants**・**Nx**。この4つはすべて `.reference/`
へシャロークローンし(`.reference/README.md` 参照)、実際のソースを読んだ
うえで設計している。具体的には:

- **依存エッジはアーティファクト媒介であり、手書きの`depends_on`リスト
  ではない。** Buck2の`BuildArtifact`は、それを生成したアクションの
  `ActionKey`を保持する(`.reference/buck2`の
  `app/buck2_artifact/src/artifact/build_artifact.rs`)。アクションの
  入力は`ArtifactGroup`であり(`app/buck2_build_api/src/
  artifact_groups.rs`)、ソースアーティファクト(葉)か他アクションの
  出力のいずれかに解決される。本プロジェクトの`Action`
  (`crates/laminaria-plan/src/types.rs`、`nim-planner/src/contract.nim`)
  も`inputs`/`outputs`のみを`ArtifactRef`として宣言し、Nimカーネル自身が
  入力を宣言済み出力と突き合わせて依存グラフを導出する。
- **サイクル検出は明示的なパス追跡DFS**であり、Bazelの
  `SimpleCycleDetector`(`.reference/bazel`の
  `skyframe/SimpleCycleDetector.java`)とNxの`findCycle`/`_findCycle`
  (`.reference/nx`の
  `packages/nx/src/tasks-runner/task-graph-utils.ts`)から移植した。
  どちらも明示的なパスリストを保持し、既にそのパス上にあるノードが
  再訪された瞬間に正確なサイクルを報告する。Buck2のDiceエンジンは
  代わりにKosaraju SCCベースのサイクル終端処理を使うが
  (`InnerGraph::terminate_cycles`)、これはDiceのグラフが巨大かつ
  実行中ノードが並行して変化し続けるために存在するものであり、単一の
  小さな事前計算済み`ExecutionPlan`には適合しない。
- **決定的順序付けはレキシコグラフィックタイブレーク付きのKahn法**で
  あり、Nxの`walkTaskGraph`(上記と同ファイル)から直接移植した:
  導出したエッジから入次数を計算し、入次数0のアクションのうち
  辞書式順序で最小のIDを繰り返し取り出し、依存先の入次数を減らしていく。
  これにより「同一入力→同一出力」(issue #8自身の受け入れ基準)が
  ハッシュマップの走査順序による偶然ではなく構造的に保証される —
  Pantsが`rule_graph`クレートで`BTreeSet`/`BTreeMap`/`IndexSet`を
  生ハッシュコレクションの代わりに徹底して使うことで同じ決定性の
  懸念を解決しているのと同じ発想である(`.reference/pants`の
  `src/rust/rule_graph/src/builder.rs`、`rules.rs:14`)。
- **需要駆動(pull-based)なスケジューリングは採用しない** — これは
  Buck2・Bazelの実際の実行モデルからの意図的な乖離である。Buck2の
  アクション実行には明示的なスケジューラが存在せず、Diceインクリメンタル
  計算エンジンによってメモ化された再帰的な非同期呼び出しである
  (`app/buck2_build_api/src/actions/calculation.rs`の
  `ActionCalculation::build_action`)。BazelのSkyframeも同じ形で、
  `SkyFunction`は依存先がまだ準備できていない場合`null`を返し、準備が
  整った時点で再開される(`skyframe/SkyFunction.java`)。どちらも巨大で
  長寿命かつインクリメンタルにキャッシュされるグラフ向けに最適化された
  ものである。本スライスにはキャッシュが一切なく、プロセス境界を跨ぐ
  単一のNimサブプロセス呼び出しで一度だけ生成される固定の小さな
  アクション集合しかない — 完全な文書を一度に返す必要のあるIPC契約に
  需要駆動のpullモデルは適合しない。代わりに完全な順序を一度だけ計算し
  (前項)、逐次実行する。
- **構造化された却下理由はフォーマット済み文字列ではなくタグ付きenum**
  であり、Buck2のプロジェクト全体に及ぶ`buck2_error`パターン
  (`#[derive(buck2_error::Error)]` + `#[buck2(tag = ...)]`、例:
  `app/buck2_configured/src/cycle.rs`の
  `ConfiguredGraphCycleError { cycle: Arc<Vec<...>> }`)と、Pantsの
  内部`NodePrunedReason`/`EdgePrunedReason`enum
  (`src/rust/rule_graph/src/builder.rs:194-207`、テキストへ変換される
  「前に」なぜそのノード/エッジが却下されたかを名指しする)を手本にした。
- **`plan_id`は正直に言えばLAMINARIA固有の追加であり、借用した慣習では
  ない。** 4つの参照プロジェクトのいずれも、単一のグラフ全体ダイジェスト
  を主たるノード識別子として保持していない: Pantsは構造的な
  `Eq`+`Hash`+interningに依存し`rule_graph`に`Digest`/`Fingerprint`型は
  一切ない。Bazelの`Artifact`識別子はパス+所有者ベースで、コンテンツ
  ダイジェストは変更検出用の別の値である。Buck2は各*アクション*ごとに
  リモート実行用の`ActionDigest`を持つが、`ActionGraph`全体に対する単一
  ダイジェストは存在しない。Nxの`Task.id`は単なる`project:target`文字列
  であり、`Task.hash`はタスクごとの別のキャッシュ用の値である。`plan_id`
  が存在するのは、issue #6が`laminaria-run`の`Run`記録における系譜/証拠
  用の「plan ID」の記録を求めているからに過ぎない — 正規化された
  `PlanningInput`の構造的な(暗号学的ではない)ハッシュであり
  (`nim-planner/src/planning_kernel.nim`の`computePlanId`)、その証拠
  目的には十分だが、キャッシュ/セキュリティ級のコンテンツアドレスでは
  ない。
- **将来のクリティカルパス計算はBuck2の実アルゴリズムを再利用すべき**
  であり、これもまた後で発明すべきではない:
  `app/buck2_critical_path/src/potential.rs`の
  `compute_critical_path_potentials`(トポロジカル順序での動的計画法、
  `cost[v] = weight[v] + max(cost[dep])`をグラフとその逆グラフの両方で
  一度ずつ実行)が、クリティカルパス解析が実際に実装される際の具体的な
  参照先である — 本スライスの範囲外。

## 契約

`PlanningInput -> ExecutionPlan`、バージョン管理あり(現在
`schema_version = "0.1.0"`)。Rustが正規の型定義を所有し
(`crates/laminaria-plan/src/types.rs`)、Nimは手動でそれを鏡写しする
(`nim-planner/src/contract.nim`)。共有スキーマ生成ツールは存在しない
ため、すべてのJSONキーはRustのserdeデフォルト出力に一致するよう選んだ
リテラルなsnake_case文字列である。両側の整合性は双方のテストで保たれる:
`nim-planner/`配下の`nimble test`、そして`cargo test -p
laminaria-plan`の、実際のNimバイナリの標準出力をそのままキャプチャした
(このクレート自身が再シリアライズした値ではない)リテラルをデシリアライズ
するテスト。

- `ArtifactRef`: `{"kind": "source", "path": "..."}` (外部の既存入力) か
  `{"kind": "declared", "artifact_id": "..."}` (同一入力内のいずれかの
  アクションが出力として宣言しなければならないID)。
- `Action`: `id`、`kind`(本スライスでは`nim_build` | `cargo_build` |
  `integrate`)、`command_identity`(論理的な説明であり、必ずしも実際に
  実行されるargvそのものではない)、`inputs`、`outputs`。
- `ExecutionPlan`(成功時): `schema_version`、`produced_by`(本物のNim
  産出計画では常に`"laminaria-nim-planning-kernel"` —
  `laminaria_plan::validate`がこれを検証するため、実際にNimバイナリから
  来ていない計画が黙って受理されることはない)、`producer_version`、
  `plan_id`、`ordered_actions`、`actions`。
- `PlanRejection`(有効かつ完結した回答であり、クラッシュではない):
  `reason_kind`(`cycle` | `unsupported_input` | `missing_producer` |
  `duplicate_producer` | `invalid_contract_version`)、`reason_detail`、
  `cycle_path`(`cycle`の場合のみ設定され、`a -> b -> c -> a`のように
  描画される)。
- `laminaria-planner`の標準出力からのトップレベルの回答は
  `{"outcome": "planned"|"rejected", "data": ...}`。

## Rust↔Nim境界: サブプロセス+JSON、FFIではない

issue #4は、より広いABIフリー研究課題を確定させることなく「明示的で
検証済みのアダプタ」を固定されたブートストラップ経路として許容している。
本スライスはサブプロセス境界を採用する: Rustがコンパイル済みの
`laminaria-planner` Nim実行ファイルを起動し、標準入力へ`PlanningInput`
のJSONを書き込み、標準出力から`ExecutionPlan`/却下のJSONを読み取る
(`crates/laminaria-plan/src/nim_planner_client.rs`)。これは意図的に
「直接的」あるいは「ABIフリー」とは主張しない — ネイティブFFIリンクが
強制するNimのARC/ORCランタイムライフサイクルおよびpanic/例外の巻き戻し
境界に関する問題を回避しつつ、issue #8の要件「プランナーはプロセス起動・
環境調査・ファイルシステム・ネットワークへの副作用を一切行わない」
(Nimバイナリは標準入力の読み取り/標準出力の書き込みのみを行う)に
合致する。`laminaria-planner`は成功した計画・整った形式の却下の*どちらも*
終了コード0で終了する。解析不能な入力の場合のみ非ゼロで終了する
(`nim-planner/src/laminaria_planner.nim`)。

## 構成

- `nim-planner/` — Nimパッケージ。`src/contract.nim`(鏡写しされた契約)、
  `src/planning_kernel.nim`(`plan`/`planFromJson`)、
  `src/laminaria_planner.nim`(`laminaria-planner`バイナリのエントリ
  ポイント)、`tests/test_planning_kernel.nim`(`nimble test`)。
- `crates/laminaria-plan/` — `types.rs`(契約のRust側の正典)、
  `nim_planner_client.rs`(サブプロセスクライアント — このクレート内で
  `PlanOutcome`を得る*唯一*の経路であり、プランナーバイナリが見つからない
  /失敗した場合は常に`Err`であり、代替計画になることは決してない)、
  `validate.rs`(2つの異なることを検証する: 元の`PlanningInput`との対応
  — 計画は要求されたアクションを要求された形状のまま過不足なく宣言し、
  要求された成果物は実際に生成されること — に加えて内部的な順序整合性。
  外部レビューが発見した実バグ: 後者だけの検証では、故障注入した空の
  `ExecutionPlan`(`{"actions": {}, "ordered_actions": []}`)が「空集合は
  自明に自分自身と等しい」ために検証を通過し、何もビルドせずに
  「成功」してしまっていた)。
- `crates/laminaria-run/src/self_build.rs` — セルフビルドの
  `PlanningInput`(3つのアクション:`compile-nim-planner`・
  `compile-rust-host`・`integrate`)を組み立て、プランナーを呼び出し、
  検証し、lockファイルからRust/Nimツールチェーンを解決・検証し
  (lockが読めない、あるいはツールチェーンが解決できない場合はPATH上の
  `cargo`/`nim`を黙って実行せず、そこで失敗する)、`ordered_actions`を
  厳密に逐次実行する(並行度の上限は1、
  明示的に保守的な選択 — issue #6は最初のローカルスライスにこれを許容
  している。ネストされたCargo/`nim c`の並列性は各ツール自身のデフォルト
  のままとし、各アクションの`Run`証拠内で隠さず明記する)。
  `nim_build`/`cargo_build`アクションは、このクレート内の他の全ての
  トレース済みコマンドが既に使っているのと同じ`run_and_record`の
  RUSTCラッパー/CCラッパートレーサ経路を通る — 本スライスの粗粒度
  (「`cargo build`丸ごと」)アクションであっても、単なる
  `Command::output()`ではなく実際のコンパイラ呼び出しごとの証拠に
  裏付けられている。`integrate`はコンパイラ呼び出しではなくファイル
  システムの組み立てであるため、トレースされた`Run`は生成しない —
  それを模擬的な`Run`でラップすることは、コンパイラ証拠ではないものを
  そう見せかけることになる。各アクションの`Run`は、再永続化される前に
  `plan_id`・プランナーバイナリ自身の解決済みダイジェスト・`generation`
  系譜ラベルでパッチされる(issue #6:「起動したビルドドライバの識別子・
  プランナーの識別子・plan ID・アクション結果・世代系譜を記録する」)。
  失敗したアクションはそれに依存するすべてを中断する。不完全な世代が
  成功として返されることは決してない。各世代は自身専用の隔離された
  `<generation_root>/.build/`ステージング領域(専用のCargo
  `--target-dir`とNimの`--nimcache`、`run_generation`呼び出しごとに
  消去される)にビルドする — 外部レビューが発見した実バグ: 以前の版は
  `repo_root`自身の共有`target/`/`nim-planner/bin/`を世代間で共有して
  おり、2つ目の世代のビルドが1つ目の世代の既にfreshな成果物を横取り
  して、実際には何も再コンパイルせずに成功と報告していた。
- `crates/laminaria-cli` — `laminaria plan-self-build`(計画のみ、実行
  なし)と`laminaria self-build --generation-root <dir>`(計画+実行)。
  どちらもデフォルトでは実行中の実行ファイルの隣にある`laminaria-planner`
  を解決する(`--planner`で上書き可能)。見つからない場合の代替経路は
  ない。

## stage0 → stage1 プロトコル

**stage0**は、外部ツールによって作られる、計画立てられていない通常の
ビルドである — 文字通り`nim-planner/`に対する`nim c`(`nimble build`
ではない。`nimble build`は本物のコンパイルエラーでも終了コード0を
返すこと、さらにUbuntuのAPT版nim/nimbleでは依存解決自体が失敗する
ことが本作業中に判明した)と、リポジトリルートでの`cargo build
--workspace --release`を、手動またはCIで、`laminaria self-build`を
一切経由せずに
実行するだけである。これはブートストラップの種であり、従来のコンパイラ
ブートストラップにおける「前バージョンのコンパイラ」と同じ役割を担う。

**stage1**は、stage0自身の`laminaria`バイナリを実行することで生成される:

```bash
# stage0: 外部ツールによるビルド(本プロジェクト自身のCLIは経由しない):
cd nim-planner && nim c --path:src -o:bin/laminaria-planner src/laminaria_planner.nim && cd ..
cargo build --workspace --release

# stage1: 実際のplan+executeパイプラインを通じて生成される。
# stage0自身のプランナーがそれを計画する:
./target/release/laminaria self-build \
  --planner nim-planner/bin/laminaria-planner \
  --generation-root target/laminaria-gen/stage1 \
  --generation-label stage0-to-stage1
```

(`--planner`が必要なのは、`target/release/laminaria`自身の隣には
兄弟の`laminaria-planner`が存在しないためである — 存在するのは
`nim-planner/bin/`だけである。`--planner`はstage0の`laminaria`に
stage0自身のプランナーを指し示す役割を果たす。`integrate`によって
生成されたstage1自身の`laminaria`バイナリには兄弟プランナーが実際に
存在するため、このフラグは不要である — 下記参照。)

`self-build`はセルフビルド用の`PlanningInput`を組み立て、stage0自身の
`laminaria-planner`バイナリ(実行中の`laminaria`実行ファイルの隣に
解決される)を呼び出し、返された`ExecutionPlan`を検証し、それを実行する:
`compile-nim-planner`(`nim c`を直接呼ぶ — `nimble build`ではない。
本作業中に、`nimble build`は本物のNimコンパイルエラーが発生しても
ビルド失敗のメッセージを表示しつつ終了コード0を返すことが判明した。
`nim c`自身の終了コードは成功/失敗を正しく反映し、直接呼ぶことで
このクレート自身の`is_nim_c_command`チェックにも認識されるため、
`nimble build`内部の`nim c`呼び出しでは隠されてしまうはずの本物の
CCラッパートレースも得られる)、`compile-rust-host`(`cargo build
--workspace --release`)、`integrate`(両方のバイナリと
`laminaria-rustc-wrapper`/`laminaria-cc-wrapper`を
`target/laminaria-gen/stage1/`へコピーし、すべての兄弟バイナリ探索が
実行中のバイナリの隣で必要なものを見つけられるようにする)。

**stage1のプランナーが実際に動作することの証明**(リンクされているだけで
未使用ではないことの証明): stage1自身の`laminaria`バイナリは、フラグ
なしで実行しても自身の隣にある`laminaria-planner`を見つけ、計画に成功
する:

```bash
target/laminaria-gen/stage1/laminaria plan-self-build --json
```

これは手動の手順としてだけでなく自動テストとしても検証されている:
`crates/laminaria-run/src/self_build.rs`の
`stage0_produces_a_stage1_whose_own_planner_actually_works`は、実際の
stage0を構築し、実際の`run_generation`を実行してstage1を生成し、その後
stage1自身の新しくビルドされたプランナーバイナリを独立に呼び出して、
`produced_by == "laminaria-nim-planning-kernel"`を持つ整った
`ExecutionPlan`が返ることを検証する。

## LAMINARIAを使うこと vs. LAMINARIA自身をビルドすること

`self-build`は**LAMINARIA自身**——その自前のRustホストとNimプランナー——
をソースから計画・ビルドするものであり、これは両ツールチェーンを正当に
常に必要とする。これは*ユーザーの*対象プロジェクトをビルドするための
汎用インターフェースではない。そのためのものは`docs/project-build.md`
(issue #26)、すなわち`laminaria build`/`plan-build`を参照——対象
プロジェクト自身が要求する成果物が実際に必要とするツールチェーンのみを
解決・起動し、同じ本番Nim plannerとRust実行系を再利用する。

## 本スライスが主張していないこと

- **stage1 → stage2**と世代間の計画/アーティファクト比較は、明示的に
  *次の*作業であり、本スライスには含まれない。
- **キャッシュ再利用なし**: `run_generation`の呼び出しはすべて、Nim
  プランナーとRustホストの両方をソースから完全に再ビルドする
  (issue #7/#12の`reuse.rs`判定コアはセルフビルドに一切組み込まれて
  いない)。
- **分散実行なし、細粒度(翻訳単位ごと)のコンパイラスケジューリングなし**:
  アクションは粗粒度(`cargo build`丸ごと、`nim c`呼び出し丸ごと)で
  あり、これはissue #6が最初のローカルスライスに対して明示的に許容
  している(「粗粒度のコンパイラアクションは最初のセルフビルドを
  確立してよく、不透明な領域は正直に露出させる。それらは細粒度の
  コンパイラスケジューリングを証明するものではない」)。本スライスが
  証明しているのは、それらの粗粒度アクションが*本物の*因果経路である
  こと(隠れた外側のビルドがなく、本物のNim計画、本物のRust実行
  アクション、本物の呼び出しごとの証拠がある)であって、スケジューラが
  最適であることではない。

## 検証済みの否定的挙動(issue #8/#6: 却下すること、成功扱いしないこと)

以下はすべて記述された挙動ではなく自動テストである:

- **決定性**: `laminaria-plan`の
  `call_planner_against_the_real_binary_produces_a_deterministic_plan`は、
  実際の`laminaria-planner`バイナリを同一入力で2回呼び出し、
  `ExecutionPlan`の出力がバイト単位で同一であることを検証する。
  `nim-planner`自身の`nimble test`もプロセス内で同じことを検証する。
- **サイクル却下**: `nimble test`(2アクションのサイクルと自己サイクル)
  と、`laminaria-plan`の
  `call_planner_reports_a_structured_cycle_rejection_from_the_real_binary`
  の両方が、正確な`cycle_path`を伴う`cycle`却下を検証する。
- **不正/未対応の入力**: `unsupported_input`/`missing_producer`/
  `duplicate_producer`/`invalid_contract_version`の各却下は、それぞれ
  `nimble test`でカバーされており、`invalid_contract_version`は他の
  どのフィールドがデコードされるよりも*前に*発生することが確認されて
  いる。
- **Rust側の代替計画がないこと**: `laminaria-plan`の
  `call_planner_against_a_missing_binary_is_a_structural_error_never_a_fallback_plan`
  と`laminaria-run`の
  `run_generation_against_a_missing_planner_binary_fails_without_producing_a_generation`
  の両方が、プランナーバイナリが見つからない場合は構造的な`Err`であり、
  黙って計算された代替物では決してないことを検証する。
- **コンパイル失敗が世代生成を中断すること**: `laminaria-run`の
  `a_compile_failure_aborts_the_generation_without_producing_a_stage_output`
  は、`nim-planner/`の使い捨てコピー(追跡されているリポジトリ自体では
  ない)に本物のNim構文エラーを注入し、生成がまさにそのアクションで
  失敗し、`integrate`が決して実行されず、stage出力バイナリが生成
  されないことを検証する。

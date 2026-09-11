# D1実装指示（草案・未発行）

- 状態: **DRAFT** — [issue-35-d0-spec.md](issue-35-d0-spec.md)セクション6の採否記録欄が指示者によって埋められ、
  同ファイルおよび[issue-35-d0-cases.yaml](issue-35-d0-cases.yaml)の該当revisionが確定するまで、本書はD1着手を許可しない。
- 発行条件: セクション6の全論点に「採用」または明示的な代替案の指定が記録されること。
- 参照する仕様revision: `docs/design/issue-35-d0-spec.md` @ 本commit、`docs/design/issue-35-d0-cases.yaml` @ `schema_version: 0.1.0-draft`（確定後は指示者が具体的なgit commit hashをここに追記する）。

---

## 1. スコープ

D1実装者は以下のみを行う。対象選定・期待値・測定条件・合格基準の変更は行わない（不整合を見つけた場合はcase IDと不整合内容を報告し、D0の改版としてissue-35-d0-spec.mdへ差し戻す）。

1. `docs/design/issue-35-d0-cases.yaml`の全case（M1〜M10、`M10-spike-nim-wasm-feasibility`を含む）について、manifest・fixture source・規模生成器・coverage表を実装する。
2. 各caseの`expected`フィールドと実測結果を照合するテスト・スクリプトを実装する。
3. `laminaria-run`の既存計測基盤（`Stats`, `ScenarioReport`, `compare_reports`, `NOISE_FLOOR_STDDEV_MULTIPLIER`）を再利用してCPU budget=1のbaseline計測を取得する。
4. `M10-spike-nim-wasm-feasibility`を最優先で実行し、A/B判定を記録する（この結果が`M10-wasm-side`の実装方針を決定するため、他のM10 caseより先に着手する）。

## 2. 実装順序（推奨）

1. **自己充足case（origin: self）の形式化**: M1-fingerprint-fanout, M1-fingerprint-leaf-edit, M2-nim-planner-shared-module, M3-self-build-independent-groups, M6-diamond-fingerprint-plan。既存の実コードに対する宣言＋検証テストの追加のみで、新規fixtureは作らない。
2. **既存fixture caseの正式化（origin: fixture-existing）**: M4-*（3件）, M5-nim-entry-rust-lib。既存fixtureへの変更は最小限（case定義に必要なメタデータ・スクリプトの追加のみ）とし、fixture本体のソース・アサーションは変更しない。
3. **新規補完サンプル（origin: fixture-new）**: M7-long-chain-wide-branches-{small,medium}, M8-many-unrequested-small（および中/大規模の追加実装）。
4. **M10-spike-nim-wasm-feasibility**: 単独で最優先実行し、判定結果をissue #35のコメントとして記録する。
5. **M9-fingerprint-compat-chain**: 4ノードの新規実装。
6. **M10-lsp-native → M10-wasm-side**: M10-spikeの結果を前提にA/B分岐して実装する。

## 3. 追加・変更する成果物

| # | 成果物 | 種別 | 入力仕様 | 期待結果 | 検証方法 |
|---|---|---|---|---|---|
| 1 | 各caseのmanifest/expected定義への相互参照テスト | 新規テストコード | `issue-35-d0-cases.yaml`の該当エントリ | `expected`と実測の一致 | `cargo test` / `nimble test` に統合 |
| 2 | `fixtures/long-chain-wide-branches/` | 新規fixture | セクション2.2の設計通り | critical path長・head-of-line非発生の実測 | 新規fixture内テスト |
| 3 | `fixtures/many-unrequested-targets/` | 新規fixture | セクション2.3の設計通り | 非要求集合の非ビルド確認 | `cargo build -v`ログ解析スクリプト |
| 4 | `crates/laminaria-fingerprint-ffi/`（新規crate） | Rust cdylib/staticlib | 既存`laminaria-fingerprint`のAPI | opaque handle経由の値取得 | 単体テスト＋M9統合テスト |
| 5 | `nim-planner/src/schema_compat.nim` | 新規Nimモジュール | toolchainバージョン文字列群 | Compatible/Degraded/Incompatibleの3値 | Nim unittest |
| 6 | `nim-planner/src/fp_report_cli.nim` | 新規Nim CLI bin | `--repo-root`, `--lock` | stdout/終了コード | 統合テスト |
| 7 | `nim-planner/src/plan_ffi.nim` | 新規Nimモジュール(既存ロジック再公開) | シリアライズ済み`PlanningInput` | シリアライズ済み`PlanOutcome`/診断 | 既存`nim_planner_client.rs`テストとの結果一致確認 |
| 8 | `laminaria-lsp`（新規crate） | Rust bin | LSP `textDocument/didChange`相当の入力 | 循環依存診断 | 統合テスト |
| 9 | M10-spike成果物 | 調査記録＋最小コード | セクション3.3の判定規則 | A/B判定 | 判定規則の3試行以内の実施記録 |
| 10 | `laminaria-plan-wasm`（新規crate、M10-spike結果依存） | Rust cdylib(wasm32) | M10-spike判定に従う | M10-lsp-nativeとの一致/同値 | 統合テスト |

## 4. 期待結果

`docs/design/issue-35-d0-cases.yaml`の各caseの`expected`/`pass_criteria.d1`フィールドをそのまま適用する。D1実装者はここに新しい期待値を追加してはならない（不足を見つけた場合はD0への差し戻し対象）。

## 5. 検証方法

- 各caseの`forbidden_work`がゼロ回であることをビルドツールの実測ログ（`cargo build -v`、Nimコンパイラの詳細出力）で確認する。
- `required_work`が実際に実行されたことを、既存の`ComputeConcurrencyProbe`方式（issue#27で確立済み）またはそれに準ずる実行区間トレースで確認する。
- 決定性が要求されるcase（M2, M3, M9）は同一入力の2回実行結果の一致を確認する。
- 速度改善は測定しない（D1のスコープ外）。CPU budget=1のbaseline計測のみ取得し、`ScenarioReport`として保存する。

## 6. 停止条件

以下がすべて満たされた時点でD1は完了とする。速度改善そのものはD4で判定するため、D1の停止条件には含まない。

- [ ] `docs/design/issue-35-d0-cases.yaml`の全case（`M10-wasm-side`を除く。これはM10-spikeの判定結果確定後に着手可能になる）についてmanifest/fixture/検証テストが実装され、`pass_criteria.d1`を満たす。
- [ ] `M10-spike-nim-wasm-feasibility`のA/B判定が記録され、`M10-wasm-side`の実装方針が確定している。
- [ ] `M10-wasm-side`が確定した方針で実装され、`pass_criteria.d1`を満たす。
- [ ] 各origin: selfのcaseについて、新規fixtureを追加していないこと（既存の実コード・実ワークスペースのみを使っていること）を差分レビューで確認する。
- [ ] 各origin: fixture-existingのcaseについて、既存fixtureのソース・アサーションが変更されていないこと（メタデータ・検証スクリプトの追加のみ）を差分レビューで確認する。
- [ ] CPU budget=1のbaseline計測が全caseについて`ScenarioReport`として保存され、再生成可能である。
- [ ] 仕様不備が見つかった場合、D0（`issue-35-d0-spec.md`/`issue-35-d0-cases.yaml`）への改版が行われている（D1実装者自身が期待値・測定条件・合格基準を変更していない）。

## 7. 未解決事項の扱い

M10-wasm-sideのA/B判定は、本書発行時点では未確定（M10-spikeの実施待ち）。指示者がセクション3.3で(ii)フォールバック直行を選んだ場合、`M10-spike-nim-wasm-feasibility`は実施せず、`M10-wasm-side`をB案のみで直接実装する。この場合も判断はD0仕様（issue-35-d0-spec.md改版）として記録し、D1実装者が独自にB案を選んだ形にはしない。

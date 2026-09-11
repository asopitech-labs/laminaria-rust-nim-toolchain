# D1実装指示（草案・未発行）

- 状態: **要改訂・未発行** — 提出commit `443b5f2`を審査し、[issue-35-d0-spec.md](issue-35-d0-spec.md)セクション6に採否と修正指示R1〜R5を記録した。以下の本文は**R1〜R5反映後の改訂版**であり、外部compilerによる比較方式の混同、M10のRust代替、D1範囲の不整合を解消した。ただし指示者が本改訂を確認し、[issue-35-d0-spec.md](issue-35-d0-spec.md)セクション6・7と[issue-35-d0-cases.yaml](issue-35-d0-cases.yaml)（`schema_version: 0.2.0-draft`）の3点一致を確定仕様revisionとして記録するまで、本書は実行指示として発行しない。
- 発行条件: 指示者がR1〜R5反映内容を確認し、3点(本書/spec.md/cases.yaml)の一致と確定仕様revisionを記録すること。採否欄・改訂内容一覧への記入だけでは発行しない。
- 参照する仕様revision: `docs/design/issue-35-d0-spec.md`（本改訂）、`docs/design/issue-35-d0-cases.yaml` @ `schema_version: 0.2.0-draft`（確定後は指示者が具体的なgit commit hashをここに追記する）。

---

## 1. スコープ（R5対応、execution_roleとreached_stageで3分類する）

D1実装者は`issue-35-d0-cases.yaml`の各caseを次の3種のいずれかとして扱い、種別を実装者が変更しない。

1. **D1で実装・実行する（execution_role: reference または owned、reached_stage到達可能）**: manifest・fixture source・規模生成器・coverage表と、宣言済みの`expected`/`forbidden_work`/`required_work`を実装・検証する。owned roleのcaseは`laminaria-run`の既存計測基盤でCPU budget=1（該当caseは1と2）のbaseline計測も取得する。
2. **referenceとして保持する**: 既存fixture（M4/M5各case）は、fixture本体のソース・アサーションを変更せず、case定義用のメタデータ・検証スクリプトのみを追加する。M4-rust-nim-c-abi-callcount等の実行時間は「参照timing」として記録し、owned baselineとして扱わない。
3. **後続能力待ちとして保持する（origin: self-planned、`M10-wasm-side`）**: `M10-wasm-feasibility-reference-spike`の判定結果が確定するまで実装に着手しない。判定が否定的な場合、`M10-wasm-side`は未達のまま保持し、`subset_scope.future_work`に不足能力を記録する。D1の完了条件には数えない。

対象選定・期待値・測定条件・合格基準の変更は行わない。不整合を見つけた場合はcase IDと不整合内容を報告し、D0の改版としてissue-35-d0-spec.mdへ差し戻す。

**明示的に禁止する実装（R1対応）**: `M3-topology`の`compile-rust-host`(`ActionKind::CargoBuild`)/`compile-nim-planner`(`ActionKind::NimBuild`)を`compiler_work_executor`へ渡すこと（同executorはこの2種別を`UnsupportedActionKind`として拒否する）。`M3-owned-independent-chains`は新規実装ではなく、既存テスト`independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`をcase登録し計測データを`ScenarioReport`化する作業に限る。

## 2. 実装順序（推奨、execution_role別）

1. **reference role（bootstrap含む）caseの形式化**: `M1-fingerprint-{cold,noop,leaf-edit}`, `M2-nim-planner-shared-module`, `M3-topology`, `M6-diamond-fingerprint-plan`。既存の実コードに対する宣言＋検証テストの追加のみで、新規fixtureは作らない。
2. **owned role caseの登録**: `M3-owned-independent-chains`, `M8-many-unrequested-nim-planner`。既存の実装済みロジック（`compiler_work_executor`, `planning_kernel.plan`のdemand_closure）をcase定義・`ScenarioReport`に登録する。
3. **既存fixture caseの正式化（reference role）**: `M4-*`（3件）, `M5-nim-entry-rust-lib`。既存fixtureへの変更は最小限（case定義に必要なメタデータ・スクリプトの追加のみ）とし、fixture本体のソース・アサーションは変更しない。
4. **新規補完サンプル（reference role）**: `M7-long-chain-wide-branches-{small,medium,large}`, `M8-many-unrequested-cargo`。数式・分岐点・編集差分はspec.md 2.2で確定済みであり、実行結果golden値のみをD1が実行して得て`pin`する。
5. **M10-wasm-feasibility-reference-spike**: 独立したreference調査タスクとして最優先実行し、判定結果をissue #35のコメントとして記録する。この結果は`M10-wasm-side`の着手可否のみに影響し、他のcaseの進行をブロックしない。
6. **self-planned caseの実装**: `M9-fingerprint-compat-chain`（4ノード新規実装）、`M10-lsp-native`（native側実装・診断値テスト）。`M10-wasm-side`はスパイク結果が肯定的な場合のみ着手する。

## 3. 追加・変更する成果物

| # | 成果物 | 種別 | execution_role | 入力仕様 | 期待結果 | 検証方法 |
|---|---|---|---|---|---|---|
| 1 | 各caseのmanifest/expected定義への相互参照テスト | 新規テストコード | 該当caseによる | `issue-35-d0-cases.yaml`の該当エントリ | `expected`と実測の一致 | `cargo test` / `nimble test` に統合 |
| 2 | `M3-owned-independent-chains`のcase登録＋`ScenarioReport`化 | 既存テストの登録作業（新規実装ではない） | owned | 既存テスト自体 | budget1/2結果一致、peak_concurrency==2 | 既存テストの再実行＋計測データ保存 |
| 3 | `fixtures/long-chain-wide-branches/` | 新規fixture | reference | spec.md 2.2の設計通り(段数/分岐点/軸1・軸2固定) | critical path長・head-of-line非発生の実測、golden値のpin | 新規fixture内テスト |
| 4 | `fixtures/many-unrequested-targets/`（Cargo構成） | 新規fixture | reference | spec.md 2.3の設計通り | 非要求集合の非ビルド確認(Cargo) | `cargo build -v`ログ解析スクリプト |
| 5 | `M8-many-unrequested-nim-planner`用のPlanningInput定義 | 新規テストデータ（fixture systemを伴わない） | owned | 同型依存形状をPlanningInputとして直接構成 | demand_closureの非展開確認(Nim側) | `nim_planner_client`テスト群の拡張 |
| 6 | `crates/laminaria-fingerprint-ffi/`（新規crate） | Rust cdylib/staticlib（native C-ABI、参照実装候補） | self-planned | 既存`laminaria-fingerprint`のAPI＋固定snapshotモード | opaque handle経由の値取得 | 単体テスト＋M9統合テスト |
| 7 | `nim-planner/src/schema_compat.nim` | 新規Nimモジュール | self-planned | 要求/提供capability集合 | Compatible/Incompatibleの2値(既存`planFromJson`と同じ厳密一致) | Nim unittest |
| 8 | `nim-planner/src/fp_report_cli.nim` | 新規Nim CLI bin | self-planned | `--repo-root`, `--lock`, `--fixed-snapshot` | stdout/終了コード | 統合テスト |
| 9 | `nim-planner/src/plan_ffi.nim` | 新規Nimモジュール(既存ロジック再公開) | self-planned | シリアライズ済み`PlanningInput` | シリアライズ済み`PlanOutcome`/診断 | 既存`nim_planner_client.rs`テストとの結果一致確認 |
| 10 | `laminaria-lsp`（新規crate） | Rust bin | self-planned | M10-lsp-nativeの固定入力manifest | 固定cycle_path/診断 | 統合テスト |
| 11 | `M10-wasm-feasibility-reference-spike`成果物 | 調査記録＋最小コード | reference | セクション3.3(spec.md)の判定規則 | 肯定/否定の判定 | 判定規則に基づく実施記録 |
| 12 | `laminaria-plan-wasm`（新規crate、spike結果依存） | Rust cdylib(wasm32) | self-planned（spike肯定時のみ） | spike判定結果 | M10-lsp-nativeとの一致(バイト同一、フォールバックなし) | 統合テスト |

## 4. 期待結果

`docs/design/issue-35-d0-cases.yaml`の各caseの`expected`/`pass_criteria.d1`フィールドをそのまま適用する。D1実装者はここに新しい期待値を追加してはならない。M7・M9の実行結果golden値（`u64`集約値、SHA-256ダイジェスト）は、spec.mdが確定した数式・分岐点・正準化書式に基づき、D1が実装時に一度実行して得た値をpinする。これは既存fixture（`deep-critical-path-graph`の`EXPECTED_RESULT`等）と同じ確立済み方法論であり、期待値決定の先送りではない。

## 5. 検証方法

- 各caseの`forbidden_work`がゼロ回であることをビルドツールの実測ログ（`cargo build -v`、Nimコンパイラの詳細出力）で確認する。
- owned roleのcaseについて、`required_work`が実際に実行されたことを`ComputeConcurrencyProbe`（opt-in設定）等の実行区間トレースで確認する。
- 決定性が要求されるcase（M2, M9）は同一入力の2回以上の**独立した**再実行結果の一致を確認する。同一`ScenarioReport`の自己比較は使用しない。
- 速度改善は測定しない（D1のスコープ外）。owned roleのcaseについてのみCPU budget=1（該当するcaseは1と2）のbaseline計測を取得し、`ScenarioReport`として保存する。reference roleのcaseは参照timingとして記録するのみで、owned baselineとして扱わない。

## 6. 停止条件

以下がすべて満たされた時点でD1は完了とする。速度改善そのものはD4で判定するため、D1の停止条件には含まない。`M10-wasm-side`は`M10-wasm-feasibility-reference-spike`の結果が否定的な場合、未達のまま保持されることをもってD1の停止条件を満たす（実装完了は要求しない）。

- [ ] `issue-35-d0-cases.yaml`の「D1で実装・実行する」区分の全case（セクション1の分類1）についてmanifest/fixture/検証テストが実装され、`pass_criteria.d1`を満たす。
- [ ] `M3-owned-independent-chains`が既存テストの登録として`ScenarioReport`化され、`pass_criteria.d1`を満たす。
- [ ] `M10-wasm-feasibility-reference-spike`の判定結果が記録され、`M10-wasm-side`の着手可否が確定している。
- [ ] `M10-wasm-side`は、spike結果が肯定的な場合は実装され`pass_criteria.d1`を満たす。否定的な場合は`subset_scope.future_work`に不足能力が記録され、未達として保持される（いずれの場合もD1完了の妨げにならない）。
- [ ] 各`origin: self`のcaseについて、新規fixtureを追加していないこと（既存の実コード・実ワークスペースのみを使っていること）を差分レビューで確認する。
- [ ] 各`origin: fixture-existing`のcaseについて、既存fixtureのソース・アサーションが変更されていないこと（メタデータ・検証スクリプトの追加のみ）を差分レビューで確認する。
- [ ] `M3-topology`の`compile-rust-host`/`compile-nim-planner`が`compiler_work_executor`へ渡されていないことをコードレビューで確認する。
- [ ] owned roleの全caseについてCPU budget=1（該当するcaseは1と2）のbaseline計測が`ScenarioReport`として保存され、再生成可能である。
- [ ] 仕様不備が見つかった場合、D0（`issue-35-d0-spec.md`/`issue-35-d0-cases.yaml`）への改版が行われている（D1実装者自身が期待値・測定条件・合格基準を変更していない）。

## 7. 未解決事項の扱い

`M10-wasm-side`のA/B判定という枠組みは撤回した（R2対応）。`M10-wasm-feasibility-reference-spike`は独立したreference調査タスクであり、その結果に関わらずM10のゴール（native/WASM両方が同一Nim実装に依存すること）自体は変更しない。判定が否定的であっても、フォールバックとしてRust実装へ置き換えることは行わず、不足している独自target/runtime生成能力（Nim→wasm32コンパイルパス、または#5 T0のtarget契約）を具体的に記録した上でD0（issue-35-d0-spec.md）への改版対象として扱う。

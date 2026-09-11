# D1実装指示（草案・未発行）

- 状態: **要改訂・未発行（第3回改訂反映済み）** — 提出commit `443b5f2`を審査し、[issue-35-d0-spec.md](issue-35-d0-spec.md)セクション6に採否と修正指示R1〜R5を記録した。第1回改訂commit `217b0c1`の読み取りレビューで6件の残件、第2回改訂commit `99a0f5c`の読み取りレビューで3件の残件（D1合格条件の範囲、M6editのコンパイル可否、M7/M9のgolden値未確定）が指摘され、いずれも本改訂で対応した（設計の再検討は不要と確認済み）。D1はcase定義（構成・入力・期待結果）の確定のみを行い、M9/M10の製品コード（FFI shim/Nim CLI/LSP/WASM）の実装は一切含まない。CPU budget=1のbaseline計測を実際に取得するのは`M3-owned-independent-chains`と`M8-many-unrequested-nim-planner`の2caseに限る。指示者が本改訂を確認し、[issue-35-d0-spec.md](issue-35-d0-spec.md)セクション6〜9と[issue-35-d0-cases.yaml](issue-35-d0-cases.yaml)（`schema_version: 0.2.0-draft`）の3点一致を確定仕様revisionとして記録するまで、本書は実行指示として発行しない。
- 発行条件: 指示者がR1〜R5反映内容（本改訂を含む）を確認し、3点(本書/spec.md/cases.yaml)の一致と確定仕様revisionを記録すること。採否欄・改訂内容一覧への記入だけでは発行しない。
- 参照する仕様revision: `docs/design/issue-35-d0-spec.md`（本改訂）、`docs/design/issue-35-d0-cases.yaml` @ `schema_version: 0.2.0-draft`（確定後は指示者が具体的なgit commit hashをここに追記する）。

---

## 1. スコープ（R5対応、execution_roleとreached_stageで3分類する）

D1実装者は`issue-35-d0-cases.yaml`の各caseを次の3種のいずれかとして扱い、種別を実装者が変更しない。

1. **D1で実装・実行する（execution_role: reference または owned、reached_stage: target-generation-executionまたはsource-ir-evaluation）**: manifest・fixture source・規模生成器・coverage表と、宣言済みの`expected`/`forbidden_work`/`required_work`を実装・検証する。**訂正（P1対応、本改訂）**: このうちCPU budget=1（該当caseは1と2）のbaseline計測を`laminaria-run`の既存計測基盤で取得するのは、execution_role: ownedかつD1でsource-ir-evaluationまで到達する`M3-owned-independent-chains`と`M8-many-unrequested-nim-planner`の2caseに限る。他のreference role caseは参照timingの記録に留め、owned baselineとして扱わない。
2. **referenceとして保持する**: 既存fixture（M4/M5各case）は、fixture本体のソース・アサーションを変更せず、case定義用のメタデータ・検証スクリプトのみを追加する。M4-rust-nim-c-abi-callcount等の実行時間は「参照timing」として記録し、owned baselineとして扱わない。
3. **case定義の確定のみを行い、実装は一切含めない（origin: self-planned、reached_stage: configuration-definition、`M9-fingerprint-compat-chain`/`M10-lsp-native`/`M10-wasm-side`）**: 訂正（P1/R2・R5対応、第2回改訂）。前版はこれらのcaseの実コード実装（新規FFI shim crate、Nim CLI、LSPバイナリ）をD1の実装順序・成果物表に含めていたが、これはR5「M9/M10の製品CLI/FFI/LSP/WASM機能一式の実装は…後続実装へ割り当てる」に反していた。D1はcase定義（固定snapshot値・manifest全文・cycle_path・正準化書式・期待exit code/診断）の確定のみを行う。実コードは一切書かない。`M10-wasm-feasibility-reference-spike`の実施もD1には含まれない（後述）。

対象選定・期待値・測定条件・合格基準の変更は行わない。不整合を見つけた場合はcase IDと不整合内容を報告し、D0の改版としてissue-35-d0-spec.mdへ差し戻す。

**明示的に禁止する実装（R1対応）**: `M3-topology`の`compile-rust-host`(`ActionKind::CargoBuild`)/`compile-nim-planner`(`ActionKind::NimBuild`)を`compiler_work_executor`へ渡すこと（同executorはこの2種別を`UnsupportedActionKind`として拒否する）。`M3-owned-independent-chains`は新規実装ではなく、既存テスト`independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`（各言語3 Action=`LowerSource→ValidateIr→EvaluateEvidence`、TransformFunctionは含まない、合計6 Action）をcase登録し計測データを`ScenarioReport`化する作業に限る。新規Transform実装・テストの追加は行わない。

**明示的にD1に含めない作業（本改訂で追加、P1対応）**: `M9-fingerprint-compat-chain`のN1〜N4実コード、`M10-lsp-native`の`laminaria-lsp`/`plan_ffi.nim`実コード、`M10-wasm-side`の`laminaria-plan-wasm`実コード、`M10-wasm-feasibility-reference-spike`の実施。これらはすべて後続実装ステージの作業であり、D1の実装順序にも成果物表にも含まれない。

## 2. 実装順序（推奨、execution_role別）

1. **reference role（bootstrap含む）caseの形式化**: `M1-fingerprint-{cold,noop,leaf-edit}`, `M2-nim-planner-shared-module`, `M3-topology`, `M6-diamond-fingerprint-plan`。既存の実コードに対する宣言＋検証テストの追加のみで、新規fixtureは作らない。
2. **owned role caseの登録**: `M3-owned-independent-chains`, `M8-many-unrequested-nim-planner`。既存の実装済みロジック（`compiler_work_executor`, `planning_kernel.plan`のdemand_closure）をcase定義・`ScenarioReport`に登録する。
3. **既存fixture caseの正式化（reference role）**: `M4-*`（3件）, `M5-nim-entry-rust-lib`。既存fixtureへの変更は最小限（case定義に必要なメタデータ・スクリプトの追加のみ）とし、fixture本体のソース・アサーションは変更しない。
4. **新規補完サンプル（reference role）**: `M7-long-chain-wide-branches-{small,medium,large}`, `M8-many-unrequested-cargo`。数式・分岐点・編集差分・aggregator集約式はspec.md 2.2で確定済みであり、各transform/leaf関数のunit test通過後に、実行結果golden値のみをD1が実行して得て`pin`する。
5. **self-planned case（M9-fingerprint-compat-chain, M10-lsp-native, M10-wasm-side）のcase定義確定のみ**: spec.md/cases.yamlが既に確定した固定snapshot・manifest・正準化書式・期待診断をcase定義として登録する（実コードは書かない）。この作業はreached_stage: configuration-definitionで完結する。

**D1に含まれない作業（訂正、第2回改訂）**: `M10-wasm-feasibility-reference-spike`の実施は、D1の実装順序から完全に除外する。前版はこれを「独立したreference調査タスクとして最優先実行」としてD1の手順5に組み込んでいたが、これはR2・R5の「D1の完了条件から外す」指示に反していた。この調査を実施する場合の時期は、D1完了後に指示者が別途判断する。

## 3. 追加・変更する成果物（D1でコードを書くものに限定、本改訂で6-12を削除）

| # | 成果物 | 種別 | execution_role | 入力仕様 | 期待結果 | 検証方法 |
|---|---|---|---|---|---|---|
| 1 | 各caseのmanifest/expected定義への相互参照テスト | 新規テストコード | 該当caseによる | `issue-35-d0-cases.yaml`の該当エントリ | `expected`と実測の一致 | `cargo test` / `nimble test` に統合 |
| 2 | `M3-owned-independent-chains`のcase登録＋`ScenarioReport`化 | 既存テストの登録作業（新規実装ではない） | owned | 既存テスト自体（6 Action、TransformFunctionなし） | budget1/2結果一致、peak_concurrency==2 | 既存テストの再実行＋計測データ保存 |
| 3 | `fixtures/long-chain-wide-branches/` | 新規fixture | reference | spec.md 2.2の設計通り(段数/分岐点/transform式/leafレシピ/軸1・軸2固定) | critical path長・head-of-line非発生の実測、各transform/leaf関数のunit test、golden値のpin | 新規fixture内テスト |
| 4 | `fixtures/many-unrequested-targets/`（Cargo構成） | 新規fixture | reference | spec.md 2.3の設計通り | 非要求集合の非ビルド確認(Cargo) | `cargo build -v`ログ解析スクリプト |
| 5 | `M8-many-unrequested-nim-planner`用のPlanningInput定義 | 新規テストデータ（fixture systemを伴わない） | owned | used-core/used-util/fixture-bin + unused-pkg-*をPlanningInputとして直接構成 | demand_closureの非展開確認(Nim側)、ordered_actionsが{used-core, used-util, fixture-bin}のみ | `nim_planner_client`テスト群の拡張 |

**本改訂で削除した行（P1対応）**: 前版の6〜12番（`laminaria-fingerprint-ffi`、`schema_compat.nim`、`fp_report_cli.nim`、`plan_ffi.nim`、`laminaria-lsp`、`M10-wasm-feasibility-reference-spike`の実施、`laminaria-plan-wasm`）はすべてD1の成果物から削除した。これらはM9/M10のcase定義自体（cases.yaml）には残るが、実コードとしての実装はD1の対象外であり、後続実装ステージの成果物表として別途扱う。

## 4. 期待結果

`docs/design/issue-35-d0-cases.yaml`の各caseの`expected`/`pass_criteria.d1`フィールドをそのまま適用する。**訂正（本改訂、採否記録`issue35-d0-accepted-c812d70-v1`対応）**: D1は、D0が確定した値（M7の4つの集約値、M9の2つのSHA-256ダイジェスト、いずれもcases.yamlに固定値として記載済み）を`expected`としてそのまま登録し、実装結果をこれと照合する。**実装結果から期待値を生成・上書きすることはしない。** 各transform/leaf関数の個別unit testは、実装がこれらの固定値と一致するかを検証する目的で書く（誤実装の検出手段であり、期待値の決定手段ではない）。M9/M10-lsp-native/M10-wasm-sideの製品コードはD1では実装・実行しない（該当caseの期待値はcases.yamlに固定値として記載済みだが、D1では照合対象がない）。

## 5. 検証方法

- 各caseの`forbidden_work`がゼロ回であることをビルドツールの実測ログ（`cargo build -v`、Nimコンパイラの詳細出力）で確認する。
- owned roleのcaseについて、`required_work`が実際に実行されたことを`ComputeConcurrencyProbe`（opt-in設定）等の実行区間トレースで確認する。M3-owned-independent-chainsは`host`固定・`state_restoration`（repetitionごとの新規ArtifactStore）・`execution_order`（budget1連続実行→budget2連続実行）・`measurement_boundary`（`run_compiler_work_plan_traced`呼び出し前後）を仕様通り再現する。
- 決定性が要求されるcase（M2）は同一入力の2回以上の**独立した**再実行結果の一致を確認する。同一`ScenarioReport`の自己比較は使用しない。
- 速度改善は測定しない（D1のスコープ外）。CPU budget=1（該当するcaseは1と2）のbaseline計測は`M3-owned-independent-chains`と`M8-many-unrequested-nim-planner`の2caseについてのみ取得し、`ScenarioReport`として保存する。他のreference roleのcaseは参照timingとして記録するのみで、owned baselineとして扱わない。M9/M10-lsp-native/M10-wasm-sideはD1では実行・計測を一切行わない。

## 6. 停止条件

以下がすべて満たされた時点でD1は完了とする。速度改善そのものはD4で判定するため、D1の停止条件には含まない。

- [ ] `issue-35-d0-cases.yaml`の「D1で実装・実行する」区分の全case（セクション1の分類1）についてmanifest/fixture/検証テストが実装され、`pass_criteria.d1`を満たす。
- [ ] `M3-owned-independent-chains`が既存テストの登録として`ScenarioReport`化され、`pass_criteria.d1`を満たす（測定手順が§5記載の通り再現可能である）。
- [ ] `M9-fingerprint-compat-chain`/`M10-lsp-native`/`M10-wasm-side`について、case定義（固定snapshot値・manifest・cycle_path・正準化書式・期待exit code/診断）が確定していることのみを確認する。実コードの実装・実行は求めない（`M10-wasm-feasibility-reference-spike`の実施も含めない）。
- [ ] 各`origin: self`のcaseについて、新規fixtureを追加していないこと（既存の実コード・実ワークスペースのみを使っていること）を差分レビューで確認する。
- [ ] 各`origin: fixture-existing`のcaseについて、既存fixtureのソース・アサーションが変更されていないこと（メタデータ・検証スクリプトの追加のみ）を差分レビューで確認する。
- [ ] `M3-topology`の`compile-rust-host`/`compile-nim-planner`が`compiler_work_executor`へ渡されていないこと、`M3-owned-independent-chains`が新規Transform実装を追加していないことをコードレビューで確認する。
- [ ] **訂正（P1対応、本改訂）**: `M3-owned-independent-chains`と`M8-many-unrequested-nim-planner`（D1で実行される唯一のownedケース2件）についてCPU budget=1（該当するcaseは1と2）のbaseline計測が`ScenarioReport`として保存され、再生成可能である。M9/M10-lsp-native/M10-wasm-sideはこの基準の対象外（D1では未実行）。
- [ ] 仕様不備が見つかった場合、D0（`issue-35-d0-spec.md`/`issue-35-d0-cases.yaml`）への改版が行われている（D1実装者自身が期待値・測定条件・合格基準を変更していない）。

## 7. 未解決事項の扱い

`M10-wasm-side`のA/B判定という枠組みは撤回した（R2対応）。`M10-wasm-feasibility-reference-spike`は独立したreference調査タスクであり、その実施時期はD1完了後に指示者が別途判断する（D1の着手・完了条件ではない、本改訂で明確化）。その結果に関わらずM10のゴール（native/WASM両方が同一Nim実装に依存すること）自体は変更しない。判定が否定的であっても、フォールバックとしてRust実装へ置き換えることは行わず、不足している独自target/runtime生成能力（Nim→wasm32コンパイルパス、または#5 T0のtarget契約）を具体的に記録した上でD0（issue-35-d0-spec.md）への改版対象として扱う。M9/M10-lsp-nativeの実コード実装も同様に、D1完了後の後続実装ステージとして別途指示される。

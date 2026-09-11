# #28 D1 完了検収

対象issue: #28（D1本体）、#35（D0、親issue #28）。検収担当: Claude
（D1-a/D1-b1/D1-b2の実装者自身）。指示に基づき、実装報告を鵜呑みにせず
一次資料を直接照合した。

## 一次資料

- `docs/design/issue-35-d0-cases.yaml`（schema_version `0.2.0-draft`、基準commit
  `c812d70`、`git log`で#35 D0受理(`0246514`)以降無変更を確認済み）
- `docs/design/issue-35-d0-d1-draft-instructions.md`（D1のスコープ・実装順序・
  検証方法・§6停止条件・§7未解決事項の扱い）
- `docs/design/issue-35-d0-spec.md` §10（採否確定記録、`issue35-d0-accepted-c812d70-v1`）
- `crates/laminaria-experiment/`（`registry.rs`, `m3_baseline.rs`, `m8_baseline.rs`,
  `owned_identity.rs`, `d1b1_reference_cases.rs`, `d1b1_planner_cases.rs`,
  `d1b1_preflight.rs`, `d1b2_new_fixtures.rs`）
- `docs/design/d1b1-m1-m6-evidence.md`（M1/M6、隔離`c812d70` worktree手順）
- #28コメント: D1-a実装・round1・round2・round3修正、D1-b1完了・完了補正、
  D1-b2完了（各commitへのリンクを含む）
- 対応するCI run: `34571930602`（D1-b1完了補正、`12cd4e6`）、`34574676209`
  （D1-b2、`37dac99`）、`34581054757`（直近green、`d950c60`）

## 方法

各caseについて、(a) `registry.rs::Case::d1_disposition()`が返す3値分類
（`Implemented`/`ReferenceHeld`/`ConfigurationOnly`、コード自体の
doc commentが明記する通り"never conflated with executed successfully"）、
(b) 対応する実装・定義ファイル、(c) 実際に保持されている証拠の種別と
その耐久性（CI上で再確認可能か、それとも本セッション内のローカル実行の
記録に留まるか）、(d) D1停止条件（d1-draft-instructions.md §6）との対応、
を個別に確認した。`runs/`配下の生ログはプロジェクト全体でgitignore対象
であり、本検収でも存在しない証拠として扱っていない — 「保持されている
証拠」欄は常に、gitで追跡可能なコード・CI run・issueコメントのいずれかを
指す。

`laminaria-case-registry`をこの検収の一環として隔離worktree（現HEAD
`d950c60`固定、`git status --porcelain`空を確認済み）で実行し、20 case
全件のロード・検証がゼロエラーであることを直接確認した:

```
loaded 20 cases from .../docs/design/issue-35-d0-cases.yaml
disposition: 11 Implemented, 4 ReferenceHeld, 5 ConfigurationOnly (none of these three counts is an execution result)
validation: OK, no errors
```

同じ隔離worktreeで`laminaria-m3-owned-baseline`/`laminaria-m8-owned-baseline`
も実行し、両方とも`pass_criteria.d1: OK`（後述）を確認した — これは本検収の
ために本日新たに取得した証拠であり、過去のissueコメントの記述をそのまま
転記したものではない。

## Case別一覧（20件）

凡例: 「証拠」列の★はCIで再確認可能な耐久的証拠、☆はコード+
`cargo test --workspace`（CI実行）による部分カバレッジ、●は本検収で
新規に取得したローカル一次確認、既存の●は前回セッション内のローカル
一次確認（本検収で再確認済みまたは対象外）。

### 分類1: D1で実装・実行する(reference/owned/bootstrap、reached_stage ≠ configuration-definition)

| case ID | D1区分 | 実装・定義ファイル | 証拠 | 停止条件適合 | 未充足事項 |
|---|---|---|---|---|---|
| M1-fingerprint-cold | Implemented, reference | `docs/design/d1b1-m1-m6-evidence.md`(手順記録)。実装コード追加なし(既存workspace実測) | ● 隔離`c812d70`worktreeでの1回のローカル実行記録(本セッション内、再現手順込み)。CI未自動化 | 適合(3/3回、`Compiling laminaria-fingerprint`exactly1) | CIで自動再確認されない。手順は固定commitに対して決定的なため再実行しても同一結果になるはずだが、独立した自動検証は存在しない |
| M1-fingerprint-noop | Implemented, reference | 同上 | ● 同上 | 適合(3/3回、Compiling行ゼロ) | 同上 |
| M1-fingerprint-leaf-edit | Implemented, reference | 同上 | ● 同上 | 適合(fingerprint/plan/run/cli各1回、laminaria-irはFresh) | 同上 |
| M2-nim-planner-shared-module | Implemented, reference | `crates/laminaria-experiment/src/d1b1_planner_cases.rs` | ★ CI run [34571930602](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/actions/runs/34571930602)、ubuntu/macos両方でPASS | 適合(両binが同一`planning_kernel`をimport、3回とも決定的一致) | なし |
| M3-topology | Implemented(コード実行あり), ConfigurationOnly(reached_stage分類上), bootstrap | `crates/laminaria-experiment/src/d1b1_planner_cases.rs` | ★ 同上CI run。既存test 2件の再実行+`compiler_work_executor.rs:820-831`のソース引用 | 適合 | **分類上の注記**: `reached_stage: configuration-definition`のため`registry.rs`の3値分類では"ConfigurationOnly"に入るが、M9/M10とは性質が異なる — pass_criteria.d1自体が「非依存性の実測」「CargoBuild/NimBuildの拒否確認」という実行可能な検証を要求しており、実際に2つの既存testを再実行して確認済み。M9/M10(コード・実行ゼロ)と同列に「未実行」として扱ってはならない |
| M3-owned-independent-chains | Implemented, owned | `crates/laminaria-experiment/src/{m3_baseline,owned_identity}.rs` | ☆● `cargo test --workspace`(CI実行)が`m3_baseline_test.rs`14testを通じて拒否パス+一部実測値(peak_concurrency等)をカバー。bin(`laminaria-m3-owned-baseline`)自体はCIで未実行。**本検収で隔離worktree(HEAD `d950c60`)にて新規に実行、`pass_criteria.d1: OK`を確認**(budget1 peak=[1,1,1,1,1], budget2 peak=[2,2,2,2,2], evidence_matches_across_budgets=true, owned_baseline_comparable_across_budgets=Ok) | 適合(budget1/2結果一致、budget2でpeak_concurrency==2が5回とも再現、`ScenarioReport`として保存・再生成可能) | binがCIで自動実行されていない(D1-b1/b2のrunnerと異なり、この点はD1-a完了時から一貫した未対応)。今回のローカル確認は耐久的CI証拠ではなく本検収時点のスナップショット |
| M6-diamond-fingerprint-plan | Implemented, reference | `docs/design/d1b1-m1-m6-evidence.md` | ● M1と同じ隔離worktree手順内で確認(本セッション内) | 適合(fingerprint/planとも収束点で重複コンパイルなし) | M1と同じCI未自動化 |
| M4-rust-nim-c-abi-scalar | ReferenceHeld | `crates/laminaria-experiment/src/d1b1_reference_cases.rs` | ★ CI run 34571930602 | 適合(既存main.rsの出力を再現、fixture本体は無変更) | なし |
| M4-rust-nim-c-abi-buffer | ReferenceHeld | `d1b1_reference_cases.rs` | ★ 同上 | 適合(内部assert_eq!通過) | なし |
| M4-rust-nim-c-abi-callcount | ReferenceHeld | `d1b1_reference_cases.rs` | ★ 同上(warmup1+測定5回) | 適合 | なし |
| M5-nim-entry-rust-lib | ReferenceHeld | `d1b1_reference_cases.rs` | ★ 同上(result=43, layout一致, SIGABRT(134), exit=1を生ログから直接grep確認) | 適合 | なし |
| M7-long-chain-wide-branches-small | Implemented, reference | `fixtures/long-chain-wide-branches/`, `d1b2_new_fixtures.rs` | ★ CI run [34574676209](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/actions/runs/34574676209) | 適合(D0確定値`152668892010644049`一致、stage-01/02各1回コンパイル) | なし |
| M7-long-chain-wide-branches-medium | Implemented, reference | 同上 | ★ 同上CI run(edit前後の値・再ビルド集合を生ログで確認) | 適合(編集前後の値一致、再ビルド集合が宣言通り10crate、stage-01/02/03対象外) | なし |
| M7-long-chain-wide-branches-large | Implemented, reference | 同上 | ★ 同上CI run(warmup1+測定3回、軸1/軸2分離記録) | 適合(D0確定値`233828575729373081`一致、3/3再現) | なし |
| M8-many-unrequested-cargo | Implemented, reference | `fixtures/many-unrequested-targets/`, `d1b2_new_fixtures.rs` | ★ 同上CI run(small/medium/large全スケール) | 適合(forbidden_workゼロ回、3スケールとも) | なし |
| M8-many-unrequested-nim-planner | Implemented, owned | `crates/laminaria-experiment/src/{m8_baseline,owned_identity}.rs` | ☆● M3-owned-independent-chainsと同様。**本検収で隔離worktreeにて新規実行、`pass_criteria.d1: OK`を確認**(全規模でneeded_set_matches=true, kernel_nanos_present=true) | 適合 | M3-owned-independent-chainsと同じCI未自動化 |

### 分類2: case定義の確定のみ(origin: self-planned、reached_stage: configuration-definition)

| case ID | D1区分 | 実装・定義ファイル | 証拠 | 停止条件適合 | 未充足事項 |
|---|---|---|---|---|---|
| M9-fingerprint-compat-chain | ConfigurationOnly | cases.yaml本体のみ | ★ 実コード不存在を本検収で直接確認(`nim-planner/src/fp_report_cli.nim`, `crates/laminaria-fingerprint-ffi`, `nim-planner/src/schema_compat.nim`いずれも存在しない) | 適合(case定義確定のみが要求される。コード実装・実行は求めない) | なし。**実行成功として扱っていない**ことを本検収で明示 |
| M10-lsp-native | ConfigurationOnly | cases.yaml本体のみ | ★ `crates/laminaria-lsp`, `nim-planner/src/plan_ffi.nim`いずれも不存在を確認 | 適合 | なし |
| M10-wasm-side | ConfigurationOnly | cases.yaml本体のみ | ★ `crates/laminaria-plan-wasm`不存在を確認 | 適合(フォールバック方式を採用していないことも確認 — Rust再実装への置換なし) | なし |
| M10-wasm-feasibility-reference-spike | ConfigurationOnly | cases.yaml本体のみ | ★ `fixtures/nim-wasm-spike`等、調査実施の痕跡が存在しないことを確認 | 適合(D1の着手・完了条件に含まれないことが明記されており、未実施のまま — これ自体が正しい状態) | なし |

## D1停止条件(d1-draft-instructions.md §6)との対応

1. 「D1で実装・実行する」区分の全caseについてmanifest/fixture/検証テストが
   実装され、`pass_criteria.d1`を満たす — **適合**(上表参照。M1×3/M3-owned/
   M6/M8-nim-plannerはCI非自動化という残差リスクを伴うが、コード・
   ローカル一次確認の両面で`pass_criteria.d1`自体は満たされている)。
2. `M3-owned-independent-chains`が`ScenarioReport`化され、`pass_criteria.d1`
   を満たす(測定手順が§5記載の通り再現可能) — **適合**。本検収での再実行
   (`owned_baseline_comparable_across_budgets=Ok(())`)で確認。
3. `M9`/`M10-lsp-native`/`M10-wasm-side`について、case定義確定のみを
   確認する(実コード不要) — **適合**。実コード不存在を本検収で直接確認。
4. `origin: self`の各caseで新規fixtureを追加していない — **適合**
   (`git log --since`で対象期間中のfixtures/配下への追加なしを確認済み。
   M1/M2/M3-topology/M3-owned/M6/M8-nim-plannerはいずれも既存workspace/
   既存ロジックの登録作業のみ)。
5. `origin: fixture-existing`の各caseで既存fixtureのソース・アサーションが
   無変更 — **適合**(`git log --since="2026-09-11" -- fixtures/rust-nim-c-abi-baseline
   fixtures/mixed-rust-nim-executable fixtures/boundary-heavy-workload
   fixtures/direct-native-link`が空であることを確認)。
6. `M3-topology`のCargoBuild/NimBuild非委譲、`M3-owned-independent-chains`の
   新規Transform非追加 — **適合**(前者はソース引用+テスト実行の両方、
   後者は`plan.actions.len()==6`のassertで確認済み)。
7. `M3-owned-independent-chains`/`M8-many-unrequested-nim-planner`の
   budget=1 baseline計測が`ScenarioReport`として保存・再生成可能 —
   **適合**(regenerate()の存在・テスト・本検収での実行確認)。
8. 仕様不備が見つかった場合のD0改版 — **該当なし**(D1-a/b1/b2を通じて
   発見された不具合はすべて実装側の欠陥であり、D0(cases.yaml/spec.md)の
   改版を要する仕様不備は見つかっていない)。

## 総合判定

**D1は完了と判定できる。** 20 case全件が、D0が定めた通りの区分
(実行/参照保持/定義のみ)のまま、それぞれのpass_criteria.d1を満たしている
ことを一次資料で確認した。D1-a/D1-b1/D1-b2いずれの完了報告も、今回の
独立した突き合わせで裏付けが取れた。

ただし以下2点は、D1停止条件の文言上は必須とされていない(「再生成可能で
あること」は要求されるが「CIで自動実行されること」は要求されていない)が、
将来の回帰検出という観点では未充足のまま残っている——具体的な不足として
報告する(期待値・合格基準は変更していない):

1. **M3-owned-independent-chains/M8-many-unrequested-nim-plannerの
   owned baseline bin(`laminaria-m3-owned-baseline`/`laminaria-m8-owned-baseline`)
   がCIで一度も実行されていない。** `cargo test --workspace`は同じロジックの
   拒否パス・一部実測値をカバーするが、bin全体の`run()`→`regenerate()`→
   `owned_baseline_comparable_across_budgets/scales()`という一連のパイプラインが
   実際にCIのクリーンな環境で`pass_criteria.d1: OK`を出力することは、
   本検収での一時的なローカル確認以外に耐久的な証拠がない。
2. **M1-fingerprint-{cold,noop,leaf-edit}/M6-diamond-fingerprint-planは
   自動テスト・CIのいずれにも組み込まれていない。** 証拠は
   `docs/design/d1b1-m1-m6-evidence.md`に記録された、隔離`c812d70`
   worktreeでの手順化されたローカル実行のみ。手順自体は固定commitに
   対する決定的な操作であり再現性は高いが、独立した自動検証は存在しない。

いずれも「D0が定めた合格基準を満たしていない」という意味の不足ではなく、
「満たしていることの継続的な検証手段が薄い」という意味の残存リスクである。
対応(CIへの組み込み等)を行うかどうかは指示者の判断に委ねる——本検収の
スコープでは期待値・合格条件・D0固定値のいずれも変更していない。

## #35(D0)closability

issue #35自身の完了条件(6項目、issue本文より)を直接照合した:

- [x] LAMINARIA自身の必須caseとしての交互言語依存鎖(M9)とLSP/WASM構成
  (M10)のnode/host/target具体化 — spec.md §3.1-3.2で確定。
- [x] 自身で覆えない依存パターン(M7/M8)の特定と補完サンプルへの割当 —
  spec.md §2.2-2.3で確定、D1-b2で実装済み。
- [x] 必須項目1〜8の具体化、D1実装者への対象選定・合格基準設定の残置なし
  — 3回の審査ラウンドを経て全項目を確定(spec.md §6-9に記録)。
- [x] M1〜M10の定義と期待結果の無矛盾、実行可能/未対応/後続待ちの明記 —
  `execution_role`/`reached_stage`/`origin`の3軸分類で達成。
- [x] 指示者による採否・仕様revision記録とD1実装指示の発行 — spec.md §10
  (`issue35-d0-accepted-c812d70-v1`)、d1-draft-instructions.md。
- [x] 仕様変更時のD0改版・実装結果への合格条件後付け変更の禁止 — D1-a/b1/b2
  を通じて一貫して遵守(上記の通り、実装側の欠陥はすべて実装側で修正し、
  D0側の期待値・合格基準を変更した事例はゼロ)。

6項目すべてが満たされていると判断する。#35は#28全体の完了を意味しない
(issue本文末尾に明記の通り)が、#35自身のスコープ(D0の確定)は完了して
いる。**#35はclose可能と判断するが、close操作自体は指示者の判断に委ねる。**

## #28(D1本体)の扱い

D1(D1-a/D1-b1/D1-b2)は完了と判定するが、#28自体はD2〜D4が残るため
closeしない(指示通り)。

# #35 D0 仕様案 — LAMINARIA自身の複雑な混成ビルド構成と補完サンプルの仕様

- 対象Issue: [#35](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/35)（親: [#28](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/28)）
- 位置付け: 本書は **ゴール設定担当（Claude）による提出仕様案**であり、#35本文が定義する「指示者」（ユーザー）の採否記録をもって初めて「仕様確定」となる。本書単独でD1着手を許可しない。
- 基準commit: `b875e43303a7ec9a5215897ed9dd08c98b45c812`（2026-09-11時点のHEAD、tree clean）
- 併読ファイル:
  - [issue-35-d0-cases.yaml](issue-35-d0-cases.yaml) — 機械可読case定義（本書各節が参照するcase IDの実体）
  - [issue-35-d0-d1-draft-instructions.md](issue-35-d0-d1-draft-instructions.md) — 指示者確定後に発行するD1実装指示の草案

## 0. 要約（先に結論）

- LAMINARIA自身の実crateグラフ（`laminaria-fingerprint` ← `laminaria-plan`/`laminaria-run`/`laminaria-cli`、および自己ビルドのRust host + Nim planner非連結統合）は、**M1・M2・M3・M6を追加サンプルなしで自己充足**している。fixturesディレクトリ単体を調べただけでは見えない事実であり、本書の中心的な発見。
- M4/M5は既存の`fixtures/rust-nim-c-abi-baseline`等（M4）と`fixtures/direct-native-link`（M5）が実機能・実値受け渡しで既に充足している。将来M9が実装されればM4/M5は自身の実構成でも証拠が得られるようになるが、D1時点ではこれら既存fixtureを正式case化して流用する。
- M7・M8は自身の構成でもfixturesの現状でも覆えないため、新規補完サンプルが必要。
- M9（Nim CLI→Rust lib→Nim lib→Rust lib）・M10（Rust LSP側/WASM側→Nim lib由来機能）は現時点で一切実装がなく（grep実証済み、後述）、本書が新規に具体設計する。
- 未確定の中心論点はNim→WASMの実現可否（M10）であり、調査に基づく推奨判断（Emscripten経路を第一候補、判定条件付きフォールバック）を付けて提出する。実装担当に判断を委ねない。

---

## 1. 棚卸し — 現行実機能・依存・不足（必須項目1）

### 1.1 実在コンポーネントと依存グラフ（`crates/*/Cargo.toml`実測、HEAD `b875e43`）

```text
laminaria-fingerprint  (leaf; env/toolchain/lock比較・doctor)
   ↑ laminaria-plan
   ↑ laminaria-run
   ↑ laminaria-cli
laminaria-ir            (leaf; source-derived IR/interpreter/transform, Rust実装)
   ↑ laminaria-run
laminaria-plan           (→ laminaria-fingerprint)
   ↑ laminaria-run
   ↑ laminaria-cli
laminaria-run            (→ fingerprint, ir, plan;  bin: laminaria-rustc-wrapper, laminaria-cc-wrapper)
   ↑ laminaria-cli
laminaria-cli             (→ fingerprint, run, plan;  bin: laminaria)
nim-planner               (Nim; contract.nim + planning_kernel.nim + laminaria_planner.nim本体、
                            tests/test_planning_kernel.nimも同じ2moduleを共有 import)
```

矢印は「上が下に依存」。出典: `crates/laminaria-cli/Cargo.toml:14-17`, `crates/laminaria-run/Cargo.toml:10-24`, `crates/laminaria-plan/Cargo.toml:11-14`, `crates/laminaria-fingerprint/Cargo.toml`（内部依存なし）, `crates/laminaria-ir/Cargo.toml`（内部依存なし）, `nim-planner/tests/test_planning_kernel.nim:8-9`, `nim-planner/src/laminaria_planner.nim:23-24`。

### 1.2 実機能インベントリ（既存 / 要拡張 / 未着手）

| 領域 | 実機能（既存） | ファイル | 未対応・要新規実装 |
|---|---|---|---|
| CLI（Rust） | `Doctor`/`Run`/`RegenerateSummary`/`ScenarioRun`/`ScenarioRegenerate`/`ScenarioCompare`/`PlanSelfBuild`/`SelfBuild`/`PlanBuild`/`Build`の10 subcommand | `crates/laminaria-cli/src/main.rs` | **Nim CLIは存在しない**（repo全体grep `-rniE "\bwasm\b|\blsp\b|language.?server"` --include=*.rs,*.toml,*.nim,*.md、`.reference/`と`target/`除外で0件、本書執筆時に実行し確認） |
| Planner/Runtime | Nim `planning_kernel.plan`（Kahn topo sort + cycle検出、demand closure、issue#27で実装）; Rust `compiler_work_executor::run_compiler_work_plan`（CPU budget付き並行dispatch、issue#27） | `nim-planner/src/planning_kernel.nim`, `crates/laminaria-run/src/compiler_work_executor.rs` | メモリ会計・キャンセルは issue#27自身の既知の残課題（本書のスコープ外、変更しない） |
| IR処理 | `laminaria-ir`: `rust_frontend`/`nim_frontend`（宣言済みsubsetのみ）、`interpreter`（実行トレース）、`transform::{checked_inline, anf_insert}`、`validate` | `crates/laminaria-ir/src/*.rs` | `laminaria-plan`の`ActionKind::{LowerSource,ValidateIr,TransformFunction,EvaluateEvidence}`は**まだ実行時に本物のNim plannerから計画に組み込まれない**（`compiler_work_executor`が直接呼ぶ経路のみ。issue#27自身の残課題であり本書は変更しない） |
| 共通ライブラリ（Rust） | `laminaria-fingerprint`（env/toolchain検出、doctor、比較） | `crates/laminaria-fingerprint/src/*.rs` | 該当機能をC-ABI越しにNimへ公開するFFI層は存在しない（M9設計で新規） |
| 共通ライブラリ（Nim） | `contract.nim`（`PlanSchemaVersion`とスキーマ相互変換）、`planning_kernel.nim` | `nim-planner/src/*.nim` | スキーマ互換性判定（バージョン範囲の許容/拒否ロジック）は**未実装**（`grep -n "schema" nim-planner/src/contract.nim`はスキーマの型・定数のみで比較ロジックなし。M9設計で新規） |
| 言語間境界契約 | (a) subprocess+JSON: `laminaria-plan::nim_planner_client`が`laminaria-planner`バイナリをspawnしJSON stdin/stdoutで通信（自己ビルドの唯一の実運用経路） / (b) native C-ABI static-lib linking（scalar/pointer/opaque-handleのみ、by-value集約体・seq/Vec値・captured-closureは非対応、panic/例外は境界を越えて伝播させない）— `fixtures/direct-native-link/NOTES.md`が実証済み | `crates/laminaria-plan/src/nim_planner_client.rs`, `fixtures/direct-native-link/NOTES.md` | (b)は現在**製品コードでは未使用**（research fixtureのみ）。M9/M10は(b)を製品functionalityへ初めて適用する |
| LSP | なし | — | 完全新規（M10で設計） |
| WASM | なし（`docs/agent-oriented-toolchain-ux.md:23`と`docs/metrics-policy.md`が将来のtarget経路として言及するのみで実装ゼロ） | — | 完全新規（M10で設計、Nim→wasm32の実現可否は未確定＝本書§4.3で調査・推奨） |

### 1.3 意思決定：本書が新規に追加する機能は「実機能」の基準を満たすか

#35本文「無意味なwrapperやリンクされるだけのライブラリで複雑さを水増ししない」を満たすため、M9/M10で新設するnodeは以下の基準を満たす場合のみ許可する。
1. 入力から出力への変換が実際に計算を行い、固定値・恒等関数ではないこと。
2. 単体テストで検証可能な、観測可能な性質を持つこと。
3. LAMINARIA自身が将来的に必要とする現実の能力（環境/ツールチェーン適合性判定、依存グラフ健全性診断など）に対応すること。

この基準に基づき、§3で設計するM9/M10の各nodeは「既存の実装を流用」と「新規だが実質を持つ実装」を明示的に区別する（§3.1/§3.2の表を参照）。

---

## 2. M1〜M8: 自身の構成での充足度と不足の切り分け（必須項目2前半・3）

`fixtures/`配下11ディレクトリの精査（Explore agent、HEAD `b875e43`同一commitで実施、file:line引用付き）と、`crates/*/Cargo.toml`実測を突き合わせた結果。

| Pattern | 自身の構成での充足 | 根拠 | 追加サンプル要否 |
|---|---|---|---|
| **M1**（Rust-only、複数app→共有lib） | **自己充足**（新規サンプル不要） | `laminaria-rustc-wrapper`/`laminaria-cc-wrapper`（`crates/laminaria-run/Cargo.toml:10-16`の2 `[[bin]]`）が`laminaria-run`自身のlibモジュール（`cargo_wrapper.rs`/`nim_wrapper.rs`)を共有。加えて`laminaria-cli`/`laminaria-run`/`laminaria-plan`の3者が`laminaria-fingerprint`を共有producerとして持つ（§2.1で詳細化） | 不要。case化のみ行う |
| **M2**（Nim-only、複数app→共有lib） | **自己充足（要D1確認）** | `nim-planner/src/laminaria_planner.nim`と`nim-planner/tests/test_planning_kernel.nim`がともに`contract.nim`+`planning_kernel.nim`を`import`（`laminaria_planner.nim:23-24`, `test_planning_kernel.nim:8-9`実測）。ただし後者はunittestバイナリであり「製品として配布される複数app」ではない点をD1が明記すること | 原則不要。ただし指示者が「テストバイナリはappと認めない」と判断した場合のみ、`nim-heavy-workspace`の`primes.nim`/`geometry.nim`を共有する第2binを追加する代替案（§付録A案）を使う |
| **M3**（独立Rust群+独立Nim群、cross-edgeなし） | **自己充足** | 自己ビルドの`SelfBuild`コマンドが生成する`PlanningInput`は`compile-rust-host`（Cargo経由、`ActionKind::CargoBuild`）と`compile-nim-planner`（Nim経由、`ActionKind::NimBuild`）を**互いにinputs/outputsで接続せず**、`integrate`アクションのみが両方を消費する — `crates/laminaria-plan/src/nim_planner_client.rs`の`sample_input()`（130-160行台）が実例そのもの。Rust側 workspace全体とNim側`nim-planner`全体が、ビルド時点で相互に非依存という条件を満たす | 不要 |
| **M4**（Rust entry→Nim lib） | 自身の製品コードでは未充足（唯一の言語間契約は(a)subprocess、"lib呼び出し"ではない）。**既存fixtureで充足**: `rust-nim-c-abi-baseline`（scalarのみ）、`mixed-rust-nim-executable`（配列ポインタ往復、チェックサム照合）、`boundary-heavy-workload`（100万回呼び出しの境界コスト測定） | Explore agent report; 各`main.rs`のassert定数（`mixed-rust-nim-executable`: `EXPECTED_CHECKSUM_BEFORE=11146`等） | 不要（既存資産を正式case化） |
| **M5**（Nim entry→Rust lib） | 同上、自身の製品コードでは未充足。**既存fixtureで充足**: `direct-native-link/nim-bin`（scalar/struct layout/pointer往復/panic越境/thread/callback、Layer1-4を実証） | `fixtures/direct-native-link/NOTES.md`全体 | 不要（既存資産を正式case化） |
| **M6**（diamond、複数独立consumer→共有producer） | **自己充足** | `laminaria-fingerprint`は`laminaria-plan`・`laminaria-run`・`laminaria-cli`という**3つの独立した直接consumer**を持つ（3つの`Cargo.toml`実測）。`laminaria-plan`自体も`laminaria-run`・`laminaria-cli`という2consumerを持つ、入れ子のdiamond。重複要求の合流・producer一回実行・変更伝播をcargoの実挙動として検証できる | 不要 |
| **M7**（長鎖+wide独立枝、不均等サイズ） | **不足**。`deep-critical-path-graph`（12段の鎖）と`wide-parallel-graph`（8独立leaf→1 aggregator）は別々のfixtureで、単一graphに統合されていない。両者ともmodule sizeが均一（Explore agent確認） | Explore agent report | **必要**: 新規補完サンプル`fixtures/long-chain-wide-branches`（§2.2） |
| **M8**（多数の未要求package/profile/target） | **不足**。自身のworkspaceは5 crate + nim-plannerのみで、全てが実際に使われる経路上にある。未要求サンプルが存在しない | Explore agent report; `Cargo.toml`各member確認 | **必要**: 新規補完サンプル`fixtures/many-unrequested-targets`（§2.3） |

### 2.1 M1/M6の具体node対応（自己充足の詳細）

| node | 言語 | 実機能 | 入力 | 出力 | 依存 |
|---|---|---|---|---|---|
| `laminaria-fingerprint`（producer） | Rust lib | 環境/ツールチェーン検出（`env::detect`, `rust_toolchain::detect`, `nim_toolchain::detect`） | repo_root, lock path | `EnvironmentFingerprint`/`ToolchainFingerprint`値 | なし |
| `laminaria-plan`（中間consumer兼producer） | Rust lib | `PlanningInput→PlanOutcome`契約型、`validate` | fingerprint型 | 契約型 | fingerprint |
| `laminaria-run`（consumer兼2 app producer） | Rust lib+2bin | tracer/wrapper/scenario/self_build/compiler_work_executor | fingerprint, ir, plan | Run記録・実行計画結果 | fingerprint, ir, plan |
| `laminaria-cli`（consumer, 1 app） | Rust bin | 10 subcommand | 上記すべて | CLI出力 | fingerprint, run, plan |

需要選択テストの一例（D1で実装）: `cargo build -p laminaria-fingerprint`のみを要求した場合に`laminaria-plan`/`laminaria-run`/`laminaria-cli`が再ビルドされないこと（真の非要求時無変更）、`laminaria-fingerprint`のsourceを1行編集した場合に3 consumer全てが再ビルドされること、を実際の`cargo build -v`出力で検証する（`incremental-semantic-edit/EDIT.md`+`STATE-CONTRACTS.md`が確立した「宣言→実測照合」の方法論をそのまま流用）。

### 2.2 M7補完サンプル設計: `fixtures/long-chain-wide-branches`

既存の2fixtureを「結合」ではなく、新規に1つのCargo workspaceとして起こす（既存2つは他issueの実験に使われている可能性があるため変更しない。既存資産は保持し、新規fixtureとして追加）。

- 構成: `stage-01`〜`stage-08`の線形鎖（`deep-critical-path-graph`と同型の実変換、ただし段数はM7専用に8段へ縮小し、鎖6段+wide層2段の合成にする）に加え、鎖の中間ノード（例: `stage-04`）から**独立して分岐する4本のwide leaf**（`wide-parallel-graph`と同型の実アルゴリズム: gcd/bubble-sort/binary-search/matrix-sum）を生やし、最終`aggregator`が鎖の末端(`stage-08`)とwide leaf 4本の**両方**を消費する。
- 不均等サイズ: 鎖側の各stageは単一関数（既存同様小さい）のままとするが、wide leaf側に意図的なサイズ差を導入する — 4本のうち1本（`leaf-matrix-sum`相当）は行列サイズを既存の10倍にした実計算量を持たせ、残り3本は既存のまま小さくする。これにより「critical pathは鎖側だが、wide側の1本が実行時間で支配的になりうる」という粒度調整の検証対象を作る。
- 検証する性質: critical path長（鎖8段）と、head-of-line blocking（wide側の重い1本が他の軽い3本を巻き込んでスケジューラをブロックしないこと）の両方を1つのgraphで再現する。
- 規模: 小=鎖4段+wide2本、中=鎖8段+wide4本（上記既定）、大=鎖16段+wide8本（うち2本を重い版に）。固定seedは`deep-critical-path-graph`と同じ流儀で`SEED: u64 = 20260909`を再利用する。

### 2.3 M8補完サンプル設計: `fixtures/many-unrequested-targets`

- 構成: 1つのCargo workspaceに、実際に要求される1本の`fixture-bin`が依存する2つのcrate（`used-core`, `used-util`）に加え、**要求されない**10個の兄弟crate（`unused-pkg-01`〜`unused-pkg-10`、うち3つはさらに独自の内部依存鎖を持つ）を配置する。加えて、`fixture-bin`自体に`[profile.release]`と`[profile.dev]`の2profile、および`[[bin]]`を2つ（要求されるのは1つのみ）宣言する。
- 検証する性質: 要求された成果物の依存閉包だけが具体化され、`unused-pkg-*`群・非要求profile・非要求binターゲットがビルドされないこと（demand-based pruningの直接テスト）。#28本文の「テストの組合せ列挙と、製品plannerが全variantの直積を展開することを混同しない」という注意を、このfixture自体の作り方にも適用する — 生成器は組合せを列挙してよいが、LAMINARIA plannerに求める性質は「必要な閉包だけを展開すること」である。
- 規模: 小=非要求4package、中=非要求10package（上記既定）、大=非要求30package（うち5つが内部依存鎖を持つ）。

---

## 3. M9/M10 の具体設計（必須項目1後半・2後半）

### 3.1 M9: `Nim CLI → Rust lib → Nim lib → Rust lib`

**言語間契約の選定**: #35本文が「ライブラリ」と明記していること、既存の(a)subprocess+JSON契約は自己ビルドで既に実証済みでありM9で繰り返す新規性がないことから、**(b) native C-ABI static-lib linking**をM9の全3 edgeに採用することを推奨する。契約の細目（scalar/pointer/opaque-handleのみ、by-value集約体は不可、panic/例外は境界を越えて伝播させない）は`fixtures/direct-native-link/NOTES.md`が実証した制約をそのまま踏襲する。

**実機能の割当て**: 「クロスツールチェーン環境適合性レポート」を題材にする。

| node | 言語/形態 | 実機能 | 既存/新規 | 入力 | 出力 | 呼出先 |
|---|---|---|---|---|---|---|
| N1 | Nim CLI（新規bin, 例: `nim-planner/src/fp_report_cli.nim`） | 引数解析(`--repo-root`, `--lock`)、レポート整形、終了コード決定(0=適合/1=不適合/2=エラー) | 新規（orchestrationのみ、既存Rust `Commands::Doctor`のUXを踏襲するが独立実装） | CLI引数 | stdout（human/JSON）、終了コード | N2 |
| N2 | Rust staticlib（新規薄いFFI shim crate `laminaria-fingerprint-ffi`） | `laminaria-fingerprint::env::detect`/`rust_toolchain::detect`/`nim_toolchain::detect`を`extern "C"`で再公開 | **既存ロジックの再公開**（`crates/laminaria-fingerprint/src/{env,rust_toolchain,nim_toolchain}.rs`を直接呼ぶ、ロジック自体は変更しない） | repo_root/lock path（C文字列） | opaque `FingerprintReportHandle*`（`direct-native-link`の`Counter`パターンに倣うhandle+create/use/freeライフサイクル） | N3 |
| N3 | Nim staticlib（新規, 例: `nim-planner/src/schema_compat.nim`） | スキーマバージョン互換性判定（`PlanSchemaVersion`とレポートが要求するバージョンのSemVer範囲比較、Compatible/Degraded/Incompatibleの3値を返す） | **新規だが実質を持つ実装**（現状`contract.nim`は定数のみで比較ロジックが存在しないことを確認済み。単体テストで3値それぞれを再現可能にする） | N2から渡されたtoolchainバージョン文字列群 | 判定結果（enum、C-ABI越しはcint） | N4 |
| N4 | Rust staticlib（既存ロジック再公開、例: `laminaria-run`の`store.rs`が持つ内容ハッシュ機構を薄いFFI shimで公開） | (環境fingerprint, toolchain fingerprint群, 互換性判定)のタプルに対する安定ダイジェスト計算 | **既存ロジックの再公開**（`laminaria-plan::compiler_work::Fnv1a`を実装参照として使う、または`sha2`crateを直接使う新規薄い関数。いずれもハッシュアルゴリズム自体は既存パターンの踏襲） | タプルのシリアライズ表現 | 固定長ダイジェスト文字列 | （末端） |

**検証すべき観測可能な性質**: 同一入力で2回実行して同一ダイジェストが得られること（決定性）、`--lock`に存在しないバージョンを与えた場合にN3がIncompatibleを返しN1が終了コード1で報告すること、panicをN2内で`catch_unwind`により捕捉しFFI境界を越える前に構造化エラーへ変換すること（`direct-native-link`のLayer4知見の直接適用）。

**native/WASM区別**: M9は全ノードがhost=実行機と同一のnativeターゲットのみを対象とする（WASM生成物は扱わない）。

### 3.2 M10: Rust LSP側／WASM側 → Nim lib由来の共通機能

**共有Nim機能**: `nim-planner/src/planning_kernel.nim`の`plan`/`findCycle`（**既存の本番ロジックそのもの**、issue#27で実装済み）を、新規の薄いFFI shim（`nim-planner/src/plan_ffi.nim`）で`{.exportc.}`公開する。入力はシリアライズされた`PlanningInput`（JSON文字列、呼出側所有のバッファ+明示的free関数）、出力はシリアライズされた`PlanOutcome`または循環診断。

| host | 言語/形態 | 実機能 | host/target | 生成物 |
|---|---|---|---|---|
| A（LSP） | Rust bin（新規, `laminaria-lsp`） | 編集中のmanifest/fixture記述ファイルに対し、`textDocument/didChange`契機で`PlanningInput`相当を再構成し、共有Nim libをnative linkで呼び出して循環依存をLSP診断（`Diagnostic.range`）として即時提示する | host=開発者機（例: aarch64-apple-darwin/x86_64-unknown-linux-gnu/x86_64-pc-windows-msvc）、target=host自身（nativeのみ、クロスコンパイルなし） | native実行バイナリ + native静的link済みNim lib |
| B（WASM） | Rust cdylib（新規, `laminaria-plan-wasm`） | 同一の共有Nim機能をブラウザ上のビルド計画ビジュアライザから呼び出す（同じ`PlanningInput`→循環/順序情報をJSに返す） | host=ビルド機（native）、target=WASMランタイム（ブラウザ/WASI） | `.wasm`モジュール（Rust cdylib + WASM向けにビルドされたNim lib、§4.3参照） |

両hostが**同一の実装済みNimロジック**（`planning_kernel.plan`/`findCycle`）に依存する点がM10の核心であり、native artifactとWASM artifactは明確に別成果物として区別する（同一バイナリとして扱わない、#35本文の要求通り）。

### 3.3 未確定事項と推奨判断（Nim→WASM経路、M10のみに影響）

**論点**: Nimソースをwasm32ターゲットへコンパイルし、Rustのwasm32 cdylibと1つの`.wasm`モジュールとしてリンクできるか。本リポジトリ内に前例は皆無（grep実証済み）。`docs/metrics-policy.md`が"WASM target-pipeline decomposition"/"wasm-ld"/"Binaryen"/"WIT/adaptation"を将来のtarget経路として言及しているのみで、実証はゼロ。

**調査結果（本書執筆時点での確認事項）**:
- Nimは`--os:wasi`（wasm32-wasi向け実験的backend、C経由）と、緊急性の低いemscripten経由（`nim c -d:emscripten`, C生成→emcc）の2経路が存在しうる（Nim公式ドキュメント上の言及。本リポジトリでの実行確認は未実施）。
- 直接linkする場合の対称性: Rust側は`wasm32-unknown-unknown`では任意のCライブラリをリンクできない制約があり、`wasm32-unknown-emscripten`か`wasm32-wasip1`をターゲットにする必要がある可能性が高い。

**推奨判断**: D1着手条件の一部として、**タイムボックス付きスパイクcase `M10-SPIKE`**を独立実行する。判定規則:
1. `nim c --os:linux -d:emscripten`（または`--os:wasi`）で`plan_ffi.nim`を単体コンパイルし、`.o`/`.a`成果物を得られるか。
2. 得られた成果物を、最小の`wasm32-unknown-emscripting`（または対応するRustターゲット）Rust cdylibとリンクし、1回の往復呼び出し（例: 空の`PlanningInput`→空の`PlanOutcome`）が成功するか。
3. 上記2ステップを**合計3回の実装試行**以内に達成できない場合、直ちにフォールバックへ切り替える。

**フォールバック（推奨判断のB案）**: WASM側限定で、Nimの`plan`/`findCycle`と**入出力が意味的に同値であることをテストで確認したRust実装**を用意し、「同一バイナリの共有」ではなく「同一ロジックの2言語提供（native側はNim実装、WASM側はRust実装、テストで同値性を保証）」としてM10を成立させる。これはコンパイラ所有契約（`docs/compiler-ownership-contract.md`）に抵触しない（LAMINARIA自身のNim/Rustロジックの範囲内であり、外部コンパイラへの委譲ではない）。この場合、M10の「共有」の主張は「同一実装」から「検証済み同値実装」へ格下げされることを仕様上明記し、指示者の採否対象とする。

この論点は指示者が次のいずれかを選ぶことで確定する: (i) スパイクを実施しA案（真のNim→WASM共有）を目指す、(ii) 最初からB案（同値性検証フォールバック）を採用しスパイクを省略する。本書はデフォルトとして(i)を推奨する（研究的価値が高く、コストはタイムボックスで制御されているため）。

---

## 4. 各caseの入力・期待結果・判定方法（必須項目4・5）

機械可読な詳細は[issue-35-d0-cases.yaml](issue-35-d0-cases.yaml)に格納する。本節はcase定義のschemaと横断的な設計判断のみを記す。

### 4.1 case定義のschema（YAML各エントリの構造）

```yaml
- id: string                      # 一意のcase ID（例: M1-A, M7-B-medium）
  pattern: M1..M10                # 対応するM-pattern
  origin: self | fixture-existing | fixture-new  # 自己構成/既存fixture流用/新規補完
  source_layout: string            # ソース配置の説明または既存パスへの参照
  dependency_edges: [...]          # 依存/非依存関係
  requested_artifacts: [...]       # 要求成果物
  demand_mode: full | partial | targeted-edit
  edit:
    kind: none | leaf-edit | shared-lib-edit | config-change
    target: string
  forbidden_work: [...]            # 実行してはいけない仕事
  required_work: [...]             # 実行すべき仕事
  expected:
    kind: value | diagnostic
    value_or_diagnostic: string
    derivation: string             # 期待値の導出根拠
  subset_scope:
    d1_verifies: [...]             # 現在のsubsetで検証する範囲
    future_work: [...]             # 後続実装が必要な範囲
  scale:
    small: {...}
    medium: {...}
    large: {...}
    seed: number
  measurement:
    comparison_modes: [...]        # #28 セクション5の1-4
    cpu_budget: [...]
    memory_state: [...]
    cache_state: [cold, warm, true-noop]
    warmup_runs: number
    repetitions: number
    noise_floor: string            # laminaria-run::scenario::NOISE_FLOOR_STDDEV_MULTIPLIER方式を参照
  pass_criteria:
    d1: string
    d4: string | not-applicable-yet
```

### 4.2 計測基盤の再利用（新規実装を最小化する）

`crates/laminaria-run/src/scenario.rs`が既に実装している`Stats::from_samples`（stddev基準のnoise floor）、`compare_reports`（`WallTimeVerdict`、`NOISE_FLOOR_STDDEV_MULTIPLIER`）、`ScenarioReport`の再現可能な保存/再生成機構を、M1〜M10の比較実行にもそのまま流用する。D1は新しい計測方式を発明せず、`laminaria scenario-run`/`scenario-compare`が受け付けるworkload種別をfixture単位で拡張する形にする。

比較する4方式は#28本文セクション5の定義をそのまま採用する:
1. LAMINARIA同一owned演算の逐次実行（CPU budget=1）
2. #27型の全体静的計画後のready並列実行
3. 需要駆動・増分協調方式（本Issueの主題、D2以降）
4. 3のablation（個別機構をOFFにした変種）

D1時点では1と2のみを取得する（3・4はD2以降）。CPU budgetは1/2/4/利用可能上限のうちhostで実行可能な値のみを使う。cold/warm/true-noopは`CacheStateLabel`の既存3値をそのまま使う。

### 4.3 D1で実行するcase・D2以降のcase（必須項目5後半）

| Pattern | D1で実装・実行 | D2以降 |
|---|---|---|
| M1, M2, M3, M6 | manifest宣言＋demand選択の正当性検証（cargo/nimの実挙動照合）、CPU budget=1のbaseline計測 | 需要駆動実行（増分Nim/Rust契約）との比較 |
| M4, M5 | 既存fixtureのcase化、実値照合（`main.rs`内assert）の再確認、CPU budget=1のbaseline計測 | M9実装後、自己構成での同型edgeとの比較 |
| M7, M8 | 新規fixture実装、宣言済み期待closure/critical pathとの照合 | ablation・distributed配置評価 |
| M9 | ノード実装（新規FFI shim含む）、決定性・診断値のテスト | 実行時計測（並行dispatchとの統合） |
| M10 | M10-SPIKE実施→native LSP側の実装・診断値テスト | WASM側の実装（スパイク結果次第でA/B分岐）、実行時計測 |

### 4.4 規模・seed・編集内容・組合せ選定規則（必須項目5）

- 小規模: 全列挙（fixture内の全ノード・全編集シナリオを実行、D1で全数消化可能な件数に抑える）。
- 中規模: §2.2/§2.3で規定した既定値を使用。
- 大規模: 固定seed（各fixtureごとに1つ、`docs`記載のとおり本プロジェクトが既に用いる`20260909`系の値を踏襲）で代表組合せのみを選び、選定規則（「鎖の中間から分岐」「wide側1本のみ重くする」等）と省略範囲をcase定義に明記する（YAML `scale.large.selection_rule`フィールド）。
- 編集対象: `full`（全要求）、`partial`（一部demand）、`leaf-edit`（末端1ファイル編集）、`shared-lib-edit`（共有producer編集）、`config-change`（依存宣言自体の変更）の5種を、既存の`incremental-semantic-edit`が確立した「宣言→実測照合」方式でcase化する。

---

## 5. D1合格条件とD4合格条件の分離（必須項目6・7）

### 5.1 D1合格条件（再現性・正当性・比較基準の取得）

- 各caseについて、宣言された依存/非依存関係が実際のビルドツール出力（`cargo build -v`のCompiling/Fresh行、Nimのコンパイル呼び出しログ）と一致すること。
- 宣言された「実行してはいけない仕事」（M8の`unused-pkg-*`、M1のfingerprint未変更時の3 consumer再ビルドなし等）が実測でゼロ回であること。
- 期待値/期待診断が、導出根拠（既存の実アルゴリズム出力、または新規実装の単体テスト）と一致すること。
- CPU budget=1の逐次baseline計測が、noise floor内で再現すること（`compare_reports`の`WallTimeVerdict`が同一run同士でBelowNoiseとなること）。
- M10-SPIKEの判定規則（§3.3）が実施され、A/Bいずれかの結果が記録されること。

D1は速度改善そのものを判定しない（#28本文に従う）。

### 5.2 D4合格条件（速度改善・回帰許容幅）

現時点では**根拠不足のため確定しない**。#28本文セクション5が要求する「チューニング前の事前登録」を満たすため、D4の具体的な改善目標・許容回帰幅は、D1で取得するCPU budget=1 baseline計測の実測値が出そろってから、その実測値を根拠として別途確定する。本書はこの手順自体（D1実測→D4目標確定という順序）を仕様として固定し、D1実装者に目標設定を委ねない。

---

## 6. 指示者の採否記録欄（必須項目8への接続）

以下は本書提出時点では空欄。指示者が採否と仕様revisionをここに記録した時点で、本書は「仕様確定」に昇格し、[issue-35-d0-d1-draft-instructions.md](issue-35-d0-d1-draft-instructions.md)がD1実装指示として発行可能になる。

| 論点 | 提案内容 | 指示者の採否 | 備考 |
|---|---|---|---|
| M1/M2/M3/M6は自己充足、新規サンプル不要 | 採用推奨 | （未記入） | |
| M4/M5は既存fixtureをそのままcase化 | 採用推奨 | （未記入） | |
| M7/M8は新規補完サンプルを追加 | 採用推奨（§2.2/§2.3の設計） | （未記入） | |
| M9の言語間契約はnative C-ABI（既存subprocess契約は使わない） | 採用推奨 | （未記入） | |
| M9のnode実機能割当て（環境適合性レポート） | 採用推奨（§3.1） | （未記入） | |
| M10のnode実機能割当て（planning_kernel.plan/findCycleの共有） | 採用推奨（§3.2） | （未記入） | |
| M10のWASM経路: スパイク実施→フォールバック規則 | 採用推奨（§3.3、(i)案をデフォルト） | （未記入） | (ii)案（フォールバック直行）も選択可 |
| D4合格条件はD1実測後に別途確定 | 採用推奨 | （未記入） | |

---

## 付録A: M2代替案（テストバイナリを"app"と認めない場合）

指示者が「`test_planning_kernel`はunittestバイナリでありproduct-facingな複数appの証拠として弱い」と判断した場合の代替: `fixtures/nim-heavy-workspace`の`primes.nim`/`geometry.nim`を共有libとして扱い、既存の`fixture.nim`に加えて第2の実バイナリ（例: 同じ2moduleを使うが異なる出力形式を持つ`fixture_cli.nim`）を追加する。この場合originは`fixture-existing`から`fixture-new-minor-extension`に変わる。D1着手前に指示者判断が必要な唯一の分岐点として明記する。

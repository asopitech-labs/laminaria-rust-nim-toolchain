# #35 D0 仕様案 — LAMINARIA自身の複雑な混成ビルド構成と補完サンプルの仕様

- 対象Issue: [#35](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/35)（親: [#28](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/28)）
- 位置付け: 本書は **ゴール設定担当（Claude）による提出仕様案**。ユーザーから採否記録を依頼された指示者（Codex）がセクション6で判定する。本書単独でD1着手を許可しない。
- 審査状態: **要改訂・D1未発行**（2026-09-11、提出commit `443b5f2959b1aa72be8cfb913f80fd92d79239f6`を審査、判定revision `issue35-d0-review-443b5f2-v1`）。セクション6は指示者の審査記録であり、提出者はこれを改変しない。
- **改訂状態（第1回改訂、commit `217b0c1`）**: セクション6のR1〜R5に対応して本書・cases.yaml・D1草案の3点を改訂した。対応内容の一覧はセクション7に記す。
- **改訂状態（第2回改訂、本改訂）**: 指示者による`217b0c1`の読み取りレビュー（発行を妨げる指摘6点、R1〜R5の残件）に対応した。対応内容の一覧はセクション8に記す。セクション0〜5・付録Aは改訂後の提出案。セクション6・7は各回の記録として保持し、改変していない。
- 基準commit: `b875e43303a7ec9a5215897ed9dd08c98b45c812`（2026-09-11時点のHEAD、tree clean）
- 併読ファイル:
  - [issue-35-d0-cases.yaml](issue-35-d0-cases.yaml) — 機械可読case定義（本書各節が参照するcase IDの実体）
  - [issue-35-d0-d1-draft-instructions.md](issue-35-d0-d1-draft-instructions.md) — 指示者確定後に発行するD1実装指示の草案

## 0. 要約（先に結論）

- LAMINARIA自身の実crateグラフは、**Cargoという既存ツールの実挙動を参照(reference)evidenceとして使う限りで**、M1・M2・M3(構成部分)・M6の構成的性質（fan-out/fan-in、非依存な独立群、共有producerの単一実体化）を新規サンプルなしで示せる。これはLAMINARIA独自スケジューラの達成証拠ではない（compiler-ownership-contractの「Reference / baseline」「Delegated-build baseline」区分に従う）。
- M3にはこれとは別に、**LAMINARIA自身が所有する`compiler_work_executor`が実際に2つの独立な言語鎖（実Rust source由来・実Nim source由来）をCPU budget 1/2で並行実行し結果一致を確認する既存テスト**（`independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`）があり、これは**owned**実行の実証済み証拠として別case化する。
- M4/M5は既存の`fixtures/rust-nim-c-abi-baseline`等（M4）と`fixtures/direct-native-link`（M5）が実値受け渡しを実証しているが、これらは外部コンパイラ(`nim c`/`rustc`)による静的link成功のreference証拠であり、LAMINARIA独自コンパイル・実行の達成を意味しない。M9実装後、自身のowned経路で同型edgeの証拠が別途必要になる。
- M7・M8は自身の構成でもfixturesの現状でも覆えないため、新規補完サンプルが必要（本改訂で数値・分岐点・編集差分を確定した）。
- M9（Nim CLI→Rust lib→Nim lib→Rust lib）は、LAMINARIA自身のIR内呼出・型・所有・失敗の意味契約をまず定義し、native C-ABI static-link実装は**その意味契約を評価するための一候補・参照実装**として位置付ける（M9本経路の必須semantic境界としては採用しない。最終的なtarget境界選定は#5 T0に委ねる）。
- M10はnative側・WASM側の両方が**同一のNim実装済み機能に実際に依存すること**を維持し、WASM側をRust実装へ置換するフォールバックは削除した。Nim→WASM経路が実現しない場合はM10を未達として保持し、不足能力を記録して後続実装待ちとする。
- D4合格条件（速度改善・回帰許容幅）はD1実測後に別タスクとして確定する（変更なし）。

---

## 1. 棚卸し — 現行実機能・依存・不足（必須項目1）

### 1.1 実在コンポーネントと依存グラフ（`crates/*/Cargo.toml`実測、HEAD `b875e43`）

```text
laminaria-fingerprint  (leaf; env/toolchain/lock比較・doctor)
   ← laminaria-plan (直接依存)
   ← laminaria-run (直接依存)
   ← laminaria-cli (直接依存)
laminaria-ir            (leaf; source-derived IR/interpreter/transform, Rust実装)
   ← laminaria-run (直接依存)
laminaria-plan           (→ laminaria-fingerprint)
   ← laminaria-run (直接依存)
   ← laminaria-cli (直接依存)
laminaria-run            (→ fingerprint, ir, plan;  bin: laminaria-rustc-wrapper, laminaria-cc-wrapper)
   ← laminaria-cli (直接依存)
laminaria-cli             (→ fingerprint, run, plan;  bin: laminaria)
nim-planner               (Nim; contract.nim + planning_kernel.nim + laminaria_planner.nim本体、
                            tests/test_planning_kernel.nimも同じ2moduleを共有 import)
```

矢印は「上が下に依存」。出典: `crates/laminaria-cli/Cargo.toml:14-17`, `crates/laminaria-run/Cargo.toml:10-24`, `crates/laminaria-plan/Cargo.toml:11-14`, `crates/laminaria-fingerprint/Cargo.toml`（内部依存なし）, `crates/laminaria-ir/Cargo.toml`（内部依存なし）, `nim-planner/tests/test_planning_kernel.nim:8-9`, `nim-planner/src/laminaria_planner.nim:23-24`。

**訂正（本改訂）**: `laminaria-cli`は`laminaria-run`と`laminaria-plan`の両方を**直接**依存に持つ（`laminaria-run`経由の間接依存だけではない）。したがって`laminaria-plan`は`laminaria-cli`から2経路（直接、および`laminaria-run`経由）で到達され、`laminaria-fingerprint`は`laminaria-cli`から少なくとも3経路（直接、`laminaria-run`経由、`laminaria-plan`経由、および`laminaria-run→laminaria-plan`経由の計4経路）で到達される。これは「独立した3 consumer」ではなく「単一の最上位要求(`laminaria-cli`)が複数経路で同一producerへ収束する」構造であり、§2.1で正確な形に訂正する。

### 1.2 実機能インベントリ（既存 / 要拡張 / 未着手）

| 領域 | 実機能（既存） | ファイル | 未対応・要新規実装 |
|---|---|---|---|
| CLI（Rust） | `Doctor`/`Run`/`RegenerateSummary`/`ScenarioRun`/`ScenarioRegenerate`/`ScenarioCompare`/`PlanSelfBuild`/`SelfBuild`/`PlanBuild`/`Build`の10 subcommand | `crates/laminaria-cli/src/main.rs` | **Nim CLIは存在しない**（repo全体grep `-rniE "\bwasm\b|\blsp\b|language.?server"` --include=*.rs,*.toml,*.nim,*.md、`.reference/`と`target/`除外で0件、本書執筆時に実行し確認） |
| Planner/Runtime | Nim `planning_kernel.plan`（Kahn topo sort + cycle検出、demand closure、issue#27で実装）; Rust `compiler_work_executor::run_compiler_work_plan`（CPU budget付き並行dispatch、issue#27） | `nim-planner/src/planning_kernel.nim`, `crates/laminaria-run/src/compiler_work_executor.rs` | メモリ会計・キャンセルは issue#27自身の既知の残課題（本書のスコープ外、変更しない） |
| IR処理 | `laminaria-ir`: `rust_frontend`/`nim_frontend`（宣言済みsubsetのみ）、`interpreter`（実行トレース）、`transform::{checked_inline, anf_insert}`、`validate` | `crates/laminaria-ir/src/*.rs` | **訂正（本改訂）**: 「実Nim plannerに未接続」という記述は誤りだった。`crates/laminaria-run/src/compiler_work_executor.rs`の`the_full_lower_validate_transform_validate_evaluate_pipeline_runs_end_to_end`と`independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`は、実際に本物の`laminaria-planner`バイナリで計画され、`laminaria_plan::validate::validate`で検証され、実行される経路を既に持つ。**実際に不足しているのは「研究用CLI／製品CLIサブコマンドとしてこの経路を任意のsourceに対して呼び出す入口が未公開」という点のみ**（`laminaria-cli`の10 subcommandのいずれもLowerSource/TransformFunction等を直接要求する経路を持たない）。issue#27自身の残課題（メモリ会計・キャンセル）とは別の、CLI表面の欠落として区別する |
| 共通ライブラリ（Rust） | `laminaria-fingerprint`（env/toolchain検出、doctor、比較） | `crates/laminaria-fingerprint/src/*.rs` | M9のnodeが要求する「固定snapshot入力モード」は未実装（§3.1参照） |
| 共通ライブラリ（Nim） | `contract.nim`（`PlanSchemaVersion`とスキーマ相互変換）、`planning_kernel.nim::planFromJson`（既存の厳密なschema/version一致gate） | `nim-planner/src/*.nim` | スキーマ/capability適合性判定を`planFromJson`の既存gateと同じ厳密一致方式でM9向けに拡張する実装は未着手（§3.1参照） |
| 言語間境界契約 | (a) subprocess+JSON: `laminaria-plan::nim_planner_client`が`laminaria-planner`バイナリをspawnしJSON stdin/stdoutで通信（自己ビルドの唯一の実運用経路） / (b) native C-ABI static-lib linking（scalar/pointer/opaque-handleのみ）— `fixtures/direct-native-link/NOTES.md`が実証済み | `crates/laminaria-plan/src/nim_planner_client.rs`, `fixtures/direct-native-link/NOTES.md` | (a)(b)いずれも**物理的な target 境界の実現手段**であり、LAMINARIA自身のIR内呼出・型・所有・失敗の**意味契約そのものではない**。この意味契約自体はまだ定義されていない（§3.1で着手） |
| LSP | なし | — | 完全新規（M10で設計） |
| WASM | なし（`docs/agent-oriented-toolchain-ux.md:23`と`docs/metrics-policy.md`が将来のtarget経路として言及するのみで実装ゼロ） | — | 完全新規（M10で設計。実現可否は#5 T0の確定と独自target生成能力に依存し、本書はM10を「未達として保持」する選択肢を残す） |

### 1.3 意思決定：本書が新規に追加する機能は「実機能」の基準を満たすか

#35本文「無意味なwrapperやリンクされるだけのライブラリで複雑さを水増ししない」を満たすため、M9/M10で新設するnodeは以下の基準を満たす場合のみ許可する。
1. 入力から出力への変換が実際に計算を行い、固定値・恒等関数ではないこと。
2. 単体テストで検証可能な、観測可能な性質を持つこと。
3. LAMINARIA自身が将来的に必要とする現実の能力（環境/ツールチェーン適合性判定、依存グラフ健全性診断など）に対応すること。
4. **（本改訂で追加）** 各nodeの入出力は、まず抽象的な意味契約（呼出形状・型・所有・失敗）として記述し、物理的なlinkage形式（C-ABI staticlib等）はその契約を評価する候補実装として別に扱う。物理形式の選択がそのまま意味契約になることを許さない。

この基準に基づき、§3で設計するM9/M10の各nodeは「既存の実装を流用」と「新規だが実質を持つ実装」を明示的に区別する（§3.1/§3.2の表を参照）。

---

## 2. M1〜M8: 自身の構成での充足度と不足の切り分け（必須項目2前半・3）

`fixtures/`配下11ディレクトリの精査（Explore agent、HEAD `b875e43`同一commitで実施、file:line引用付き）と、`crates/*/Cargo.toml`実測を突き合わせた結果。**本改訂で全行に`execution_role`（owned/reference/bootstrap）を追加し、独立性の主張を実際の依存関係に合わせて訂正した。**

`execution_role`の定義（compiler-ownership-contractの役割表に対応）:
- **owned**: LAMINARIA自身のIR/scheduler(`compiler_work_executor`)が実際に計算・実行する経路の証拠。
- **reference**: 既存ツール（Cargo/Nimコンパイラ等）の実挙動を観測する証拠。LAMINARIA独自経路の達成を意味しない。
- **bootstrap**: 自己ビルド等、外部ツールでLAMINARIA自身の実行体を組み立てる経路の証拠（compiler-ownership-contractの「External bootstrap」「Delegated-build baseline」に対応）。

| Pattern | execution_role | 自身の構成での充足 | 根拠 | 追加サンプル要否 |
|---|---|---|---|---|
| **M1**（Rust-only、複数app→共有lib） | reference | **構成として自己充足**（Cargoの実挙動として） | `laminaria-rustc-wrapper`/`laminaria-cc-wrapper`（`crates/laminaria-run/Cargo.toml:10-16`の2 `[[bin]]`）が`laminaria-run`自身のlibモジュールを共有。加えて`laminaria-cli`が複数経路で`laminaria-fingerprint`/`laminaria-plan`へ到達する（§2.1） | 不要。case化のみ行う。Cargoの重複排除自体はLAMINARIA独自scheduler証拠として数えない |
| **M2**（Nim-only、複数app→共有lib） | reference | **自己充足** | `nim-planner/src/laminaria_planner.nim`と`nim-planner/tests/test_planning_kernel.nim`がともに`contract.nim`+`planning_kernel.nim`を`import`（`laminaria_planner.nim:23-24`, `test_planning_kernel.nim:8-9`実測）。指示者はこのunittestバイナリを第2consumerとして採用した（付録A不要） | 不要 |
| **M3-topology**（独立Rust群+独立Nim群、cross-edgeなし・構成のみ） | bootstrap | **自己充足（構成のみ）** | 自己ビルドの`SelfBuild`が構成する`PlanningInput`は`compile-rust-host`（`ActionKind::CargoBuild`）と`compile-nim-planner`（`ActionKind::NimBuild`）を互いのinputs/outputsで接続せず、`integrate`のみが両方を消費する（`nim_planner_client.rs::sample_input()`）。ただし`CargoBuild`/`NimBuild`は`compiler_work_executor`が扱わない委譲ビルド種別であり、この経路の並行実行はLAMINARIA独自scheduler証拠ではない | 不要（構成の証拠として） |
| **M3-owned**（独立な2つの実言語鎖をowned schedulerがCPU budget 1/2で並行実行） | **owned** | **自己充足・実装済み** | `crates/laminaria-run/src/compiler_work_executor.rs`の`independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`（同ファイル1604行目付近）が、実Rust source由来チェーンと実Nim source由来チェーンをそれぞれ`LowerSource→ValidateIr→EvaluateEvidence`（3 Action、TransformFunctionは含まない、`plan.actions.len()==6`のassertで確認済み）で独立に並行実行し、budget1/2の結果一致と実測並行度(peak=1/2)を確認済み。**訂正（本改訂）**: 前版は別テスト（`the_full_lower_validate_transform_validate_evaluate_pipeline_runs_end_to_end`）が使う5段(Transform込み)のパイプラインと本テストの3段パイプラインを混同していた。新規Transform実装・テストは追加しない | 不要。既存テストをcase登録するのみ |
| **M4**（Rust entry→Nim lib） | reference | 自身の製品コードでは未充足。**既存fixtureで充足（reference）**: `rust-nim-c-abi-baseline`（scalarのみ）、`mixed-rust-nim-executable`（配列ポインタ往復）、`boundary-heavy-workload`（100万回呼び出し） | Explore agent report; 各`main.rs`のassert定数 | 不要（既存資産を正式case化。owned証拠ではないと明記） |
| **M5**（Nim entry→Rust lib） | reference | 同上、自身の製品コードでは未充足。**既存fixtureで充足（reference）**: `direct-native-link/nim-bin` | `fixtures/direct-native-link/NOTES.md`全体 | 不要（既存資産を正式case化。owned証拠ではないと明記） |
| **M6**（複数経路が単一producerへ収束、重複要求の合流） | reference | **構成として自己充足（訂正版）** | `laminaria-cli`（単一の最上位要求）は`laminaria-plan`へ2経路（直接依存、および`laminaria-run`経由）、`laminaria-fingerprint`へ3経路以上で収束する。Cargoが`laminaria-plan`/`laminaria-fingerprint`を経路数に関わらず1回だけコンパイルすることを実測で確認できる（§2.1） | 不要 |
| **M7**（長鎖+wide独立枝、不均等サイズ） | 混在（fixture自体の実行は将来ownedへ拡張予定、D1では図形状の正当性検証まで） | **不足**。既存2fixtureは統合されず、module sizeも均一 | Explore agent report | **必要**: 新規補完サンプル`fixtures/long-chain-wide-branches`（§2.2、本改訂で数値確定） |
| **M8**（多数の未要求package/profile/target） | reference（Cargo非build）+ owned（Nim planner非展開、対応する場合） | **不足** | Explore agent report | **必要**: 新規補完サンプル`fixtures/many-unrequested-targets`（§2.3、Cargo/Nimの判定を分離） |

### 2.1 M1/M6の具体node対応（自己充足の詳細、訂正版）

| node | 言語 | 実機能 | 入力 | 出力 | 直接依存 |
|---|---|---|---|---|---|
| `laminaria-fingerprint`（producer） | Rust lib | 環境/ツールチェーン検出 | repo_root, lock path | `EnvironmentFingerprint`/`ToolchainFingerprint`値 | なし |
| `laminaria-plan`（中間producer、`laminaria-fingerprint`のconsumer） | Rust lib | `PlanningInput→PlanOutcome`契約型、`validate` | fingerprint型 | 契約型 | fingerprint |
| `laminaria-run`（`laminaria-fingerprint`と`laminaria-plan`両方のconsumer、2bin producer） | Rust lib+2bin | tracer/wrapper/scenario/self_build/compiler_work_executor | fingerprint, ir, plan | Run記録・実行計画結果 | fingerprint, ir, plan |
| `laminaria-cli`（単一の最上位consumer、1bin producer） | Rust bin | 10 subcommand | 上記すべて | CLI出力 | fingerprint, run, plan（**3つとも直接依存**） |

**訂正した収束構造**: `laminaria-cli`は`laminaria-fingerprint`へ次の4経路すべてで到達する — (1) 直接、(2) `laminaria-run`経由、(3) `laminaria-plan`経由、(4) `laminaria-run→laminaria-plan`経由。`laminaria-plan`へは(1)直接、(2)`laminaria-run`経由の2経路。これらは独立した別々の要求者ではなく、**単一の要求(`cargo build -p laminaria-cli`)が持つ推移的閉包内での複数経路収束**である。M6が検証すべき性質は「経路数に関わらずCargoが各producerを1回だけコンパイルすること」であり、「独立した複数要求者の重複要求合流」ではない（後者は将来owned schedulerで別途検証する対象として区別する）。

需要選択テストの一例（D1で実装、cases: `M1-fingerprint-cold`/`M1-fingerprint-noop`/`M1-fingerprint-leaf-edit`、`M6-diamond-fingerprint-plan`）: `cargo build -p laminaria-fingerprint`のみを要求した場合に`laminaria-plan`/`laminaria-run`/`laminaria-cli`が再ビルドされないこと、`laminaria-fingerprint`のsourceを1行編集した場合に全4crateが再ビルドされ`laminaria-fingerprint`自体が1回のみコンパイルされること（4経路の収束点で重複コンパイルが起きないこと）を、実際の`cargo build -v`出力で検証する。**coldケースとnoop/warmケースは別case（`M1-fingerprint-cold`と`M1-fingerprint-noop`）に分離し、同一caseで「初回生成を要求しつつ未変更時は生成禁止」を同時に求めない（R4対応）。**

### 2.2 M7補完サンプル設計: `fixtures/long-chain-wide-branches`（本改訂で数値確定）

既存の2fixture（`deep-critical-path-graph`, `wide-parallel-graph`）は変更せず、新規に1つのCargo workspaceとして起こす。

**確定した規模と分岐点**（指示者指定通り）:

| 規模 | 鎖段数 | wide leaf数 | 分岐点 |
|---|---|---|---|
| 小 | 4 | 2 | `stage-02` |
| 中 | 8 | 4 | `stage-04` |
| 大 | 16 | 8 | `stage-08` |

**鎖側の実装（本改訂で完全に閉じた式として確定、P1/R3対応 — 「splitmix64風」のような非閉形式表現は用いない）**: `deep-critical-path-graph`は参照インスピレーションとしてのみ引用し、本fixture自身の変換は以下の3つの独立した変換として新規に定義する（`deep-critical-path-graph`の12段の具体的な各段実装を流用するのではない）。

```text
transform_0(x: u64) -> u64 {              // avalanche_mix
    let mut z = x;
    z ^= z >> 33;
    z = z.wrapping_mul(0xff51afd7ed558ccd);
    z ^= z >> 33;
    z
}
transform_1(x: u64) -> u64 {              // popcount_fold
    let p = x.count_ones() as u64;
    x.wrapping_add(p.wrapping_mul(p))
}
transform_2(x: u64) -> u64 {              // modulus_normalize
    (x % 1_000_000_007).wrapping_add(x.trailing_zeros() as u64)
}

stage_01::run() -> u64 { transform_0(SEED) }             // SEED: u64 = 20260909
stage_NN::run() -> u64 for NN >= 2 {
    transform[(NN - 1) % 3](stage_(NN-1)::run())
}
```

**分岐の実装**: 分岐点`stage-BB`（BBは上表）の出力を、鎖の継続（`stage-(BB+1)`）と、wide leaf群の入力の両方に渡す（`stage-BB`は2つ以上のconsumerを持つ、実際のfan-outノード。依存方向は「leaf-* は stage-BB に依存する」「stage-(BB+1) は stage-BB に依存する」であり、データがstage-BBの出力から下流へ流れる向きと、依存が指す向きは逆であることに注意する — cases.yamlの`dependency_edges`は依存方向のみを表す）。

**wide leaf側（本改訂で完全に閉じた式として確定）**:
```text
leaf_a::run(input: u64) -> u64 { gcd_u64(input, 97) }    // 実Euclidean算法
leaf_b::run(input: u64) -> u64 {
    // input.to_le_bytes()の8バイトを実bubble sortで昇順に並べ替え、
    // 並べ替え後のバイト列をu64::from_le_bytesで再解釈する
}
leaf_c::run(input: u64) -> u64 {
    // PRIMES_UNDER_256(固定の昇順ソート済みu8配列、2,3,5,7,...,251)から
    // target=(input % 256) as u8 を実binary searchで検索し、
    // 見つかった場合はindexをu64として、見つからない場合は255を返す
}
leaf_matrix_sum::run(input: u64, n: u64) -> u64 {
    // n*n行列、matrix[i][j] = input.wrapping_add(i).wrapping_mul(j + 1)
    // を構築し、全要素をwrapping_addで合計する(source側は4つの補助関数
    // 「行生成」「セル計算」「行合計」「全体合計」に分割してIRノード数を
    // 意図的に増やす。他のleafは単一関数のまま)
}
```

不均等サイズは2つの独立軸で表現する（R4対応）:
- 軸1（source/IR量、LAMINARIA所有の解析・変換コストに影響）: `leaf_matrix_sum`のみ4補助関数に分割（他leafは単一関数）。
- 軸2（実行時payload量、interpreter/実バイナリの実行時間に影響）: `leaf_matrix_sum`の`n`を既定`n=8`、大規模設定でのみ選定された2本を`n=80`（10倍）に拡大する。
- この2軸は独立に変化させる。「行列を大きくしたので compiler work が重くなった」という主張はしない。軸1（source量）はLAMINARIA所有の解析コストの計測対象、軸2（payload量）はEvaluateEvidenceの実行時間の計測対象として、測定結果を別々の指標で記録する。

**大規模leafレシピ（8本、選定規則を完全に閉じる）**: `leaf-01`から`leaf-08`のうち、`leaf-01`/`leaf-02`は`leaf_matrix_sum(n=80)`（重い、軸1・軸2とも大）とし、`leaf-03`から`leaf-08`は`i mod 3`（`i`は`leaf-NN`のNNから3を引いた0始まり番号、すなわち`leaf-03`→`i=0`）で`leaf_a`/`leaf_b`/`leaf_c`のいずれかを選ぶ（`leaf_a`にはさらに`i`を定数へ加算し重複を避ける: `gcd_u64(input, 97 + i)`）。

**aggregatorの集約式（本改訂で確定、P1/R3対応）**:
```text
aggregator::run() -> u64 {
    let mut acc = stage_final::run();          // stage_04 (small), stage_08 (medium), stage_16 (large)
    for leaf in leaves {                        // 分岐点の出力を各leafへ渡した結果
        acc = acc.wrapping_add(leaf::run(branch_point_output));
    }
    acc
}
```

**編集シナリオ（具体的before/after、閉じた式で確定）**: `M7-*-medium`のedit caseは、`stage-04`の変換式（`transform_0`、`(04-1) % 3 == 0`なのでavalanche_mixが選ばれる）に使う`wrapping_mul`定数を`0xff51afd7ed558ccd`から`0xc4ceb9fe1a85ec53`へ変更する（MurmurHash3のfinalizer定数対で実在する値の入れ替えであり恣意的な値ではない）。この編集の結果、`stage-04`以降の鎖（`stage-05..08`）、`leaf-a/b/c/leaf-matrix-sum`全4本（分岐点より下流）、`aggregator`が再コンパイル対象になり、`stage-01..03`は対象外になる。

**期待値の扱い（既存fixtureと同じ確立済み方法論）**: 上記の式・分岐点・編集差分・分離軸・aggregator集約式・大規模leaf選定規則はすべて本節で閉じた形式として確定した。実行結果の具体的な数値（`u64`の最終集約値）のみ、`deep-critical-path-graph`の`EXPECTED_RESULT`が実際にコードを実行して得た値を定数として固定したのと同じ方法で、D1が実装時に一度だけ実行して得た値をpinする。「実装結果をそのままgoldenにすると誤実装まで正解になる」という懸念には、上記の閉じた式・regression test（各transform/leaf関数を個別にunit testする）をD1実装の必須要件として明記することで対応する — 誤実装は個別unit testで検出され、集約値のpinはunit test通過後にのみ行う。

**検証する性質**: critical path長（鎖側の段数）、head-of-line blocking非発生（wide側の重いleafが他の軽いleafの実行を不必要にブロックしないこと、軸2の実行時間が長くても軸1の解析コストとは独立に扱われること）。

### 2.3 M8補完サンプル設計: `fixtures/many-unrequested-targets`（Cargo/Nimの判定を分離、R1/R3対応）

- 構成: 1つのCargo workspaceに、実際に要求される1本の`fixture-bin`が依存する2つのcrate（`used-core`, `used-util`）に加え、**要求されない**10個の兄弟crate（`unused-pkg-01`〜`unused-pkg-10`、うち3つはさらに独自の内部依存鎖を持つ）を配置する。加えて、`fixture-bin`自体に`[profile.release]`と`[profile.dev]`の2profile、および`[[bin]]`を2つ（要求されるのは1つのみ）宣言する。
- **2つの独立した判定に分離する（R1/R4対応）**:
  1. **Cargo非buildの判定（execution_role: reference）**: `cargo build -v`のログに`unused-pkg-*`のCompiling行が一切出現しないこと。これはCargo自身の依存解決の実挙動であり、LAMINARIA独自plannerの証拠ではない。
  2. **Nim planner非展開の判定（execution_role: owned、対応するcaseがある場合のみ）**: 同型の「多数の未要求package」構成をNim側の`PlanningInput`として与えた場合に、`planning_kernel.plan`が要求されたdemand closure外のactionを`ExecutionPlan`へ一切含めないこと（issue#27で実装済みの`demand_closure`ロジックの実測確認）。これは既存の`nim_planner_client.rs`のテスト群を土台にでき、新規fixtureのCargo構成とは別に、`PlanningInput`のみで軽量に構成できる。
- 規模: 小=非要求4package、中=非要求10package（上記既定）、大=非要求30package（うち5つが内部依存鎖を持つ）。

---

## 3. M9/M10 の具体設計（必須項目1後半・2後半）

### 3.1 M9: `Nim CLI → Rust lib → Nim lib → Rust lib`

**位置付けの訂正（本改訂、R1/R2対応）**: M9の主目的は「LAMINARIA自身のIR内呼出・型・所有・失敗の意味契約」を各edgeについて定義することである。native C-ABI static-lib linkingは、この意味契約を物理的に評価するための**候補実装の一つ**であり、M9本経路が必須とするsemantic境界ではない。最終的なtarget境界（どの物理形式を採用するか）は#5 T0の決定に従う。以下の各edgeは、まず抽象契約（呼出形状・所有・失敗）を記述し、そのあとで「D0時点での参照実装候補」としてnative C-ABIを併記する。

**実機能の割当て**: 「クロスツールチェーン環境適合性レポート」を題材にする。

| edge | 抽象意味契約（所有・失敗） | 参照実装候補（native C-ABI、#5 T0確定まで暫定） |
|---|---|---|
| N1→N2 | N1はrepo_root/lock pathの2文字列を**貸与**（呼び出し後もN1が所有権を保持、N2はコピーのみ許容）。N2は環境snapshot値を**譲渡**返却（以後の解放責任はN1側、または明示的な解放関数呼び出し）。失敗はN2内部で捕捉し、構造化エラー値としてのみ返す（panicの越境不可） | opaque `FingerprintReportHandle*`（`direct-native-link`の`Counter`パターン、create/use/free） |
| N2→N3 | N2は正規化済みの要求（requested capability集合）を**貸与**。N3は判定結果（Compatible/Incompatible）と、判定に使った正準化入力を**譲渡**返却 | cint enum + 正準化入力を指すopaque handle |
| N3→N4 | N3は判定結果と正準化入力を**貸与**。N4はSHA-256ダイジェスト（固定長バイト列）を**譲渡**返却 | 固定長バッファへのポインタ書き込み（呼出側確保） |

**訂正したnode設計（R3対応、各nodeの責務を具体化）**:

| node | 言語 | 実機能（責務を限定） | 既存/新規 |
|---|---|---|---|
| N1 | Nim CLI（新規bin, 例: `nim-planner/src/fp_report_cli.nim`） | 引数解析(`--repo-root`, `--lock`, テスト時は`--fixed-snapshot <path>`)、N2〜N4を経て返る判定結果とダイジェストの整形出力、終了コード決定(0=Compatible/1=Incompatible/2=入力エラー) | 新規（orchestrationのみ） |
| N2 | Rust staticlib（新規薄いFFI shim crate `laminaria-fingerprint-ffi`） | (a) 通常モード: `laminaria-fingerprint::env::detect`等を呼び出し**実環境**をsnapshotする。(b) **固定snapshotモード（本改訂で追加、R3対応）**: `--fixed-snapshot`経由で注入された固定値をそのまま採用し、ambient環境に依存しない決定的な入力を可能にする。加えて要求されたcapability集合を正規化する | (a)既存ロジックの再公開。(b)は新規（テストの決定性のために必須） |
| N3 | Nim staticlib（新規, 例: `nim-planner/src/schema_compat.nim`） | **再訂正（本改訂、P1/R3対応）**: 前版は「capability完全一致」と書きつつ実装記述に部分集合(⊆)判定が残っており、かつ`contract.nim::PlanSchemaVersion`（実際の値は単一文字列`"0.2.0"`であり、capability集合ではない）とcapability集合を混同していた。正しくは**2つの独立した判定**: (判定1) `schema_version`の**単一文字列としての完全一致**（`PlanSchemaVersion`実値`"0.2.0"`との比較、`planFromJson`と同一のgate）。(判定2) `requested_capabilities`と`provided_capabilities`という別概念の**集合としての完全一致**（過不足なしの等価性、部分集合(⊆)では成立しない）。判定1が偽の場合は判定2を評価せず即座に構造化エラーを返す | 新規だが実質を持つ（既存gateと同じ「厳密一致」の考え方を、schema_versionとcapability集合という2つの別対象へそれぞれ適用） |
| N4 | Rust staticlib（既存ロジック再公開） | N3の判定結果と正準化入力を受け取り、固定の正準化書式（下記）でSHA-256ダイジェストを計算する。**このダイジェストはN3→N2→N1へ返却される値の一部であり、装飾的な末端ではない**（結果に含まれ、N1の最終出力に表示される） | 既存の`sha2`依存（`laminaria-fingerprint`/`laminaria-run`の既存Cargo依存）を直接使う新規薄い関数 |

**固定入力・正準化規則（本改訂で3系列に確定、P1/R3対応）**:
- (a) 成功系列: `schema_version="0.2.0"`（実値と一致）、`requested_capabilities = provided_capabilities = ["env-fingerprint-v1", "toolchain-fingerprint-v1"]`（集合として完全一致） → 判定1=真・判定2=真 → exit 0, Compatible。
- (b) capability不一致系列: `schema_version="0.2.0"`（一致）、`requested_capabilities = ["env-fingerprint-v1", "toolchain-fingerprint-v1", "unknown-capability-v1"]`、`provided_capabilities`は(a)と同じ2要素（部分集合にはなるが完全一致ではない） → 判定1=真・判定2=偽 → exit 1, Incompatible。
- (c) 不正入力系列: `schema_version="0.1.0"`（`contract.nim`実値`"0.2.0"`と不一致） → 判定1=偽 → exit 2, 構造化エラー（`rrkInvalidContractVersion`相当）。判定2は評価されない。
- 正準化書式（(a)(b)いずれの判定にも適用、(c)はN4に到達しない）: `requested_capabilities`と`provided_capabilities`をそれぞれ辞書順にソートし、`"|"`区切りで連結した文字列を`"SCHEMA:{schema_version}\nREQ:{req}\nPROV:{prov}\nRESULT:{Compatible|Incompatible}"`の形にまとめ、UTF-8バイト列としてSHA-256にかける。この書式自体はD0で確定済み。実際の64桁16進ダイジェスト値は、既存fixtureと同じ方法論（実装時に一度実行して得た値をpinする）で確定する。

**検証すべき観測可能な性質**: 同一系列を2回実行して同一ダイジェストが得られること（決定性）、(b)系列でN3がIncompatibleを返しN1が終了コード1で報告すること、(c)系列でN1が終了コード2を返しN4に到達しないこと、panicをN2内で`catch_unwind`により捕捉しFFI境界を越える前に構造化エラーへ変換すること。

**native/WASM区別**: M9は全ノードがhost=実行機と同一のnativeターゲットのみを対象とする（WASM生成物は扱わない）。

**D1の範囲（本改訂、P1/R5対応）**: D1は上記3系列・正準化書式・期待出力をcase定義として確定するのみで、N1〜N4の実コード（`fp_report_cli.nim`/`laminaria-fingerprint-ffi`/`schema_compat.nim`/digest shim）とnative C-ABIリンクの実装は行わない。実装は確定した本設計を入力とする後続実装ステージの作業とする（reached_stage: configuration-definitionまでがD1の到達範囲）。

### 3.2 M10: Rust LSP側／WASM側 → Nim lib由来の共通機能

**共有Nim機能**: `nim-planner/src/planning_kernel.nim`の`plan`/`findCycle`（既存の本番ロジックそのもの）を、新規の薄いFFI shim（`nim-planner/src/plan_ffi.nim`）で公開する。

**訂正（R2対応、必須）**: native側・WASM側の**両方**が、この同一Nim実装に実際に依存することをM10の必須条件として維持する。WASM側をRust実装に置換するフォールバックは**削除した**。Nim→WASMの経路が実現しない場合、M10-wasm-sideは「未達」として保持し、不足している独自target/runtime生成能力を具体的に記録した上で後続実装待ちとする（D1で無理に完了させない）。

| host | 言語/形態 | 実機能 | host/target | 生成物 |
|---|---|---|---|---|
| A（LSP） | Rust bin（新規, `laminaria-lsp`） | 編集中のmanifest記述ファイルに対し循環依存をLSP診断として提示する（下記の固定入力・期待診断を参照） | host=開発者機（native）、target=host自身 | native実行バイナリ + native静的link済みNim lib |
| B（WASM） | Rust cdylib（新規, `laminaria-plan-wasm`） | 同一のNim実装（`plan_ffi`）が提供する循環検出/順序決定機能を、WASM側でも**同一実装**として利用する | host=ビルド機（native）、target=WASMランタイム | `.wasm`モジュール（Nim由来ロジックを含む） |

**M10-lsp-nativeの固定入力・期待診断（本改訂で行番号まで確定、P1/R3対応）**:

BEFORE（循環なし、1-indexed行番号）:
```text
1: demand: [artifact-a]
2: actions:
3:   action-a: produces artifact-a, requires artifact-b
4:   action-b: produces artifact-b
```
AFTER（4行目を編集、`requires artifact-a`を追加して循環を導入）:
```text
1: demand: [artifact-a]
2: actions:
3:   action-a: produces artifact-a, requires artifact-b
4:   action-b: produces artifact-b, requires artifact-a
```
期待診断（AFTER状態）: `RejectionReasonKind::Cycle`、`cycle_path = ["action-a", "action-b", "action-a"]`（既存の`call_planner_reports_a_structured_cycle_rejection_from_the_real_binary`テストと同一形の循環）。`Diagnostic.range`は3行目の先頭（`action-a`記述の開始位置）から4行目の行末（`action-b`記述の終端、AFTER状態の4行目）までとする。行番号はこのmanifestテキスト自体（本節に確定済み）から一意に導出され、D1が実装時に決定する余地はない。BEFORE状態では循環がなく、正常にplanされることも合わせて検証する（偽陽性の否定）。

**D1の範囲（本改訂、P1/R5対応）**: D1は上記のBEFORE/AFTER manifest・cycle_path・Diagnostic.rangeをcase定義として確定するのみで、`laminaria-lsp`/`plan_ffi.nim`の実コードとnative linkの実装は行わない。実装は確定した本設計を入力とする後続実装ステージの作業とする（reached_stage: configuration-definitionまでがD1の到達範囲）。

### 3.3 Nim→WASM経路: 参照調査タスクとして分離（R2対応、フォールバック削除）

**訂正（本改訂）**: 前版のスパイク→Rustフォールバック方式は撤回する。フォールバックはM10が検証すべき「Nim依存の共有」というedgeそのものを消してしまうため、合格規則として採用しない。

- Nim→WASMの実現可否調査（`nim c -d:emscripten`または`--os:wasi`経由での`plan_ffi.nim`単体コンパイルと、対応するRust wasm32ターゲットとのリンク試行）は、**M10とは独立したreference調査タスク**として切り出す（execution_role: reference、M10の合格には数えない）。**訂正（本改訂、P1/R2・R5対応）**: 前版はこの調査をD1の実行順序の一部（「最優先で実行」）に組み込んでいたが、これは誤りだった。この調査の実施自体をD1の着手・完了条件から外す。実施時期はD1完了後、指示者が別途判断する。実施する場合、正確なrevision・target・command・成功/失敗の判定条件を固定した上でなければ着手しない。
- 調査の結果が肯定的であれば、M10-wasm-sideを（D1後続の確定した実装ステージで）本実装する。
- 調査の結果が否定的、または#5 T0のtarget/runtime契約確定を待つ必要がある場合、M10-wasm-sideは`subset_scope.future_work`に「不足している独自target生成能力」を具体的に記録した上で、**未達（not-yet-satisfied）として保持する**。
- **M10-lsp-native自体もD1では実コードを実装しない**（P1/R5対応）。D1は固定manifest・cycle_path・Diagnostic.rangeというcase定義の確定までを行う（reached_stage: configuration-definition）。native側の実コード実装も、WASM側と同じく後続実装ステージの作業である。

---

## 4. 各caseの入力・期待結果・判定方法（必須項目4・5）

機械可読な詳細は[issue-35-d0-cases.yaml](issue-35-d0-cases.yaml)に格納する。本節はcase定義のschemaと横断的な設計判断のみを記す。

### 4.1 case定義のschema（YAML各エントリの構造、本改訂でexecution_role/reached_stage/refsを追加）

```yaml
- id: string                      # 一意のcase ID（例: M1-fingerprint-cold）
  pattern: M1..M10                # 対応するM-pattern
  execution_role: owned | reference | bootstrap  # このcaseが示す証拠の種別
  reached_stage: configuration-definition | source-ir-evaluation | target-generation-execution
  origin: self | fixture-existing | fixture-new | self-planned
  # self-planned: M9/M10のように自身が将来持つべき機能として計画されたnode構成
  refs: [case-id, ...]              # 値・入力を共有する他caseへの明示参照（"同上"の禁止、R3対応）
  source_layout: string
  dependency_edges: [...]
  requested_artifacts: [...]
  demand_mode: full | partial | targeted-edit
  edit:
    kind: none | leaf-edit | shared-lib-edit | config-change
    target: string                  # ファイルパスと変更内容を具体的に記す。"1行編集"のような曖昧な記述を禁止
  forbidden_work: [...]
  required_work: [...]
  expected:
    kind: value | diagnostic
    value_or_diagnostic: string     # 具体値または"D1実装時に一度実行して得た値をpinする"旨+その導出規則への参照
    derivation: string
  subset_scope:
    d1_verifies: [...]
    future_work: [...]
  scale:
    small: {...}
    medium: {...}
    large: {...}
    seed: number
  measurement:
    comparison_modes: [...]        # owned役割のcaseにのみ許可。reference/bootstrapは空配列
    cpu_budget: [...]
    memory_state: [...]
    cache_state: [cold, warm, true-noop]
    warmup_runs: number
    repetitions: number
    noise_floor: string
    reproducibility: "独立した複数回の再実行間で判定する。同一reportの自己比較は使用しない"
  pass_criteria:
    d1: string
    d4: string | not-applicable-yet
```

### 4.2 計測基盤の再利用（新規実装を最小化する、役割別の適用ルールを追加）

`crates/laminaria-run/src/scenario.rs`が既に実装している`Stats::from_samples`、`compare_reports`、`ScenarioReport`の再現可能な保存/再生成機構を流用する。**`comparison_modes`はexecution_role: ownedのcaseにのみ設定する**（reference/bootstrap roleのcaseは計測目的が異なるため空配列とし、必要なら`reference_timing`という別軸で記録する）。

比較する4方式は#28本文セクション5の定義をそのまま採用するが、**owned roleのcaseにのみ適用する**:
1. LAMINARIA同一owned演算の逐次実行（CPU budget=1）
2. #27型の全体静的計画後のready並列実行
3. 需要駆動・増分協調方式（D2以降）
4. 3のablation（D2以降）

D1時点では、owned roleのcaseについてのみ1と2を取得する。M3-owned（既存の`independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`）はこの対象の代表例であり、新規実装ではなく既存テストの計測データを`ScenarioReport`形式で登録し直す作業になる。

### 4.3 D1で実行するcase・D2以降のcase（必須項目5後半）

| Pattern | execution_role | D1で実装・実行 | D2以降 |
|---|---|---|---|
| M1, M2, M6 | reference | manifest宣言＋demand選択の正当性検証（cargo/nimの実挙動照合） | 需要駆動実行との比較（owned版が実装された場合） |
| M3-topology | bootstrap | 構成の正当性検証（既存planner呼び出しテストの確認） | — |
| M3-owned | owned | 既存テストのcase登録、CPU budget 1/2の計測データ取得 | 需要駆動実行との比較 |
| M4, M5 | reference | 既存fixtureのcase化、実値照合の再確認 | M9実装後、自己構成での同型edgeとの比較（owned版） |
| M7, M8 | 混在（本文参照） | 新規fixture実装、宣言済み期待closure/critical pathとの照合。M8のNim planner側判定はowned | ablation・distributed配置評価 |
| M9 | owned（origin: self-planned） | **訂正（本改訂、P1/R5対応）**: case定義（3系列の固定snapshot・正準化書式・期待exit code/診断）の確定のみ。N1〜N4の実コード（新規FFI shim含む）・native C-ABIリンクの実装は**D1では行わない**（reached_stage: configuration-definitionまで） | 後続実装ステージでN1〜N4を実装し、決定性・診断値のテスト、実行時計測（並行dispatchとの統合） |
| M10-lsp-native | owned（origin: self-planned） | **訂正**: case定義（BEFORE/AFTER manifest・cycle_path・Diagnostic.range）の確定のみ。`laminaria-lsp`/`plan_ffi.nim`の実コード実装は**D1では行わない** | 後続実装ステージで実装、診断値テスト、実行時計測 |
| M10-wasm-side | owned（origin: self-planned） | **訂正**: case定義（native側とバイト同一という期待値、フォールバック不採用の明記）の確定のみ。実装は行わない | 参照調査タスク（M10-wasm-feasibility-reference-spike、D1完了後に指示者が実施時期を判断）の結果に応じて後続実装ステージで実装、または不足能力の記録を保持 |

### 4.4 規模・seed・編集内容・組合せ選定規則（必須項目5）

- 小規模: 全列挙。
- 中規模: §2.2/§2.3で規定した既定値を使用。
- 大規模: 固定seed(`20260909`)で代表組合せのみを選び、選定規則と省略範囲をcase定義に明記する。
- 編集対象: `full`/`partial`/`leaf-edit`/`shared-lib-edit`/`config-change`の5種を、`incremental-semantic-edit`が確立した「宣言→実測照合」方式でcase化する。編集内容は具体的なファイルパスと変更内容（変更前後の値）で記述し、「1行編集」のような曖昧な表現を使わない。

---

## 5. D1合格条件とD4合格条件の分離（必須項目6・7）

### 5.1 D1合格条件（再現性・正当性・比較基準の取得）

- 各caseについて、宣言された依存/非依存関係が実際のビルドツール出力（`cargo build -v`のCompiling/Fresh行、Nimのコンパイル呼び出しログ）と一致すること。execution_role: ownedのcaseは、実測を`ComputeConcurrencyProbe`等の実行区間トレースで確認する。
- 宣言された「実行してはいけない仕事」が実測でゼロ回であること。
- 期待値/期待診断が、導出根拠と一致すること。M7等の実行結果golden値は、D0確定済みの形状・数式・分岐点・編集差分から、D1が一度実行して得た値としてpinされる。
- execution_role: ownedのcaseについて、CPU budget=1の逐次baseline計測が取得されること。**再現性は独立した複数回の再実行間の一致で判定し、同一`ScenarioReport`の自己比較は使用しない。**
- M10-wasm-sideの参照調査タスクの結果（肯定/否定いずれか）が記録されること。否定の場合、不足能力がsubset_scope.future_workに具体的に記録され、M10-wasm-sideは未達として保持されること。

D1は速度改善そのものを判定しない（#28本文に従う）。

### 5.2 D4合格条件（速度改善・回帰許容幅）

現時点では根拠不足のため確定しない。#28本文セクション5が要求する「チューニング前の事前登録」を満たすため、D4の具体的な改善目標・許容回帰幅は、D1で取得するexecution_role: ownedのCPU budget=1 baseline計測の実測値が出そろってから、その実測値を根拠として**指示者が担当する別の目標設定タスク**として確定する。対象baseline・必要な証拠・確定の担当者・確定時期（チューニング開始前）をその目標設定タスク自体に明記する。本書はこの手順（D1実測→目標設定タスク→D4目標確定という順序）を仕様として固定し、D1実装者に目標設定を委ねない。

---

## 6. 指示者の採否記録（必須項目8への接続）

- 審査者: Codex（ユーザーから本節の記入を依頼された指示者）
- 審査日: 2026-09-11
- 審査対象: 提出commit `443b5f2959b1aa72be8cfb913f80fd92d79239f6`の3点
- 判定: **要改訂。D0未確定、D1指示は未発行。**
- 判定revision: `issue35-d0-review-443b5f2-v1`。これは審査記録の版であり、確定実装仕様の版ではない。

自身の実機能を使う構成案は保持する。一方、構成の出典、既存compilerによる比較実績、LAMINARIA自身によるコンパイル・実行の達成度を同じ「充足」にまとめた点、および実装者に期待値決定を残した点は修正が必要である。既存fixtureを全て再実行することは、この設計審査の修正条件にしない。

| 論点 | 指示者の採否 | 確定判断・理由 |
|---|---|---|
| M1/M2/M3/M6は自己充足、新規サンプル不要 | 構成出典の再利用を採用。「独自経路も充足」は不採用 | 既存source/module構成を使う。M2のテストbinは第2のconsumerとして認め、付録Aの第2bin追加は不要。M1/M6のconsumer同士にも依存があるので「3独立consumer」は訂正する。Cargo/Nimによるbuild結果は独自schedulerの証拠に数えない。 |
| M4/M5は既存fixtureをそのままcase化 | source・比較資料の再利用を採用。主経路の充足扱いは不採用 | reference/bootstrapとownedのcaseを識別する。既存fixtureの外部compiler成功でM4/M5の独自コンパイルを合格にしない。自身のM9から得る同型edgeとの対応も保持する。 |
| M7/M8は新規補完サンプルを追加 | 構成の目的・追加方針を採用。case仕様は要改訂 | 長鎖＋独立枝、非要求集合の補完を行う。M7の数値・正確なedge・編集差分が未確定。生成プログラムの計算量とcompiler workの量を区別する。M8はCargoの非buildとNim plannerの非展開を別に判定する。 |
| M9の言語間契約はnative C-ABI | 限定したnative比較境界の候補として採用。本経路の必須semantic境界としては不採用 | C ABI自体を禁止する判断ではない。独自IR内の呼出・型・所有・失敗の意味契約を先に定義し、採用するtarget境界は#5 T0に対応付ける。既存compilerで作ったstaticlibのlink成功は参考証拠である。 |
| M9の実機能割当て（適合性レポート） | 題材と4段の言語配置を採用。現行のnode契約は要改訂 | N3がtoolchain versionとPlanSchemaVersionを混同している。下記R3の責務と固定入力に改訂する。単なるFFI公開だけで各edgeの実機能を検証したとは扱わない。 |
| M10の実機能割当て（Nim planning kernelの共有） | 採用 | native LSPとWASMの両consumerが同じNim source機能を利用する。native/WASMのartifactは分ける。具体的入力、循環pathとsource range、target/runtimeは要固定。 |
| M10のWASM経路: スパイク→Rustフォールバック | 不採用（A/Bとも本経路の合格規則として不採用） | Nim compiler→C→Emscriptenはreference実験に限る。Rustへの置換はM10の検証対象のedgeを消す。失敗時は未対応理由と必要な独自生成能力を記録し、M10は未達とする。3回の試行終了は実現不能の証明にならない。 |
| D4条件はD1実測後に確定 | 条件設定の順序を採用。現行の測定合格規則は不採用 | 独自経路のbaselineを先に取得し、指示者が別の目標設定タスクでD4閾値を確定する。チューニングはその後。同一runとの自己比較を再現性の証拠にしない。 |

### 6.1 再提出に必要な修正（R1〜R5）

これは#35の既存必須項目1〜8を満たすためのD0改訂指示である。実装タスクではなく、元の3提出物を改訂する。以下の判断を実装者へ再選択させない。

**R1 — 実行経路と構成の充足度を分ける（必須項目1〜4、6）**

- 全caseに`execution_role`（`owned` / `reference` / `bootstrap`）と到達段階（構成定義、source/IR評価、target生成・実行）を明示する。共有するsourceとgraphにcase間の参照を付け、役割の異なる測定を混ぜない。
- #28の比較方式1/2は同じLAMINARIA-owned演算の予算1/並列実行である。Cargo/Nim buildのcaseに`comparison_modes: [1]`を割り当てない。比較用compiler結果は別roleで保持する。
- M3の`CargoBuild`/`NimBuild`を`compiler_work_executor`へ渡す指定は削除する。同executorはその2種類を`UnsupportedActionKind`として拒否する。既存#27のsource-derived Rust/Nim経路と対応付け、扱えない製品sourceは不足を明記する。拒否を解除して外部compiler実行を追加しない。
- inventoryの「実Nim planner未接続」は訂正する。`compiler_work_executor.rs`の`the_full_lower_validate_transform_validate_evaluate_pipeline_runs_end_to_end`と`independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`には、実planner→validate→dispatchが既に存在する。研究用CLI不足と接続自体の不足は区別する。
- `laminaria-plan → fingerprint`、`laminaria-run → plan/fingerprint/ir`、`laminaria-cli → run/plan/fingerprint`を全て含めてgraphを訂正する。fan-out/diamondの出典として採用するが、plan/run/cliは独立した実行枝ではない。M2では共有sourceを使う複数consumerの構成と、共有compiler workの一回実行を別の主張にする。

**R2 — M10の必須依存を維持する（必須項目1〜4、8）**

- 同じNim機能をnativeとWASMへ生成するM10を維持し、WASM側のRust再実装による代替合格は削除する。A/B選択をD1へ渡さない。
- 本経路の設計は#5 T0の独自生成契約へ接続する。必要なIR・target/runtime能力が不足する場合は、対象caseと不足能力を固定して後続待ちとして記録する。D1でM10全機能を実装し終える条件は削除する。
- 外部Nim/Rust/Emscriptenの調査を残すなら、独立したreference設計・実験タスクにする。正確なrevision、target、command、成功・失敗の判定条件が固定されるまで実行指示として発行しない。可否結果でM10のゴールを変更しない。

**R3 — 期待結果とデータの流れをD0で閉じる（必須項目2〜5、8）**

- M9の4段を、N1=Nim CLIの入出力・終了コード、N2=Rustの固定環境snapshotと要求の正規化、N3=Nimの要求schema/capabilityと提供schema/capabilityの適合性判定、N4=Rustの正準化された判定入力・結果のSHA-256、として設計を具体化する。環境収集そのものの実機能への接続は保持し、期待値試験はambient toolchainに左右されない固定snapshotで行う。
- N3は要求schemaと提供schema、要求capabilityと提供capabilityを比較する。compiler versionとwire schema versionを比較しない。初回は既存の厳密なschema一致規則を踏襲し、根拠のないSemVer範囲互換やDegraded分類は導入しない。既存のversion gateは`planning_kernel.nim::planFromJson`にある。
- N3で判定した結果と入力をN4へ渡し、digest付き判定をN3→N2→N1へ返す。N4を通らなくても成立する装飾的edgeや、結果計算に循環するデータ依存にしない。正準化規則、固定入力、成功・不適合・不正入力時の正確な出力をD0で記載する。
- M7は小=4段/2枝、中=8段/4枝、大=16段/8枝を保持し、分岐点をそれぞれstage-02/04/08へ固定する。全edge、各nodeの処理、aggregate式、入力、編集のbefore/after、数値期待結果をD0で記載する。YAMLの「D1実装時に確定」と「1行編集」を具体値・差分へ置き換える。
- M10の診断試験は入力manifest全文、編集差分、期待cycle path、診断code/rangeを固定する。LSP側との一致だけで正解を定義せず、graphからの導出根拠を併記する。
- YAMLの「同上」「同一」「同型」等は、同じ型を保つ明示的なcase参照とoverride規則、または完全な値に置き換える。node/edge/需要/期待集合を読取り可能な構造として表す。M9/M10のoriginは自身の予定機能であることを表し、補完fixtureと区別する。

**R4 — 比較対象・cache状態・仕事量を固定する（必須項目5〜7）**

- M1のcoldとwarm/no-opは別実行caseにする。coldでfingerprintの生成1回を要求しながら「未変更なので生成禁止」を同時に要求しない。各caseの前処理・snapshot・期待実行集合を固定する。
- M7の行列を大きくして生成プログラムのruntimeを増やしただけではcompiler workを重くした証拠にならない。source/IRの量で増やす計算と、EvaluateEvidenceで評価する計算を指定し、計測対象を分ける。実測前に「重い枝」「最長時間のcritical path」を確定した事実にしない。
- D1のowned比較は同じ対応済みsource/IR処理について予算1と2を取得する。非対応caseを全て走らせる指示や、Cargo側の並列性をowned方式へ帰属させる指示は削除する。
- 反復ごとの状態復元、warmupの扱い、測定順・開始終了境界・適用hostを固定する。再現性は別の実行で取得したraw samplesで判断し、同一reportの自己比較は使用しない。
- D4の数値閾値は現時点で推測して埋めない。対象baseline・必要な証拠・指示者の判定成果物・チューニング前という期限を指定した目標設定タスクとして明示する。

**R5 — D1の範囲と発行条件を3点で一致させる（必須項目4、8）**

- D1は確定したsource構成・manifest・期待graph/result・規模生成・対応表と、実行可能なowned比較基準を実装する。M9/M10の製品CLI/FFI/LSP/WASM機能一式の実装は、確定した機能・生成契約を入力とする後続実装へ割り当てる。自身を対象にする目的とM9/M10の必須構成は維持する。
- caseごとにD1で実装・実行、referenceとして保持、後続能力待ちのいずれかを明記し、草案の「全case実装」と本文の段階分けの矛盾を解消する。後続待ちを合格と数えない。
- 発行条件を「採否が記入済み」から「R1〜R5が反映され、3点が一致し、指示者が確定仕様revisionを記録したこと」へ変更する。ユーザーへゴールの再設定を求めない。

### 6.2 修正確認の停止条件

R1〜R5について、修正箇所と満たした条件を一覧で提出する。3点が同じcase・role・段階・期待結果を指し、実装担当に対象や期待値の決定が残らないことを指示者が確認して確定する。既存の構成再利用、M2の第2consumer、M9/M10の題材選定は上表の判断を再審議する必要はない。今回、compiler実装の追加や既存全テストの再実行は要求しない。

---

## 7. 改訂内容一覧（R1〜R5対応、提出者による本改訂の要約）

| R項目 | 本改訂での対応 | 反映箇所 |
|---|---|---|
| R1 | 全caseに`execution_role`(owned/reference/bootstrap)と`reached_stage`を追加。M3を`M3-topology`(bootstrap)と`M3-owned`(owned、既存の`independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`を登録)に分割。M3のCargoBuild/NimBuildをcompiler_work_executorへ渡す記述を削除。inventoryの「実Nim planner未接続」を訂正し、CLI表面欠落とissue#27残課題を区別。依存graphに`cli→run`/`cli→plan`直接依存を明記し、「3独立consumer」を「単一最上位要求からの複数経路収束」へ訂正 | §1.1, §1.2, §2 表, §2.1, cases.yaml M3-* |
| R2 | M10のRust fallbackを削除。native/WASM両方が同一Nim実装に依存することを必須として維持。Nim→WASM調査を独立したreference調査タスクに分離（M10合格には数えない）。否定的結果の場合はM10-wasm-sideを未達として保持し不足能力を記録 | §3.2, §3.3, cases.yaml M10-wasm-side |
| R3 | M9の4nodeをN1=Nim CLI I/O、N2=固定環境snapshot+要求正規化、N3=capability完全一致判定（`planFromJson`と同じ厳密規則）、N4=正準化digest（結果に含まれる、装飾的でない）として具体化。正準化書式を明記。M7の小/中/大を4/8/16段+2/4/8枝、分岐点stage-02/04/08に固定し、編集差分（定数入れ替え）と2軸分離（source量/payload量）を明記。M10診断の入力manifest全文・cycle_path・診断根拠を明記。YAMLの「同上」を`refs`フィールドによる明示参照へ置換 | §3.1, §3.2, §2.2, cases.yaml 全体 |
| R4 | M1をcold/noop別caseに分離。M7の「行列拡大=compiler work増加」の混同を解消（軸分離）。owned比較はowned役割のcaseにのみ許可。反復・warmup・再現性判定を「独立した複数回の再実行間」に明記し自己比較を排除。D4は目標設定タスクへの委譲を明記 | §2.1, §2.2, §4.2, §5.2, cases.yaml measurement |
| R5 | scopeを`execution_role`と`reached_stage`で「D1実装・実行」「reference保持」「後続能力待ち」に3分類。M9/M10の製品機能一式実装は確定済み契約を入力とする後続実装へ割当て。発行条件を「R1〜R5反映・3点一致・確定仕様revision記録」へ変更 | §4.3, D1草案の発行条件, cases.yaml `origin: self-planned`とD1適用範囲 |

---

## 8. 第2回改訂内容一覧（commit `217b0c1`の読み取りレビュー6点への対応）

指示者は`217b0c1`をYAML解析・実コード照合により審査し、R1〜R5に対する「発行を妨げる指摘」6点（P1×3, P2×3）を残件として指摘した。以下は各指摘への対応。新しい要求への対応ではなく、既存R1〜R5の完全な反映を目的とする。

| # | 指摘 | 対応 | 反映箇所 |
|---|---|---|---|
| 1 (P1/R2・R5) | D1草案がNim→WASM調査を最優先実行としつつM9/native LSPの製品実装も要求しており、R2「D1完了条件から外す」に反していた | Nim→WASM調査・M9(N1〜N4)・M10-lsp-nativeの実コード実装をすべてD1のスコープから除外した。D1はcase定義（固定snapshot・manifest・cycle_path・診断range等）の確定のみを行い、reached_stage: configuration-definitionまでで止める。実装は後続実装ステージへ完全に切り出した | §3.1末尾, §3.2末尾, §3.3, D1草案全体, cases.yaml M9/M10-*の`subset_scope`/`pass_criteria` |
| 2 (P1/R3) | M9のN3判定規則が「capability完全一致」と書きつつYAMLは部分集合(⊆)判定になっており、かつcapabilityと`PlanSchemaVersion`(単一文字列`"0.2.0"`)を混同していた | schema_version一致(判定1、単一文字列の完全一致)とcapability集合一致(判定2、集合としての完全一致、部分集合は不可)を独立した2判定に分離した。固定snapshotを3系列(成功/capability不一致/schema不一致)へ具体化し、各系列の判定1・判定2の値とexit code/診断を明記した | §3.1「固定入力・正準化規則」, cases.yaml M9-fingerprint-compat-chain |
| 3 (P1/R3) | M1のedit対象、M6の追加フィールド、M10の診断rangeがD1実装時の決定に委ねられており、M7の数式も「splitmix64風」等で閉じていなかった | M1editは`ENV_ALLOWLIST`配列への具体的な要素追加に確定。M6editは`EnvironmentFingerprint`構造体への具体的なフィールド名・型(`pub schema_note: Option<String>`)に確定。M10診断は4行のBEFORE/AFTER manifestテキストと行番号レベルのDiagnostic.rangeに確定。M7は`transform_0/1/2`・`leaf_a/b/c/matrix_sum`・aggregator集約式を閉じた擬似コードとして記載し、誤実装が個別unit testで検出されることを明記した | §2.2, §3.2, cases.yaml M1-fingerprint-leaf-edit/M6-diamond-fingerprint-plan/M10-lsp-native/M7-* |
| 4 (P2/R1・R3・R5) | YAML中3件の`execution_role`が宣言外の`self-planned`、4件の`dependency_edges`が配列でなく説明文だった。M7は依存方向とデータ流方向を混在させ、M8はdemandと期待集合が不整合だった | M9/M10-lsp-native/M10-wasm-sideの`execution_role`を`owned`へ修正（`origin: self-planned`は維持、reached_stageで進捗段階を区別）。4件の`dependency_edges`を全て配列化した。M7の全edgeを「左が右に依存」の統一規則へ修正（データ流方向の注記と依存方向を明確に分離）。M8のNim planner側caseをdemand=fixture-bin-out(used-core/used-utilの両方を推移的に含む)に修正し、used-utilのみのdemandでused-coreも要求する矛盾を解消した | cases.yaml 全体（execution_role/dependency_edges/M7-*/M8-many-unrequested-nim-planner） |
| 5 (P2/R1) | `independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`の実コードは各言語3 Action(`LowerSource→ValidateIr→EvaluateEvidence`)・合計6 Actionであり、改訂案の5段(Transform込み)という記述は別テストとの混同だった | 実コード(1604行目付近、`plan.actions.len()==6`のassert)を確認し、3 Action×2チェーン=6 Actionへ訂正した。新規Transform実装・テストは追加しない | §2 表(M3-owned行), cases.yaml M3-owned-independent-chains |
| 6 (P2/R4) | M3の計測欄に反復数はあるがhost・状態復元・budget実行順・計測境界がなく、`Stats::from_samples`だけでは再現性の合否を決められなかった | `host`(単一host固定)・`state_restoration`(repetitionごとの新規ArtifactStore)・`execution_order`(budget1連続実行→budget2連続実行、インターリーブなし)・`measurement_boundary`(`run_compiler_work_plan_traced`呼び出し前後)の4項目を追加した。D4の速度目標確定は今回の対応範囲に含めない(既存方針を維持) | cases.yaml M3-owned-independent-chainsの`measurement` |

---

## 付録A: M2（解決済み）

指示者はunittestバイナリ(`test_planning_kernel`)を`contract.nim`/`planning_kernel.nim`の第2consumerとして採用した。`fixtures/nim-heavy-workspace`への第2bin追加は不要。本付録は経緯の記録として残す。

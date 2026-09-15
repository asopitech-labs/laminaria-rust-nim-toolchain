# 異種依存義務をbuild時にdischargeするartifact contract

## 核心

LAMINARIAの価値は、解決したdependency graphをそのまま利用者に配ることではない。Cargo、Nimble、C、C++のpackage選択、source semantics、language IR、intermediate IR、ABI、symbol、linkにまたがる依存義務を一つの解決・変換過程で**discharge（充足して消込）**し、最終native artifactを元のecosystem分断から切り離すことである。

```text
Cargo / Nimble / C / C++ package obligations
  + source/module/type/FFI obligations
  + language-IR -> intermediate-IR lowering obligations
  + ABI/symbol/link/runtime obligations
                       |
                       | resolve / specialize / lower / generate
                       | inline / link / embed / externalize explicitly
                       v
             DependencyDischargedArtifact
```

最終artifactのidentityは「どのpackage manager commandを再実行するか」ではなく、解決済みsemantic closureとtarget contractで決まる。元のgraphはbuildの導出と説明の証拠として保存するが、artifact利用時の再解決グラフとはしない。

## 利用者にとっての価値

LAMINARIAで生成したartifactの利用者は、元projectをbuildするために必要だったCargo、Nimble、CMake、C/C++ header、compiler version、feature selection、ABI choice、link orderを再び解決しない。それらの義務は、選択、semantic validation、specialization、IR lowering、code generation、linkによりbuild時に充足済みである。

これは「依存関係が物理的にゼロになる」という主張ではない。より正確には、**依存関係の探索・選択・衝突・buildを利用者へ持ち越さない**という性質である。最終artifactにOS ABI、dynamic library、driver、kernel service、certificate store、locale、plugin等が必要なら、それらは消えず、明示されたruntime contractとして残る。

この性質を本書では`DependencyDischargedArtifact`と呼ぶ。対外的には「依存関係の罠／蟻地獄から解放されたartifact」と説明できるが、技術契約では「元の依存義務をdischargeした」ことと、「runtime外部依存が物理的にゼロである」ことを区別する。

## Build closureとruntime closure

```text
Build closure
  source / package metadata / generators / compilers / headers
  toolchains / build scripts / intermediate IR / objects / link actions

                  resolve + transform + discharge
                                      |
                                      v

DependencyDischargedArtifact
  executable or bundle
  discharged semantic/code/link obligations
  required runtime artifacts only
  explicit platform/ABI contract
  resolution + discharge + provenance evidence
```

Nixもderivation closureをbuild-time dependency、output-path closureをruntime dependencyとして区別し、正しいdeploymentにはruntime closure全体が必要だと定義している。[^nix-closure] Nixはstore path参照をscanして潜在runtime dependencyを登録できる。[^nix-output]

Nixはclosure deploymentの重要なbaselineだが、LAMINARIAの中心差分はclosureを運ぶこと自体ではない。package／store objectより内側のsource semantics、language/intermediate IR、FFI、symbol、archive member、link relationまで解き、それぞれの依存義務をnative artifactへ変換・消込する。runtimeへ残るものだけをexternal contractまたはbundled artifactとして明示する。

## Dependency obligationのdischarge

graph edgeは単に最終artifactから到達可能なだけでは不十分である。各要求がどの操作で充足され、利用時に残るかを記録する。

```text
ObligationState = Unresolved | Selected | Satisfied | Discharged
                | Externalized | Rejected

DischargeKind = Specialized | Lowered | Generated | Inlined
              | StaticallyLinked | Embedded | ProvenIrrelevant
              | ReplacedByEquivalent | ExternalRuntimeContract
```

例えばCargo featureは選択されるだけでなく、それによるsemantic/IR差分が最終codeへ反映された時点でdischargeされる。Nim module要求は必要itemがloweringされた時点、C/C++ library要求は必要symbolがstatic linkされるか明示runtime contractへ移された時点でdischargeされる。この結果、元のecosystem edgeは配備時の未解決問題としては残らない。

### いつ・どの情報源で確定できるか(確定可能性フェーズ)

`ObligationState`の遷移(`Unresolved -> Selected -> Satisfied -> Discharged`)は、義務の種類によって全く異なる段階でしか確定できない。これはCargo/rustc/linkerの実行フェーズ(parse→typecheck→monomorphize→codegen→link)という**プロデューサー側の時系列**とは別の軸であり、alopexDBの実測([issue #62](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/62))で3段階に区別できることが確認された。

```text
Phase 0: Pull-Determinable（要求から静的に確定可能）
  情報源: package lockfile等の宣言的メタデータのみ
  実行: 不要（ビルドを一切走らせない）
  ObligationState対応: Unresolved -> Selected がこの段階で完結しうる

Phase 1: Semantic-Determinable（意味解析後にのみ確定可能）
  情報源: source/module/type/FFI事実、monomorphization対象
  実行: parse・型検査・monomorphization collectionが必要
  ObligationState対応: Selected -> Satisfied はこの段階でしか判定できない

Phase 2: Execution-Cost-Opaque（package/semantic情報からコストが不透明）
  情報源: 実行環境依存の計算コスト（native compiler呼び出し、link処理等）
  実行: 実際にnative build・linkを実行しないとコストの大きさが分からない
  ObligationState対応: Satisfied -> Discharged のコストが、義務の種類によって
    全く異なる尺度（source言語の意味解析コスト vs 外部native toolchain実行コスト）になる
```

alopex-cliの実測で、クリティカルパス上の全ノード(TLS依存連鎖を含む12 crate)はCargo.lockの静的解析だけで100%到達可能と判定でき(Phase 0)、alopex-cli自身が生成するmonomorphized itemの個数(29,129個)は実際にコンパイルするまで確定できず(Phase 1)、そのうちクリティカルパス最長区間を占めた`aws-lc-sys`(C言語のAWS-LC暗号ライブラリをbindgen経由で使うcrate)はmonomorphized item数(371個)がごく少数であるにもかかわらずビルド時間はクリティカルパス上最大だった(Phase 2)。`cargo build --timings`のunit別section分解(frontend/codegen区分)は、native compiler呼び出し区間には適用されない(`sections: None`として現れる)——これはCargo/rustcの既存フェーズへ全ての義務を一様にマッピングすることが構造的に不可能であることの直接証拠である。

このフェーズ区分が示す帰結は、**プル駆動の枝刈り設計はPhase 0/1/2を区別しなければならない**ということである。Phase 0の義務は静的な事前枝刈りを追求できるが、Phase 1/2の義務はプル側から早期に「不要」と判定する余地が原理的に限られ、代わりに「早期に着手を開始する」スケジューリング上の工夫（[Lane B](../execution/lane-b-efficient-compiler-computation-foundations_ja.md)が扱う領域）に軸足を移す必要がある。

Phase 0の情報源自体が誤っている場合、この段階の確定は無効になる。alopexDBの`alopex-sql`が依存するNim SQLパーサーのvendor manifest(`contract_version: 0.4.0`)は、実際にRust側が要求する`REQUIRED_CONTRACT_VERSION`(`0.25.0`)と不整合であり、静的な宣言だけでは義務がdischarge可能かどうか判定できない状態だった([issue #62 P0](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/62)実測時に発覚)。Phase 0による早期確定は、その情報源自体の正しさを別途検証する仕組みを要求する。

### Phase 0内部の粒度：到達可能性と質的コスト予見は別問題

Phase 0を「package lockfileのみ」と定義すると、義務が到達可能かどうかは判定できても、その義務が`Satisfied -> Discharged`でどの種類のコスト(source言語のsemantic処理コストか、外部native toolchain実行コストか)を要求するかまでは読み取れない。alopex-cliの実測([issue #62 P1/P2](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/62))では、`aws-lc-sys`/`zstd-sys`という2 crateが、Cargo.lockには存在しない`links`フィールドや`cc`/`cmake`/`pkg-config`向けbuild-dependency宣言を、依存解決時に取得済みのCargo.tomlに持っており、これを機械的シグナルとして走査すると、実際にnative build costが支配的な義務をfalse negativeゼロで(ただしfalse positiveありで)絞り込めた。この走査自体は351ファイルで0.032秒であり、native build本体(45.73秒)に対して無視できるコストで、かつcargoが依存解決のために既に行うfetch/展開のI/Oに相乗りでき、実際のCPU集約的コンパイル開始前という手薄な区間に収まることを確認した。

これは**Phase 0がPull-Determinable(到達可能性)とCost-Determinable(質的コスト予見)という異なる問いを一つの箱に押し込めていた**ことを示す。ただし今回確認した「fetch直後の隙間に相乗りする」という設計は、Cargoという既存エコシステムが偶然この情報をこの形で保持していたことに依存した、**複数ありうる設計仮説のうちの一つ**にすぎない。少なくとも次の代替案が考えられ、いずれも未検証である。

- **仮説A(複数実プロジェクトで再現検証済み、[issue #63 P0](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/63)): 既存メタデータの走査への相乗り** — 依存解決時に取得済みのソースメタデータ(Cargo.tomlのbuild-dependency宣言等)を、fetch直後のCPU/I/O手薄区間で走査する。追加コストは最小だが、走査対象のシグナル(`links`等)が「native実行コストの重さ」まで表現する保証はなく、false positiveを許容する必要がある。またCargo以外のecosystem(Nimble/C/C++)に同種のシグナルが存在するとは限らない。alopexDB以外の2つの実Cargoプロジェクト(cargo本体・rust本体、`scripts/research/scan-native-build-signals.py`で再現)でもfalse negative率0%を維持できたが、false positive率は35〜44%(alopexDBの実測より悪化)だった。加えて、`cargo vendor`/`cargo publish`が正規化する`[build-dependencies.<name>]`ヘッダ形式(単一`[build-dependencies]`テーブル内`name = "..."`とは別の表現)を当初のスキャン正規表現が見落とし、`ring`/`cxx`/`link-cplusplus`等の真陽性を取りこぼす具体的な実装上の脆弱性が判明した(修正後は0件)。この見落としパターン自体が「シグナルの表現形式がエコシステムのツール側の正規化規則に依存し、単純な文字列走査では網羅できない」という、仮説Aの脆弱性の実例である。詳細は次節「仮説A実測記録(issue #63 P0)」を参照。
- **仮説B: 過去実行の計測結果をprovenance付きでキャッシュする** — LAMINARIA自身が一度そのcrate/versionをビルドした際の実測コスト(wall-clock、CPU、I/O)を`resolution_certificate`相当の構造へ記録し、同一obligationの再要求時にキャッシュを引く。初回コストは避けられないが、2回目以降は実測ベースの正確な見積もりになる。ecosystem横断で一様に適用できる利点があるが、初回実行前(cold start)には無力で、環境依存の計測値がどこまで別環境へ転用可能かという妥当性問題が残る。
- **仮説C: LAMINARIA独自のcost contractフィールドを定義し、crate作者ではなくLAMINARIA側のprovenance DBへ外部から充填する** — crate作者に新しい登録義務を課さず(ユーザーオペレーション不変の制約を満たす)、LAMINARIAまたはコミュニティが観測データを蓄積してcontract化する。仮説Bのキャッシュを恒久化・共有可能にした形だが、DBの整備・配布・信頼性検証という新たな運用コストを持ち込む。
- **仮説D: 質的分類を諦め、保守的に「未知の義務は重いかもしれない」と仮定してスケジューリングだけで対処する** — [Lane B](../execution/lane-b-efficient-compiler-computation-foundations_ja.md)のB-H4(obligation-aware scheduling)が扱う領域で、コストの事前予見自体を放棄し、代わりに「未確定な義務を優先的に早く着手する」というスケジューリング側の保守化で全体最適を狙う。予見精度に依存しない代わりに、並列度に余裕がない環境では効果が薄い。

いずれの仮説も、[cross-layer枝刈り](cross-layer-reachability-pruning_ja.md)の原則(「静的に精密なtarget setが得られない場合は、対象集合をover-approximateして保持する」)に従い、コスト予見の失敗を安全側(悲観的スケジューリング)に倒す必要がある。どの仮説を採るか、または組み合わせるかは、対象ecosystemの多様性(仮説Aはecosystem固有のメタデータ形式に依存)と、M1で扱う実際のmixed workloadでの反証実験を経て判断する。

### 仮説A実測記録(issue #63 P0)

alopexDB以外の実Cargoプロジェクトで仮説Aがどれだけ再現性を持つかを検証するため、`scripts/research/scan-native-build-signals.py`で`.reference/cargo`(cargo本体、コミット`e7506208`)と`.reference/rust`(rust本体、コミット`55c4dfed`)のCargo.lock全登録パッケージを対象に、`links`フィールドおよび`cc`/`cmake`/`pkg-config`向けbuild-dependency宣言を機械的シグナルとして走査した。ground truthは各パッケージのbuild.rsを直接確認し、実際にネイティブCコンパイラ/cmake/pkg-configを起動するものを手動で判定した(スクリプト内`GROUND_TRUTH`に記録)。

| 対象 | 登録パッケージ数 | ground truth数 | 予測陽性数 | false negative率 | false positive率 | 走査時間 |
| --- | --- | --- | --- | --- | --- | --- |
| cargo本体 | 524 | 9 | 14 | 0% | 35.7%(5/14) | 0.048秒 |
| rust本体 | 524 | 9 | 16 | 0% | 43.75%(7/16) | 0.039秒 |

**false negative率0%は両プロジェクトで維持された**が、**false positive率はalopexDBの実測(2 crate中0件、ただし「false positiveあり」とのみ記載され具体数値は未記録)より明確に高い**。false positiveの内訳は、`links`フィールドを持つがbuild.rsが実際にはnativeコンパイラを呼ばない用途(モジュール分割目的のリンクユニーク化等、例: `rayon-core`/`wasm-bindgen-shared`/`defmt`)、および`links`はあるがbuild.rs自体を持たないケース(例: `blake3`のfeature構成)だった。

**実装上の重要な発見**: 初回実装では正規表現が単一`[build-dependencies]`テーブル内`cc = "1.0"`形式のみを検出しており、`cargo vendor`/`cargo publish`が正規化する`[build-dependencies.cc]`ヘッダ形式(依存ごとに個別テーブルへ分解される表現)を見落としていた。この結果、実際にnative build costを要求する`ring`/`cxx`/`link-cplusplus`が誤ってfalse negativeとして扱われるところだった。両形式を検出するよう修正後、false negativeは0件を維持した。この見落とし自体が、「Cargo.tomlのシグナル表現形式はツール(cargo自身)の正規化規則に依存し、単純な文字列走査は表現形式のバリエーションを列挙し尽くす必要がある」という、仮説Aの脆弱性の具体例である——crates.io配布物(=依存解決時に実際に取得されるCargo.toml)は`cargo publish`によって`[build-dependencies.<name>]`形式に正規化されるため、この見落としは実運用でも起こり得る。

停止条件との照合: 「仮説Aが複数実プロジェクトでfalse negativeゼロを維持できず」という停止条件には該当しなかった(false negative率は0%を維持)。従って本P0の結果は、仮説Aを主軸から外す根拠にはならないが、false positive率のばらつき(alopexDBの2件→今回35〜44%)と表現形式依存の脆弱性は、仮説Aを「唯一の解」として採用しないための追加的な留保事項として記録する。

### 仮説AのNimbleへの一般化机上調査(issue #63 P1)

仮説Aが検出に使う機械的シグナル(Cargo.tomlの`links`フィールド、`cc`/`cmake`/`pkg-config`向けbuild-dependency宣言)が、Nimbleパッケージのメタデータ(`.nimble`ファイル)に同種のフィールドとして存在するかを、Nimble公式リファレンス(`nimble-reference.html`)と本リポジトリが実際に保持する4件の`.nimble`ファイル(`nim-planner/laminaria_planner.nimble`、`fixtures/nim-heavy-workspace/fixture.nimble`、`fixtures/cross-ecosystem-native-executable/nimble/doubler/doubler.nimble`、`.reference/alopex/.../nim_sql_parser.nimble`)を突き合わせて机上調査した。

**`.nimble`ファイルの構造**: package sectionは`name`/`version`/`author`/`description`/`license`/`srcDir`/`binDir`/`bin`/`namedBin`/`backend`/`skipDirs`/`skipFiles`/`skipExt`/`installDirs`/`installFiles`/`installExt`/`paths`/`entryPoints`/`requires`等の宣言的(静的パース可能)フィールドを持つ。加えて`task <name>, "<description>": <nimscriptコード>`という命令的ブロックを任意個定義でき、ここでは`exec`(シェルコマンド実行)や`gorgeEx`(コマンド実行して出力取得)など、Nim言語そのものの実行能力をフルに使える。

**Cargoの`links`/`cc`/`cmake`/`pkg-config`に直接対応するフィールドは存在しない**。近い候補として`foreignDep`(例: `foreignDep "openssl"`のようにシステムライブラリ名を宣言する)が公式wikiに言及されているが、これは「このパッケージが外部ライブラリを要求する」という人間向けの注記に近く、Cargoの`links`(同一ネイティブライブラリへの重複リンクを検出するビルドシステム制御用の識別子)や`cc`/`cmake`build-dependency(native toolchain呼び出しの機械的予告)が持つ「ビルドグラフ制御のための構造化契約」としての性質を持たない。`backend`フィールドは"c"/"cpp"/"js"等の値を取るが、これはNimコンパイル自体が常に経由する言語バックエンド選択であり、Cargoの`*-sys`crateが示す「追加のC/C++コンパイル・リンク作業が必要になる」という差分シグナルには対応しない(Nimコンパイルは常にC/C++コードを生成し既定でCコンパイラを呼ぶため、この選択自体はnative build costの有無を分けない)。

**実例による裏付け**: 本リポジトリの`nim_sql_parser.nimble`(実際にCのvendorライブラリをビルド・静的リンクし、Rust側からFFI消費される実在パッケージ)を調べると、native連携に関する情報(`--passC:-fPIC`、`--passL:-static`、OS別出力ファイル名`libalopex_sql_parser.so`/`.dylib`/`.dll`の切り替え等)は全て`task lib`/`task staticlib`ブロック内の自由形式コマンドライン文字列として埋め込まれており、`.nimble`ファイルのトップレベルの宣言的フィールドには一切現れない。他3件の`.nimble`ファイル(`laminaria_planner`/`fixture`/`doubler`)も`foreignDep`/`passC`/`passL`を一切使用していない。`nimble dump --json`は宣言的フィールドをJSON化できるが、taskブロック内のnimscriptコードは評価対象外であり(公式issueでも指摘される既知の制限)、native build cost予告が仮にtaskへ埋め込まれていたとしても`dump --json`の出力には現れない。

**結論(反例として記録)**: 仮説Aの前提である「依存解決時に取得済みのソースメタデータへ、構造化フィールドとして機械的にnative build costシグナルが宣言されている」という性質は、Cargoエコシステム固有の慣習であり、Nimbleエコシステムには同種のフィールドが存在しない。Nimbleパッケージのnative連携情報は、静的パースでは原理的に確定できない任意のnimscriptコード(`task`ブロック)の中に置かれる——これはPhase 0で扱う「静的メタデータの機械走査」という設計そのものがNimbleに対して成立しないことを意味し、仮説Aをecosystem横断の恒久解として主軸に据えることはできないという、issue #63の停止条件(「仮説AのシグナルがNimbleに存在しないことが確認された場合」)に該当する具体的根拠である。

停止条件との照合: 該当。仮説Aは「静的メタデータ走査」というアプローチ自体がCargo固有であり、LAMINARIAがCargo/Nimble/C/C++を横断する以上、少なくとも仮説Aを主軸には据えられないという、適用範囲を絞った否定的証拠として記録する(非ゴール節の通り、Nimble側への新フィールド追加提案は行わない)。

### 仮説A/Bのcold/warm比較と`resolution_certificate`組み込み評価(issue #63 P2)

P0/P1で仮説Aが「false negativeゼロを維持するが、false positive率が案件依存で悪化しうる」「Cargo固有でNimbleへ一般化できない」ことが判明した。P2は、仮説Aの代替である仮説B(実行時キャッシュ)を、実装せずに机上・既存実測データの引用で評価し、`resolution_certificate`(前掲「Artifactが持つ証拠」節)への組み込みやすさを比較する。実際のcache層の実装は本P2の完了条件に含めない(issue #63非ゴール)。

**現行`resolution_certificate`のスキーマ**は`selected package/version/feature/provider/toolchain`と`rejected alternatives and reasons`のみを持ち、コスト予見に使えるフィールドを一切持たない。仮説A/Bのどちらを採るにせよ、新規フィールドの追加が必要である。

| 観点 | 仮説A(静的メタデータ走査) | 仮説B(実行時キャッシュ) |
| --- | --- | --- |
| cold start(初回) | 即座に使える(P0実測: cargo本体0.048秒、rust本体0.039秒、524パッケージ走査)。義務充足前に予見できる。 | **無力**。初回はキャッシュが空で予見できず、フォールバック(仮説D的な保守的スケジューリングか、実測完了を待つ同期実行)が別途必要。 |
| warm(2回目以降) | **精度が変わらない**。何度実行してもP0で確認したfalse positive率(35.7%/43.75%)はそのまま——シグナルの表現(`links`等)自体は同じ情報しか持たないため、繰り返しによる改善余地がない。 | **精度が実測値に置き換わる**。2回目以降はwall-clock/CPU/I/Oの実測値を直接使え、false positive/negativeという概念自体が消える(「重い」か「軽い」かの二値予測ではなく連続値の実測)。 |
| `resolution_certificate`への追加フィールド | `native_build_signal: bool`(またはシグナル種別の列挙)程度で足りる。既存のselected package/version情報から独立して計算でき、他フィールドとの依存関係が薄い。 | `measured_cost { wall_clock, cpu_time, io_bytes, environment_fingerprint_digest }`のような複合フィールドが必要。**`environment_fingerprint_digest`は本リポジトリ既存の`EnvironmentFingerprint`/`environments_comparable()`(`crates/laminaria-fingerprint/src/comparability.rs`)と自然に接続でき、実測値の転用可否を機械的に判定する足場が既にある**。ただし`environments_comparable`はOS/architecture/environment_class/os_version/cpu_model/filesystem_type等の一致を要求する「同一性判定」であり、「多少違う環境でも近似的に転用可能」という妥当性判定までは提供しない——仮説Bが残す「環境依存の計測値がどこまで別環境へ転用可能か」という妥当性問題(`dependency-resolved-artifact-closure_ja.md`仮説B項目で既述)は、比較不可の判定はできても、"どこまでなら近似可能か"には未対応のまま残る。 |
| 環境非依存性 | 高い(Cargo.tomlの文字列走査のみで、実行環境のCPU/OS等に依存しない)。 | 低い。同一crateでもビルド環境(CPUコア数、I/Oデバイス種別、既存キャッシュ状態)により実測コストが大きく変動しうる——alopexDBの実測でも`aws-lc-sys`が45.73秒だったのは特定のCI/開発環境下の一点計測であり、他環境での再現性は未検証。 |
| ecosystem横断性 | Cargo固有(P1で確認)。Nimble/C/C++には同種のメタデータシグナルが存在しない。 | ecosystem非依存。ビルド自体を実行して計測するため、Cargo/Nimble/C/C++のどのビルドシステムでも同じ計測手順(wall-clock計測)が適用できる——ただし「計測しやすさ」が均一というだけで、計測タイミング(ビルド開始前に予見できない)という制約は仮説Aより弱い。 |

**評価**: `resolution_certificate`への組み込みやすさという狭い観点では、仮説Aは既存スキーマに軽量な追加で済み、仮説Bは`EnvironmentFingerprint`という既存インフラと自然に接続できる分だけ「妥当性検証つきキャッシュ」の土台は本リポジトリに部分的に存在する。しかし`environments_comparable()`は同一性判定であり近似的転用可能性の判定ではないため、仮説Bのcold start弱点(初回無力)と環境転用問題は依然未解決のまま残る。

両仮説はcold/warmで相補的な強みを持つ——**仮説A(cold-start即応、warm精度は頭打ち)と仮説B(cold-start無力、warm精度は実測ベースへ向上)は排他的選択肢ではなく、仮説Aをcold-start時のフォールバック予見、仮説Bをwarm-cache-hit時の精密値として層状に組み合わせる設計が、`resolution_certificate`の両フィールド(`native_build_signal`と`measured_cost`)を共存させることで机上では矛盾なく成立する**、という組み合わせ仮説が本P2で新たに浮かび上がった。これはissue #63の4仮説のいずれか単独を採用する二者択一ではなく、M1での反証実験に先立つ設計上の選択肢として記録する。

### P0/P1/P2の自己批判：問いの立て方自体がプッシュ型だった

上記P0/P1/P2は、いずれも**「Cargo/Nimbleというpackage managerが何を宣言しているか」(push側)を比較する**という共通の構造を持っていた——P0は「`links`フィールドをcrate作者が書いたか」、P1は「Nimbleに同種フィールドがあるか」、P2は「その宣言をどう`resolution_certificate`へ記録するか」を問うており、いずれも「最終的に要求されるnative artifactが、その義務を実際にどれだけ必要とするか」(pull側)を一度も実測していなかった。

これはissue #62が名指しした失敗——「これまでの調査が繰り返してきた『プッシュ型の分析』(プロデューサー側が今何をしているかを観察し、そこに非効率を見つける)」——を、issue #63自身がPhase 0のコスト予見という別の切り口で再演していたことを意味する。加えて、P0のground truth判定(`ring`/`cxx`/`aws-lc-sys`等を「native build costが重い」と分類した基準)自体も、build.rsが`cc`/`cmake`を呼ぶかどうかというpush側の記述を見ていただけであり、「その義務が最終的に生成する成果物(リンク後バイナリのどのシンボル)にどれだけ帰着するか」というpull側の事実を一度も検証していなかった。

### pull駆動の反証実験：alopex-cliの最終シンボルにaws-lc-sys/zstd-sysがどれだけ帰着するか(issue #63 P0再構成)

**新しい問い**: alopex-cliのエントリポイントから到達可能な最終リンク後バイナリにおいて、issue #59がクリティカルパス上で最もビルド時間を要すると特定した`aws-lc-sys`(45.73秒相当)/`zstd-sys`(23.56秒相当)が、実際に何個のシンボルとしてバイナリに寄与しているかを、Cargo.tomlの`links`宣言(push側)を一切参照せず、**バイナリの中身(`nm`実測)だけから先に確定する**。push側シグナルとの相関は、この実測の後に検証する。

**実測方法**: `.reference/alopex/Dockerfile.issue62-monoitems`(issue #62 P0で作成済みの使い捨て計測用コンテナ)を再利用し、手順として確立した。

```bash
cd .reference/alopex
docker build -f Dockerfile.issue62-monoitems -t laminaria-issue62-monoitems .
docker run --rm -d --name <container> laminaria-issue62-monoitems sleep infinity
# alopexDB自体のバグ(後述)を回避するため、Nim SQLパーサーをコンテナ内でローカルビルド
docker exec <container> bash -c '
  curl -sSf https://nim-lang.org/choosenim/init.sh | sh -s -- -y
  /root/.nimble/bin/choosenim 2.2.10 --yes
  export PATH=/root/.choosenim/toolchains/nim-2.2.10/bin:/root/.nimble/bin:$PATH
  cd /work/crates/alopex-sql/nim-sql-parser
  nimble install -y npeg@1.3.0 msgpack4nim@0.4.4
  nimble lib && nimble staticlib
  mkdir -p /work/local-nim-parser
  cp libalopex_sql_parser.so libalopex_sql_parser.a /work/local-nim-parser/
  printf "0.25.0\n" > /work/local-nim-parser/CONTRACT_VERSION
  cd /work/local-nim-parser && sha256sum libalopex_sql_parser.so libalopex_sql_parser.a > SHA256SUMS
'
docker exec <container> bash -c '
  cd /work
  ALOPEX_NIM_PARSER_ALLOW_LOCAL_BUILD=1 NIM_SQL_PARSER_LIB_DIR=/work/local-nim-parser \
    cargo build -p alopex-cli --bin alopex
'
# scripts/research/pull-symbol-attribution.sh と classify-c-abi-symbols.py で
# 最終バイナリのnm --defined-onlyシンボルをcrate起源へ帰属させる
```

**ビルド障害と回避(記録)**: 素の`cargo build -p alopex-cli`はvendor済みNim SQLパーサーの`CONTRACT_VERSION`(`0.4.0`)がbuild.rsの要求(`0.25.0`)と不整合で失敗した。これはissue #62で既に発見済みの、alopexDB側の既知バグ(`docs/.../dependency-resolved-artifact-closure_ja.md`「Phase 0の情報源自体が誤っている場合」節に既述)である。`ALOPEX_NIM_PARSER_ALLOW_LOCAL_BUILD=1`+`NIM_SQL_PARSER_LIB_DIR`によるローカルビルドフォールバック(alopex-sql側に実装済みの回避経路)で、Nim 2.2.10を`choosenim`でコンテナに導入し、`nim_sql_parser.nimble`の`task lib`/`task staticlib`を直接実行して回避した。この回避のコストと手順自体を、今後同種の実測をやり直す際の固定手順として本節に記録する。

**実測結果**: ビルドされたバイナリは513,711,880 bytes(issue #62実測の514.5MBとほぼ一致)、`nm --defined-only`で244,595個の定義済みシンボル(issue #62実測245,058とほぼ一致)を確認した。シンボルをRust v0マングル形式(`rustfilt`でデマングル、191,484個)とC ABI形式(裸の関数名、53,111個)に分け、それぞれをcrate/ライブラリ起源へ帰属させた。

| 起源 | Rust側(demangle後`crate::path`) | C側(裸のABI名、プレフィックス規則で分類) | 合計 |
| --- | --- | --- | --- |
| `aws-lc-sys`/`aws-lc-rs`(TLS暗号、issue #59クリティカルパス45.73秒相当) | `aws_lc_rs`: 489 | `aws-lc-sys`: 1,682 | 2,171 |
| `zstd-sys`/`zstd`(圧縮、issue #59クリティカルパス23.56秒相当) | `zstd`: 93、`zstd_safe`: 62 | `zstd-sys`: 1,242 | 1,397 |
| 全体(`nm --defined-only`) | 191,484 | 53,111 | **244,595** |

**帰結**: `aws-lc-sys`+`aws-lc-rs`は最終シンボルの**2,171/244,595 ≈ 0.89%**、`zstd-sys`+`zstd`は**1,397/244,595 ≈ 0.57%**しか占めない。issue #59の実測でこの2 crateはビルド時間のクリティカルパス上で最大級(合計69.29秒、総wall time215.4秒の32%)を占めていたにもかかわらず、**最終成果物(pull側)への寄与はシンボル数で見て1%未満**である。これは「ビルド時間が重い義務」と「最終成果物への寄与が大きい義務」が全く別の量であることを、push側シグナル(`links`)を一切参照せずに実測で直接示した——issue #62が「245,058個の内部シンボルのうち、実際にそこへ到達可能性を持つものはどれだけあるか」として提起した問いに対する、具体的な内訳の第一歩である。

**push側シグナルとの相関(事後検証)**: 上記実測を先に確定した上で、これがP0のpush側シグナル(Cargo.tomlの`links`宣言)とどう対応するかを見ると、`aws-lc-sys`/`zstd-sys`はいずれも`links`フィールドを持ち(P0で確認済み)、P0のground truthでも「native build costが重い」と正しく分類されていた。**push側シグナルは「ビルド時間が重くなりうる」ことの予告としては機能したが、「最終成果物への寄与がわずか1%未満である」ことは一切示していなかった**——シグナルの有無と、実際にpull側で確定すべき量(最終成果物への寄与)は、独立した別の軸であることが実測で確認できた。この意味で、P0/P1/P2で検証した仮説A/Bは、いずれも「ビルド時間という一つの側面」だけを予見対象にしており、「最終成果物に何が実際に必要か」という、issue #62が本来確立しようとしていたpull駆動モデルの核心的な問い(バイナリが何を要求するかについての一次知識)には未着手のままだったことが、本節の実測で明らかになった。

### C ABIシンボルの完全帰属、および`-Z print-mono-items`との突き合わせ(issue #63 P0続き)

前節で未着手とした2点(53,111個のC ABIシンボルの完全帰属、`到達可能性`実測との突き合わせ)を、同じ計測手順(Dockerコンテナ)を再実行して完了した。

**C ABIシンボルの完全帰属**: `aws-lc-sys`のパターン(post-quantum実装`mldsa`/`mlkem`、楕円曲線内部関数`p224_`/`p256_`等、`AWS_LC_TRAMPOLINE_aws_lc_0_42_0_*`等crateバージョンを含む決定的文字列)と`zstd-sys`のパターン(`MEM_32bits`、レガシーバージョン付き`ZSTDv05_`/`FSEv05_`等)を拡充し、加えて「コンパイラ/リンカが関数・翻訳単位ごとに生成しcrate起源に帰属しない」シンボル(LLVMのCFI型メタデータ`TM__<hash>_<N>`、`__PRETTY_FUNCTION__.0`)を新規カテゴリとして分離した結果、53,111個中の未分類は971個(1.8%)まで減った。分類済み内訳: コンパイラ生成物47,101個(88.7%、crate起源そのものが無意味)、`aws-lc-sys`2,817個、`zstd-sys`1,633個、Nim SQLパーサー(Cargo package非経由のFFI)576個。

この過程で判明した重要な事実: **53,111個のC ABIシンボルの88.7%は、そもそもどのcrateにも帰属しない性質のシンボル(コンパイラ/リンカが1関数・1翻訳単位ごとに生成する付随物)である**。Rust側マングル形式の集計と合わせても、244,595個の最終シンボルのうち「crateの実装コードそのもの」と呼べるのは半分に満たない可能性が高い——これは「最終シンボル数」という指標自体、義務ごとの計算コストの代理指標として単純に使うには粗すぎることを示す、当初想定していなかった副産物の発見である。

**`-Z print-mono-items=yes`との突き合わせ**: 同じビルドに`RUSTFLAGS='-Zprint-mono-items=yes'`を付与し、406,313行の`MONO_ITEM`出力(issue #62実測406,312行とほぼ完全一致)を得た。各行の`@@ <crate_name>.<hash>-cgu.<n>[Linkage]`という末尾からcrate名を抽出したところ、**issue #62が「cgu名がハッシュ化されているためcrate名での厳密な境界特定はできない」としていた限界は、大部分のcrateについては誤りだった**——crate名はハッシュの前に文字列プレフィックスとして残っており、機械的に抽出できる(`scripts/research/classify-mono-items.py`)。ただし限界がゼロになったわけではない: build.rs(ビルドスクリプトバイナリ)自身のcguと、各crateの一部のcgu(検証した範囲では`alopex`/`alopex-sql`/`alopex-cli`自身を含む)はcrate名プレフィックスを持たない裸のハッシュ名になり(406,313行中106,743行、26.3%)、これらはcgu名だけでは帰属できない——issue #62が「モジュールパスからの推定」という代替手法に頼った理由はここにある。

crate別mono-item数: `aws_lc_sys`(C FFIバインディング側)371個——**issue #62本文が実測した「371個」と完全一致**——、`aws_lc_rs`(Rustラッパー側)1,704個、`zstd`/`zstd_safe`263個(`zstd_sys`はcgu名から検出できず0件、上記の裸ハッシュ限界に該当する可能性がある)。全体406,313個に対し、aws-lc系合計2,075個(0.51%)、zstd系263個(0.06%)。

**3指標の一致**: ビルド時間(issue #59: aws-lc-sys系クリティカルパス45.73秒/zstd-sys系23.56秒、全体215.4秒の32%)・mono-item数(0.51%/0.06%)・最終リンク後シンボル数(1.35%/0.73%)という独立な3つの指標いずれで見ても、この2 crateの「計算コストの重さ」は最終成果物への寄与としては小さいという結論が一貫して再現された。これはissue #62本文の「monomorphized item数(371個)がごく少数であるにもかかわらずビルド時間はクリティカルパス上最大だった」という観察を、独立した実測(最終シンボル数)でも裏付ける。

**結論**: pull駆動で先に確定した「最終成果物への寄与は小さい」という事実は、3つの異なる指標で頑健に成立する。一方で「ビルド時間が重い」という事実(issue #59実測)も動かない。この2つは矛盾ではなく、issue #62が最初から指摘していた通り「pull側の必要性」と「その義務を満たすための計算コスト」が別の軸であることの定量的な確認である——push側シグナル(`links`)は後者(ビルド時間が重くなりうること)の予告にはなるが、前者(最終成果物への寄与)については何も語らない。

詳細な抽出スクリプトは`scripts/research/pull-symbol-attribution.sh`/`scripts/research/classify-c-abi-symbols.py`/`scripts/research/classify-mono-items.py`に記録した。

### `aws-lc-sys`のfeature構成調査：到達不能コード仮説の棄却(issue #64 P0)

**issue #63自身の前提不備**: 上記のP0〜P2はいずれも「native build costが重い義務を、いつ・どう早期に見分けてスケジューリングするか」という、issue #59が定義した2つの全体最適目的関数のうち「クリティカルパス長の最小化」にしか向いておらず、もう一つの目的関数「要求されるnative artifactにとって本質的な計算量の最小化」には一度も触れていなかった。`aws-lc-sys`が最終成果物に2%程度しか寄与しないのに45.73秒を要求する原因そのもの(なぜ重いか)は未検証のまま残されていた。issue #64はこの前提不備を指摘し、3仮説(α: 到達不能コードの過剰コンパイル、β: コンパイル自体の構造的コスト、γ: プロセス起動/I/Oオーバーヘッド)のうち仮説αをP0で検証する。

**静的feature解析**: alopex-cli(pinned commit `06cd95941857ea44e657de57289bcff41a3645e2`)の依存グラフを`cargo tree -e features`で解決すると、`aws-lc-sys`は`rustls`→`aws-lc-rs`経由(TLS)と`object_store`→`aws-lc-rs`経由(S3ストレージ、`alopex-cli`の`s3` feature)の2系統から到達する。`aws-lc-rs`は`aws-lc-sys`に対し`default-features = false`で依存しており、`aws-lc-sys`自身の`default = ["all-bindings"]`は既に無効化されている。実際に有効化されるのは`rustls`が要求する`aws-lc-rs/prebuilt-nasm`経由の`aws-lc-sys/prebuilt-nasm`のみである。FIPS実装(`aws-lc-fips-sys`、別クレート、Go toolchain必須)は`aws-lc-rs`のoptional dependencyで、`fips` featureは`aws-lc-rs`のdefaultに含まれず、依存グラフのどこからも有効化されていない——**post-quantum/FIPS検証機構はそもそも最初からデフォルト無効**であり、仮説αが前提していた「無効化可能な過剰機能」は依存グラフ上に存在しなかった。

**実測による確認**: 単一依存(`aws-lc-sys = "=0.42.0"`)のみの隔離crateをDocker(`rust:1.96-bookworm`、alopex-cli pin済みツールチェーンと同一)でビルドし、(A) alopex-cliの実際の解決結果を再現した構成(`default-features = false, features = ["prebuilt-nasm"]`)と、(B) `all-bindings`を含む完全デフォルト構成を比較した(`scripts/research/measure-aws-lc-sys-feature-cost.sh`で再現可能)。

| 構成 | ビルド時間(壁時計、2回試行) | 静的ライブラリサイズ | エクスポートされたtext シンボル数 | 静的ライブラリSHA256 |
| --- | --- | --- | --- | --- |
| A: 解決済み構成(prebuilt-nasmのみ) | 53.94秒 / 29秒 | 7,050,908 bytes | 3,686 | `b0f2aabb...` |
| B: 完全デフォルト(all-bindings込み) | 51.56秒 / 32秒 | 7,050,908 bytes | 3,686 | `b0f2aabb...`(A と完全一致) |

2回の独立試行で、構成A/Bのビルドが生成する`libaws_lc_0_42_0_crypto.a`は**バイト単位・SHA256ハッシュまで完全に同一**だった。ビルド時間差(1回目53.94秒 vs 51.56秒、2回目29秒 vs 32秒)はDockerレイヤーキャッシュ状態に起因するノイズであり、構成間の系統的な差ではない。`all-bindings` featureは実際にはRust側のbindgenバインディング生成範囲(どのC関数をRustから呼べるようにするか)にのみ影響し、AWS-LC本体のCソースがコンパイルされる範囲(=native build costの実体)には一切影響しない。

**結論(仮説α棄却)**: `aws-lc-sys`のビルド時間45.73秒は、feature flagで無効化可能な「到達不能コード」(FIPS/post-quantum/未使用バインディング)によるものではない。これらは依存グラフ上で最初からデフォルト無効であり(FIPS/post-quantum)、有効なfeature(`all-bindings`)を切り替えてもコンパイル対象のCソース量はビットレベルで変化しない(未使用バインディング)。issue #62が提起した「到達可能性による枝刈り」は、この45.73秒に対しては効果を持たない——枝刈りで削れる「到達不能なコード」がそもそも存在しないため。停止条件に従い、焦点は仮説β(コンパイル自体の構造的コスト)/仮説γ(プロセス起動・I/Oオーバーヘッド)、すなわちAWS-LC本体のCソース行数・最適化パス自体の重さへ移す。

詳細な計測スクリプトは`scripts/research/measure-aws-lc-sys-feature-cost.sh`に記録した(使い捨てDocker隔離環境、実行のたびに一時ディレクトリへcrateを生成しビルド後は自動削除)。

### `bcm.c` unity buildの構造分析：post-quantumコードの無条件混入(issue #64 仮説β)

仮説α棄却(feature flagで無効化可能な過剰機能は存在しない)を受け、停止条件に従い仮説β(到達可能だが、コンパイル自体のコストがsource行数/複雑度に対して不釣り合いに重い)を検証する。

**構造分析**: `aws-lc-sys`のビルダー選択ロジック(`builder/main.rs`)は、FIPSビルドでない限り`CcBuilder`(個別`cc`呼び出し、CMakeを経由しない)を優先する。事前生成済みバインディング(`src/x86_64_unknown_linux_gnu_crypto.rs`等、プラットフォームごとに同梱)が存在するため、`is_bindgen_required()`は`false`を返しbindgenは実行されない——仮説βが当初着目していた「bindgen生成コード量」はそもそも実行されていない工程であり的外れだった。

`CcBuilder`のソースリスト(`builder/cc_builder/universal.rs`)には253個の`.c`翻訳単位が並ぶが、その1つ`crypto/fipsmodule/bcm.c`(BoringCrypto Module)は**unity build方式**で118個の内部`.c`ファイルを`#include`によって単一翻訳単位に集約しており、その中に以下がある。

```c
#include "ml_dsa/ml_dsa.c"   // ML-DSA (post-quantum署名)
#include "ml_kem/ml_kem.c"   // ML-KEM (post-quantum鍵カプセル化)
#include "pqdsa/pqdsa.c"
#include "evp/p_kem.c"
#include "evp/p_pqdsa.c"
```

これらの`#include`は**feature flagやプリプロセッサ条件分岐で保護されていない**——`bcm.c`をコンパイルする限り、ML-DSA/ML-KEMは常にコンパイルされる。これは仮説αの枠組み(「無効化可能な過剰機能」の有無)では捉えられない構造的事実であり、仮説αとは独立に検証が必要だった理由でもある。

**実測**: `bcm.c`をAWS-LCビルドシステムから切り離して単体で`cc -c -O2`コンパイルし、(a)無改変版と(b)上記5つの`#include`行を削除した版を比較した(`scripts/research/measure-bcm-unity-build-cost.sh`で再現可能、リンクは行わずオブジェクト生成のみで比較、3回試行)。

| 構成 | プリプロセス後行数(コメント/空行除く) | コンパイル時間(3回平均) |
| --- | --- | --- |
| bcm.c(無改変、post-quantum込み) | 76,897行 | 12.30秒 |
| bcm.c(post-quantum `#include` 5行削除) | (未計測、差分は約21,139行相当) | 10.34秒 |
| ML-DSA単体(`ml_dsa.c`、再帰include込み) | 12,821行 | (bcm.c内に混入のため単独計測不可) |
| ML-KEM単体(`ml_kem.c`、再帰include込み) | 8,318行 | (同上) |

post-quantumコードの除去でコンパイル時間は**12.30秒→10.34秒、1.96秒(約16.0%)削減**。3回試行(12.33/12.26/12.32秒 vs 10.20/10.48/10.34秒)で安定して再現し、初回試行(12.60秒 vs 10.58秒、16.08%)とも一致する。post-quantumコードは全体行数の約25%(21,139/83,955行、初回プリプロセス計測)を占めるが、コンパイル時間への寄与は16%とやや小さい——行数比率とコンパイル時間比率が完全一致しないこと自体も、仮説β「コンパイルコストがsource行数に単純比例しない」ことの部分的な裏付けである。

`aws-lc-sys`全体のビルド時間はissue #59実測で45.73秒(pkg全体、bcm.c以外の252翻訳単位のコンパイル+リンク等を含む)であり、今回計測した`bcm.c`単体12.30秒はその一部にすぎない。従って**post-quantumコード(1.96秒)がaws-lc-sys全体ビルド時間に占める割合は45.73秒に対し約4.3%**——issue #63が特定した「最終成果物寄与2%未満」ほど無視できる量ではないが、45.73秒の主因でもない。

**結論(仮説β部分支持)**: `bcm.c`のunity build構造は、post-quantumコードをfeature flagで一切保護せず常時コンパイルする——これは仮説αが前提とした「feature flagで無効化可能」という枠組みの外側にある構造的コストであり、仮説αとβが指す原因は独立に存在することが確認された。ただしpost-quantumコード単独(1.96秒)はaws-lc-sys全体45.73秒の主要因(過半)ではなく、**bcm.c自体(12.30秒、全体の約27%)が単一の重い翻訳単位であること**、および残り252翻訳単位の合計(45.73秒 - 12.30秒 ≈ 33秒相当)がより大きい割合を占めることが今回の計測で新たに判明した。post-quantumコード除去は「無視できない改善(4.3%)だが単独では45.73秒の大半を説明しない」ため、仮説βは部分的に支持されるが、残り33秒相当の内訳(他の251翻訳単位、リンク工程、ビルドスクリプト自体のオーバーヘッド)は未検証のまま残る。

**次の検証対象**: bcm.c以外の252翻訳単位のうちどれが重いか(cURVE25519/RSA/EC等、上位候補は`self_check.c`3,042行/`curve25519_nohw.c`2,080行/`e_aes.c`1,672行/`rsa.c`1,548行等、preprocessed行数上位で確認済み)、および仮説γ(プロセス起動・I/Oオーバーヘッド、253個の`cc`プロセス起動コストの合計)の検証は後続issueで扱う。

詳細な計測スクリプトは`scripts/research/measure-bcm-unity-build-cost.sh`に記録した(ローカルの`cc`とCargoレジストリキャッシュ済みのaws-lc-sysソースツリーを使用、Docker不要——`bcm.c`単体のプリプロセス/コンパイルはAWS-LCのビルド設定に依存しない標準Cコンパイルであるため)。

### `bcm.c`の分解可能性実測：issue #47 B-H3定式化の問い1(issue #64/#47共同記録)

issue #47でB-H3(summary/body分離)を`aws-lc-sys`/`bcm.c`に対する計算問題として定式化した際、3つの具体的な問い(1: 分解可能性、2: root集合確定コスト、3: 分解の利得)を立てた。本節は問い1(118個のunity-buildメンバーのうち何個が独立翻訳単位として分解可能か)を実測した結果を記録する。

**方法**: `bcm.c`が`#include`する118個のメンバー`.c`ファイルそれぞれを、`bcm.c`を経由せず単独で`cc -c`コンパイルし、成功/失敗を機械的に判定した(`scripts/research/measure-bcm-decomposability.sh`)。当初の素朴な単独コンパイルでは、`bcm.c`自身が118メンバーの37個目以降でのみ`cpucap/internal.h`(`SET_DIT_AUTO_RESET`マクロ等の定義元)を`#include`する構造になっているため、26個が見かけ上失敗した——これは真の構造的結合ではなく、単独コンパイル時に`bcm.c`と同じマクロ可視性を再現していなかったテストハーネス側の不備だったため、`cpucap/internal.h`を`-include`で強制的に先読みするよう補正し、再測定した。

**結果**: 117個中98個(83.8%)が単独翻訳単位として分解可能(標準的な`#include`パス・マクロ定義の可視性さえ揃えれば`bcm.c`を経由する必要がない)。残り19個(16.2%)は真に構造的結合がある——原因を追跡すると、AWS-LCの`fipsmodule/delocate.h`が定義する`DEFINE_METHOD_FUNCTION`/`DEFINE_LOCAL_DATA`等のマクロが、`BORINGSSL_FIPS`ビルドでは`static`スコープの関数・変数を生成する設計であり(非FIPSビルドでも同様に`static`)、これらのシンボルはunity build(=同一翻訳単位)内でしか他メンバーから参照できない。加えて`aes/mode_wrappers.c`の`aes_hw_encrypt_wrapper`のような`static inline`ヘルパーも同種の結合を生む。

**post-quantumコードとの関係**: 前節(仮説β)で除去した`ml_dsa.c`/`ml_kem.c`自体は、この分解可能性テストで**単独コンパイル可能(OK)** と判定された——つまり「post-quantumを除去して16%短縮できる」という前節の実測は、構造的結合を回避したのではなく、単に`bcm.c`から対応する`#include`行を削っただけで達成できていた。一方で分解不能な19個(`self_check.c`、`evp/p_ec.c`等のFIPS関連メソッドディスパッチ層)は、post-quantumとは別の理由(delocate.hのstatic化設計)で真に不可分である。**「分解可能か」と「除去して利得があるか」は独立した軸であり、post-quantumはこの実測で両方を偶然満たしていたに過ぎない**。

**問い1への回答**: `bcm.c`の118メンバーのうち、Phase 1(到達可能性が意味的に確定した)情報だけをもとに個別に materialize/非materializeを判断できる対象は**最大でも83.8%**であり、残り16.2%はAWS-LC自身のFIPS境界設計(`delocate.h`)により、到達可能性の判定結果によらず常に`bcm.c`全体と一緒にコンパイルせざるを得ない。B-H3(summary/body分離)がこの具体例で実現しうる理論上の上限は、この83.8%という分解可能な部分集合に制約される。

詳細な計測スクリプトは`scripts/research/measure-bcm-decomposability.sh`に記録した(118メンバー全件を機械的に判定、CSV形式で結果を出力)。

### root集合確定コストの実測：issue #47 B-H3定式化の問い2(issue #64/#47共同記録)

問い2「$E_{call}$(rustls→aws-lc-rs→bcm.cメンバーの呼び出し関係)をPhase 1情報だけで確定するコストは、$C_{body}(b_{bcm})$=12.30秒に対してどれだけ小さいか」を実測した(`scripts/research/measure-phase1-root-set-cost.sh`)。

**方法**: alopex-cliが解決する`aws-lc-rs`のfeature構成(`default-features = false` + `prebuilt-nasm`、issue #64 P0で確認済み)に一致させた隔離crateを作り、`cargo check --verbose`(型チェックのみ、コード生成なし=Phase 1相当の作業)をクリーン状態から実行し、`aws-lc-sys`の`build.rs`(`build-script-main`、`bcm.c`を`cc`でコンパイルする本体)が実際に実行されるかをverboseログで確認した。

**結果**: `cargo check`は**必ず`aws-lc-sys`の`build-script-main`を実行する**(`Running .../aws-lc-sys-<hash>/build-script-main`が観測される)。この実行こそが`bcm.c`を含むAWS-LC全体のコンパイルそのものであり、`aws_lc_sys`クレート自体のRust側メタデータ生成(`--emit=dep-info,metadata`、コード生成なし)ですら、生成済みの静的ライブラリ(`-l static=aws_lc_0_42_0_crypto`)へのリンク指定を要求する——つまりCargoのRust側コンパイル単位は、native部分の`build.rs`が生成した成果物の**存在**を前提としており、型チェックだけを得ようとしても`build.rs`の実行(=Phase 2の実行コスト)を回避する経路が存在しない。

**問い2への回答(想定と異なる形で確定)**: 当初の問い2は「root集合確定コストは12.30秒よりどれだけ小さいか」という比較を想定していたが、実測結果はそれ以前の、より根本的な障壁を示した——**現行のCargo/build.rs設計では、Phase 1(型チェックによる到達可能性確定)とPhase 2(native実行コスト)が、原理的に分離不可能である**。`cargo check`(Phase 1を得る最も軽い手段のはずの操作)自体が、必ず`build.rs`実行(Phase 2の主要コスト源)を通過する。これはissue #64が確認した「`bcm.c`のunity build構造がPhase 1とPhase 2を1つの翻訳単位へ結合している」という事実(仮説β)と同型だが、より上位のレイヤ(Cargoのビルドグラフそのもの)で起きている——**LAMINARIA自身のビルドグラフでこの分離を実現するには、Cargoのbuild-dependency解決モデル自体を経由しない、独自のPhase 1確定経路(root集合を先に確定してからのみnative buildをスケジューリングする経路)が必要である**、という設計上の要請が本実測で具体化された。

**B-H3への影響**: 問い1(分解可能性、83.8%)と問い2(root集合確定コスト)を合わせると、B-H3(summary/body分離)がこの具体例で機能するためには、(a) 分解可能な98/117メンバーへのアクセスと、(b) `cargo check`を経由しないroot集合確定手段の両方が必要であり、**いずれもCargoの既存ビルドモデルの外側にLAMINARIA自身が持つ必要がある**ことが確認された。これは`docs/near-term-research-program.md`の「Package resolution must not be confused with opaque build scripts, compilers, or linkers launched by a package manager」という原則が、この具体例で機能するために構造的に必要とされることを示す実測的裏付けである。

詳細な計測スクリプトは`scripts/research/measure-phase1-root-set-cost.sh`に記録した。

## Artifact profile

全platformで一律の「単一完全static binary」を要求しない。targetとdependencyに応じて、少なくとも次を明示する。

### Self-contained

実行に必要なuser-space code／dataを可能な限り一つのnative executableへ含める。外部package managerや同梱shared libraryを要求しない。OS kernel／ABI等の不可避なplatform contractは残る。

### Relocatable bundle

native executableと、必要なshared library、resource、runtime、loader metadataを一つの移動可能なbundleへ閉じる。利用者はCargo/Nimble/CMakeを実行せず、bundle entryを起動する。

### System-integrated

system library、GPU driver、framework等を外部に残す。その場合はpackage名だけでなく、ABI、version range、required symbol/capability、loader rule、検証方法をmachine-readable runtime contractへ記録する。このprofileは「完全に依存から解放」とは呼ばない。

profileはsolverの結果で暗黙決定せず、要求artifact／target policyの一部とする。license、security update、binary size、platform convention、dynamic plugin要件によりstaticとbundleの最適解は異なる。

## Artifactが持つ証拠

```text
DependencyDischargedArtifact
  artifact_digest
  target_triple / platform / minimum_os
  entrypoint
  runtime_closure[]
    identity / digest / ABI / required symbols / location policy
  external_runtime_contracts[]
  resolution_certificate
    selected package/version/feature/provider/toolchain
    rejected alternatives and reasons
  reachability_certificate
    roots / retained nodes / conservatively retained nodes
    pruned nodes and reasons
  build_provenance
    source/material identities / builder identity / operations
  reproducibility_contract
  test_contracts[]
    exact subject identity / controls / observations / oracle
  test_results[]
    subject + harness + environment identities / raw evidence
```

SLSA provenanceはartifactを`subject`、取得した依存を`resolvedDependencies`として記録し、どのbuild definitionとbuilderがartifactを生成したかをattestationにする。[^slsa] LAMINARIAのcertificateはこの形式を置き換えず、SLSA等へexport可能な内部provenanceを持つ。

重要なのは、SBOMやprovenance fileがあるだけでclosure完成としないことだ。実際のbinary／bundleに含まれるloader dependency、symbol、resourceとcertificateを照合し、宣言されていないruntime dependencyを検出する。

同様に、一度起動できただけではartifact完成としない。成果物はexact artifact digestに結び付いたtest contractとtest resultを持ち、再実行可能なharnessからfunctional、cross-language、ABI、runtime、negative dependency条件を検証できなければならない。instrumented／test-profile binaryと利用者へ渡すproduction binaryは別identityとして扱う。詳細は[Testable Native Artifactと第一級Test Harness](testable-native-artifact-harness_ja.md)に定める。

## 「依存から解放」の具体的意味

利用者は次を要求されない。

- Cargo、Nimble、CMake、Meson等による再resolution
- Rust、Nim、C、C++ compilerの導入
- compatible package versionやfeatureの手選択
- header/include path、link flag、library orderの復元
- package manager間の同名／異version dependency conflictの調停
- build machineに偶然あったundeclared libraryの再現

代わりに必要なのは次のいずれかだけである。

- self-contained executableを対象platformで起動する
- relocatable bundleを展開してentrypointを起動する
- system-integrated profileの明示runtime contractを検証して起動する

## 枝刈りとの関係

[Cross-layer枝刈り](cross-layer-reachability-pruning_ja.md)はdischarge後のartifactを小さく、速く、省メモリにする重要な手段だが、この価値の本体ではない。枝刈りを一切しなくても、異種dependency obligationをsemantic validation、lowering、code generation、linkでdischargeできれば、利用者は元のpackage/compiler graphを再解決しない。

枝刈りの役割は、dischargeする必要がない義務を`ProvenIrrelevant`と証明し、生成workとruntime closureを最小化することである。単にclosureをbundleすると、build ecosystemの蟻地獄を巨大な配布物へ移しただけになるため、NativeExecutableと外部公開contractから到達しないpackage、source、IR、object、archive member、symbol、runtimeを除く。

一方、最小化を理由にdynamic lookup、FFI export、constructor、plugin、platform runtimeを推測で消してはならない。`RetainedConservatively`を第一級状態にし、なぜ残ったかを利用者と開発者が確認できるようにする。

## 解決時の不変条件

- 全てのpackage／semantic／IR／artifact／link obligationは`Discharged`、`Externalized`、または`Rejected`であり、未解決状態をartifactへ持ち越さない。
- `Discharged`には、それを充足したspecialization／lowering／generation／link／embedding操作へのprovenanceがある。
- build-only dependencyはruntime closureへ漏らさない。
- runtime dependencyはartifact内に含めるか、external runtime contractとして列挙する。
- runtime closureの各nodeにartifact内locationまたは外部provider requirementがある。
- required symbol／resourceには実体とproducerがある。
- target、ABI、loader、runtime policyが変わればclosure identityも変わる。
- pruned dependencyをprovenanceから消さず、「候補だったが成果物へ影響しなかった」と区別できる。
- secret、absolute temporary path、build-host固有の偶然をartifact contractへ固定しない。
- closureの完全性はmanifestだけでなく、生成artifactの直接検査と起動で確認する。
- exact production artifact identityにtest contractとtarget上の実行証拠が結び付き、test-only dependencyはrelease artifactへ漏れない。

## 直接的な実行可能テスト

最初のmixed Cargo/Nimble/C/C++ native artifactについて次を直接検証する。

1. cleanな実行環境にCargo、Nimble、Rust/Nim/C/C++ compilerを置かずに起動できる。
2. production graph上の全依存義務が`Discharged`、`Externalized`、または`Rejected`に到達し、その根拠操作を追跡できる。
3. self-containedまたはbundle profileでは、宣言されたartifact集合だけを移動して起動できる。
4. system-integrated profileでは、runtime contractを満たさない環境を起動前に構造化診断で拒否する。
5. runtime loader dependency、required/provided symbol、resourceをproduction graph/certificateと照合する。
6. build-only dependencyがruntime closureに含まれない。
7. 不要package/codeを加えても枝刈り後runtime closureとobservable behaviorが変わらない。
8. 必要runtime dependencyを一つ除くと、欠落したidentity／ABI／symbolを特定して失敗する。
9. exact production binaryをtest subjectとしてharnessから実行し、instrumented binaryだけの成功で代替しない。
10. test-only package／symbol／runtimeがrelease artifactへ含まれず、test profileでは必要rootが保持される。

手書きfixture manifestを正本にせず、production resolver、planner、compiler、linkerが生成したartifactとcertificateをtest対象にする。

## 測定

- obligation総数と`Discharged`／`Externalized`／`Rejected`／未解決数
- discharge kind別件数と、packageから最終artifactまでのobligation critical path
- build closure node数とruntime closure node数
- pruned package／source／IR／artifact／symbol数
- executable／bundle sizeとclosure size
- runtime shared-library／resource数
- undeclared runtime dependency数
- clean environmentでの起動成功率とstartup time
- artifact移送bytesとcache hit率
- rebuildなしで利用可能なtarget environment数
- external contractごとの互換／非互換診断時間

Nixのclosure sizeはstore objectと全到達objectの合計として計測される。[^nix-size] LAMINARIAもtop-level artifact sizeだけでなくruntime closure全体を測る。

## 現時点の主張

主張してよいこと:

> LAMINARIAはCargo/Nimble/C/C++のpackage選択、source semantics、language/intermediate IR、ABI、symbol、linkにまたがる異種dependency obligationをbuild時に解決・変換・dischargeする。元のecosystem graphはprovenanceとして追跡できるが、利用者が再解決すべき問題としては残さない。最終artifactに不可避なruntime要件だけを内包するか明示contractへexternalizeし、枝刈りはその解決・生成workを最小化する。

まだ主張しないこと:

- 全targetで依存ゼロの単一static binaryになる
- OS、kernel、driver、system serviceから独立する
- dynamic plugin／reflectionを完全に静的発見できる
- Nix／Guix等に存在するclosure deployment自体がLAMINARIA独自である
- certificateがあるだけでsupply-chain securityが自動的に保証される

## Sources

[^nix-closure]: Nix Project, “[Glossary — closure](https://nix.dev/manual/nix/2.20/glossary#gloss-closure).” Accessed 2026-09-13.
[^nix-output]: Nix Project, “[Building — Processing outputs](https://nix.dev/manual/nix/2.30/store/building.html#processing-outputs).” Accessed 2026-09-13.
[^slsa]: SLSA, “[Build: Provenance](https://slsa.dev/spec/v1.2-rc2/build-provenance).” Accessed 2026-09-13.
[^nix-size]: Nix Project, “[Store Object Info — Closure Size](https://nix.dev/manual/nix/2.35/protocols/json/store-object-info.html#property-closuresize).” Accessed 2026-09-13.

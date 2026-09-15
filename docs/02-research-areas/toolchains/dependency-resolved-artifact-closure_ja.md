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

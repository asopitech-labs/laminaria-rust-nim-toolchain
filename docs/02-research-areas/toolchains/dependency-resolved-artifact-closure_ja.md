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

# LAMINARIAエンティティモデル：Rust/Nim/foreign-native連鎖の実測ベースERD

## 目的

[独自コンパイラの責務契約](../../01-foundations/compiler-ownership-contract_ja.md)が要求する「LAMINARIA自身が内部で保持すべきエンティティ・エンティティ間のリレーション・状態遷移/アクティビティ」を、既存パイプライン（Cargo/rustc/LLVM、`nim c`/`nim cpp`）の実測に基づいて確定した記録。issue #52（Rust側パイプライン解体）・issue #71（エンティティモデル確定）・issue #74（ビルド全体フロー統合）・issue #76（Nimビルドパス実測分解）で得られた結論を統合する。

いずれも既存パイプラインの**参照/比較実験**（[責務契約](../../01-foundations/compiler-ownership-contract_ja.md)のReference/baseline欄）としての解体であり、既存の`nim c`/`cargo`/`rustc`をLAMINARIA自身の実装経路としてそのまま採用するものではない。

## Rust側エンティティ連鎖（issue #52/#71）

issue #52がCargo/rustc/LLVMを実測分解し、issue #71がそれをLAMINARIA自身のエンティティとして再定義した。

### 確定エンティティ（10、`Cgu`は意図的に除外）

`WorkspaceManifest`/`MemberManifest`、`PackageResolution`、`SourceUnit`（feature条件グラフとして保持、[SourceUnitGraph](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/71#issuecomment-5708615948)）、`SemanticFact`、`GenericDefinition`、`InstantiationKey`（5要素キー：宣言識別子, 型引数, const generic値, ABI/ターゲット, 有効feature集合）、`OptimizationDecision`、`LoweredModule`、`NativeObject`、`LinkSymbol`、`LinkedArtifact`。

いずれもRust MIR/LLVM IRという既存表現をそのまま使い、LAMINARIA独自の別中間表現には変換しない——変わるのは「どの単位でキーを割り当て、どこに保持するか」という管理方法のみである（[project_laminaria_ir_priority]方針との整合）。

### アクティビティ・状態遷移（A0〜A12）

- **A0**: `BuildStrategyContext`判定（最尤ストラテジ/環境適応ストラテジ、issue #52で導入）。**パイプライン全体で唯一の真の起点**——外部入力（ビルド呼び出しコンテキスト：CI設定ファイルの有無、直前のビルド履歴の有無）のみに依存し、他のいかなるアクティビティの出力も必要としない。A1・N1（→N3）は**A0の後にのみ起動でき、A0の完了以降は互いに完全並行**（言語間の優先順位はない、後述「Rust側/Nim側/foreign-native連鎖の統合」節の並行実行境界を参照）。ターゲットパラメータの確定方法（既定値を即座に使うか検出するか）のみに影響し、A1〜A8/A10〜A12の出力内容には影響しない。
- **A1〜A5**: マニフェスト構成（ワークスペース階層）→依存解決→feature条件グラフ構築→`SourceUnit`選択。
- **A6**: 意味解析（`SourceUnit`→`SemanticFact`）。
- **A7**: 単相化（`GenericDefinition`→`InstantiationKey`）。依存クローズ全体で一意なキーとして共有し重複を許さない——境界撤廃の核心。
- **A8**: 最適化判断（`InstantiationKey`→`OptimizationDecision`）。
- **A9**: コード生成準備（`InstantiationKey`+ターゲットパラメータ→`LoweredModule`）。**訂正済み**：当初「複数の`InstantiationKey`をグルーピングする」設計だったが、これは`Cgu`型の後付け収束点を別名で再導入するものだったため、**1:1の関数適用のみ**に訂正した。並列コンパイルのための実行単位の切り方はA10のスケジューリング上の都合として完全に分離する。
- **A10〜A12**: コード生成→シンボル登録→リンク。**収束点はA12（リンク）ただ1箇所**——情報が広がる箇所はA7（単相化）とA11（シンボル登録）のみ。**A11の責務保証**：A7と同型の重複排除保証を持つ——同一の`(識別子, リンケージ)`に対する登録操作は冪等であり、並列実行される複数のA10出力が同時に到達しても、`LinkSymbol`ストアへの挿入は一意なキーの下で1回のみ確定する（挿入順序に関わらず最終状態は同じ、後着の同一キー登録は既存エントリと照合し重複を作らない）。issue #82の検証で「A11には冪等性保証が未記述」という欠落が発見されたため、A7の設計原則をA11にも横展開する形でここに確定する。

## Nim側エンティティ連鎖（issue #76）

issue #76が`nim c`/`nim cpp`を実測分解した（Nim 2.2.10、実fixture）。

### 確定エンティティ（12）

`NimblePackageManifest`、`NimBuildInvocation`（1回の`nim c`プロセス実行、Rust側に直接対応するエンティティがない新規概念）、`NimModuleUnit`、`ModuleImportEdge`、`StdlibModuleUnit`、`GeneratedCModule`、`GenericDeclaration`、`NimInstantiationKey`、`ExportedSymbol`、`CCompileCommand`、`CObjectFile`、`LinkCommand`/`NimExecutable`。

### アクティビティ・状態遷移（N1〜N9）

N1(Manifest構成)→N2(依存解決)→N3(`NimBuildInvocation`起動)→N4(モジュール到達可能性解決)→N5(モジュール単位の意味解析+C生成、1:1)→N6(単相化)→N7(Cコンパイル)→N8(リンク、収束点)→N9(`{.exportc.}`登録)。

**N1の起動条件**：A0（`BuildStrategyContext`判定）の完了後にのみ起動する——A0はパイプライン全体で唯一の真の起点であり、Rust側A1と同じくN1もA0の出力（ストラテジ判定結果）を前提とする。A0完了後は、N1（→N2〜N3を経てNimBuildInvocation起動）とA1はRust/Nim間で優先順位を持たず完全並行に起動してよい。

**N6の責務保証**：同一`NimBuildInvocation`内では、A7と同型の重複排除保証を持つ——同一の`NimInstantiationKey`に対する実体配置は1回のみ確定し、複数の呼び出し元から同時に要求されても冪等に扱われる（[実測6](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/76#issuecomment-5713582004)で確認済み）。ただし`NimBuildInvocation`境界を越えた重複排除は保証されない（前掲「Nim側との構造的差異」参照）——これはA11の`LinkSymbol`挿入保証（言語・実行境界を問わず依存クローズ全体で一意）とは異なるスコープの保証であることに注意。

### Rust側との構造的差異（実測で確定）

1. **意味解析とコード生成がN5に統合**——Rust側の`SemanticFact`(A6)と`LoweredModule`(A9)の分離がNim側にはない。
2. **`NimInstantiationKey`の多重度が2階層**：同一`NimBuildInvocation`内では1:1（重複排除）、`NimBuildInvocation`をまたぐと1:0..N（重複発生）。実測により、Rust側`Cgu`問題と同型の重複コンパイルがNim側にも実在することを確認した（独立コンパイル間で標準ライブラリ・単相化インスタンスが完全に重複生成される）。
3. **巻き戻し検査で発見した病理**：既存`nim c`が単相化実体を識別する実際のキーには、宣言識別子・型引数・const generic値に加えて「最初に構文的に要求したモジュール」という意味的に無関係な情報が混入している。同一プログラムでも`import`の記述順序を変えるだけでシンボル名が変わることを実証した——Rust側`Cgu`問題より恣意的な要因に依存する分、悪い性質を持つ。LAMINARIA自身の`NimInstantiationKey`設計は、Rust側と同じ3要素（宣言識別子・型引数・const generic値）のみで構成し、呼び出し元情報を意図的に排除する必要がある。
4. **`{.exportc.}`境界は呼び出し元非依存で安定**——非exportc内部シンボルの不安定性と異なり、FFI境界のシンボル名はマングルされず常に安定している。

### コスト規模の判断（issue #76・issue #81で訂正）

issue #76の初期実測（1関数+3標準ライブラリimportという小規模fixture）では、重複コスト（重複1箇所あたり数十〜数百ms）がissue #52がRust側で確認した規模（215秒中45秒、Phase-2コスト）より2〜3桁小さいという結論だった。issue #81で標準ライブラリ10モジュールを使う実規模に近いfixtureで再実測したところ、**gccコンパイル部分だけで1.339秒**という、無視できない規模の重複コストが確認された。

**訂正**：「規模が小さいから重複コストは無視できる」という一般化は誤りであり、規模依存の判断である。ただし今回測った1.3秒はRust側issue #52の45秒とはまだ1桁以上の差があり、[project_laminaria_goal_and_bottleneck]（LAMINARIAの一次目標＝ビルド時間高速化、主戦場はRust側LLVM）に照らした「現状の優先度は低い」という結論自体は覆らない。実在するLAMINARIA自身の依存クローズ規模でこの重複コストが実際にどこまで積算されるかは、さらなる実測が必要な未確定事項として残る。

### `when defined(...)`条件グラフの実測（issue #78）

issue #71のNim側対応表が「`when defined(...)`はRustの`cfg`属性と同型」と評価した性質を実測で裏付けた。50個の独立した`when defined(featureN)`分岐（それぞれ1関数を条件付きで宣言）を持つfixtureで、全て無効化した場合と全て有効化した場合のコンパイル時間を比較したところ、ほぼ同じだった（0.278秒 vs 0.287秒）——**組み合わせ数(2^50)に応じた指数的コスト増加は一切なく、宣言数(50個)に対して線形**であることを確認した。issue #71のSourceUnitGraph設計原則（グラフのノード数はfeature組み合わせ数ではなく宣言数に線形）がNim側でも成立する。

ただし、defineの有無で意味解析対象自体が変わる点はRust側`cfg`属性と異なる——`when`で無効化された宣言は構文木を保持したまま選択されないのではなく、**マングル名の連番割り当てすら受けず、意味解析対象に最初から入らない**（有効化されたブランチの有無が後続宣言のマングル番号に影響することを実測で確認）。

### Nimbleの依存解決の実態（issue #77・#79）

issue #71がRust側`PackageResolution`について行った「依存グラフ全体で一貫した単一の解を出す」という性質確認を、Nim側でも実測で裏付けた。実在の公開パッケージ(`zero_functional`)を用い、矛盾する制約（`>= 1.0.0`と`< 1.0.0`を同時に課す）を試したところ、`Unsatisfiable dependencies`として明示的にフェイルファストで拒否された。矛盾しない制約（`>= 1.0.0`）は決定的に単一バージョン（`1.3.0`）へ解決された。**Nimbleは複数バージョンの共存を一切許さない**——Cargoが場合によって複数バージョン共存を許容しうるのとは異なる設計である。この性質により、`NimInstantiationKey`にバージョン情報を含める必要はないと結論づけた（依存クローズ全体で各パッケージは常に単一バージョンに確定するため）。

なお、当初試みた「単一のローカルパッケージに複数のgitタグ付きバージョンを持たせ、直接解決させる」という検証経路（issue #77）は、Nimbleの構造的制約（未登録パッケージ名の解決不可、`file://`スキームのタグ選択非対応）により実施できなかった——これは公式パッケージインデックス登録済みパッケージのみが名前ベース依存解決の対象になるという、Rust側Cargoの`path = "../local-crate"`とは異なる設計の発見でもある。

### マクロ/テンプレート展開の実態（issue #80）

issue #74が「N2とN4の間」と暫定配置したマクロ/テンプレート展開の挿入点を、実測により訂正した。`template`宣言と`when defined`分岐を組み合わせたfixtureで検証したところ：

- テンプレート宣言自体は、実際に使われるか（有効な`when`分岐内にあるか）に関わらず常に意味解析される（未使用でも`XDeclaredButNotUsed`診断が出る）。
- テンプレート呼び出しの展開は、**N5（意味解析+C生成）の内部で透過的に行われ、定数畳み込み等の最適化とも一体化している**——`double(21)`という呼び出しが生成Cコードでは`42`という定数に完全に畳み込まれ、展開前/展開後の中間状態を独立したエンティティとして保持した形跡はなかった。

これにより、既存`nim c`は「展開前構文木・展開後構文木を別エンティティとして持つ」方式ではなく「透過的な前処理」方式を採っていることが実測で確定した。LAMINARIA自身がマクロ/テンプレート展開を実装する場合、この既存の一体化方式をそのまま踏襲すべきかは独自に判断する必要がある（既存Nimの方式が正しさ・保守性の観点で最善とは限らない）。キャッシュ無効化条件は既存のN5の無効化軸（対象`SourceUnit`自体の内容変更）にそのまま従うことも確認した。

## Rust側/Nim側/foreign-native連鎖の統合（issue #74）

3連鎖を1本のフロー図として統合し、並行実行境界と合流点を確定した。

- **Rust側A1〜A5とNim側N1〜N4は完全並行**——言語ごとに独立した前段処理であり、共有状態を持たない。
- **意味解析段階（A6/N5相当）も完全並行**——[当初「FFI境界を持つSourceUnit同士でのみ同期」と訂正した設計は、さらに訂正して撤回した](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/74#issuecomment-5710568243)。型・ABI突合をA6/N5段階のブロッキング同期点として持ち込むこと自体が、A9で一度排除した「後付けの人為的収束点」の再導入だったため。
- **foreign-native連鎖（`ForeignDecl`→`ForeignLibraryTarget`→`ResolvedForeignArtifact`→[`AdapterUnit`]→`ForeignCompileUnit`→`ForeignNativeObject`）はA12でのみ合流する独立連鎖**。Rust/Nim側の意味解析対象（`SourceUnit`/`NimModuleUnit`）には一切ならない——これは境界契約そのものが要求する分離であり、設計の自由度ではない。
- **型・ABI突合はA11〜A12間のアドバイザリな下流突合**（ブロッキング同期ではない）。実行粒度は`LinkSymbol`単位（issue #75の`ReachabilityReport`実装が既にこの粒度で作られている事実の追認）。突合失敗は`Unmatched`（既存A12定義に合流）と`arity_mismatch`（新規カテゴリ、既存の`LoweringError`とは別レイヤーの`LinkGraphError`として設計する方針）を区別する。
- **収束点は全連鎖を通じてA12（リンク）ただ1箇所**——この原則は3連鎖統合後も維持される。

## C/C++ foreign-native連鎖の詳細（issue #71/#73）

[Nim C/C++ library integration](nim-c-cpp-library-integration_ja.md)が定める要件を実装するための具体的なエンティティ・粒度。

### エンティティ（6）

`ForeignDecl`（Rust/Nim側`SemanticFact`から参照生成、`SourceUnit`化はしない）、`ForeignLibraryTarget`、`ResolvedForeignArtifact`、`AdapterUnit`（C++のみ、条件付き）、`ForeignCompileUnit`、`ForeignNativeObject`。

### `AdapterUnit`の粒度（issue #73で確定）

**1未解決コンストラクト = 1AdapterUnit**（`ForeignDecl`:`AdapterUnit` = 1:0..1）。既存`nim c`/`nim cpp`は自動生成ロジックを持たず（実測で確認：開発者手書きの固定アダプタファイルを消費するのみ）、この粒度は実測データではなくA9訂正の原則（集約グルーピングをエンティティモデルに持ち込まない）からの演繹で確定した。外部コンパイラ起動コストの最適化が必要になった場合は、A9と同じ「キャッシュキー単位≠実行スケジューリング単位」の区別を適用し、実行層の最適化として扱う（エンティティ定義自体は変更しない）。

## importc/importcpp/importobjcの実機検証結果（issue #44）

既存`nim c`/`nim cpp`/`nim objc`バックエンドを実機（Nim 2.2.10、Docker）で検証した。

### importc（C FFI）

`header`プラグマで指定したヘッダがそのまま`#include`され、宣言はNimマングルなしで直接C関数呼び出しとして生成される。ただし**明示的なリンク入力管理を行わず**、システムライブラリの暗黙解決（gccのデフォルトリンク挙動）に依存している——`compiler-ownership-contract.md`が要求する「foreign inputs, headers, flags, toolchain, outputs and link edgesの可視化」を満たすには、LAMINARIA側で`ResolvedForeignArtifact`として明示管理する設計が必要である。

### importcpp（C++テンプレート/header-only API）

実際のテンプレートクラスで検証したところ、**独立したアダプタ/インスタンス化ユニット（別ファイル）は生成されない**——テンプレート実体化は呼び出し元モジュールに対応する生成`.cpp`ファイルに直接インライン埋め込みされ、C++コンパイラの通常のテンプレート機構にそのまま委ねられる。これは、issue #73で確定した「AdapterUnitを独立コンパイル単位として明示的に切り出す」という設計方針が、既存`nim cpp`の挙動の流用ではなく**LAMINARIA独自の新規設計判断**であることを実測で裏付けている。

### importobjc（Objective-C FFI）——既存Nimの実装不備、LAMINARIA側の対応課題ではない

Web調査でNimがObjective-Cを第3のFFI言語としてサポートすると確認したが、実機検証の結果、**`{.importobjc.}`が生成するコードは構文的に不正で、実際のObjective-Cコンパイラ（gobjc）でコンパイルできない**——`importcpp`のパターン置換構文（`#`/`@`）をそのまま転用しているが、Objective-Cのメッセージ送信構文`[receiver selector: arg]`はC関数宣言マクロの名前位置に埋め込むと構文的に破綻する。またObjective-Cオブジェクトの静的（値型）確保という言語仕様違反も生成コードに含まれる。バックエンド条件分岐機構自体（`when defined(objc)`）は健全に機能することは別途確認済みだが、これはFFIコード生成ロジックとは独立した話である。

**結論**: これは既存Nimコンパイラの実装不備であり、LAMINARIA側が解決すべき設計課題ではない。既存の[Nim C/C++ library integration](nim-c-cpp-library-integration_ja.md)（C/C++のみを対象）というスコープ判断は妥当なまま変更しない。将来Objective-C対応を検討する場合も、既存`nim objc`を参照実装として扱わない（動作しないため）という事実の記録に留める。

## 出典issue（すべてクローズ済み）

- issue #52: Cargo/rustc/LLVMの実測分解、暫定ERD作成
- issue #71: エンティティモデル確定（10エンティティ、多重度、A0〜A12、Nim側対応表、foreign-native連鎖）
- issue #72: マクロ/テンプレート展開のサポート範囲確定（Rust/Nimとも現状は全面拒否、Nim側に明示的検出を実装）
- issue #73: `AdapterUnit`粒度確定（1:0..1）
- issue #74: ビルド全体フロー図統合、型・ABI突合のアドバイザリ化、突合粒度・失敗カテゴリ確定
- issue #75: FFI境界照合の型・ABIレベル拡張（arity検出）、Rust↔Nim新規照合実装
- issue #76: Nimビルドパスの実測分解、N1〜N9確定、巻き戻し検査
- issue #44: importc/importcpp/importobjcの実機検証
- issue #77: Nimbleの依存解決の実測（単一解検証は#79で補完達成、パッケージ名解決の構造的制約を発見）
- issue #78: `when defined(...)`条件グラフの実測確定（組み合わせ爆発なし、宣言数に線形）
- issue #79: Nimbleパッケージ間バージョン制約競合の実測（フェイルファスト、複数バージョン共存なし）
- issue #80: マクロ/テンプレート展開の具体的挿入点確定（N5に一体化、透過的前処理方式）
- issue #81: Nim重複コンパイルコストの規模別実測（小規模では無視できるが規模依存、実規模では秒オーダーに達する）

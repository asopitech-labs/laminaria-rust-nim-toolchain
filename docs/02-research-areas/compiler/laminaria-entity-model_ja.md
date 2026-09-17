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

- **A0**: `BuildStrategyContext`判定（最尤ストラテジ/環境適応ストラテジ、issue #52で導入）。ビルド呼び出しコンテキストからA1より前に判定する独立した制御エンティティ。ターゲットパラメータの確定方法（既定値を即座に使うか検出するか）のみに影響し、A1〜A8/A10〜A12には影響しない。
- **A1〜A5**: マニフェスト構成（ワークスペース階層）→依存解決→feature条件グラフ構築→`SourceUnit`選択。
- **A6**: 意味解析（`SourceUnit`→`SemanticFact`）。
- **A7**: 単相化（`GenericDefinition`→`InstantiationKey`）。依存クローズ全体で一意なキーとして共有し重複を許さない——境界撤廃の核心。
- **A8**: 最適化判断（`InstantiationKey`→`OptimizationDecision`）。
- **A9**: コード生成準備（`InstantiationKey`+ターゲットパラメータ→`LoweredModule`）。**訂正済み**：当初「複数の`InstantiationKey`をグルーピングする」設計だったが、これは`Cgu`型の後付け収束点を別名で再導入するものだったため、**1:1の関数適用のみ**に訂正した。並列コンパイルのための実行単位の切り方はA10のスケジューリング上の都合として完全に分離する。
- **A10〜A12**: コード生成→シンボル登録→リンク。**収束点はA12（リンク）ただ1箇所**——情報が広がる箇所はA7（単相化）とA11（シンボル登録）のみ。

## Nim側エンティティ連鎖（issue #76）

issue #76が`nim c`/`nim cpp`を実測分解した（Nim 2.2.10、実fixture）。

### 確定エンティティ（12）

`NimblePackageManifest`、`NimBuildInvocation`（1回の`nim c`プロセス実行、Rust側に直接対応するエンティティがない新規概念）、`NimModuleUnit`、`ModuleImportEdge`、`StdlibModuleUnit`、`GeneratedCModule`、`GenericDeclaration`、`NimInstantiationKey`、`ExportedSymbol`、`CCompileCommand`、`CObjectFile`、`LinkCommand`/`NimExecutable`。

### アクティビティ・状態遷移（N1〜N9）

N1(Manifest構成)→N2(依存解決)→N3(`NimBuildInvocation`起動)→N4(モジュール到達可能性解決)→N5(モジュール単位の意味解析+C生成、1:1)→N6(単相化)→N7(Cコンパイル)→N8(リンク、収束点)→N9(`{.exportc.}`登録)。

### Rust側との構造的差異（実測で確定）

1. **意味解析とコード生成がN5に統合**——Rust側の`SemanticFact`(A6)と`LoweredModule`(A9)の分離がNim側にはない。
2. **`NimInstantiationKey`の多重度が2階層**：同一`NimBuildInvocation`内では1:1（重複排除）、`NimBuildInvocation`をまたぐと1:0..N（重複発生）。実測により、Rust側`Cgu`問題と同型の重複コンパイルがNim側にも実在することを確認した（独立コンパイル間で標準ライブラリ・単相化インスタンスが完全に重複生成される）。
3. **巻き戻し検査で発見した病理**：既存`nim c`が単相化実体を識別する実際のキーには、宣言識別子・型引数・const generic値に加えて「最初に構文的に要求したモジュール」という意味的に無関係な情報が混入している。同一プログラムでも`import`の記述順序を変えるだけでシンボル名が変わることを実証した——Rust側`Cgu`問題より恣意的な要因に依存する分、悪い性質を持つ。LAMINARIA自身の`NimInstantiationKey`設計は、Rust側と同じ3要素（宣言識別子・型引数・const generic値）のみで構成し、呼び出し元情報を意図的に排除する必要がある。
4. **`{.exportc.}`境界は呼び出し元非依存で安定**——非exportc内部シンボルの不安定性と異なり、FFI境界のシンボル名はマングルされず常に安定している。

### コスト規模の判断

Nim側の重複コンパイル問題は構造として実在するが、実測コスト（重複1箇所あたり数十〜数百ms）はissue #52がRust側で確認した規模（215秒中45秒、Phase-2コスト）より2〜3桁小さい。[project_laminaria_goal_and_bottleneck]（LAMINARIAの一次目標＝ビルド時間高速化、主戦場はRust側LLVM）に照らし、構造上正しく設計すべきだが、Rust側と同等の優先度で最適化対象とする根拠は実測上得られなかった。

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

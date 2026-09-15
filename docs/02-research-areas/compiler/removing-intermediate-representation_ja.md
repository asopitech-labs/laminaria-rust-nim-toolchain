# 意味論から「中間表現」を削除する — Rust IR → LLVM IR → バイナリの研究

## 目的

[全体設計ドキュメント](../../01-foundations/joint-symbol-schedule-layout-design_ja.md)の問題D(target固有ビルド直後テスト実行を第一級制約とし、汎用中間表現を経由しない)を受け、本書はその前提となる問い — **「中間表現(IR)という層は、コンパイラの意味論に本質的に必要な構造なのか、それとも削除可能な仲介層に過ぎないのか」** — を、Rust IR(HIR/MIR)→LLVM IR→機械語という実在するパイプライン全体に対して、Web一次資料と実装コードの両方から徹底的に検証する。

結論を先取りしない。削除できる根拠とできない(必須の)根拠の両方を、実装コードの具体的な箇所(ファイル・関数・行)を引用しながら並べ、最終的にLAMINARIAが何を削除でき、何を別の形で残さざるを得ないかを判定する。

## 1. HIR — 表現形式の選択であり、必須の計算モデルではない

rustc-dev-guideとRFC文書、および実装コードを確認した結果、HIRの存在理由は次の2つに整理できる。

1. **脱糖(desugaring)**: `for`ループのような糖衣構文を、より単純な形へ変換する。これは意味を変えない構文変換であり、「脱糖後の意味を直接構築する」実装(パース時に直接、脱糖済みの内部表現を組み立てる)に置き換え可能である。HIRという独立した中間段階を経由する**必然性はない**。
2. **依存追跡単位**: HIRの`Crate`構造はitemをIDで間接参照し、アクセスを観測してインクリメンタルコンパイルの依存関係を記録する。これも中間表現でなくとも、依存追跡システムを直接ソース側の構造に実装すれば代替可能な設計選択である。

**判定**: HIRは削除可能な仲介層である。「HIRという名前の層」を経由せずとも、脱糖と依存追跡を別の場所(パーサー自体、あるいは意味解析の直接的な副産物)へ移すことができる。

参照: rustc-dev-guide HIR章(<https://rustc-dev-guide.rust-lang.org/hir.html>)、RFC 1211。

## 2. MIR — 「層」は消せても、CFG上の不動点計算という計算モデルは残る

MIRの存在理由はRFC 1211(2015)に列挙されており、実装コードでの裏取りにより、その中に**削除できない構造的理由**が含まれていることが確認できた。

### 2.1 RFC 1211理由6: 意味論の所在を固定する境界としての機能

RFC 1211時点で「LLVMからの移行がほぼ不可能だった」原因は、当時Rustの意味論そのものがLLVM IR生成(旧`trans`)段階に直接埋め込まれていたためである。MIR導入によって、意味論をAST→MIR変換側に確定させ、LLVM側を「最適化のみを行う交換可能なバックエンド」として切り離せるようになった。

これは中間表現を単なる仲介層としてではなく、**「意味論をどこで確定させるか」という境界**として機能させている証拠である。LAMINARIAが「target固有の専用コンパイラを束ねる」設計を取る場合も、この同じ問題 — 複数の専用バックエンドに意味論の実装が分散・重複しないよう、意味論をどこか1箇所で確定させる必要がある — に直面する。MIRという「層」自体は削除できても、「意味論を1箇所に固定する」という設計要求は残る。

### 2.2 決定的な実装根拠: drop elaborationはCFG上のデータフロー解析でしか成立しない

`compiler/rustc_mir_transform/src/elaborate_drops.rs`を確認した結果、Rustのデストラクタ挿入は、コンパイル時に静的に「このドロップが実際に実行されるか」を決定できない場合がある(条件分岐後の部分初期化状態など)。これを`MaybeInitializedPlaces`/`MaybeUninitializedPlaces`という**制御フローグラフ上のデータフロー解析(不動点計算)**で解決し、dropフラグを算出している。

これは分岐後の変数初期化状態を合流点でマージする計算であり、**木構造のAST上では原理的に表現できない**。同様に`rustc_borrowck`(NLL)は`Body`(MIR)を直接入力として、CFG上のregion制約解析を行う(`compiler/rustc_borrowck/src/lib.rs`)。

**判定**: 「MIRという名前の層」を消去しても、drop elaborationとborrow checkingという**CFG上の不動点計算が要求する計算モデル自体は消えない**。これは別の形(直接ソースASTに付随するグラフ構造、あるいは制御フローグラフを直接持つ別のデータ構造)で存在し続けることになる。したがって「中間表現を削除する」という主張は、「MIRという名前・データ型を消す」ことと「CFG+データフロー解析という計算モデルを消す」ことを区別しなければ、実装不可能な主張になってしまう。

参照: RFC 1211(<https://github.com/rust-lang/rfcs/blob/master/text/1211-mir.md>)、MIR発表ブログ(Niko Matsakis, 2016-04-19、<https://blog.rust-lang.org/2016/04/19/MIR/>)、`compiler/rustc_mir_transform/src/elaborate_drops.rs`、`compiler/rustc_borrowck/src/lib.rs`(39, 135, 476, 516行)、`compiler/rustc_mir_transform/src/pass_manager.rs`(`MirPhase::Built/Analysis/Runtime`という段階的不変条件、230-398行)。

## 3. LLVM IR — 情報の再エンコードが実際のバグ源になっている、削除を強く支持する実害証拠

### 3.1 LLVM IRの設計動機

Lattner & Adve, "LLVM: A Compilation Framework for Lifelong Program Analysis & Transformation" (2004) は、コンパイル時・リンク時・実行時・アイドル時という複数段階にわたる「生涯にわたる最適化」を単一の低水準SSA表現で支援することを目的に掲げている。複数フロントエンド言語(C, C++, Fortran等)を1つの基盤コード上で扱う再利用性を狙った設計である。

### 3.2 rustc→LLVM IR変換で実際に失われる情報: `noalias`という再エンコード

`compiler/rustc_ty_utils/src/abi.rs`の`arg_attrs_for_rust_scalar`関数(322-411行)が決定的な証拠を持つ。`&T`(凍結)、`&mut T`(Unpin)、`Box<T>`から`ArgAttribute::NoAlias`を明示的に導出しているコードが存在し、コメントには「LLVMのnoalias定義はメモリ依存性のみに基づく」と明記されている。

これは、Rustの型システムが持つ一意性保証(borrow checkerによって静的に保証される、より豊かな意味論)を、一度LLVM IRという層に落とす過程で失い、`noalias`という限定的な属性語彙に**再エンコード**していることの直接証拠である。

### 3.3 この再エンコードが構造的脆弱性を生んだ実例

- issue #31681「Mark &mut pointers as noalias once LLVM no longer miscompiles them」
- issue #84958「Regression: Miscompilation due to bug in "mutable noalias" logic」
- issue #54878「Enable noalias annotations」

`&mut`へのnoalias付与は、LLVM側のインライン化とループ展開の組み合わせで`alias.scope`/`noalias`メタデータのIDが衝突し、誤ったエイリアス無し判定による**ミスコンパイル**を繰り返し引き起こしてきた(2015年提起、2018年・2021年に再発)。何年にもわたりデフォルト無効化と再有効化を繰り返している。

**判定**: これは「一度失った意味論を属性として復元する」という中間層特有の情報損失が、単なる非効率ではなく**実際のミスコンパイル(正しさの破壊)を繰り返し引き起こしている**ことを示す一次証拠である。中間表現を削除する最も強い根拠は、性能でも設計美学でもなく、**この種の再エンコードが正しさそのものを脅かしている**という事実にある。

### 3.4 target依存の意味論はそもそも単一表現に収まらない

`compiler/rustc_codegen_ssa/src/mir/block.rs`には`landing_pad_for`/`cleanup_ret`/funcletという概念が実装されており、MIRの`Unwind`ターミネータがLLVMの例外処理表現(Itanium `landingpad` vs. Windows SEH funclet)へ変換される。この変換はtarget(OS/ABI)によって異なる分岐が必要であり、単一のtarget非依存IRとして表現しきれていない箇所である。

これは[全体設計ドキュメントの問題D](../../01-foundations/joint-symbol-schedule-layout-design_ja.md#問題d-ビルド直後のtarget実行--汎用ツールという前提そのものを手放す)の主張(target固有の専用パスを持つべき)と直接整合する — panic/unwind lowering は最初からtarget固有に分岐するしかない意味論であり、汎用IRという層を経由してもtarget独立には表現できていない。

### 3.5 rustc自身が既にバックエンド非依存層とLLVM固有層を分離している

`rustc_codegen_ssa`は`BuilderMethods` traitを介して`rustc_codegen_llvm`や他バックエンド(Cranelift等)へ委譲する設計であり、MIR→LLVM IR変換の各関数(`codegen_rvalue`, `codegen_statement`等、`compiler/rustc_codegen_ssa/src/mir/{rvalue,statement,block}.rs`)自体はLLVM専用ではなくジェネリックに書かれている。

これは、rustc側が既に「意味論を確定させる層(MIR、および`BuilderMethods`が要求するcodegen手順)」と「特定のバックエンド実装(LLVM)」を分離しているという事実であり、target固有バックエンドへの直接lowering経路をこの既存の抽象の上に追加することは、既存構造と矛盾しない。

参照: LLVM論文(<https://llvm.org/pubs/2004-01-30-CGO-LLVM.html>)、`compiler/rustc_ty_utils/src/abi.rs`(322-411行)、`compiler/rustc_codegen_llvm/src/attributes.rs`(618, 634行)、issue #31681・#84958・#54878、`compiler/rustc_codegen_ssa/src/mir/block.rs`、`compiler/rustc_codegen_ssa/src/mir/{rvalue,statement}.rs`。

## 4. Craneliftとの比較: 「単一IR」でも「IRという層」自体は消えていない

Cranelift(Bytecode Allianceによる代替バックエンド)の公式比較文書(`cranelift/docs/compare-llvm.md`)を確認した結果:

- LLVMがLLVM IR→SelectionDAG→MachineInstr→MCという**4段階の異なる表現**を使うのに対し、Craneliftは**単一のIR(CLIF)**でこれら全ての抽象度をカバーする設計を取る。
- ISA固有の符号化は「正規化・命令選択の後」に各命令へ注釈として付加され、レジスタ割り当て後にISAレジスタ/スタック位置が値に注釈される、という形で段階的にtarget情報を後付けする点はLLVMと類似する。
- 規模はLLVM(2000万行超)の約1/100(20万行)。

**判定**: Craneliftは中間表現の**数を1つに削減**しているが、「1つの共通表現を経由する」という構造自体は保持している。これはユーザーが主張する「中間表現という概念そのものの削除」とは異なる位置にある — CLIFは「複数の中間表現層」の統合ではあるが、依然として「意味解析結果とtarget固有コード生成の間に立つ共通の交換フォーマット」である点は変わらない。

なお、Craneliftを実際にalopex-cliのビルドに使う実測(issue #48)では、`-Zcodegen-backend`統合自体が実運用規模の依存グラフに対してまだ不安定であることが確認されている(`llvm-internals-observed_ja.md`、Cranelift実測失敗の記録参照)。これは中間表現の設計思想とは別の、統合の未成熟さの問題である。

## 5. 先行事例: 「完全にゼロの中間表現」は実在するか

Web一次資料と実装コード(GitHub)の両方で調査した結果、**完全にゼロの中間表現を持つコンパイラは実在しない**。すべての実例は次のいずれかに分類される。

### 5.1 中間表現を外部化しない(TinyCC)

TinyCC(tcc)の実装を確認した結果、`tccgen.c`(パーサー+意味解析)は`SValue *vtop`という「値スタック」をグローバル状態として持ち、式を構文解析しながらそのスタックを直接操作してコード生成器を呼び出す。**ASTノードという中間構造そのものは存在しない**。GCC比で約9倍速いとされる(Bellard自身の主張)。

ただし各`<arch>-gen.c`(x86_64/arm/arm64/riscv64/c67向け)は、レジスタ割付・命令選択という「極小IR相当の内部状態」(`SValue`型のレジスタ/スタック配置情報)を持つため、「完全にゼロの中間表現」ではなく**「中間表現を独立したデータ構造として外部化しない」設計**というべきである。

### 5.2 中間表現を縮小する(QBE)

QBEは"70% of the performance of industrial optimizing compilers in 10% of the code"を掲げるミニマルバックエンドで、SSAベースの中間言語(IL)を持つ。LLVMより遥かに単純だが、**これもIRであることは公式ドキュメント自身が明言している**。「中間表現の削除」ではなく「中間表現の縮小」を狙った設計であり、LAMINARIAの方向性とは異なる。

### 5.3 単一パスコンパイラ(Wirth系/Oberon)の構造的トレードオフ

識別子の事前宣言必須という言語制約と引き換えに、シンボルテーブル参照だけでコード生成を完了できる設計。ASTを構築せず、固定コードパターンに基づき直接コード生成する。トレードオフとして、相互再帰の禁止、最適化能力の制限(AST全体を見渡す最適化ができない)、デバッガ支援やコードスライシングのような後年の機能追加の困難、を負う。

### 5.4 複数IRを保持しつつtarget固有直接コード生成を持つ(Zig) — 最も近い実例

Zigのソース(`AstGen.zig`、`Sema.zig`/`Sema/`、`Air.zig`、`codegen.zig`/`codegen/`)をGitHub上で直接確認した結果、`AstGen`(AST→Zir)、`Sema`(Zir→Air、型チェック含む)、`codegen`(Air→target固有機械語)という段階的パイプラインを持つ。**LLVMを経由せずx86_64ネイティブ機械語を生成する自前バックエンドがDebugビルドで既定になっている。**

設計動機として、公式ブログは「コンパイル時間はLLVMが支配的」であり、独自バックエンドだけが速度改善の道である旨を明示的に述べており、LAMINARIAの問題意識(節3.3のnoaliasバグ、LLVM処理速度の実測)と一致する。

コード生成が意味解析から分離された独立スレッドで並列実行できる点(1意味解析スレッド+複数コード生成スレッド+1リンクスレッド)は、LAMINARIAの並列化設計(`SharedSymbolGraph`の各realmフロントエンドスレッド構成)と直接比較可能である。

ただしZirとAirという**2段階の中間表現自体は保持しており**、「中間表現の削除」ではなく「target固有バックエンドの複数化(LLVM/自前)」という設計である点に注意する。

### 5.5 Tiered JIT(V8) — 逆方向の設計

V8はAST→Ignitionバイトコード(汎用中間表現)→Sparkplug/Maglev/TurboFanという多段最適化を持ち、各段は**中間表現を保持し続けることが前提**(実行時プロファイルに基づく再最適化のため)。LAMINARIAが目指す方向とは逆で、むしろ中間表現を積極的に維持・活用する設計である。参考にはなるが、直接のモデルにはならない。

## 6. 総合判定

3つの調査を統合すると、「中間表現を削除する」という主張は、対象によって成否が異なる。

| 対象 | 削除可能か | 根拠 |
| --- | --- | --- |
| HIR(脱糖・依存追跡) | **削除可能** | 表現形式の選択に過ぎず、意味を変えない構文変換と依存追跡は別の場所に実装できる |
| MIRの「層」自体(データ型・名前) | **削除可能** | 「MIR」という独立した中間データ型を経由する必然性はない |
| MIRが担うCFG上の不動点計算(drop elaboration、borrow checking) | **削除不可能** | AST(木構造)では原理的に表現できない計算モデルそのものであり、どこかに何らかの形で残る |
| LLVM IRという「単一汎用表現を経由する」設計 | **削除すべき(強い実害根拠あり)** | `noalias`のような再エンコードが、正しさを脅かす実際のミスコンパイル(issue #31681/#84958/#54878)を繰り返し引き起こしている |
| target依存の意味論をtarget非依存表現で扱おうとする試み(panic/unwind等) | **そもそも成立しない** | Itanium/SEHのような差異は、単一表現を経由しても回避できない |
| 「意味論を1箇所に確定させる」という設計要求 | **削除不可能** | 複数の専用バックエンドを束ねる場合でも、意味論の重複実装を避けるためにはどこかで一度確定させる必要がある(RFC 1211理由6と同じ問題) |

したがって、LAMINARIAが実際に削除すべきは「**LLVM IRのような、target非依存を標榜しながらtarget依存の意味論(panic/unwind等)を扱いきれず、かつRustの型システムが持つ豊かな意味論を限定的な属性語彙に再エンコードすることで正しさを脅かす、単一の汎用中間表現という層**」である。

削除しても残らざるを得ないもの:

1. **CFG上のデータフロー解析という計算モデル**(drop elaboration、borrow checking相当の処理) — 名前を「IR」と呼ばない形で実装しても、この計算構造自体は必要。
2. **意味論を確定させる1箇所の境界** — target固有の専用バックエンドを複数束ねる場合、各バックエンドに意味論を重複実装させないための「意味論の所在」をどこかに置く必要がある。これは新しい「中間表現」ではなく、[全体設計ドキュメント](../../01-foundations/joint-symbol-schedule-layout-design_ja.md)で既に設計している`SharedSymbolGraph`(境界シンボルのみを共有し、各realmの内部意味論は外部へ露出しない)と同じ形の境界として実装できる可能性がある — これは次の設計課題である。

最も近い先行実装パターンはTinyCC(意味解析結果を外部化されたグラフ構造として保持しない)とZig(target固有の直接コード生成をLLVM非依存で持つ)のハイブリッドであり、これは`unified-symbol-graph`の既存設計(`CodeBody`が「意味解析結果」ではなく「機械語バイト列+再配置」を直接保持する)と既に整合している。

## 7. まだ解けていないこと

- MIRが担う不動点計算(drop elaboration相当)を、独立した「IR」という名前の層を持たずに、どのようなデータ構造で実装するかの具体設計はまだ無い。
- 「意味論を確定させる1箇所の境界」を`SharedSymbolGraph`とどう統合するか(あるいは別の構造にするか)は未設計。
- panic/unwind lowering のようなtarget固有の分岐処理を、node 4で示した「target固有専用コンパイラ」それぞれにどう実装させるか(重複実装を許容するか、共通実装をどこかに置くか)は未設計。
- Zigの並列化モデル(意味解析1スレッド+コード生成複数スレッド+リンク1スレッド)と、LAMINARIAの`SharedSymbolGraph`が想定する並列モデル(各realmフロントエンドが独立して書き込む)の異同を、具体的な比較実験としてまだ検証していない。

## 関連文書

- [全体設計 — なぜ「一つの計算システム」でなければならないか](../../01-foundations/joint-symbol-schedule-layout-design_ja.md) — 問題D(汎用中間表現を手放す理由)
- [LLVM再発見研究](llvm-rediscovery-research_ja.md) — LLVM概念の再導出方法論
- [LLVM内部の実測記録](llvm-internals-observed_ja.md) — MC層・CGU粒度・並列化・ThinLTO・Cranelift実測失敗の記録
- issue #48 — CGU粒度、LLVM処理速度、Cranelift実測

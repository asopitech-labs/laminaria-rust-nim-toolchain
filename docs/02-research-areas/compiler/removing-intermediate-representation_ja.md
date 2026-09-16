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

## 2.5 学術的形式化・独立実装が同じ結論を裏付ける

節2の判定(CFG上の不動点計算という計算モデル自体は削除不可能)を、rustc本体とは独立にRustの意味論を形式化・実装した複数の学術研究・独立実装から裏付ける。

### RustBelt — 所有権を分離論理で検証するが、CFGの外側の話である

RustBelt(Jung, Jourdan, Krebbers, Dreyer, POPL 2018)は、Rustの現実的な部分集合を表す核言語`λRust`に対し、Coqによる初の機械検証済み安全性証明を与えた。所有権ベースの型システムが、`unsafe`を内部的に使うライブラリ(`Arc`, `Rc`, `Cell`, `RefCell`, `Mutex`, `RwLock`, `mem::swap`, `thread::spawn`)の安全な拡張として成立する条件を、Iris(並行分離論理の汎用フレームワーク)を使って証明した。

RustBeltは「型システムの健全性」という、borrow checkingとは異なる層の性質(型が守られていれば安全性が保たれる、という定理)を扱っており、節2で確認したCFG上の不動点計算(drop elaboration、NLL)そのものを置き換える設計ではない。むしろ、CFG上の解析が正しく機能した後に、その結果として得られる型付けが本当に安全性を保証するかという、**一段上の階層の検証**を提供する。中間表現の削除可否には直接関与しないが、「所有権情報を型として保持し続けることの正当性」を独立に裏付ける材料になる。

参照: <https://plv.mpi-sws.org/rustbelt/popl18/>、論文PDF <https://people.mpi-sws.org/~dreyer/papers/rustbelt/paper.pdf>。

### Oxide — borrow checkingを型システムの導出規則として定式化できることの証拠

Oxide(Weiss, Gierczak, Patterson, Matsakis, Ahmed, arXiv:1903.00982)は、rustcとは独立に、ソースレベルRustに近い(型注釈を完全に持つ)言語`Oxide`を定義し、借用チェックを**substructural typing judgment(構造制限付き型付け導出)**として定式化した。lifetime(ライフタイム)を「参照の出自(provenance)の近似」として捉え直し、この情報を型システムが自動的に計算できることを示した。

これは、borrow checkingが「MIRという特定のデータ構造」を必要とするのではなく、**型付け規則(導出木)として定式化できる**ことの証拠である。ただし、Oxideの型付け導出は静的なプログラムテキスト全体に対する規則であり、節2で確認したCFGの合流点でのデータフロー解析(実行時の分岐に応じた状態のマージ)とは異なる定式化である。Oxideは「借用の正しさ」を型付け規則として表現する道を示したが、drop elaborationのような**実行時パス依存の状態計算**をどう型付け規則へ落とすかは、Oxide論文の主眼ではない。

参照: <https://arxiv.org/abs/1903.00982>。

### Polonius — borrow checkingをグラフ到達可能性問題として再定式化、`unified-symbol-graph`と同型の設計

決定的な発見。rustc自身が新しいborrow checker(NLLの後継)として開発しているPoloniusの実装コードを直接確認した結果、borrow checkingは**グラフ上の到達可能性(reachability)問題**として実装されている。

`compiler/rustc_borrowck/src/polonius/constraints.rs`の`LocalizedConstraintGraph`は、`(region, point)`のペアを1つの頂点(`LocalizedNode`)とし、2種類のエッジを持つ:

- **物理エッジ(`edges`)**: ある時点での型検査制約(代入・呼び出し等)から生じる、`a@p: b@p`という同時点間のoutlives制約。
- **論理エッジ(`logical_edges`)**: CFG上の隣接する点を辿る、`a@p: a@q`という制約(regionのliveness・varianceに依存)。ただしCFGが大きい場合に全点へ物理エッジを張ると爆発するため、遅延的(on-demand)に局所化する設計を取っている。

このコメント自体が「loanの伝播をグラフ上の到達可能性としてモデル化する(`model the flow-sensitive loan propagation via reachability within a graph of localized constraints`)」と明記している。

**この設計は、LAMINARIAの`unified-symbol-graph`(`SharedSymbolGraph`)と構造的に同型である**: どちらも「全状態を1つのモノリシックな構造に事前展開する」のではなく、「頂点(Poloniusでは`(region, point)`、`unified-symbol-graph`では境界シンボル)」と「エッジ(制約・要求関係)」だけを保持し、解決(到達可能性・要求充足)を**遅延的なグラフ走査**として行う。しかも、CFGの全点に物理エッジを張らず論理エッジで遅延展開するというPoloniusの最適化は、`unified-symbol-graph`が「境界を越えるシンボルだけをノードにする」(領域内部の状態を露出しない)という設計判断と同じ動機(共有・展開する状態を最小化する)を持つ。

これは、節2で示した「MIRという名前の層は消せても、CFG上の不動点計算という計算モデルは残る」という判定に対する、rustc自身による実例での裏付けである。ただしPoloniusが示しているのはさらに強いことで、**その計算モデルは「木構造」や「1本のCFG」に固定される必要すらなく、グラフ到達可能性問題として抽象化できる**ということである。LAMINARIAが「MIR相当の処理」を独自に実装する場合、Poloniusのグラフ定式化は、`SharedSymbolGraph`と同じ抽象(頂点+エッジ+到達可能性)の上でborrow checking相当の処理を統合できる可能性を示唆する — 別々の中間表現(MIR的な木/CFG構造とシンボルグラフ)を持つ必要すらなく、両方を同じグラフ構造の異なる頂点・エッジ種別として表現できるかもしれない、という新しい設計仮説がここから導出できる。

参照: <https://rust-lang.github.io/polonius/current_status.html>、<https://github.com/rust-lang/polonius>、rustc実装 `compiler/rustc_borrowck/src/polonius/constraints.rs`(1-30行目のdocコメント、`LocalizedNode`/`LocalizedConstraintGraph`型定義)。

### rust-analyzer + Salsa — インクリメンタル性を、意味解析そのものと同じ抽象(依存グラフ)で扱う先例

rust-analyzerは、rustcのソースを一切再利用せず、IDE用途(エラー耐性・低遅延)に特化して独自にRustのパース・名前解決・型推論を実装している。その基盤である**Salsa**は、関数呼び出しの依存関係を記録した呼び出しグラフ(四角ノード=ユーザー入力、丸ノード=導出値)として計算全体をモデル化し、入力が変化した際に依存グラフを辿って再計算範囲を最小化する汎用インクリメンタル計算エンジンである。

Salsaが示しているのは、「意味解析の各段階(パース、名前解決、型推論)」と「インクリメンタルな依存追跡」を、**別々の機構ではなく同じグラフ抽象の上に統合できる**ということである。これは節1(HIRの依存追跡単位としての役割)で「HIRという層でなくとも依存追跡は実装できる」と判定した根拠を補強する具体例であり、LAMINARIAの`SharedSymbolGraph`が`mutation_seq`(いつ書き込まれたか)を持つ設計とも、依存関係を明示的なグラフ構造として保持するという方向性で一致する。

参照: <https://rust-analyzer.github.io/book/contributing/architecture.html>、<https://salsa-rs.github.io/salsa/overview.html>、<https://rust-analyzer.github.io/blog/2023/07/24/durable-incrementality.html>。

### Stacked Borrows / Tree Borrows — `noalias`問題への、より正確な形式的代替案

節3.3で確認した`noalias`の反復的ミスコンパイル(issue #31681/#84958/#54878)に対し、Miri(Rustの実験的インタプリタ)が採用する**Stacked Borrows**、およびその後継**Tree Borrows**は、Rustのポインタエイリアシング規則を形式的な操作的意味論として定義する試みである。

- **Stacked Borrows**(Ralf Jung他)は、各メモリ位置に対する参照・ポインタへタグを付与し、アクセス権限とエイリアシング要求をスタック上で管理する。新しい参照の作成はタグのプッシュ、使用時はタグの存在・権限を検査し、時にタグをポップする。
- **Tree Borrows**(2023年〜)は、Stacked Borrowsの線形スタック構造を木構造へ置き換えた後継モデルである。Web一次資料(Ralf Jung氏自身のブログ)で確認した具体的な改善点:
  1. **二段階借用(two-phase borrows)の正式サポート** — メソッド呼び出しで生じる「予約フェーズ(他ポインタからの読み取りを許容)→初回書き込みでアクティブ化」というパターンを、Stacked Borrowsは「rawポインタのように扱う」ことで回避していたが、Tree Borrowsは正式にモデル化する。
  2. **可変参照のユニーク性を遅延評価** — Stacked Borrowsは`&mut`のユニーク性を過度に厳密に強制していたが、Tree Borrowsは実際に書き込まれるまでユニーク性を要求しない。これにより、以前は未定義動作とされていた正当なコードパターンが合法化される。
  3. **Protectorsは`noalias`正当化に不可欠な要素として残る** — Tree Borrowsでも、LLVMの`noalias`最適化を正当化するには「protector」という追加の保護機構が必要であり、これ自体が消えたわけではない。

**判定**: Stacked/Tree Borrowsは、`noalias`という「LLVM側の限定的な属性語彙への再エンコード」を、より正確な形式的意味論(操作的意味論としてのタグ・木構造)に置き換える方向の研究である。これは節3.3で確認したミスコンパイル問題(Rustの所有権保証がLLVMのnoalias属性へ落ちる際に情報が失われ、誤ったエイリアス無し判定を招く)に対する、**属性への再エンコードそのものをやめて、より豊かな形式的モデルを直接保持する**という代替案を提示している。

ただし重要な限定: Stacked/Tree Borrowsは依然として「Rustの意味論を、LLVM(あるいは何らかのバックエンド)へ伝えるための中間的なモデル」であることに変わりはなく、これも一種の「中間表現」である。したがって、この研究群は「中間表現を完全に無くす」根拠にはならないが、「LLVM IRの`noalias`という**特に貧弱な**再エンコード先を、より表現力の高いモデルに置き換えるべきだ」という節3.3の主張を独立に補強する。LAMINARIAがtarget固有の専用コンパイラでRustの所有権情報を機械語へ落とす場合、Stacked/Tree Borrowsの操作的意味論(タグ+木構造)を直接の設計材料として使える可能性がある — LLVM IRを経由せず、所有権情報を最初からこの形式で保持し、target固有コード生成の段階まで運ぶという設計である。

参照: Ralf Jung氏ブログ(<https://www.ralfj.de/blog/2023/06/02/tree-borrows.html>)、Tree Borrows論文(PLDI 2025、<https://iris-project.org/pdfs/2025-pldi-treeborrows.pdf>)、Stacked Borrows論文(<https://dl.acm.org/doi/pdf/10.1145/3371109>)、Miri実装(<https://deepwiki.com/rust-lang/miri/3.3-tree-borrows-model>)。

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

## 5. Rustそのものを対象とした代替コンパイラ実装

「Rust言語自体を、rustc/LLVM以外の経路でコンパイルする」ことを実際に試みたプロジェクトを、Web一次資料と実装コード(GitHub)の両方で調査した。単なる代替バックエンド(節4のCraneliftのように、rustcのMIRまでは共有する)ではなく、フロントエンド(意味解析)自体を別実装している事例と、コード生成自体を行わない事例の両方を含める。

### 5.1 mrustc — 意味解析を単純化し、コード生成を外部委譲する設計

mrustc(thepowersgang/mrustc、C++実装、スター数2525)は、rustc自身をソースからブートストラップする目的で作られた代替Rustコンパイラである。README(`github.com/thepowersgang/mrustc`)に設計思想が明記されている。

> Code generation is done by emitting a high-level assembly (currently very ugly C, but LLVM/cretone/GIMPLE/... could work) and getting an external tool (i.e. `gcc`) to do the heavy-lifting of optimising and machine code generation.

**mrustc自身は最適化・機械語生成を一切行わない**。生成するのは「醜いC」であり、実際の最適化・コード生成はGCCへ委譲する。これはNimのCバックエンドと同型の構造(節「Every real toolchain invocation」で`unified-symbol-graph`が既に確認した`nim c`の挙動と同じパターン)であり、GNU/GIMPLE/RTLという別の汎用中間表現へ委譲しているに過ぎず、「中間表現を削除した」わけではない。

さらに決定的な設計判断として、mrustcは**borrow checkingを行わない**(README中「terrible error messages」という自己申告、および設計文書`docs/`が示す通り、コンパイルするコードが既にvalidであることを前提とする)。これはLAMINARIAが検討すべき重要な反例である。「意味論を確定させる層」からborrow checkingという計算コストの高い解析を外すことで、mrustcは(1)実装の複雑さを大幅に減らし、(2)rustc自身をブートストラップできる程度の実用性を達成した。ただし、これは「意味論の正しさをmrustc自身が保証しない」という代償の上に成り立っており、mrustcが処理するコードは既にrustcで一度検証済みである(ブートストラップ専用という用途に閉じている)ことに注意が必要である。

2025年時点でrustc 1.90.0までのバイナリ完全一致ブートストラップに成功しており、x86-64 Linux GNU、x86-64 Windows MSVC、x86-64/ARM64 macOSをターゲットとして掲げている。

### 5.2 gccrs (GCC Rust) — フロントエンドを独自実装し、GIMPLE/RTLという別の汎用中間表現へ接続する設計

gccrs(rust-gcc/gccrs、スター数2945)は、GCC本体へRustフロントエンドを追加するプロジェクトである。GCCソースツリー内の`gcc/rust/`ディレクトリを実際に確認した結果、`ast`、`hir`、`resolve`、`checks`、`backend`という、rustc自身のパイプラインと類似した段階的構造を持つ独自実装であることが確認できた。`backend/`配下には`rust-compile-drop.cc`、`rust-mangle-v0.cc`等、rustc_codegen_ssaと概念的に対応するファイル群がある。

決定的な点: gccrsは**LLVM非依存だが、汎用中間表現そのものは手放していない**。GCC本体が持つGIMPLE(木構造SSA的表現)→RTL(register transfer language)という、LLVM IRとは別の、しかし同じ位置付けの汎用中間表現へ接続する設計である。「中間表現からの脱却」ではなく、「どの汎用中間表現を使うか」という選択の違いに過ぎない。

成熟度についてはプロジェクト自身のREADME(`github.com/rust-gcc/gccrs`)が明記している。

> Please note, the compiler is in a very early stage and not usable yet for compiling real Rust programs.

実運用に耐える段階には至っていない。

### 5.3 rustc_codegen_gcc — rustcのMIRを保ちつつバックエンドだけをGCCへ差し替える設計

rustc_codegen_gcc(rust-lang/rustc_codegen_gcc、スター数1166)は、gccrsとは全く別のプロジェクトである。rustc本体のフロントエンド・MIRをそのまま使い、バックエンドだけをLLVMからGCCのlibgccjitへ差し替える(Cranelift・LLVMと並ぶ第3の`-Zcodegen-backend`)。

プロジェクト自身のモチベーション記述(`Readme.md`)が、LAMINARIAの問題Dと直接一致する。

> The primary goal of this project is to be able to compile Rust code on platforms unsupported by LLVM.

つまりこのプロジェクトの存在自体が、「LLVMという単一の汎用バックエンドでは全targetをカバーできない」という事実の証拠であり、target固有の専用パスを複数持つ必要性(全体設計ドキュメントの問題D)を裏付けている。ただし、これはMIRという中間表現自体は変えず、その先のバックエンドだけを複数化する設計であり、節4のCraneliftと同じ位置付け(中間表現の削除ではなく、中間表現から先の複線化)にある。

### 5.4 Miri — コード生成を一切行わず、MIRを直接解釈実行する

Miri(rust-lang/miri)はコード生成を全く行わない点で、他の全事例と根本的に異なる。README(`github.com/rust-lang/miri`)が示す通り、Miriは未定義動作検出ツールであり、MIRを機械語へ変換せず、その意味論を**MIRインタプリタとして直接解釈実行する**。Stacked Borrows/Tree Borrowsという、借用の意味論そのものを実行時にチェックする機構を持つ。

Miriが示す事実: **機械語生成を経由しなくても、Rustの動的意味論(借用規則、メモリ安全性)は検証できる。** これはLAMINARIAの中間表現研究にとって、「コード生成」と「意味論の検証・実行」が分離可能な問題であることの直接証拠になる。ただしMiriは実行速度が遅く(インタプリタである以上当然)、本番ビルド用途には使えない。それでも、「機械語を生成する前に、MIR相当の構造の上で意味論を検証する」という段階自体は、target固有の専用コード生成へ直結する設計であっても必要になりうることを示唆する(節2で確認した「意味論を確定させる境界」という要求と一致する)。

### 5.5 総合: Rust代替実装のいずれも「中間表現の完全な削除」には至っていない

| プロジェクト | LLVM依存 | 独自中間表現 | borrow checking | 成熟度 |
| --- | --- | --- | --- | --- |
| mrustc | 非依存(Cを生成しgccへ委譲) | 独自コード生成なし、Cという別言語へ委譲 | **行わない** | rustc 1.90.0まで完全ブートストラップ済み |
| gccrs | 非依存(GCC本体) | GIMPLE/RTL(GCCの汎用中間表現) | 実装中 | 実運用不可、非常に早期段階 |
| rustc_codegen_gcc | 非依存(GCC/libgccjit) | rustcのMIRをそのまま使用 | rustc本体に依存 | 開発中、複数targetでのビルド実績あり |
| Miri | 該当なし(コード生成せず) | MIRを直接解釈実行 | rustc本体に依存 | 成熟(nightly配布、CIで広く使用) |

いずれのプロジェクトも、「中間表現という概念そのものを完全に削除する」ことには至っていない。mrustcが最も近いが、それは「意味論の正しさの検証(borrow checking)を放棄する」という重い代償と引き換えであり、「Cという別の(GCCにとっての)中間言語へ委譲する」ことでコード生成部分の中間表現を単に外部化しているに過ぎない。gccrs/rustc_codegen_gccはいずれもLLVMには依存しないが、GIMPLE/RTLという「別の」汎用中間表現、あるいはMIRという既存の中間表現を経由しており、「中間表現からの脱却」ではなく「どの中間表現を使うか」の選択に留まる。

LAMINARIAが目指す設計(target固有専用コンパイラ、汎用中間表現を経由しない)は、これら既存の代替実装のいずれとも異なる、より野心的な主張であることが改めて確認できる。ただし、mrustcの「borrow checkingを外すことで実装を大幅に単純化できる」という知見、rustc_codegen_gccの「LLVM非対応targetのために専用バックエンドを追加する」という動機、Miriの「コード生成と意味論検証は分離可能」という知見は、いずれもLAMINARIAの設計判断に直接活用できる。

参照: mrustc README(<https://raw.githubusercontent.com/thepowersgang/mrustc/master/README.md>)、gccrs README(<https://raw.githubusercontent.com/rust-gcc/gccrs/master/README.md>)、gccrsディレクトリ構造(`https://api.github.com/repos/rust-gcc/gccrs/contents/gcc/rust`)、rustc_codegen_gcc README(<https://raw.githubusercontent.com/rust-lang/rustc_codegen_gcc/master/Readme.md>)、Miri README(<https://raw.githubusercontent.com/rust-lang/miri/master/README.md>)。

## 6. 先行事例(非Rust系): 「完全にゼロの中間表現」は実在するか

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

## 7. 総合判定

3つの調査を統合すると、「中間表現を削除する」という主張は、対象によって成否が異なる。

| 対象 | 削除可能か | 根拠 |
| --- | --- | --- |
| HIR(脱糖・依存追跡) | **削除可能** | 表現形式の選択に過ぎず、意味を変えない構文変換と依存追跡は別の場所に実装できる |
| MIRの「層」自体(データ型・名前) | **削除可能** | 「MIR」という独立した中間データ型を経由する必然性はない |
| MIRが担うCFG上の不動点計算(drop elaboration、borrow checking) | **削除不可能** | AST(木構造)では原理的に表現できない計算モデルそのものであり、どこかに何らかの形で残る |
| LLVM IRという「単一汎用表現を経由する」設計 | **削除すべき(強い実害根拠あり)** | `noalias`のような再エンコードが、正しさを脅かす実際のミスコンパイル(issue #31681/#84958/#54878)を繰り返し引き起こしている |
| target依存の意味論をtarget非依存表現で扱おうとする試み(panic/unwind等) | **そもそも成立しない** | Itanium/SEHのような差異は、単一表現を経由しても回避できない |
| 「意味論を1箇所に確定させる」という設計要求 | **削除不可能** | 複数の専用バックエンドを束ねる場合でも、意味論の重複実装を避けるためにはどこかで一度確定させる必要がある(RFC 1211理由6と同じ問題) |
| `noalias`という特定の再エンコード形式 | **削除すべき(より正確な代替の実在が確認できた)** | Stacked Borrows/Tree Borrowsが、より豊かな操作的意味論(タグ+木構造)を形式化しており、LLVM属性への再エンコードを経由しない代替設計の実在証拠になる |
| CFG上の不動点計算を「木/CFG構造」に固定する必要性 | **削除可能(より一般的な形へ置換可能)** | rustc自身のPolonius実装(`LocalizedConstraintGraph`)がborrow checkingをグラフ到達可能性問題として定式化しており、`SharedSymbolGraph`と同型の頂点+エッジ抽象の上に統合できる可能性がある(節2.5) |

したがって、LAMINARIAが実際に削除すべきは「**LLVM IRのような、target非依存を標榜しながらtarget依存の意味論(panic/unwind等)を扱いきれず、かつRustの型システムが持つ豊かな意味論を限定的な属性語彙に再エンコードすることで正しさを脅かす、単一の汎用中間表現という層**」である。

削除しても残らざるを得ないもの:

1. **CFG上のデータフロー解析という計算モデル**(drop elaboration、borrow checking相当の処理) — 名前を「IR」と呼ばない形で実装しても、この計算構造自体は必要。
2. **意味論を確定させる1箇所の境界** — target固有の専用バックエンドを複数束ねる場合、各バックエンドに意味論を重複実装させないための「意味論の所在」をどこかに置く必要がある。これは新しい「中間表現」ではなく、[全体設計ドキュメント](../../01-foundations/joint-symbol-schedule-layout-design_ja.md)で既に設計している`SharedSymbolGraph`(境界シンボルのみを共有し、各realmの内部意味論は外部へ露出しない)と同じ形の境界として実装できる可能性がある — これは次の設計課題である。

節2.5で確認したPoloniusのグラフ定式化(`LocalizedConstraintGraph`、`(region, point)`頂点+到達可能性)は、この「CFG上の不動点計算」という残らざるを得ない構造自体を、`SharedSymbolGraph`と**同じ抽象(頂点+エッジ+遅延的グラフ走査)の上に統合できる可能性**を具体的に示している。これは、削除できない計算モデル(節2.5の判定)を、既存の他設計(unified-symbol-graph)とは別の新しい構造として追加するのではなく、**同じグラフ構造の異なる頂点・エッジ種別として一体化する**という、まだ検証していない具体的な次の設計仮説である。

最も近い先行実装パターンはTinyCC(意味解析結果を外部化されたグラフ構造として保持しない)とZig(target固有の直接コード生成をLLVM非依存で持つ)のハイブリッドであり、これは`unified-symbol-graph`の既存設計(`CodeBody`が「意味解析結果」ではなく「機械語バイト列+再配置」を直接保持する)と既に整合している。学術的形式化の側からは、Polonius(グラフ到達可能性への再定式化)とStacked/Tree Borrows(`noalias`への再エンコードを経由しない、より豊かな操作的意味論)が、同じ方向性を独立に裏付けている。

節5で確認したRust自身の代替実装(mrustc/gccrs/rustc_codegen_gcc/Miri)は、いずれもLLVMという特定の中間表現には依存しないことに成功しているが、「中間表現という概念そのもの」からは誰も脱却できていない。この事実は、節6の非Rust系事例(TinyCC/QBE/Zig/V8)から得た結論をRust固有の文脈で再確認するものであり、LAMINARIAの主張(中間表現という概念自体の削除)が、Rust言語を対象とした既存の代替実装のいずれよりも野心的な主張であることを裏付けている。それでも、mrustcの「borrow checkingを外すことで実装を大幅に単純化できる」という知見は、節2.1で述べた「意味論を1箇所に固定する境界」の設計コストを下げる手段として、Miriの「コード生成と意味論検証は分離可能」という知見は「意味論の確定」と「target固有コード生成」を別の計算段階として設計できる可能性として、それぞれLAMINARIAの設計判断に直接活用できる。

## 8. まだ解けていないこと

- MIRが担う不動点計算(drop elaboration相当)を、独立した「IR」という名前の層を持たずに、どのようなデータ構造で実装するかの具体設計はまだ無い。**Poloniusのグラフ定式化(節2.5)が有力な出発点になり得るが、まだ`SharedSymbolGraph`との統合設計を実装していない。**
- 「意味論を確定させる1箇所の境界」を`SharedSymbolGraph`とどう統合するか(あるいは別の構造にするか)は未設計。
- panic/unwind lowering のようなtarget固有の分岐処理を、node 4で示した「target固有専用コンパイラ」それぞれにどう実装させるか(重複実装を許容するか、共通実装をどこかに置くか)は未設計。
- Zigの並列化モデル(意味解析1スレッド+コード生成複数スレッド+リンク1スレッド)と、LAMINARIAの`SharedSymbolGraph`が想定する並列モデル(各realmフロントエンドが独立して書き込む)の異同を、具体的な比較実験としてまだ検証していない。
- LAMINARIA自身がborrow checking相当の検証をどこまで・どの段階で行うか(mrustcのように単純化して実用性を優先するか、rustc同等の厳密さを保つか)は、正しさの保証範囲を左右する未決定の設計判断であり、まだ議論していない。
- Stacked/Tree Borrowsの操作的意味論(タグ+木構造)を、`ElfX86_64PendingReloc`のようなtarget固有の実データ型へどう落とし込むかの具体設計はまだ無い — 現状`unified-symbol-graph`は所有権・エイリアシング情報を一切保持していない(`CodeBody`は機械語バイト列+再配置のみ)。
- RustBelt/Oxideが示した「型付け規則としての定式化」と、Polonius/`SharedSymbolGraph`が示した「グラフ到達可能性としての定式化」は、異なる形式化の階層(型の健全性 vs. 実行時パス依存の状態計算)にあり、両者をLAMINARIAの設計の中でどう役割分担させるかはまだ整理していない。

## 関連文書

- [全体設計 — なぜ「一つの計算システム」でなければならないか](../../01-foundations/joint-symbol-schedule-layout-design_ja.md) — 問題D(汎用中間表現を手放す理由)
- [LLVM再発見研究](llvm-rediscovery-research_ja.md) — LLVM概念の再導出方法論
- [LLVM内部の実測記録](llvm-internals-observed_ja.md) — MC層・CGU粒度・並列化・ThinLTO・Cranelift実測失敗の記録
- [Rustそのもののコンパイルを研究・実験した先行プロジェクト・事例](rust-self-compilation-precedents_ja.md) — rustcのstage0/1/2ブートストラップ史、`CodegenBackend` trait経由のバックエンド差し替え機構、no_std/no_core最小構成
- issue #48 — CGU粒度、LLVM処理速度、Cranelift実測

# LLVM IRを経由しないtarget固有中間表現の設計 — issue #68

## 目的とスコープ

[意味論から「中間表現」を削除する](removing-intermediate-representation_ja.md)(以下「前回研究」)は節7の総合判定表で、「LLVM IRという特定の、target非依存を標榜する単一の汎用表現」は削除すべきだが、「MIRが担うCFG上の不動点計算という計算モデル」と「意味論を1箇所に確定させる境界」は削除不可能であると判定した。

本書は issue #68 として、この判定の先にある未解決の設計課題 — **MIR(またはMIRが担う計算モデル)から、LLVM IRを経由せず、target固有のコード生成へ直接接続するための中間表現を設計できるか** — に取り組む。

主張は「中間表現を完全に持たない」ことではない。前回研究の節6が確認した通り、完全にゼロの中間表現を持つコンパイラは実在しない(TinyCCですら`SValue`という極小の内部状態を持つ)。本書が扱うのは、「LLVM IRという特定の、target非依存を標榜する単一の汎用表現」を、target固有の直接的な表現へ置き換えるという具体的な設計課題である。

スコープは issue #68 の指示通り x86_64-unknown-linux-gnu に限定する。`experiments/unified-symbol-graph/`の`ElfX86_64PendingReloc`/`CodeBody`(issue #67で確立済み)が既に生成された**後**の話ではなく、`CodeBody`が生成される**前**の変換規則(MIR相当→CodeBody)を対象とする。

## 1. 調査結果: Cranelift CLIF — 「単一IR」でも内部では2つの表現が残る

`cranelift/codegen/src/`(bytecodealliance/wasmtime)を直接確認した結果、前回研究の節4の判定(「CraneliftはIRの数を1つに削減しているが、単一の共通表現を経由する構造自体は保持している」)を、より具体的な内部構造のレベルで補強する事実が見つかった。

CLIFは表面上「1つのIR」だが、実行時には構造的に異なる2つの表現が存在する。

1. **`InstructionData`**(`cranelift/codegen/src/ir/instructions.rs`): target非依存のSSA命令。抽象的な`Type`(`ir/types.rs`)のみを持ち、レジスタ・ABIの概念を一切含まない。
2. **`MachInst`**(`cranelift/codegen/src/machinst/mod.rs:272`): target固有の命令表現。`pub trait MachInst { type ABIMachineSpec: ABIMachineSpec<I = Self>; fn get_operands(...); fn is_move(...); fn gen_move(...); ... }` — レジスタ・アドレッシングモードを持つ、`InstructionData`とは構造的に別のデータ型。

この2つを繋ぐのが`LowerBackend` trait(`cranelift/codegen/src/machinst/lower.rs:121`):

```rust
pub trait LowerBackend {
    type MInst;
    fn lower(&self, ctx: &mut Lower<Self::MInst>, inst: Inst) -> Option<InstOutput>;
    fn lower_branch(...) -> Option<()>;
}
```

各ISA(`isa/x64/`, `isa/aarch64/`, `isa/riscv64/`, `isa/s390x/`, `isa/pulley_shared/`)がこの trait を実装し、CLIFの`Inst`を自分の`MachInst`へ変換する。target識別情報(triple、呼び出し規約、ポインタ幅、unwind情報の形)が実際に付与される単一の接続点は`TargetIsa` trait(`cranelift/codegen/src/isa/mod.rs:284`)であり、`compile_function`(309行)、`emit_unwind_info`(331行)、`default_call_conv`/`endianness`/`pointer_type`(442-479行)を持つ。

**この構造から得られる設計材料**: 「1つのIRで済ませる」という見た目上のシンプルさは、実際には「target非依存の構築用表現」と「target固有の下位表現」という2層構造を、1つの型名(CLIF)の内部に隠しているに過ぎない。LAMINARIAが目指す「target固有の直接表現」を設計する際、この2層構造自体を消せるかどうかが論点になる — Craneliftはこれを消していない(`InstructionData`→`MachInst`という変換パスは常に存在する)。

## 2. 調査結果: QBEの縮小設計 — 型システムを意図的に貧弱にすることでIRを薄く保つ

`c9x.me/compile/doc/il.html`(QBE公式IL仕様)とリポジトリのディレクトリ構成を確認した結果、前回研究の節6.2の判定(「IRの削除ではなく縮小」)を裏付ける、具体的な縮小の設計判断が確認できた。

- **型: 4つの基本型(`w`, `l`, `s`, `d`)+ 2つの拡張型(`b`, `h`)のみ**。ポインタ型は存在しない。仕様書が明記: *"There are no pointer types available; pointers are typed by an integer type sufficiently wide to represent all memory addresses."* — ポインタを「アドレスを表現できるだけの幅の整数型」として扱い、独立した型カテゴリを設けないという判断である。
- **命令数: 約60個**(算術/ビット演算、メモリ、8種の比較演算ファミリ、変換、キャスト/コピー、呼び出し/可変長引数、制御(`phi`/`jmp`/`jnz`/`ret`/`hlt`))。LLVMの命令+intrinsic群と比べて一桁少ない。
- **型を安全性の手段として使わない**という設計方針が明言されている: *"QBE is not using types as a means to safety; they are only here for semantic purposes."* また、フロントエンドが厳密なSSAを構築する義務を負わせず、QBE内部で修復する設計を取る(構築側の負担を減らすための縮小)。
- **target分岐**: リポジトリルートの`abi.c`(target非依存のABI変換の入口)に対し、`amd64/`, `arm64/`, `rv64/`というtarget別ディレクトリが、それぞれの命令選択・出力を担う。Craneliftの`isa/<arch>/`と同じ形の分岐だが、規模ははるかに小さい。

**判定**: QBEは「型システムを意図的に貧弱にする」ことでIRの表現力そのものを削り、1つの薄いIRで済ませている。これは「中間表現の削除」ではなく「中間表現の縮小」であり、前回研究の判定を覆さない。ただし縮小の具体的な手段(ポインタ型を持たない、型を安全性の根拠に使わない)は、LAMINARIAがtarget固有表現を設計する際の反面教師になる — QBEの型の貧弱さは、まさに前回研究の節3.2-3.3で確認した「Rustの豊かな意味論を限定的な語彙へ再エンコードする」問題を、LLVM IRより小さいスケールで再現する危険性を持つ。LAMINARIAが目指すtarget固有IRは、QBEのように型を犠牲にして薄くするのではなく、Rustの型・所有権情報をtarget固有表現の中でも保持し続ける必要がある(前回研究節3.3のStacked/Tree Borrowsの知見と整合)。

## 3. 調査結果: rustcのMIRは実際にtarget非依存であり、`BuilderMethods`はLLVMの語彙を継承する

`rust-lang/rust`のソースを直接確認した。

### 3.1 `Body`構造体は完全にtarget非依存

`compiler/rustc_middle/src/mir/mod.rs:206-260`で確認した`Body`構造体のフィールド(`basic_blocks`, `phase: MirPhase`, `source_scopes`, `coroutine`, `local_decls`, `user_type_annotations`)には、`DataLayout`・ポインタ幅・呼び出し規約に相当するフィールドが**一切存在しない**。target情報は`Body`自体には埋め込まれておらず、`TyCtxt`/`Target`(`compiler/rustc_target/src/spec/mod.rs:2001`、`pub struct Target`)経由で、コード生成時に`TyAndLayout`やABI照会のタイミングで初めて解決される。

これは前回研究が未検証のまま残していた問いに対する直接的な裏取りであり、「MIR相当の入力からtarget固有表現へ直接変換する」というLAMINARIAの設計方針にとって有利な事実である — MIR自体はtarget情報を持たないため、LAMINARIAが独自にMIR相当の表現を実装する場合も、target情報をこの表現自体に埋め込む必要がない(埋め込むべきではない)ことが、rustc自身の設計から確認できる。

### 3.2 呼び出し規約/ABI変換は、既にバックエンド非依存の場所で完結している

`compiler/rustc_target/src/callconv/mod.rs:609`の`pub struct FnAbi<'a, Ty>`、およびその`adjust_for_foreign_abi`(646行)、`adjust_for_rust_abi`(733行)を確認した。これらは`rustc_target`クレート — `rustc_codegen_ssa`にすら属さない、あらゆるバックエンドの外側 — に実装されている。すなわち、ABIの決定(引数のレジスタ/スタック割り当て、構造体の展開/パス方法)は、LLVM/Cranelift/GCCいずれのバックエンドが選ばれるより**前**に、共通の場所で1回だけ行われる。

これは前回研究の節3.5(「rustc自身が既にバックエンド非依存層とLLVM固有層を分離している」)をより具体的に補強する。ABI層は`BuilderMethods`より下(呼び出す側)に既に存在し、LAMINARIAが独自のtarget固有バックエンドを実装する場合も、この`FnAbi`相当の計算をゼロから再設計する必要はなく、既存の分離パターンをそのまま再利用できる可能性がある。

### 3.3 `BuilderMethods`は再利用可能な接続点だが、LLVMの命令粒度を継承している — 重要な限定

`compiler/rustc_codegen_ssa/src/traits/builder.rs:51`の`BuilderMethods` traitを確認した。これは`LayoutOf`, `FnAbiOf`, `ArgAbiBuilderMethods`, `AbiBuilderMethods`, `IntrinsicCallBuilderMethods`, `AsmBuilderMethods`, `StaticBuilderMethods`, `CoverageInfoBuilderMethods`, `DebugInfoBuilderMethods`という複数のtraitを束ねた、150以上のメソッドを持つ大きなtraitである。メソッド群は次のように分類できる。

- **制御フロー**: `ret`/`br`/`cond_br`/`switch`/`invoke`/`unreachable`
- **算術**: `add`/`fadd`/`mul`/`udiv`等、LLVMの命令1つに1メソッドが対応
- **メモリ**: `alloca`/`load`/`store`/`gep`、およびatomicバリアント
- **キャスト**: `trunc`/`sext`/`fptoui`/`ptrtoint`
- **呼び出し**: `call`は`rustc_target::callconv`で既に計算済みの`&'tcx FnAbi<'tcx, Ty<'tcx>>`を直接受け取る(`compiler/rustc_codegen_ssa/src/mir/block.rs`の`codegen_call_terminator`で確認)

前回研究の節3.5は「`BuilderMethods`は既にジェネリックに書かれている」と述べたが、本調査はこれをより限定的に確定させる: **`BuilderMethods`はバックエンド非依存ではあるが、target形状に対して中立ではない**。メソッド集合そのものがLLVMのSSA命令の語彙(`add`, `gep`, `trunc`という個別命令1つ1つに対応するメソッド)をそのまま踏襲しており、LAMINARIAが独自のtarget固有バックエンドとしてこのtraitを実装する場合、**LLVMの命令粒度を交換フォーマットとして受け入れる**ことを意味する。QBEの非SSA許容モデルや、Craneliftの`MachInst`のような、より異なる形状の内部表現を採用したい場合、このtraitをそのまま実装するのではなく、独自のtraitを新設する必要がある。

**判定**: `BuilderMethods`は「MIRからLLVM IRを経由せずtarget固有コードへ接続する」ための実在する接続点として使えるが、それは「LLVM IRというデータ構造を経由しない」ことと引き換えに「LLVM IRの命令粒度という語彙」を引き継ぐという妥協を伴う。前回研究が「LLVM IRを削除すべき理由」として挙げた`noalias`のような属性再エンコード問題(節3.2-3.3)は、`BuilderMethods`経由でも構造的には解消されない可能性がある — `BuilderMethods`のメソッド自体が呼び出し側に渡す情報(引数属性等)はLLVM属性の語彙に依存しているため、`BuilderMethods`を実装するだけでは前回研究が指摘した実害(ミスコンパイル)の根本原因を回避できない。

## 4. 設計方針: `SemanticFacts`をCFGベースの表現へ拡張し、`CodeBody`への直接lowering規則を`AddressState`に統合する

以上の調査を統合し、次の設計方針を採る。

### 4.1 基本方針: `BuilderMethods`は参照するが、直接実装しない

`BuilderMethods`はLLVMの命令粒度を継承するため、そのまま実装すると前回研究が指摘したnoalias型の再エンコード問題を再導入するリスクがある。代わりに、`BuilderMethods`が示す**構造上の教訓**(ABI決定はバックエンドに依存しない層で完結させる、呼び出し規約はコード生成の入り口で既に確定している)だけを採用し、メソッド粒度(LLVM命令語彙)は採用しない。

### 4.2 MIR相当の表現: CFG + 所有権タグを直接保持する、新しいSSA非依存表現

前回研究の節2.5(Polonius)・節3.3(Stacked/Tree Borrows)の知見を統合し、次の3つの性質を持つ最小の中間表現`TargetIr`を設計する(節5で実装)。

1. **CFGとデータフロー解析(不動点計算)という計算モデルは維持する**(前回研究節2.2の判定により削除不可能)。ただし独立した「MIR」という名前の層としてではなく、`SharedSymbolGraph`の`SemanticFacts`を拡張した`ControlFlowFacts`として、既存のグラフ構造の中に統合する。
2. **所有権・エイリアシング情報は、LLVM属性(`noalias`)への再エンコードを経由せず、タグとして表現全体に保持し続ける**(前回研究節3.3のStacked/Tree Borrowsの知見)。QBEのように型を犠牲にして薄くする(節2の反面教師)ことは避ける。
3. **target固有の命令選択・レジスタ割り当ては、CLIFの`MachInst`のように独立した第2の表現へ分離してよい**が、その変換規則自体を本設計のスコープに含める(Craneliftの`LowerBackend`に相当する変換を、x86_64-unknown-linux-gnu専用に直接実装し、汎用traitとして抽象化しない — `unified-symbol-graph`の既存方針`ElfX86_64PendingReloc`と同じ「無理に汎用化しない」原則)。

### 4.3 `unified-symbol-graph`との統合点

`AddressState::Analyzed(SemanticFacts)`(issue #67で確立済み)は現状「シグネチャ+依存シンボルのリスト」という最小限の情報しか持たない。本設計はこれを次のように拡張する候補を示す(節5のPoCで最小限のみ実装)。

- `SemanticFacts`に、CFG構造(基本ブロックのリストと分岐関係)と所有権タグ(各値がuniqueかsharedか)を追加できるようにする。
- `Analyzed`→`Committed`への昇格(`promote_analyzed_to_committed_if_needed`)の中で実行される`finish_codegen`クロージャが、この拡張された`SemanticFacts`を受け取り、target固有の命令選択を行い、`CodeBody`を直接生成する — この変換規則こそが「LLVM IRを経由しないlowering」の実体であり、LLVM IRのようなテキスト/バイナリ形式の中間成果物を一切生成しない。

## 5. 最小限の概念実証

`experiments/unified-symbol-graph/src/target_ir.rs`に、次を実装する。

- 制御フローを持つ最小のMIR相当表現`TargetIr`(基本ブロック+分岐+所有権タグ付き値)。
- x86_64-unknown-linux-gnu専用の直接lowering関数`lower_target_ir_to_code_body`(LLVM IRを一切経由せず、`TargetIr`から`CodeBody`のバイト列を直接組み立てる)。
- 分岐を含む最小のワークロード(条件によって異なる定数を返す関数)で、生成された機械語バイト列が実際に意図通りの分岐命令になっていることを、命令バイト単位で検証するテスト。

この概念実証は「production-quality command generator」ではなく、「MIR相当のCFG入力から、LLVM IRという中間成果物を一度も生成せずにCodeBodyへ到達できる」という設計方針そのものが実装可能であることを示す最小の証拠である。

### 5.1 実際のRust構文からの経路: `target_ir::mir_text`

節5の`TargetIr`は当初、開発者が手で組み立てた値のみを入力としていた。これでは「実際のRust言語の構文」を実験したことにならないという指摘を受け、`experiments/unified-symbol-graph/src/target_ir.rs`内の`mir_text`サブモジュールとして、次の経路を追加した。

1. 実際に`rustc --edition 2021 --crate-type lib -C debuginfo=0 --emit=mir`を、次の3つの実Rustソースに対して本セッション内で実行し、実際の出力を直接取得した(手元での推測や過去の記憶からの再現ではない)。
   - `pub fn branch(param0: i32) -> i32 { if param0 != 0 { 7 } else { 9 } }`(`!=`分岐)
   - `pub fn branch2(param0: i32) -> i32 { if param0 == 0 { 1 } else { 2 } }`(`==`分岐)
   - `pub fn passthrough(param0: i32) -> i32 { param0 }`(直線コード)
2. これらの実出力(`switchInt(move _2) -> [0: bbX, otherwise: bbY]`、`goto -> bbZ`、`_0 = const K_i32`等、rustcが実際に出力したテキストそのもの)を`mir_text::parse_mir_text`がパースし、節5の`TargetIr`へ変換する。
3. 変換された`TargetIr`を、節5と同じ`lower_target_ir_to_code_body`へそのまま渡し、x86_64機械語バイト列を生成する。`!=`分岐のケースでは、手組みの`TargetIr`で既に`objdump`検証済みの命令バイト列と**完全に一致する**ことをテストで確認した(`real_rustc_mir_dump_for_ne_branch_parses_and_lowers_to_the_verified_bytes`)。

これにより、「実際のRustソースコード → 実際の`rustc`によるMIR生成 → 本設計のTargetIrへの変換 → LLVM IRを経由しないx86_64機械語生成」という経路全体を、開発者の手組みデータに頼らず実データで実証した。ただし対応範囲は、直線コードと2分岐+共通合流点(`goto`)を持つ`switchInt`の2パターンに限定されており(節6の限界を参照)、`match`の3分岐以上、ループ、関数呼び出し、`i32`以外の型は`ParseError::UnrecognizedShape`として明示的に拒否する。

## 5.2 LLVM IRとの「省エネ」比較 — コンパイル時間・中間成果物サイズ・実行時性能を実測

前回研究(節3)は「LLVM IRを削除すべき」という判定を、正しさ(`noalias`ミスコンパイル)の観点から下した。本節では、この判定とは別軸の問い — 「LLVM IRを経由しないことで、実際に何らかの意味で"省エネ"(効率化)になるのか」— を、`if param0 != 0 { 7 } else { 9 }`という同一のRust関数を対象に、3つの観点(コンパイル時間、中間成果物サイズ、実行時性能)で実測した。結論を先取りすると、**3つのうち2つ(コンパイル時間・中間成果物サイズ)は本設計が有利、1つ(実行時性能)はLLVMが明確に有利**という、単純な「省エネ」ではない結果になった。

### 5.2.1 コンパイル時間: この規模ではrustc起動オーバーヘッドが支配的で、公平な比較ができない

同一の`branch.rs`に対し、`rustc --emit=mir`(LLVM未呼び出し)と`rustc --emit=obj`(LLVM経由)をそれぞれ5回実行し、実測した。

| 経路 | 中央値(5回) |
| --- | --- |
| `rustc --emit=mir`(LLVM未呼び出し) | 0.123秒 |
| `rustc --emit=obj -O`(LLVM経由) | 0.123秒 |

**両者はほぼ同一**であり、これは「LLVMの処理が速い」ことを意味しない。`rustc`プロセスの起動・クレートメタデータ準備自体のコストが、この極小の1関数に対する処理時間を完全に支配しており、LLVM自体のコード生成時間は測定誤差に埋もれている。前回研究(`llvm-internals-observed_ja.md`)が指摘した「LLVM処理速度がボトルネック」という問題意識は、実運用規模のクレート(多数の関数・codegen unit)でこそ観測可能であり、本実験のような1関数単位の比較では公平な計測ができないことを正直に記録する。

一方、本設計の直接lowering部分(`mir_text::parse_mir_text`+`lower_target_ir_to_code_body`)は、プロセス起動を含まないRustプロセス内呼び出しとして計測すると、1000回反復で合計約19-21ミリ秒、**1回あたり約20マイクロ秒**だった(`target_ir::energy_comparison_bench::in_process_parse_and_lower_wall_clock_time_for_1000_iterations`)。ただしこれは「rustcプロセス全体」対「本設計の関数呼び出し1回」という、測定範囲が異なる比較であり、同一条件での対比ではない。公平な比較には、本設計を実際の`rustc`ドライバ(`-Zcodegen-backend`相当)へ統合し、プロセス起動を含めた全体時間で計測する必要があるが、これは節6の未解決事項である。

### 5.2.2 中間成果物サイズ: 本設計はtargetメタデータを持たない分、明確に小さい

同一関数に対し`rustc --emit=llvm-ir -C opt-level=0`(関数本体が最適化で消えない設定)で実際のLLVM IRテキストを取得し、本設計の`TargetIr`のインメモリサイズと比較した。

| 表現 | サイズ |
| --- | --- |
| 実LLVM IRテキスト(`branch_o0.ll`、実測) | 1139バイト |
| `TargetIr`(構造体+`Vec<BasicBlock>`ヒープ確保、実測) | 112バイト |

約10分の1。ただしこれは公平な比較ではないことを明記する。LLVM IRテキストには`target datalayout`/`target triple`/`PIC Level`/`uwtable`/rustcバージョン文字列といった、**本設計の`TargetIr`には対応物が一切存在しないモジュールレベルのメタデータ**が含まれる。本設計はx86_64-unknown-linux-gnu専用にスコープを限定しており(節4.2)、target識別情報を値自体に埋め込む必要がない。前回研究(節1.2)で確認した「rustcの`Body`構造体自体はtarget非依存」という事実、および`lower_target_ir_to_code_body`という関数名にtarget情報を静的に固定するという本設計の方針(節4.2、Craneliftの`TargetIsa`のような実行時ディスパッチを持たない)が、このサイズ差の直接の理由である。「IRの表現力を削って小さくした」のではなく、「target情報をテキスト表現内に持ち回す必要自体をなくした」ことによる差であり、QBE型の縮小(節2)とは異なる種類の省エネである。

### 5.2.3 実行時性能: LLVMの最適化(分岐除去)により、LLVM側が約2.7〜3.0倍高速

同一関数を実際に実行し、実行時間を比較した。`rustc -O`でコンパイルした`#[inline(never)]`版のRust関数(`objdump`で実際に`call`命令が残っていることを確認済み)と、本設計の`lower_target_ir_to_code_body`が生成したx86_64バイト列を実際に`mmap`(`PROT_EXEC`)して実行可能にしたものを、同一のSystem V呼び出し規約でそれぞれ1億回呼び出し、経過時間を比較した(`experiments/unified-symbol-graph/examples/target_ir_energy_bench.rs`)。

5回の実行結果(N=1億回呼び出し):

| 実行 | 本設計(直接lowering) | LLVM(`rustc -O`) | 比率(本設計/LLVM) |
| --- | --- | --- | --- |
| 1 | 276.5ms | 98.7ms | 2.80x |
| 2 | 265.4ms | 99.0ms | 2.68x |
| 3 | 259.9ms | 88.1ms | 2.95x |
| 4 | 278.0ms | 94.7ms | 2.94x |
| 5 | 269.8ms | 95.2ms | 2.83x |

**LLVMの方が一貫して約2.7〜3.0倍高速**である。原因は`objdump`で直接確認済み: LLVMはこの関数を`xor eax,eax; test edi,edi; sete al; lea eax,[rax*2+7]; ret`という、**分岐命令を一切使わないブランチレスコード**へ変換していた。一方、本設計の`lower_target_ir_to_code_body`は最適化パスを一切持たず、`cmp`/`je`という実際の条件分岐命令を素直に発行する(節5の設計方針通り)。両者が実行結果として完全に一致すること(`0`/`1`/`-1`/`42`の4入力で照合済み)は確認した上での比較であり、正しさの差ではなく、最適化の有無による差である。

### 5.2.4 総合: 「省エネ」は一様な答えを持たない

| 観点 | 本設計が有利 | 根拠 |
| --- | --- | --- |
| コンパイル時間 | **未確定**(この規模では測定不能) | rustcプロセス起動オーバーヘッドが支配的で、LLVM自体の処理時間が埋もれる。実運用規模での再測定が必要(節6) |
| 中間成果物サイズ | **本設計が有利**(約10分の1) | target非依存を標榜するために持つ必要のあるメタデータ(datalayout/triple等)を、target固有スコープの本設計は最初から持たない |
| 実行時性能 | **LLVMが有利**(約2.7〜3.0倍) | LLVMの最適化パス(分岐除去等)による。本設計は最適化を一切実装していないため、この差は「lowering機構の設計」ではなく「最適化パスの有無」に起因する |

前回研究の主張(節7)は「LLVM IRという特定の汎用中間表現を削除すべき」という**正しさ**の観点の主張であり、「LLVM IRを経由しないことが常に効率的である」という主張ではない。本実測はこれを裏付ける: LLVMが持つ最適化パスという蓄積された資産は、本設計のような直接lowering方式では代替できておらず(節6の未解決事項)、実行時性能の観点では現時点で明確な劣位にある。一方、中間成果物のサイズという観点では、target固有スコープに限定するという設計方針(前回研究の`unified-symbol-graph`自身の既存方針、`ElfX86_64PendingReloc`の「無理に汎用化しない」原則と同じ)が、目に見える形で効率化に寄与することを確認した。

## 5.3 「Rustで使えるかどうか」の検証: `mir_text`の限界と、rustc内部APIによる型・借用チェック済みMIRの直接取得

節5.1の`mir_text::parse_mir_text`には、ユーザーから決定的な指摘を受けた: このパーサーが実際に受理しているのは`rustc --emit=mir`が出力した**人間可読テキスト**の特定パターン(`switchInt`/`goto`/`const`という文字列)にすぎず、Rustの型システム・所有権・借用チェックを一切経由していない。`Ownership`タグも、lowering処理がその値を一度も参照しない(節5節末尾のテストが確認する通り、`Ownership::Unique`でも`Shared`でもlowering結果は変わらない)という意味で、値として持ち回っているだけの飾りだった。これは「Rustを扱った」ことにはならず、「`rustc`がたまたま出力した文字列を受理する自作パーサーを書いた」ことと区別がつかない、という指摘は正当である。

この指摘に応え、`experiments/rustc-driver-poc/`(nightlyツールチェーン + `rustc-dev`/`llvm-tools`コンポーネント、本セッションでインストール済み)に、`rustc_driver`/`rustc_interface`という**rustc本体の内部API**を実際に呼び出すプログラムを実装した。これは前回研究(節5.3、rustc_codegen_gcc)が確認した「rustc本体のフロントエンド・型検査・借用チェックはそのまま使い、バックエンドだけを差し替える」という構造を、本セッションで実際にコードとして動かした初めての実証である。

### 5.3.1 実装と実測結果

`Callbacks::after_analysis`フック(`compiler/rustc_interface/src/passes.rs`の`run_required_analyses`→`analysis`ゲート、前回研究節5.6.1で確認済みの、borrow check完了・codegen開始前の地点)を使い、実際にコンパイルされた関数の`rustc_middle::mir::Body`から、次を**文字列パースではなく型システムのデータ構造として直接**取得した。

- `tcx.mir_borrowck(local_def_id)`を実際に呼び出し、`Ok(_)`(借用チェック**合格**)であることを確認。
- 各ローカル変数の実際の`Ty<'tcx>`(例: `&'{erased} mut i32`)を取得し、`TyKind::Ref(_, _, Mutability)`という実フィールド(前回研究が`arg_attrs_for_rust_scalar`で確認した`noalias`導出と同じフィールド)から`Ownership::Unique`/`Shared`を導出。

実測結果(`cargo +nightly test`、3件全て合格):

| 入力(実Rustソース) | 導出された`Ownership` | `mir_borrowck`結果 |
| --- | --- | --- |
| `fn f(x: &mut i32)` | `Unique` | `Ok(())` |
| `fn f(x: &i32)` | `Shared` | `Ok(())` |
| `fn f(x: i32)`(非参照) | `NotAReference` | `Ok(())` |

これは`mir_text`のテキストパーサーとは質的に異なる: **実際の型検査器・借用チェッカーが生成した`Ty<'tcx>`を読んでいる**のであって、テキスト出力の文字列パターンマッチではない。

### 5.3.2 副産物として確認できた、borrow checkゲートの実挙動(前回研究の裏取り強化)

実装過程で、意図的に借用違反を含むコード(`let y = &mut *x; let z = &mut *x;`)を同じ入力として与えたところ、`after_analysis`フック自体に到達せず、コンパイルがE0499で中断した。この時点では「`rustc_driver::run_compiler`がプロセスを中断させた」とだけ記録していたが、この判定は**不正確だった**ことが節5.4の調査で判明した。正しい説明は節5.4を参照。

### 5.3.3 この段階でもまだ「本格的にRustで使える」ことの証明ではない、と明記する

節5.3の実証は、次の限定の中でのみ成立していた(節5.4で一部は解消)。

- `experiments/rustc-driver-poc/`は`unified-symbol-graph`クレート(stableツールチェーンでビルド)とは別の、nightly専用のスクラッチクレートである。`#![feature(rustc_private)]`はnightly限定の機能であり、本クレートの正式なビルドパイプラインへ組み込むには、ツールチェーン要件自体を変更する必要がある(節6の未解決事項、現状もこの制約自体は解消していない)。
- 取得した`Ownership`は、まだ`target_ir::lower_target_ir_to_code_body`(節5)へ実際に繋がっていなかった。「型システムから正しい情報を取得できる」ことと、「その情報をtarget固有コード生成が実際に消費する」ことは別の作業であり、後者は未実装だった。**節5.4.2で解消**。
- ジェネリクス単相化後のコード、トレイト境界、`Drop`実装、構造体レイアウトといった、Rustの型システムのより広い部分は未検証だった。今回検証したのは`&mut i32`/`&i32`/`i32`という最小の3パターンのみだった。**節5.4.1で一部(ジェネリクス単相化・`Box`・enum判別子読み取り)を解消**。

## 5.4 ユーザーによる二度目の批判的レビューへの応答: 検証範囲の拡大と、以前の記述の訂正

節5.3の内容自体に対し、ユーザーから2つの重ねての批判を受けた。(1) 検証対象が`&mut`/`&`/`i32`のみで、これはC言語の`const`修飾子でも表現できる区分であり、ジェネリクス・`Box`・パターンマッチというRust特有の機構を一切検証していない。(2) 取得した`Ownership`情報が実際のコード生成に一度も使われておらず、「取得できる」ことの確認に留まっている。加えて「本番実装に入ってからやっぱりできませんでしたはあり得ない」という強い指摘を受け、以下を実装した。

### 5.4.1 ジェネリクス単相化・`Box`・パターンマッチの実データ検証

`rustc --emit=mir`の実出力を直接取得し(本セッションで`rustc`コマンドラインから実行、以下は実際の出力から抜粋):

- **ジェネリクス単相化**: `fn identity<T>(x: T) -> T`を`Instance::instantiate_mir_and_normalize_erasing_regions`(`compiler/rustc_middle/src/ty/instance.rs`)で`i32`/`i64`それぞれに単相化し、`TyCtxt::layout_of`で実際のレイアウトサイズを取得。結果: `identity<i32>`は4バイト、`identity<i64>`は8バイトと、**同じ関数が型ごとに異なる実レイアウトを持つ**ことを実データで確認した(テスト`generic_function_monomorphized_with_different_types_yields_different_real_layout_sizes`)。
- **`Box<T>`(ヒープ所有権+drop)**: `pub fn consumes_box(b: Box<i32>) -> i32 { *b }`の実MIRを取得したところ、単純なデリファレンスだけで`drop(_1) -> [return: bb1, unwind continue];`という実際の`TerminatorKind::Drop`、およびアライメント/nullチェックの`assert`ターミネータが生成されることを確認した。`Ty::is_box()`(`compiler/rustc_middle/src/ty/sty.rs`)で`Box<i32>`を検出し、`Ownership::Boxed`という新しい分類を追加、`drop_terminator_count`を実際に`Body`の全基本ブロックを走査して数えるロジックを実装し、期待通り`1`であることを確認した。
- **パターンマッチ(enum discriminant)**: 3バリアントの`enum Shape { Circle(i32), Square(i32), Point }`に対する`match`の実MIRを取得したところ、`_2 = discriminant(_1); switchInt(move _2) -> [0: bb4, 1: bb3, 2: bb2, otherwise: bb1];`という、`Rvalue::Discriminant`読み取り+3分岐+到達不能アームへ変換されることを確認した。これは節5.1の`mir_text::parse_mir_text`(2分岐・定数returnのみ対応)が全く扱えない構造である。`Rvalue::Discriminant`の出現回数を実際にカウントし、期待通り`1`であることを確認した。

これら3つとも、C言語の型システムには存在しない、Rust特有の機構(型ごとの単相化、所有権を伴うヒープ確保・破棄、タグ付きunionの判別)を、実際のrustc内部データ構造から取得している。

### 5.4.2 `Ownership`情報を実際にコード生成へ接続する

節5.3.3が指摘していた最も重要なギャップ——「取得した型情報が、実際のコード生成に一度も使われていない」——を解消した。

`experiments/unified-symbol-graph/src/target_ir.rs`に、`Ownership`を実際に消費する新しい関数`lower_double_load_to_code_body`を実装した。`*p + *p`(ポインタが指す値を2回加算する)という最小の意味論に対し:

- `Ownership::Unique`(`&mut i32`/`Box<i32>`の類推): 他に別名が存在しないため、1回のロードで済ませ、レジスタ内で2倍にする——`mov eax, [rdi]; add eax, eax; ret`(5バイト)。
- `Ownership::Shared`(`&i32`の類推、ただし本実装は保守的側に倒し、常に2回読み直す設計とした——実際のRustの`&i32`不変性保証はさらに強い一括読み込みも安全に許すが、この実装は意図的にその精密さまでは踏み込まず、2つの分岐が視覚的に異なる出力になることだけを保証する最小実装とした): 2回別々にロードして加算——`mov eax, [rdi]; mov ecx, [rdi]; add eax, ecx; ret`(7バイト)。

生成されたバイト列は`objdump`で実際に正しい命令列であることを確認し、さらに`experiments/unified-symbol-graph/examples/ownership_consumption_check.rs`で両方の経路を実際に`mmap`実行し、`i32::MAX`/`i32::MIN`を含む7つの入力全てで同じ(正しい)結果を返すことを実行時に確認した。これにより「`Ownership`が違えば生成バイト列も異なり、かつ両方とも正しい」ことを実測で示した。

さらに、`experiments/rustc-driver-poc/`から`unified-symbol-graph`(stableツールチェーン)へのpath依存を追加し(逆方向の依存は作らない——`unified-symbol-graph`自体はrustc内部APIを一切知らない)、次の完全なパイプラインを実装・実測した。

```
実Rustソース → rustc_driver::run_compiler(borrow check含む)
             → Ty<'tcx>からOwnership導出
             → unified_symbol_graph::target_ir::lower_double_load_to_code_body呼び出し
             → 実x86_64機械語バイト列
```

`&mut i32`関数は`[0x8B, 0x07, 0x01, 0xC0, 0xC3]`(1回ロード版)、`&i32`関数は`[0x8B, 0x07, 0x8B, 0x0F, 0x01, 0xC8, 0xC3]`(2回ロード版)を実際に生成し、これは`unified-symbol-graph`側の単体テストが`objdump`で個別に検証済みのバイト列と完全一致することを確認した(テスト`real_ownership_from_borrow_checked_types_reaches_and_changes_generated_code`)。

### 5.4.3 borrow checkゲートを、実際にコード生成パイプラインへ統合する(前回の記述の訂正を含む)

節5.3.2の記述——「`rustc_driver::run_compiler`がプロセスを中断させた」——は不正確だったことが判明した。`compiler/rustc_span/src/fatal_error.rs`を直接確認したところ、前回研究(節5.6.1)が確認した`raise_fatal()`は、実際には`std::panic::resume_unwind(Box::new(FatalErrorMarker))`という**アンワインドパニック**であり、プロセスの強制終了ではない。これは`rustc_errors::catch_fatal_errors`(内部は`panic::catch_unwind`)で捕捉できるよう、意図的にこの実装になっている。

この事実を使い、`inspect_result`関数を`catch_fatal_errors`でラップし、借用チェック失敗を`Result::Err`として捕捉できるようにした。これにより、次の統合関数`compile_pointer_function_respecting_borrowck_gate`を実装した: 実Rustソースを渡し、(1) borrow checkが成功すれば型情報から`Ownership`を導出し`unified-symbol-graph`のコード生成を呼び出して`Some(bytes)`を返す、(2) borrow checkが失敗すれば(パニックを`catch_fatal_errors`で捕捉し)コード生成に一切到達せず`None`を返す。

実測結果:

| 入力 | 結果 |
| --- | --- |
| 有効な`&mut i32`関数 | `Some([0x8B, 0x07, 0x01, 0xC0, 0xC3])` |
| 有効な`&i32`関数 | `Some([0x8B, 0x07, 0x8B, 0x0F, 0x01, 0xC8, 0xC3])` |
| 借用違反(`&mut`の二重取得)を含む関数 | `None`(コード生成に到達せず) |

これは前回研究(節5.6.6)が要求した「borrow checkは省略可能な診断ではなく、コード生成へ進む前提条件を検査するゲートである」という設計要求を、**本セッションで初めて実際に動くパイプラインとして実装した**。前回の「E0499でプロセスが中断した」という観察は、単に「エラーを捕捉していなかったので、パニックがそのままプロセスへ伝播した」ことの誤解釈であり、実際には`catch_fatal_errors`という正規の捕捉機構が存在し、それを使えばRustプログラムとして正常に継続動作できる。この訂正自体、批判的レビューが実装の理解不足を明らかにした具体例である。

### 5.4.4 それでも残る限界(正直な記録)

- `Ownership::Shared`の保守的な2回ロード実装は、Rustの`&T`が実際に持つより強い不変性保証(一度も書き換えられないことが保証される)を活かしきっていない。より精密な実装にはStacked/Tree Borrowsレベルの解析が必要であり(前回研究節3.3)、これは意図的にスコープ外としている。
- 検証したのは`i32`という単一の値型のみ。構造体・タプル・トレイトオブジェクト・クロージャのレイアウト・キャプチャは一切検証していない。
- ジェネリクス単相化(節5.4.1)は`Layout`のみを比較し、単相化された`Body`自体を`target_ir`/`lower_double_load_to_code_body`へ接続するところまでは行っていない——「型ごとに異なるレイアウトが取得できる」ことと「その型ごとの`Body`を正しくtarget固有コードへ変換する」ことは別の作業であり、後者はまだ未接続である。
- `Box`(節5.4.1)の`drop_terminator_count`は数えられるようになったが、この`Drop`ターミネータ自体をtarget固有コード(実際のヒープ解放呼び出し)へ変換するところまでは実装していない——検出しただけで、まだ「使って」いない。
- `match`(節5.4.1)の`discriminant_read_count`も同様に、検出のみで、`discriminant`読み取り+3分岐以上の`switchInt`を`target_ir`側で実際にlowerする経路は未実装。既存の`mir_text`/`lower_target_ir_to_code_body`は依然として2分岐限定のままである。
- borrow checkゲート統合(節5.4.3)は`&mut i32`/`&i32`という1引数関数のみを対象としており、複数引数、複数の借用が絡む複雑な借用関係(前回研究節2.5のPolonius定式化が扱うような、複数の`(region, point)`ペア)は未検証。

## 5.5 3度目のユーザー指摘への応答: 他のRust言語機能の並列検証(dyn Trait・複数借用・レイアウト・unsafe)

ユーザーから「他にもRust言語の機能は?」という指摘を受け、`&mut`/`&`/ジェネリクス/`Box`/`match`に続く4つの機能領域(トレイトオブジェクト動的ディスパッチ、複数借用の絡み合い、構造体・クロージャのレイアウト、unsafe/生ポインタ)を並列で実データ検証した。`experiments/rustc-driver-poc/`を`src/lib.rs`(共有基盤)+`src/main.rs`+`src/bin/{dyn_trait_check,lifetime_check,layout_check,unsafe_check}.rs`という構成に再編し、各機能領域を独立ファイルとして実装した(計26テスト、全て合格)。

### 5.5.1 トレイトオブジェクト・動的ディスパッチ(`dyn Trait`)

`experiments/rustc-driver-poc/src/bin/dyn_trait_check.rs`。決定的な発見: **MIRテキストレベルでは動的ディスパッチと未解決の静的ジェネリクス呼び出しがほぼ区別できない**——`<dyn Shape as Shape>::area(copy _1)`と`<T as Shape>::area(copy _1)`という、構文上ほぼ同一の形で出力される。両者の違いは`Ty<'tcx>`の実際の型カインド(`TyKind::Dynamic` vs `TyKind::Param`)にのみ存在し、`mir_text`のようなテキストパーサーでは原理的に区別不可能であることを実証した。

さらに`tcx.vtable_entries(trait_ref)`(`compiler/rustc_middle/src/ty/vtable.rs`)を実際に呼び出し、`Circle`が`Shape`を実装した場合の実vtableが`[MetadataDropInPlace, MetadataSize, MetadataAlign, Method(<Circle as Shape>::area)]`という4エントリ(先頭3つは固定のメタデータヘッダ、4番目が実メソッド)であることを確認した。当初「vtableはメソッドポインタから始まる」という誤った想定を持っていたが、実際の`VtblEntry` enum定義を読んで訂正した。これはtarget固有バックエンドが動的ディスパッチをサポートする際に必須となる、実際のvtableレイアウト情報である。

### 5.5.2 複数借用の絡み合い(ライフタイム)とPoloniusへの実アクセス可否

`experiments/rustc-driver-poc/src/bin/lifetime_check.rs`。重要な訂正: 以前の節5.3の記述は`tcx.mir_borrowck`がリージョン制約情報を含意しているかのように書いていたが、実際のクエリ定義(`compiler/rustc_middle/src/queries.rs`)を確認したところ、`mir_borrowck`の戻り値は`Result<&'tcx FxIndexMap<LocalDefId, ty::DefinitionSiteHiddenType<'tcx>>, ErrorGuaranteed>`——**opaque型のhidden type推論結果のマップ**であり、リージョン制約グラフとは無関係だった。この誤りを正直に訂正した。

前回研究(節2.5)が言及した`LocalizedConstraintGraph`(Polonius定式化)への、`Callbacks`経由の直接アクセス経路は本セッションでは見つからなかった。代わりに、`-Z nll-facts`という別のコンパイラフラグ(`Callbacks`フックとは独立したプロセスレベルの機構)を実際に実行し、`(region, region, point)`という実際のoutlives制約ファクトを取得した。`fn first_or_second<'a>(cond: bool, x: &'a i32, y: &'a i32) -> &'a i32`という、3引数が同じライフタイム`'a`を共有する関数に対し、`'?1`(共有`'a`)と`'?4`/`'?5`(各引数の内部リージョン)の間の相互outlives制約が、`Start(bb0[0])`/`Mid(bb0[0])`という実際のCFG上の点ごとに記録されていることを確認した。これは前回研究が言及したPolonius入力データの実例だが、`rustc_driver::Callbacks`ベースのパイプラインへの統合はできていないことを正直に記録する。

また、異なる変数への複数`&mut`(合法)と、同一変数への複数`&mut`(E0499で拒否)の両方を実際にコンパイルし、前者は成功・後者は失敗することを確認した。

### 5.5.3 構造体・クロージャのメモリレイアウト

`experiments/rustc-driver-poc/src/bin/layout_check.rs`(計6テスト)。`struct Reordered { a: u8, b: u64, c: u8 }`(デフォルトの`#[repr(Rust)]`)が実際に24バイトではなく16バイトに縮小され、`b`(u64)がオフセット0へ並び替えられることを`TyAndLayout::fields()`(`FieldsShape::Arbitrary`)で実測した。`#[repr(C)]`版は宣言順を保持し24バイトのままであることも対比確認した。さらに独立した`-Z print-type-sizes`フラグでも同じ結果を再確認しており、単一の測定手段だけに依存していない。

`Option<&i32>`(参照型、null不可)がニッチ最適化により追加の判別子バイトなしで8バイトのまま(`TagEncoding::Niche`)である一方、`Option<i32>`は`i32`に無効なビットパターンが存在しないため実際の判別子フィールドを持つ(`TagEncoding::Direct`)ことも確認した。当初「`Option<i32>`は8バイトを超えて成長するはず」という仮説を持っていたが、実測で誤りと判明し、正直に記録している。クロージャのキャプチャ環境(`move || x + y`の`x: i32, y: i64`)も実際には匿名の構造体型として表現され、通常の構造体と同じフィールド並び替え最適化(16バイト、24バイトではない)を受けることを確認した。

### 5.5.4 unsafe/生ポインタ: 前回研究節5.6.3の実証

`experiments/rustc-driver-poc/src/bin/unsafe_check.rs`(計4テスト)。前回研究の主張——「`unsafe`はborrow checkingを無効化しない、`check_unsafety`と`mir_borrowck`は独立クエリである」——を、本セッションで初めて実際に動くコードとして実証した。`unsafe`ブロックの内側に借用違反(`&mut`の二重取得)を配置しても依然としてE0499で拒否されること、生ポインタのデリファレンスは`unsafe`なしではE0133で拒否され`unsafe`ブロック内でのみ許可されることの両方を実データで確認した。また、生ポインタ型(`*const i32`)は既存の`Ownership`分類(`Unique`/`Shared`/`Boxed`)のいずれにも該当せず`NotAReference`に分類されることを確認し、これが「生ポインタには型システムによるエイリアシング保証が一切ない」という事実を正しく反映した、意図的なスコープ限定であることを明記した。

### 5.5.5 総合的な限界(正直な記録)

- vtableエントリ(5.5.1)は取得できたが、これを実際に`target_ir`のx86_64コード生成(間接呼び出し命令の発行)へ接続する作業は未着手。
- Polonius定式化への直接アクセス(5.5.2)は`-Z nll-facts`という別プロセスフラグ経由でのみ確認でき、`rustc_driver::Callbacks`ベースの統合パイプラインには組み込めていない。
- レイアウト情報(5.5.3)は構造体・クロージャについて実測できたが、これを`target_ir`のスタックフレーム設計へ接続する作業は未着手。
- unsafe/生ポインタ(5.5.4)の検証は、既存の`Ownership`分類の限界を明確にしただけであり、生ポインタを安全に扱うための新しい分類軸の設計は行っていない。
- 4つの検証は全て独立した`src/bin/*.rs`ファイルであり、相互の統合(例: 動的ディスパッチ+複数借用が同時に絡む関数)は未検証。

## 5.6 4度目のユーザー指摘への応答: 独立レビュアーによる徹底的批判とその是正

ユーザーから再度「本当に『Rustでの価値』を検証しているか」という問いを受け、今回はこのセッションの文脈を一切持たないフレッシュなサブエージェントに、コード・設計文書のみを一次資料として渡し、独立レビューを依頼した。結論は「部分的」——機構(x86_64直接lowering)は動くが、「rustcから取得した型情報が実際にコード生成の質を左右した」という主張の裏付けは、当時`lower_double_load_to_code_body`(節5.4.2)というたった1つの、しかも恣意的な例に留まっていた、という指摘だった。

### 5.6.1 指摘の核心: `lower_double_load_to_code_body`は正当な最適化ではなく人工的な差にすぎない

独立レビュアーの指摘を要約する。「`Ownership::Shared`(`&i32`)の2回ロードは、実際のRustの`&T`不変性保証(1回のロードで十分)を活かしきっていない保守的な実装であり、『2つの分岐が視覚的に異なる出力になることだけを保証する最小実装』と自己申告している。すなわちこの唯一の実例は、実際のRust最適化としての正当性を持たない、デモのためだけに作られた人工的な差である」。これは正しい。前回(節5.4.2)の自分自身の記述も、実はこの欠陥を暗に認めていた(`target_ir.rs`のdoc commentが「a manufactured pessimization」という言葉を既に使っていた)にもかかわらず、それを「まだ解けていないこと」として先送りするのではなく「解決済みの接続」として提示してしまっていた。

さらにレビュアーは、26テスト中「型が分類できた」で終わるものと「生成バイト列/実行結果が正しい」まで踏み込むものを区別し、後者は`target_ir.rs`内2件のみ(全体の1割未満)であることを指摘した。加えて、4機能検証(dyn Trait/lifetime/layout/unsafe)がそれぞれ独立バイナリのままで一切統合されていないことも、issue #68の本来の目的(target固有中間表現の設計)からの逸脱として指摘された。

### 5.6.2 対応: 正当な対比への置き換えと、分岐+所有権の実統合

**最優先提案(分岐とOwnershipを統合する)を実装した。** `experiments/unified-symbol-graph/src/target_ir.rs`に新関数`lower_load_or_reload_to_code_body`を追加し、`Ownership`列挙型を`Unique`/`Shared`/`Boxed`/`NotAReference`の4値へ拡張した(`experiments/rustc-driver-poc`の`Ownership`分類と1対1で対応させた)。

この新関数がモデル化するのは、実際のRustコード`fn f(p: *const i32) -> i32 { if *p != 0 { *p } else { -*p } }`のような、**分岐の両アームで同じメモリ位置を再度読む**関数である。今回の対比は、旧関数の欠陥(恣意的な悲観化)を修正した、正当な区別に置き換えた:

- `Unique`/`Shared`/`Boxed`(型システムがエイリアシングを保証する3種)は**全て同一**の、1回ロード+キャッシュ済みレジスタを両アームで再利用するコードを生成する(`&T`の不変性保証は`&mut T`/`Box<T>`と同じ強さでこの最適化を正当化するため、`Shared`を人為的に劣化させることはやめた)。
- `NotAReference`(型システムによるエイリアシング保証が一切ない生ポインタの類推)のみ、各テキスト上のアクセスごとにメモリから再読み込みする、より長いコードを生成する。

これを`objdump`でバイト単位検証し(`je`相対オフセットの計算ミスを2箇所発見・修正した——後述)、さらに`experiments/unified-symbol-graph/examples/ownership_consumption_check.rs`で全4種のOwnership×5入力(`i32::MIN`/`MAX`含む)×両分岐方向を実際に`mmap`実行し、全て正しい結果を返すことを確認した。

さらに`experiments/rustc-driver-poc/src/main.rs`に新しい統合関数`compile_load_or_reload_function_respecting_borrowck_gate`を実装し、実際の4つのRustソース(`&mut i32`/`&i32`/`Box<i32>`/`*const i32`、いずれも`fn f(p: ...) -> i32 { if *p != 0 { *p } else { -*p } }`という同一の意味論)をrustc内部APIでborrow check・型検査した上で、`unified-symbol-graph`の分岐統合コード生成へ渡した。実行結果: `&mut i32`/`&i32`/`Box<i32>`の3つは全てバイト単位で同一の(短い)コードを生成し、`*const i32`のみ異なる(長い)コードを生成することを確認した(`real_ownership_reaches_the_branch_integrated_codegen_and_distinguishes_raw_pointers`テスト)。これは旧`compile_pointer_function_respecting_borrowck_gate`(節5.4.2、`Boxed`を`Unique`へ縮退させ`NotAReference`を拒否していた)よりも忠実な統合であり、4値全てを個別に扱う。

### 5.6.3 レビュー中に発見・修正した実バグ: `je`命令の相対オフセット計算ミス

`lower_load_or_reload_to_code_body`の実装過程で、`objdump`による検証が2箇所の実際のオフバイエラーを検出した。1つ目は`Unique`等の共通パス(`je +2`と書いたが、スキップすべき`ret`は1バイトのみのため正しくは`je +1`)、2つ目は`NotAReference`パス(`je +4`と書いたが、正しくは`je +3`)である。両方とも「`objdump`の逆アセンブル結果が着地点をずらして表示する」という直接的な兆候から発見し、修正後に再度`objdump`で正しい着地を確認した。これは本セッションが「バイト列を手で組み立てる」という方式自体に内在するリスク(オフバイエラーが混入しやすい)を裏付ける実例であり、正直に記録する。

### 5.6.4 この対応でも解消されていないこと(正直な記録)

- 独立レビュアーが指摘した「26テストの9割が疎通確認に留まる」という比率の問題は、今回の対応(分岐+所有権の統合を1件追加)だけでは大きく改善していない。vtable(節5.5.1)・Polonius facts(節5.5.2)・レイアウト情報(節5.5.3)は依然として`target_ir`へ未接続のままである。
- 4機能検証(dyn Trait/lifetime/layout/unsafe)の相互統合(レビュアー提案3: 複合ケースの実接続)はまだ着手していない。
- `lower_load_or_reload_to_code_body`自体も、実際のStacked/Tree Borrowsレベルの精密さ(前回研究節3.3)には遠く及ばない、`i32`単一型・1ポインタ引数のみの最小実装である。

## 5.7 5度目のユーザー指摘への応答: 文字列・配列・HashMap操作、および`Call`ターミネータ

ユーザーから「文字列、配列、ハッシュマップの操作全般についての検証は?」という指摘を受けた。境界チェック(bounds check)を`target_ir`へ実接続する方針で対応した。加えて、追加指示「これ等を含めた『リテラル』の検証もやって」を受け、リテラルのMIR表現も併せて確認した。

### 5.7.1 スライス境界チェックの実データと`TargetIr`への接続

`pub fn get_elem(s: &[i32], i: usize) -> i32 { s[i] }`の実MIRを取得した結果、`_3 = PtrMetadata(copy _1); _4 = Lt(copy _2, copy _3); assert(move _4, "index out of bounds...", ...) -> [success: bb1, unwind continue];`という構造を確認した。これは`compiler/rustc_middle/src/mir/syntax.rs`で定義される`TerminatorKind::Assert { cond, expected, msg, target, unwind }`であり、既存の`target_ir::Terminator`(`Return`/`Branch`のみ)には存在しない、新しいターミネータ種別である。

`Vec<i32>`の`v[i]`は`<Vec<i32> as Index<usize>>::index(...)`という**トレイト呼び出し**へ脱糖され、境界チェック自体は標準ライブラリの`index`関数内部に隠れて呼び出し元のMIRには現れないことも確認した。境界チェックを直接MIRレベルで観測するには、生スライス経由(`&[i32]`)が必要である。

`experiments/unified-symbol-graph/src/target_ir.rs`に`lower_bounds_checked_slice_index_to_code_body`を実装した。これは実際に`rustc -O --emit=asm`が`pub extern "C" fn get_elem(ptr: *const i32, len: usize, i: usize) -> i32`に対して生成する呼び出し規約(`ptr`=`rdi`、`len`=`rsi`、`i`=`rdx`)と`cmp %rsi, %rdx; jae <fail>; mov (%rdi,%rdx,4),%eax; ret`という命令列を、実際に`rustc -O --emit=asm`の出力から確認した上で再現した。ただし本実装は、実際のパニック/unwind機構(前回研究のスコープ外)を実装する代わりに、範囲外アクセス時に固定センチネル値(`i32::MIN`)を返すという意図的な簡略化を行っている——これは「範囲外アクセスからの正しい回復」ではなく「範囲外アクセスの検出」のみを主張する、と明記した。

`objdump`によるバイト単位検証、および`examples/bounds_check_check.rs`による`mmap`実行検証(5要素配列に対し、範囲内0〜4は正しい要素値、範囲外5・6・105は全てセンチネル値、メモリ範囲外への読み取りが実際に発生しないことを確認)を実施した。さらに`experiments/rustc-driver-poc`側にも`assert_terminator_count`という新しいカウンタを追加し、実際の`get_elem`関数が`TerminatorKind::Assert`をちょうど1個生成することを実データで確認した(`real_slice_index_lowers_to_a_real_assert_terminator`テスト)。

### 5.7.2 文字列・スライスのfat pointer表現とリテラルの実データ

`"hello"`という文字列リテラルの実MIRを取得した結果、`_0 = const "hello";`という一見単純な代入の裏に、`alloc1 (size: 5, align: 1) { 68 65 6c 6c 6f │ hello }`という**独立したメモリ内容(実バイト列)の宣言**が存在することを確認した。`&'static str`は`(ptr, len)`のfat pointerであり、リテラルの実体は別領域に確保され、変数はそこへのポインタ+長さを持つ。バイト文字列リテラル`b"bytes"`は`&[u8; 5]`(固定長配列)として確保された後、`PointerCoercion(Unsize, Implicit)`という明示的な型強制変換を経て`&[u8]`(可変長スライス)へ変換されることも確認した。数値リテラル(`42_i32`)・浮動小数点リテラル(`3.14f64`が実際には`3.1400000000000001f64`という丸め誤差を伴う内部表現になる)・真偽値・文字リテラルは、いずれも単純な`const`定数として`Operand::Const`相当の形に収まり、既存の`target_ir::Operand::Const`が既にこの構造に対応済みであることを確認した。配列リテラル`[1, 2, 3]`は`Aggregate` Rvalue(`_0 = [const 1_i32, const 2_i32, const 3_i32];`)として表現される。

`.len()`が実際には計算ではなく、fat pointerの第2ワードの直接読み取りであることを、実際の`rustc -O --emit=asm`出力(`str_len`関数が`movq %rsi, %rax; retq`というゼロ命令本体にコンパイルされる)で確認し、`lower_str_or_slice_len_to_code_body`として実装した。空文字列を含む安全な先頭バイトアクセス(`if s.is_empty() { 0 } else { s[0] }`)も、実際の`rustc -O --emit=asm`出力(`testq %rsi,%rsi; je .empty; movzbl (%rdi),%eax; ret; .empty: xorl %eax,%eax; ret`)を再現する`lower_first_byte_or_zero_to_code_body`として実装した。`examples/string_slice_check.rs`で、`"hello"`・空文字列・`"a"`・複数語の文字列・日本語(`"文字列"`、UTF-8マルチバイト、9バイト)を含む実データで、両関数の実行結果を検証した。

### 5.7.3 HashMapの決定的な発見: 新しいMIR構造を追加しない、`Call`ターミネータが本質

`use std::collections::HashMap; pub fn get_or_default(m: &HashMap<i32, i32>, k: i32) -> i32 { match m.get(&k) { Some(v) => *v, None => -1 } }`の実MIRを取得した結果、**`HashMap::get`は単一の`TerminatorKind::Call`(`_3 = HashMap::<i32, i32>::get::<i32>(copy _1, copy _4) -> [return: bb1, unwind continue];`)へ脱糖され、その戻り値`Option<&i32>`に対する処理は、既に節5.5.1で検証済みの`discriminant`読み取り+2分岐`switchInt`という、enumの`match`と全く同じ構造であることが判明した**。ハッシュ計算・バケット探索・衝突解決といったHashMap特有の処理は、全て呼び出し先(標準ライブラリの`get`関数本体)の内部にあり、呼び出し元のMIRには一切現れない。

この発見は、issue #68の「HashMapの検証」という問いに対する誠実な答えを与える: **HashMapのアクセス自体は、呼び出し元のMIRレベルでは何も新しい構造を要求しない**。真に新しい構造は、この呼び出し自体を実現する`TerminatorKind::Call`である。`compiler/rustc_middle/src/mir/syntax.rs`で確認した`Call { func, args, destination, target: Option<BasicBlock>, unwind, call_source, fn_span }`は、既存の`target_ir::Terminator`が扱っていない、実際のx86_64`call`命令に対応する構造である。

`lower_call_and_increment_to_code_body`を実装し、issue #67で確立済みの`ElfX86_64PendingReloc`/`CodeBody`/`SharedSymbolGraph::apply_elf_x86_64_relocations`という既存の再配置機構を**そのまま再利用**して(新しい再配置の仕組みを発明せず)、実際に外部の別コンパイル済み関数を`call rel32`で呼び出し、戻り値に`+1`するコードを生成した。

`examples/call_relocation_check.rs`で、`SharedSymbolGraph`に呼び出し元・トランポリン(実関数`triple`への`jmp`)の両方を宣言し、`assign_layout`+`apply_elf_x86_64_relocations`で実際にrelocationをパッチし、その結果を`mmap`で実行して、別コンパイルされた実関数を正しく呼び出せることを実証した。

**この実装過程で発見・修正した2つの実際の問題**:

1. **診断の誤り**: relocationパッチ後のバイト列が全てゼロのままに見え、当初「relocationパイプラインのバグ」と誤診断した。実際には、そのレイアウト(呼び出し元オフセット0、呼び出し先オフセット9)でPC相対変位を手計算すると、真の値が偶然`0`になる(`(9-4)-(0+1+4)=0`)ことが原因であり、「ゼロでないことを確認する」という検証方法自体が誤りだった。独立した計算式による再検証(`lib.rs`の既存テストと同じ規律)に置き換えて解消した。
2. **実際のバグ**: `call rel32 = 0`は「次の命令へのフォールスルー」を意味するため、呼び出し元とトランポリンのオフセットが近すぎると、実際に呼び出しが発生せず(no-op)、それでも何らかの値(直前の呼び出しの残骸)を返してしまうという、**もっともらしく見えるが誤った実行結果**を生んでいた。2つのシンボルの間に十分な間隔を持つパディング用シンボルを追加することで解消した。

いずれも「実際に動かして確認する」という規律がなければ発見できなかった問題であり、正直に記録する。

### 5.7.4 この対応でも解消されていないこと(正直な記録)

- `HashMap`自体の内部実装(ハッシュ関数、バケット、衝突解決)は一切検証していない——節5.7.3の発見が示す通り、これは呼び出し元のMIRには現れないため、検証するには標準ライブラリの`HashMap::get`関数自体のMIR/実装を追う必要があり、本セッションのスコープ外とした。
- `lower_call_and_increment_to_code_body`は固定で1つの`i32`引数を取る外部関数呼び出しのみを扱う。複数引数、複数の戻り値型、可変長引数、トレイトオブジェクト経由の動的呼び出し(節5.5.1のvtable)は未接続のままである。
- 文字列操作(`lower_str_or_slice_len_to_code_body`/`lower_first_byte_or_zero_to_code_body`)は`.len()`と安全な先頭バイトアクセスのみ。文字列比較、UTF-8境界検証、`String`の可変操作(push、resize、reallocation)は未検証。
- 配列/`Vec`のリテラル(`Aggregate` Rvalue)自体を`target_ir`のコード生成で実際に構築する(スタック上に複数要素を書き込む)処理は未実装。

## 5.8 6度目のユーザー指摘への応答: 境界チェック機能でのLLVM IRとの「省エネ」再比較

ユーザーから「上記の機能を追加しました。再度、LLVM IRとの『省エネ』を比較して」という指摘を受け、節5.2で`branch`関数に対して行った3観点比較(コンパイル時間・中間成果物サイズ・実行時性能)を、今回追加した`lower_bounds_checked_slice_index_to_code_body`(境界チェック付きスライスインデックス、`pub fn get_elem(s: &[i32], i: usize) -> i32 { s[i] }`)に対して再実施した。

### 5.8.1 コンパイル時間: 前回と同じ限界を再確認

`get_elem`関数に対し`rustc --emit=mir`(LLVM未呼び出し)と`rustc --emit=obj -O`(LLVM経由)を5回ずつ実行した。中央値はそれぞれ0.140秒・0.122秒であり、前回の`branch`関数と同様、この規模ではrustc起動オーバーヘッドが支配的で、LLVM自体の処理時間差を検出できないことを再確認した。**新しい発見はなく、節5.2.1の判定が別の関数でも再現することを確認しただけである。**

### 5.8.2 中間成果物サイズ: 今回はLLVM側が本物のパニック機構を含むため、前回よりも差が拡大

`rustc --emit=llvm-ir -C opt-level=0`で実際のLLVM IRテキストを取得したところ、2118バイトだった(前回の`branch`関数は1139バイト)。この増加分の主因は、LLVM側が**実際のパニック機構**を含んでいることである——生成されたLLVM IRには`call void @...panic_bounds_check(...)`という本物の外部関数呼び出しと、ソースファイルの絶対パス・行番号情報を含む定数データ(`@alloc_...`、182バイトの文字列を含む)が埋め込まれている。一方、本設計の`lower_bounds_checked_slice_index_to_code_body`が生成する機械語バイト列は15バイトのみである(パニック機構を実装せず、範囲外アクセス時に固定センチネル値`i32::MIN`を返す簡略実装のため)。

**この比較は前回よりも一層公平ではないことを明記する**: 本設計はパニック/unwind機構(前回研究のスコープ外、節5.7.1で明記済み)を実装しておらず、LLVM側は実際にRustの完全なパニック意味論(ソース位置情報の保持、unwindを含む)を実装している。サイズの差は「同じ機能をより効率的に実装した」結果ではなく、「実装している機能の範囲が異なる」ことの反映である。

### 5.8.3 実行時性能: 成功パスの命令列は完全に一致するが、実測では本設計側がわずかに遅い

`objdump`による逆アセンブルの結果、驚くべき事実が判明した: LLVMが生成する成功パス(境界内アクセス)の命令列——`cmp %rsi,%rdx; jae <fail>; mov (%rdi,%rdx,4),%eax; ret`——は、本設計の`lower_bounds_checked_slice_index_to_code_body`が生成する命令列と**バイト単位で完全に一致していた**(`48 39 f2 73 04 8b 04 97 c3`)。これは本設計のコード生成が、少なくともこの成功パスに関しては、LLVMの最適化されたコード生成と同等であることを示す、前回の`branch`関数の実験にはなかった結果である。

実行時性能を実測した結果は次の通り(境界内アクセスのみを1億回実行、5回試行):

| 実行 | LLVM(`rustc -O`) | 本設計(直接lowering) |
| --- | --- | --- |
| 1 | 142.1ms | 164.4ms |
| 2 | 144.5ms | 165.3ms |
| 3 | 143.8ms | 172.7ms |
| 4 | 143.0ms | 169.9ms |
| 5 | 132.1ms | 179.8ms |

**成功パスの命令列がバイト単位で同一であるにもかかわらず、本設計側が一貫して約15〜25%遅い**。両者とも累積値`3000000000`で完全に一致しており(正しさは確認済み)、差は関数呼び出し自体のオーバーヘッドに起因すると考えられる——本設計側は`extern "C" fn`型の関数ポインタを介した間接呼び出し(`mmap`されたメモリ上のコードを、実行時に取得したポインタ経由で呼ぶ)であるのに対し、LLVM側は同一バイナリ内の直接`call`命令である。この差は「生成された命令列そのものの質」の差ではなく、「生成されたコードをどう呼び出すか」という、この実験のベンチマーク手法自体に起因する可能性が高い。今回はこの切り分けまでは行っておらず、正直に「本設計のコード生成自体はLLVMと同等の命令列を生成できるが、実測ではベンチマーク手法に起因すると見られる差が残った」とだけ記録する。

### 5.8.4 総合: 前回の判定を補強しつつ、新しい知見(成功パスの命令列一致)を追加

| 観点 | 前回(`branch`関数) | 今回(`get_elem`関数) |
| --- | --- | --- |
| コンパイル時間 | 未確定(測定不能) | **未確定(測定不能、再確認)** |
| 中間成果物サイズ | 本設計が有利(約10分の1) | **本設計が有利(約141分の1)だが、実装機能範囲の違いによる差** |
| 実行時性能 | LLVMが有利(約2.7〜3.0倍、最適化の有無による) | **LLVMがわずかに有利(約1.15〜1.25倍)だが、成功パスの命令列自体は同一** |

前回の`branch`関数実験では、本設計がLLVMより約2.7〜3.0倍遅かったのは「LLVMが分岐除去等の最適化を行い、本設計は最適化を一切持たない」という、命令列レベルでの実質的な差が原因だった。今回の`get_elem`関数実験では、命令列自体はバイト単位で一致しているにもかかわらず性能差が残っており、これは前回とは異なる性質の差(呼び出しオーバーヘッド起因の可能性)である。この違いは、単純な分岐+定数returnという最小のケース(前回)と、より実際のRustコードに近い境界チェック付きインデックスアクセス(今回)とで、LLVMとの性能差の**原因が異なりうる**ことを示しており、「LLVM IRを削除すれば常に一定の性能差が生じる」という単純な一般化はできないことを裏付ける。

## 5.9 仮説検証の観点での見直し: H1〜H6への分解と、H4(CFG不動点計算モデル)の実装

ユーザーから、issue #68を仮説検証として捉え直す指摘を受けた。要旨: 前回研究がLLVM IR削除の根拠にした「noalias再エンコード問題」の具体的内容(rustc自身の`PointerKind::SharedRef { frozen }`が`Freeze`条件を持つこと)と対比した場合、本設計の`Ownership`タグは「置き換え対象より豊かか」を判定できておらず、これまでの検証(節5.1〜5.8)は「2ブロックのCFGをLLVM IRなしでx86_64バイト列にできる」という、Cranelift/QBE/TinyCCが既に示している自明な存在証明に留まっていた。issue #68が実際に問うた3つの争点——(1)所有権情報を属性再エンコードなしで保持できるか、(2)単一層のtarget固有表現で済むか、(3)CFG上の不動点計算モデルを維持できるか——のいずれも、真に試される場所(合流点をまたぐ値の生存、ループ)を実験から意図的に除外していたため、判定不能だったという指摘である。

この指摘を受け、仮説を次のように分解し、優先順位を付けて検証する。

| 検証すべき主張 | 状況(このユーザー指摘の時点) |
| --- | --- |
| H1: MIR相当→x86_64をLLVM IRなしで接続できる | 示されたが自明。争点ではない |
| H2: 所有権をタグとして保持すればnoalias再エンコードの欠陥を回避できる | 未検証。`Ownership::Shared`が`Freeze`条件を欠く、むしろ反証あり |
| H3: Craneliftの2層構造を消せるか | 判定不能。構造を追加するたびに別関数を書いており、単一IRが複数の構造に耐えるかを試していない |
| H4: CFG不動点計算モデルの維持 | 未検証。合流点(diamond)とループが実験から除外されていた |
| H5: `AddressState`/`SemanticFacts`との統合 | 未着手 |
| H6: 8 target への分岐 | 未着手 |

本節では、この分解のうちH4(最優先)を実装・検証した。H2・事前登録・H3・効率比較の再検証は、以降の節で順次記録する。

### 5.9.1 H4の実装: diamond CFGとループの実データ確認

`pub fn diamond(param0: i32) -> i32 { let x = if param0 != 0 { param0 + 1 } else { param0 - 1 }; x * 2 }`の実MIRを取得した結果、次の構造を確認した:

```text
bb0: { _3 = Ne(copy _1, const 0_i32); switchInt(move _3) -> [0: bb3, otherwise: bb1]; }
bb1: { _2 = ...(param0+1)...; goto -> bb5; }
bb3: { _2 = ...(param0-1)...; goto -> bb5; }
bb5: { _6 = copy _2; ...(_6 * 2)...; return; }
```

`_2`という単一のローカル変数が**両方の分岐アーム(bb1/bb3)で書き込まれ、合流点(bb5)で読み出される**という、ユーザー指摘が「仮説が試される唯一の場所」と呼んだ構造そのものである。同様に`pub fn countdown(param0: i32) -> i32 { let mut n = param0; let mut acc = 0; while n > 0 { acc += n; n -= 1; } acc }`の実MIRから、`bb4: { goto -> bb1; }`という**真の後方分岐(back-edge)**を確認した——これは既存の`Terminator::Branch`(前方分岐のみ、各アームが独立してreturnする)では一切表現できない構造である。

### 5.9.2 実装: `target_ir::diamond_and_loop_cfg`

既存の`Terminator`/`lower_target_ir_to_code_body`(2ブロック限定、合流点なし)は変更せず、新しい独立サブモジュール`diamond_and_loop_cfg`を実装した。設計判断:

- **スタックスロットによる値の受け渡し**: 合流点をまたいで生存する値は、固定の`rbp`相対スタックスロット(`LocalId`)に格納する。これはLLVM IRが`mem2reg`最適化パスで行うSSA昇格以前の、素朴だが正しい方式であり、レジスタ割り当てという別の難しい設計問題には踏み込まないことを明記した。
- **2パスレイアウト**: ブロックの並び順を保持したまま、まず各ブロックのバイト長を計算し(パス1)、その後全ての`Goto`/`Branch`ターゲットを実際の相対オフセットへパッチする(パス2)。これは既存の1パス実装(前方分岐のみを前提)では対応できない、ループのback-edge(プログラム順で**前方の**ブロックへ戻る分岐)を正しく解決するために必須の変更である。
- `Terminator::Goto(BlockId)`を新設(既存の`Terminator`にはなかった無条件分岐)。

`examples/diamond_and_loop_check.rs`で、diamond構造(7入力: 0, 1, -1, 5, -5, 100, -100)とループ構造(5入力: 0, 1, 5, 10, 100)の両方を実際に`mmap`実行し、対応する実Rust関数の結果と全て一致することを確認した。`countdown(100)=5050`(ガウスの公式`100*101/2`と一致)を含む。

`objdump`による独立検証で、diamond構造の`je`/`jmp`双方が正しいブロック開始点へ着地し、合流点が両アームが書き込んだ**同一のスタックスロット**(`[rbp-0x10]`)を読み出していることを確認した。ループ構造では、`jmp 0x36`という**真の後方分岐**(ループヘッダへ戻る)を確認した——これは前回までのどの実装にも存在しなかった、質的に新しい命令パターンである。

テストとして、diamond構造の全バイト列(`objdump`検証済みの値と完全一致)と、ループのback-edgeが独立した位置特定ロジック(実装自身の計算式をミラーしない、固定バイトパターンでの位置特定)で正しいヘッダー位置へ着地することを固定化した。

### 5.9.3 この実装が示すこと、示さないこと

**示すこと**: 本設計のスタックスロット方式は、diamond構造とループという、CFG上の不動点計算モデル(前回研究節2.2、drop elaboration/borrow checkingが要求する構造)が実際に要求する2つの基本パターンに対して、正しく動作するx86_64コードを生成できる。これは前回までの2ブロック限定実装では判定不可能だった主張である。

**示さないこと**: 
- レジスタ割り当て(全ての値を毎回メモリへ読み書きする、素朴で非効率な方式のまま)。
- 実際のdrop elaboration/borrow checkingが要求する、より複雑な不動点計算(前回研究節2.2が引用した`MaybeInitializedPlaces`/`MaybeUninitializedPlaces`のような、複数の合流点にまたがる複雑なデータフロー解析)。今回実装したのは最小の2パターン(1つのdiamond、1つの単純ループ)のみである。
- オーバーフローチェック(実MIRの`AddWithOverflow`+`assert`)は意図的にモデル化せず、算術は無条件にラップする——この点はテスト自体のドキュメントコメントで明記している。
- `Ownership`タグとの統合(diamond/ループの値がOwnership情報を持つケース)はまだ実装していない。

## 5.10 H2(所有権情報の再エンコード回避)の検証: `Ownership::Shared`の反証と修正

節5.9の分解に従い、H2(所有権をタグとして保持すればnoalias再エンコードの欠陥を回避できるか)を検証した。結論: **反証された**。前回までの`Ownership::Shared`は、置き換え対象であるLLVMのnoalias導出よりも貧弱な語彙で同じ問題を再生産していた。

### 5.10.1 反証の内容: rustc自身の`PointerKind`との実データ比較

`compiler/rustc_abi/src/lib.rs`で実際の`PointerKind`定義を確認した:

```rust
pub enum PointerKind {
    /// Shared reference. `frozen` indicates the absence of any `UnsafeCell`.
    SharedRef { frozen: bool },
    /// Mutable reference. `unpin` indicates the absence of any pinned data.
    MutableRef { unpin: bool },
    Box { unpin: bool, global: bool },
}
```

さらに、この情報を実際に消費する`arg_attrs_for_rust_scalar`(`compiler/rustc_ty_utils/src/abi.rs`366-370行)を確認した:

```rust
let no_alias = match kind {
    PointerKind::SharedRef { frozen } => frozen,
    PointerKind::MutableRef { unpin } => unpin,
    PointerKind::Box { unpin, global } => unpin && global && noalias_for_box,
};
```

**rustc自身は、素の`&T`に対して無条件に`noalias`を付与しない**。`frozen`(pointeeのどこにも`UnsafeCell`が存在しないこと)が真である場合のみである。前回までの`target_ir::Ownership::Shared`はこの条件を一切持たず、全ての`&T`を無条件にキャッシュ可能とみなしていた——これは`&Cell<i32>`(`UnsafeCell`を内部に持つ)のケースで**不健全**(誤った結果を生みうる)であり、単なる非効率ではない、実際の正しさの欠陥だった。この事実は`lower_load_or_reload_to_code_body`(節5.6.2)の当時のdoc comment自身が「Rust's own `&T` immutability guarantee」と誤って一般化して書いていたことでも裏付けられる。

### 5.10.2 修正: `Ownership::Shared`を`Shared { frozen: bool }`へ拡張

`experiments/unified-symbol-graph/src/target_ir.rs`の`Ownership::Shared`を`Shared { frozen: bool }`へ拡張し、`experiments/rustc-driver-poc/src/lib.rs`の`ownership_from_real_ty`を、実際の公開API`Ty::is_freeze(tcx, typing_env)`(`compiler/rustc_middle/src/ty/util.rs`で公開が確認済み)から`frozen`値を取得するよう修正した。`lower_load_or_reload_to_code_body`のロジックも修正し、`frozen: true`の場合のみ`Unique`/`Boxed`と同じキャッシュパスを取り、`frozen: false`の場合は`NotAReference`と同じ再ロードパスを取るようにした。

### 5.10.3 実証: 実際の`&Cell<i32>`が異なる(かつ正しい)コードへ到達する

`experiments/rustc-driver-poc/src/main.rs`に、実際の`&i32`関数と実際の`&Cell<i32>`関数(`p.get()`経由でCellから値を読む)を両方ともrustc内部API経由でborrow check・型検査し、`unified-symbol-graph`のコード生成へ渡すテストを実装した。結果: **`&i32`(frozen: true)と`&Cell<i32>`(frozen: false)は異なるバイト列のコードへ到達する**ことを確認した(`real_cell_shared_reference_reaches_different_codegen_than_a_real_frozen_shared_reference`テスト)。

さらに`examples/ownership_consumption_check.rs`に`Ownership::Shared { frozen: false }`のケースを追加し、`mmap`実行で正しい結果(`i32::MIN`を含む5入力)を返すことを確認した。

### 5.10.4 この修正が示すこと、示さないこと

**示すこと**: 本設計の`Ownership`タグは、少なくとも`frozen`という1つの軸について、rustc自身が実際に使う語彙と同等の精度を持つよう修正できた。これは節5.9が指摘した「置き換え対象より貧弱な語彙で同じ再エンコード問題を縮小再生産している」という反証への直接的な応答である。

**示さないこと**: 
- 前回研究(節3.3)が代替として挙げたStacked/Tree Borrowsのタグ+木構造は、依然として実装していない。今回の修正は「`frozen`という1ビットの情報を追加した」だけであり、Tree Borrowsが提供するような、借用の生存期間・階層関係を追跡する仕組みには程遠い。
- `MutableRef { unpin }`の`unpin`条件、`Box { unpin, global }`の`unpin`/`global`条件は、まだ`Ownership::Unique`/`Boxed`に反映していない——これらも同様の反証を受ける可能性がある(例えば`Pin<&mut T>`のケース)。
- `frozen`の判定は`Ty::is_freeze`という1つのクエリ呼び出しのみに依存しており、このクエリ自体がどこまで正確か(例えば、ジェネリック型パラメータを含む型に対する`is_freeze`の挙動)は検証していない。

## 5.11 事前登録: 表現カバレッジ行列と、以降の実験(H3・効率比較)の棄却条件

ユーザー指摘(節5.9)が要求した「成功基準の事前定義」を、H4・H2の実装が既に完了した時点で遡って記録する(本来は実装前に行うべきだったという指摘自体は正当であり、節6に反省として記録する)。以降の実験(H3・効率比較の再検証)については、実施前に本節へ棄却条件を追記してから着手する。

### 5.11.1 MIR表現カバレッジ行列(実装済み/未実装)

`compiler/rustc_middle/src/mir/syntax.rs`から実際に確認した`TerminatorKind`全14variantと`Rvalue`全12variantを対象に、`target_ir`クレート全体(`Terminator`/`diamond_and_loop_cfg::Terminator`/専用lowering関数群)での対応状況を棚卸しする。

**`TerminatorKind`(14 variant)**:

| Variant | 対応状況 | 備考 |
| --- | --- | --- |
| `Goto` | 対応済み | `diamond_and_loop_cfg::Terminator::Goto`(節5.9) |
| `SwitchInt` | 部分対応 | 2分岐(bool相当)のみ。`Terminator::Branch`(既存)。3分岐以上(`match`の複数バリアント、節5.5.1で実データ確認済み)は未対応 |
| `Return` | 対応済み | `Terminator::Return`/`diamond_and_loop_cfg::Terminator::Return` |
| `Assert` | 部分対応 | `lower_bounds_checked_slice_index_to_code_body`(節5.7.1)で1つの具体形状(境界チェック)のみ固定バイト列として実装。汎用的な`Terminator`のvariantとしては未統合 |
| `Call` | 部分対応 | `lower_call_and_increment_to_code_body`(節5.7.3)で1引数・1戻り値の固定形状のみ。汎用的な`Terminator`のvariantとしては未統合 |
| `UnwindResume` | 未対応 | パニック/unwind機構自体が本設計のスコープ外(前回研究、節5.7.1で明記) |
| `Unreachable` | 未対応 | |
| `Drop` | 未対応 | `rustc-driver-poc`側で`drop_terminator_count`として検出(節5.5.1)のみ。`target_ir`側のコード生成には未接続 |
| `TailCall` | 未対応 | |
| `Yield` | 未対応 | コルーチン/async関連、本設計のスコープ外 |
| `CoroutineDrop` | 未対応 | 同上 |
| `FalseEdge` | 未対応 | 借用チェック専用の疑似エッジ(実行時には存在しない) |
| `FalseUnwind` | 未対応 | 同上 |
| `InlineAsm` | 未対応 | |

**`Rvalue`(12 variant)**:

| Variant | 対応状況 | 備考 |
| --- | --- | --- |
| `Use` | 対応済み | `Operand::Const`/`Operand::Param0`、`diamond_and_loop_cfg::Rvalue::Copy`/`Const` |
| `BinaryOp` | 部分対応 | `Add`/`Sub`(`diamond_and_loop_cfg::Rvalue`)、比較(`NotEqualZero`/`GreaterThanZero`)のみ。乗算・除算・ビット演算は未対応 |
| `Discriminant` | 未対応 | `rustc-driver-poc`側で`discriminant_read_count`として検出(節5.5.1)のみ。`target_ir`側は未接続 |
| `Ref` | 未対応 | 借用の生成自体(`&x`/`&mut x`)。本設計は「既に借用として渡された値」の使用のみを扱い、借用の生成そのものは扱っていない |
| `Repeat` | 未対応 | 配列リテラル`[x; N]` |
| `ThreadLocalRef` | 未対応 | |
| `RawPtr` | 未対応 | 生ポインタの構築 |
| `Cast` | 未対応 | 型変換 |
| `UnaryOp` | 部分対応 | `neg`(`lower_load_or_reload_to_code_body`内、節5.6.2)のみ | 
| `Aggregate` | 未対応 | 配列/構造体リテラルの構築(節5.7.2で実MIR確認済みだが未実装) |
| `CopyForDeref` | 未対応 | |
| `WrapUnsafeBinder` | 未対応 | |

**総合評価**: `TerminatorKind`14種中、対応済み2種・部分対応3種・未対応9種。`Rvalue`12種中、対応済み1種・部分対応2種・未対応9種。カバレッジは低く、issue #68が要求する「target固有中間表現の設計」全体からすれば、ごく限られた部分集合の実証に留まっていることを正直に記録する。

### 5.11.2 以降の実験の棄却条件(事前登録)

**H3(第2target・AArch64への分岐判定)**:
- **支持条件**: Assert/Callを`Terminator`の正式なvariantとしてIRへ統合した上で、同じ`CfgBody`をAArch64向けにlowerする第2の関数を実装し、両target間で「target非依存の共通ロジック」(ブロックレイアウト計算、relocation解決等)と「target固有のロジック」(命令エンコーディング)が、コードの追加なしに自然に分離できることを示せた場合。
- **棄却条件**: 第2の関数実装が、既存のx86_64専用関数の構造をほぼそのまま複製する形になり、共通化できる部分が「関数のシグネチャ」程度に留まる場合。または、共通化を試みた結果、両target固有の詳細(x86_64の`rel32`直接相対分岐 vs AArch64の`CALL26`/`JUMP26`という26ビットフィールド、既に`lib.rs`のdoc commentで確認済みの構造的差異)が、共通コードの抽象化を破壊する場合。
- **中間評価は行わない**: 実装前に「たぶんこうなるだろう」という予測を記録に残さない。

**効率比較の統制されたやり直し**:
- **支持条件**: `opt-level=0`同士・同一呼び出し方式(両者関数ポインタ経由、またはビルド時リンクで統一)・N≥30試行で、本設計がLLVMと同等またはそれ以上の実行時性能を示す、あるいは有意な性能差の原因が特定の最適化パス1つに帰着できる場合。
- **棄却条件**: 統制条件下でも原因不明の性能差が残る場合(節6の未解決事項として記録済みの、命令列が同一なのに性能差が生じる現象が再現する場合)。この場合、「本設計のコード生成自体の質」ではなく「測定方法自体に still 何らかの見落としがある」と判断し、これ以上の効率比較実験は打ち切る。

## 5.12 H3(第2target・AArch64への分岐判定)の実装と判定

### 5.12.1 実装: `lower_cfg_body_to_aarch64_code_body`

節5.11.2で事前登録した支持条件のうち、Assert/Call variantの統合は今回のH3実装には含めていない(スコープを「同じ`CfgBody`を第2のtargetへlowerできるか」に絞り、節5.11.1のカバレッジ行列自体を拡張する作業とは分離した。これ自体、事前登録からの逸脱として正直に記録する)。

実装した内容は、`diamond_and_loop_cfg`モジュールに存在する`CfgBody`(x86_64版`lower_cfg_body_to_code_body`が受け取るのと全く同じ型)を、AArch64(aarch64-unknown-linux-gnu)向けの機械語列へlowerする第2の関数`lower_cfg_body_to_aarch64_code_body`である。全命令エンコーディングは、`aarch64-linux-gnu-as`でアセンブル→`objdump`で逆アセンブルという実データ照合によって検証し、この過程で3件の実装バグ(`cmp`のRdフィールド誤り、`cset w9, ne`のエンコーディング誤り、`movk`のhwフィールド欠落)を発見・修正した。

### 5.12.2 実行時検証: QEMU user-modeによるセマンティクス確認

`objdump`による逆アセンブル検証は「有効な命令列であること」までしか確認しない。実際に意味的に正しい実行結果を返すかどうかは別に検証する必要があった(x86_64側では`mmap`実行による検証が最初から存在した: `examples/diamond_and_loop_check.rs`)。

x86_64ホスト上ではAArch64バイナリを直接実行できないため、Dockerコンテナ内に`qemu-user`(`qemu-aarch64`)を追加し、生成した機械語をCハーネス経由で`mmap`+`mprotect`実行した。結果:

- diamond CFG(`if param0 != 0 { param0+1 } else { param0-1 }` の後 `*2`): 7入力(`0, 1, -1, 5, -5, 100, -100`)全てでx86_64側の`mmap`実行結果と完全一致(`diamond(-1) = 0`、`diamond(-5) = -8`等)。
- countdown CFG(`while n > 0 { acc += n; n -= 1; }`、実際の後方分岐(back-edge)を含む): 5入力(`0, 1, 5, 10, 100`)全てで期待値と一致。

**重要な事実**: countdown CFG(ループ・後方分岐)をAArch64へ通す際、`lower_cfg_body_to_aarch64_code_body`関数自体には一切のコード変更が不要だった。これは、この関数のブロックレイアウト計算とbranch fixup機構が、最初から`CfgBody`全体に対して汎用的に実装されていたためである(後方分岐は「fixupの対象ブロックの開始オフセットが分岐命令のサイト自身より前にある」というだけの、diamond CFGの前方分岐と全く同じ処理経路)。

### 5.12.3 判定: 部分的に支持、ただし主要な部分は棄却条件に該当

節5.11.2の事前登録した基準に照らすと:

- **支持される部分**: 「ブロックレイアウト計算(`block_starts`)とrelocation解決(fixup機構)」という、CFGの構造そのものを扱うロジックは、x86_64版とAArch64版で**同じアルゴリズム**(2パス: 命令列を仮組み→各ブロック開始オフセットを計算→fixup値をパッチ)を採用しており、これはコードの追加(新しいbranchやCFG形状への対応)なしに両target間で成立した。countdown CFGへの無変更対応がその直接証拠である。
- **棄却条件に該当する部分**: しかし、この「同じアルゴリズム」は**独立した2つの関数として実装されており、実際のコードは1行も共有していない**。分岐オフセットの算術(x86_64: 命令の**終了位置**からのバイトスケールオフセット、AArch64: 命令の**開始位置**からのワードスケールオフセット)、命令エンコーディング全体、スタックスロットのサイズ(x86_64: 8バイト、AArch64: 4バイト)が構造的に異なり、これは事前登録した棄却条件「共通化できる部分が『関数のシグネチャ』程度に留まる」に正確に一致する。

**結論**: `CfgBody`という中間表現自体はtarget非依存に設計・再利用できた(入力IRレベルでの統合は成功)が、それをコード生成のロジックレベルでの共通化(Craneliftの`InstructionData`のような共有された中間層)に転化することはできなかった。H3が検証しようとした問いは「target非依存の中間表現があれば、Craneliftの2層構造(target非依存の`InstructionData` + target固有の`MachInst`)を消せるか」であり、実験結果は「表現(データ型)は消せるが、変換ロジック(2つの完全に独立したlowering関数)は消せない」という部分的棄却である。事前登録した支持条件の「コードの追加なしに自然に分離できる」は、"分離"はできたが"共通化"はできなかった、という意味で厳密には満たされていない。

**方法論上の注記(独立レビュアーの指摘)**: 5.12.1で述べた通り、事前登録の支持条件は「Assert/Callを`Terminator`の正式variantとしてIRへ統合した上で」という前提を含んでいたが、今回のH3実装はこの前提を実施せず、diamond/loopのみの縮小版で実施した。したがって上記の判定は、事前登録した実験そのものではなく、その前提を欠いた縮小版の実験結果を、事前登録した支持/棄却条件と照合したものである。判定の方向性(部分的棄却)自体はコードの実態(共有関数ゼロ)と一致しており覆らないと考えるが、「事前登録通りに実験した上での判定」と「事前登録の一部を省略した実験に事前登録の基準を当てはめた判定」は区別されるべきであり、後者である。Assert/Call variantを統合した上でのH3再実験は未実施のまま残る。

## 5.13 効率比較の統制されたやり直し: `opt-level=0`同士・同一呼び出し方式・N=30試行

### 5.13.1 何を統制したか

節5.8までの効率比較(`examples/target_ir_energy_bench.rs`)には、事前登録(節5.11.2)が指摘した2つの未統制要因があった:

1. **最適化レベルの不一致**: LLVM参照関数はこのクレートの既定`--release`プロファイル(`opt-level=3`)でビルドされており、その結果LLVMが分岐そのものを削除していた(`sete`+`lea`のみ、`cmp`/`je`なし。同ファイル自身のコメントで確認済み)。`lower_target_ir_to_code_body`自体は最適化パスを一切持たないため、これは「最適化されたLLVM vs 無最適化の直接lowering」という不公平な比較だった。
2. **呼び出し方式の違い**: `target_ir_fn`は`mmap`されたコードへの関数ポインタ経由で呼び出す一方、`llvm_branch`は静的リンクされた直接呼び出しだった。

これを是正するため、`Cargo.toml`に`opt0`プロファイル(`opt-level=0`を明示、`release`から継承)を追加し、新しい比較例`examples/energy_comparison_controlled.rs`を作成した。この例は両関数を全く同じ`extern "C" fn(i32) -> i32`という関数ポインタ型経由で呼び出し、N=30回の独立試行(それぞれ500万回のループ)を行い、min/中央値/平均/最大値/標準偏差を報告する。

`opt-level=0`ビルドで本当にLLVM側が真の分岐(`cmp`/`jne`)を保持していることは、`objdump`によって独立に確認した(スタンドアロンの`rustc -C opt-level=0`コンパイル、および実際にビルドされたベンチバイナリの両方で確認)。

### 5.13.2 初回結果とその誤り: A/A対照実験を欠いたまま「同等」と判定した

初回は3回の独立実行を行い、比率が0.996〜1.026の範囲に収まったことから「測定誤差の範囲内で両者は同等」と判定した。

```
1回目: target_ir median=0.044815s, llvm median=0.044990s, ratio=0.9961x
2回目: target_ir median=0.047745s, llvm median=0.046520s, ratio=1.0263x
3回目: target_ir median=0.046807s, llvm median=0.046406s, ratio=1.0087x
```

**この判定は誤りだった**。独立レビュアーの指摘: 「比率が1.0に近い」ことは、それ単体では何の証拠にもならない。この測定系自身のノイズ床(全く同じコードを2回測っても生じる測定誤差)を先に測定していなければ、観測された比率0.996〜1.026がその測定系で意味を持つ差なのか、単なるノイズなのか判定できない。これはA/B比較を行う前にA/A対照実験(同一のものを2つとして測る)を行うべきという、実験計画の基本的な欠落であり、指摘の通り「実験前に分かった問題」だった。

### 5.13.3 A/A対照実験の追加とその結果: この測定系はノイズ床が主張された差を上回る

指摘を受け、`energy_comparison_controlled.rs`に、`target_ir_fn`(全く同じ機械語バイト列)を「A側」「B側」として同じ測定ループで比較するA/A対照を追加した。ホストの`load average`(実行時9.68/12コア、他プロセスと共有のマルチテナント環境)も記録する。

4回の独立実行結果(iters=5,000,000、trials=30):

```
1回目: 本実験比率=0.9456x (偏差 5.44%), A/A対照比率=0.9841x (偏差 1.59%)
2回目: 本実験比率=0.9664x (偏差 3.36%), A/A対照比率=1.0148x (偏差 1.48%)
3回目: 本実験比率=1.0577x (偏差 5.77%), A/A対照比率=0.9635x (偏差 3.65%)
```

いずれの回でも標準偏差は中央値の約15〜45%に達しており、`load average`約9.68/12コアという、ホストが他の負荷と共有された状態での測定であることが直接の原因と考えられる。A/A対照比率の1.0からの偏差(1.48〜3.65%)は、本実験比率の1.0からの偏差(3.36〜5.77%)と同じ桁に収まっており、後者を「有意な同等性の証拠」として区別することはできない。初回に報告した0.996〜1.026という「1.0に近い」比率も、同じ理由で有意性を持たない。

### 5.13.4 判定: 撤回(打ち切り)

節5.13.3初版で下した「支持」の判定を撤回する。事前登録(節5.11.2)の棄却条件「統制条件下でも原因不明の性能差が残る場合」を、より正確に言えば「この測定系はA/A対照でさえ数%の偏差を示し、統制条件下でも測定系自身のノイズが主張したい差と同じ桁にある場合」に該当する。事前登録の指示通り、これ以上の効率比較実験はここで打ち切る。

この撤回は、当初の判定プロセス自体に欠陥があったことを示す: `opt-level=0`統一・同一呼び出し方式・N≥30試行という事前登録の条件は、いずれも測定の「公平性」(両側を同じ土俵で比較すること)を保証するものであって、測定系の「分解能」(その土俵上でどれだけ小さい差を検出できるか)を保証するものではない。両者は別の問題であり、後者を検証するA/A対照実験は事前登録(節5.11.2)自体に含めるべきだった。今後、実行時性能比較を再度行う場合は、事前登録の時点でA/A対照実験を必須の前提条件として明記する。

## 5.14 仮説全体(H1〜H6)の現状

独立レビュアーの指摘を受け、issue #68が当初分解した6つの仮説それぞれの現在の到達点を一覧する:

| 仮説 | 内容 | 状況 |
| --- | --- | --- |
| H1 | LLVM IRを経由せず`TargetIr`から直接コード生成に接続できる | 支持(節5.1〜5.4、自明の範囲で決着済み) |
| H2 | 所有権情報(`Ownership`)がtarget固有コード生成に必要十分な精度で再エンコードできる | 部分的支持。`frozen`(UnsafeCell)軸のみ解決(節5.10)。`unpin`(`Pin<&mut T>`)軸、単相化前の`Ty::is_freeze`精度は未着手(節6参照) |
| H3 | target非依存の中間表現1つで、Craneliftの`InstructionData`/`MachInst`という2層構造を消せる | 部分的棄却(節5.12)。表現は共有できるが、変換ロジックは共有できない。Assert/Call未統合のまま判定した点は事前登録からの逸脱(節5.12.3の方法論上の注記参照) |
| H4 | 分岐+ループを含む実CFG(diamond・back-edge)の不動点計算モデルを正しく実装できる | 支持(節5.9)。独立レビュアーによる自作CFG・境界値での再現でも全件一致を確認 |
| H5 | `AddressState`(このIR設計の別の柱)へ`CfgBody`をどう統合するか | 未着手 |
| H6 | 8個の実targetそれぞれへの分岐可能性 | 未着手(AArch64のみ、8個中1個の部分的前進、かつH3の判定通り共有ロジックはほぼゼロ) |

H3が「変換ロジックの共通化はできない」と判定した以上、次の設計上の争点はH5(`CfgBody`をこの中間表現のもう一つの柱である`AddressState`とどう関係づけるか)であり、本セッションではまだ着手していない。

## 6. まだ解けていないこと

- 節5.11の事前登録(成功基準の明文化)は、H4・H2の実装が完了した後に遡って行われた。ユーザー指摘が求めた「実装前の事前登録」という規律には従えておらず、これ自体が本セッションの検証プロセスの限界として記録する。以降のH3・効率比較の実験は、節5.11.2の棄却条件を実装前に確定させた上で着手する。
- 節5.11.1のカバレッジ行列が示す通り、`TerminatorKind`14種中対応済みはわずか2種(部分対応3種)、`Rvalue`12種中対応済みは1種(部分対応2種)にとどまる。issue #68が要求する「target固有中間表現の設計」全体からすれば、実装できているのはごく限られた部分集合であることを正直に記録する。
- 節5.10で修正した`Ownership::Shared { frozen }`は`UnsafeCell`の有無のみを追跡する。`MutableRef`/`Box`が実際に持つ`unpin`条件(`Pin<&mut T>`等)は未反映であり、同様の反証が存在する可能性が高い。前回研究が挙げたStacked/Tree Borrowsのタグ+木構造という、より豊かな代替への道筋もまだない。さらに、節5.10の検証は`TypingEnv::fully_monomorphized()`固定のもとでの`Ty::is_freeze`呼び出しのみを確認しており、ジェネリック型パラメータを含む`&T`(単相化前)でこのクエリがどう振る舞うか(保守的にfalseを返す可能性)は未検証のまま。H2は「所有権情報の再エンコード回避」という3軸(frozen性、unpin性、単相化境界での精度)のうち1軸のみを解決したに過ぎない。
- 節5.9で実装した`diamond_and_loop_cfg`は、既存の`Terminator`/`lower_target_ir_to_code_body`(2ブロック限定)、`lower_load_or_reload_to_code_body`(Ownership統合)、`lower_bounds_checked_slice_index_to_code_body`(境界チェック)とは独立した、別々の型体系・別々の関数として存在する。「1つの汎用IRが、分岐+ループ+所有権+境界チェックを同時に扱う」という統合はまだ行っていない——これはH3(単一層のtarget固有表現で済むか)の判定に直接関わる未解決事項であり、節5.9.1で確認した通り、機能を追加するたびに別関数を書いている現状は、H3への否定的な材料になりうる。
- `diamond_and_loop_cfg`はレジスタ割り当てを一切行わず、全ての値を毎回メモリ(スタックスロット)へ読み書きする素朴な方式である。これはSSA構築・mem2reg相当の最適化を意図的に持たないという設計判断だが、この方式のままでは前回研究が指摘したLLVMとの実行時性能差(節5.2.3、節5.8.3)がさらに拡大する可能性が高い。
- 節5.8.3で実測した通り、`get_elem`関数では本設計とLLVMの成功パス命令列がバイト単位で完全一致しているにもかかわらず、実行時性能に約1.15〜1.25倍の差が観測された。これが「ベンチマーク手法(関数ポインタ経由の間接呼び出し vs. 直接call)に起因する測定誤差」なのか、「本設計側に何らかの実質的な追加コストがある」のかは切り分けていない。同一の呼び出し方式(例えば両方とも関数ポインタ経由にする、あるいは両方ともバイナリに直接リンクする)で再測定する必要がある。
- vtableエントリ(節5.5.1)・Poloniusファクト(節5.5.2)・レイアウト情報(節5.5.3)は、いずれも実データとして取得できたが、`target_ir`のx86_64コード生成パイプラインへの接続は一切行っていない。動的ディスパッチ・複数借用・複合型レイアウトを実際に扱うtarget固有中間表現の設計は、本セッションでは着手していない。
- **(節5.6.2でほぼ解消)** 節5.3で実証した`rustc_driver`ベースの型・借用チェック済みMIR取得を、`target_ir`のコード生成へ接続する作業は、独立レビュアーの指摘(節5.6.1)を受け`lower_load_or_reload_to_code_body`として実装し直し、分岐(両アームで同じメモリ位置を再読み込みする関数)と4値の`Ownership`(`Unique`/`Shared`/`Boxed`/`NotAReference`)を同時に扱う統合を達成した。ただし`lower_target_ir_to_code_body`(元々の、分岐のみを扱うCFG lowering関数)自体は依然として`Ownership`を消費しておらず、新関数`lower_load_or_reload_to_code_body`は独立した別関数として存在する——「1つの汎用lowering関数が任意の分岐構造と任意の所有権情報を同時に扱う」という、より一般化された統合はまだ行っていない。
- `experiments/rustc-driver-poc/`はnightly専用(`rustc-private` feature)であり、`unified-symbol-graph`クレート自体のstableツールチェーンでのビルドとは両立しない。現状は「nightly専用クレートがstable専用クレートへpath依存する」という一方向の依存で回避しているが(節5.4.2)、これはビルド設定が2つのツールチェーンにまたがるという複雑さを本質的には解消していない。本格的な統合には、`unified-symbol-graph`自体をnightly専用にするか、この依存構造を恒久的な設計として採用するかの判断が未決定。
- 節5.2.3で実測した通り、`lower_target_ir_to_code_body`は最適化パスを一切持たず、LLVMが行う分岐除去等の最適化(この実験では約2.7〜3.0倍の実行時性能差の原因)を代替できていない。本設計がLLVM IRを削除した後も「意味論を確定させる境界」(前回研究節7)の先に、何らかの最適化層を独自に持つ必要があるのか、あるいは実行時性能を犠牲にしてでも中間成果物の軽量さ(節5.2.2)を優先するのかは、未決定の設計判断である。
- 節5.2.1で確認した通り、コンパイル時間の公平な比較には、本設計を実際の`rustc`ドライバへ統合し(`-Zcodegen-backend`相当)、プロセス起動オーバーヘッドを除いた実運用規模での計測が必要。1関数単位の計測では、rustc自身の起動コストに埋もれてしまい、LLVM自体の処理時間差を検出できないことを本セッションで確認した。
- `mir_text::parse_mir_text`が対応するRust構文は「直線コード」と「2分岐+`switchInt`+共通`goto`合流点」の2パターンのみ。`match`式(3分岐以上)、ループ、関数呼び出し、構造体/参照型、`i32`以外の数値型は未対応であり、`rustc`の出力フォーマット自体も"human-readable"であり将来変更されうる非公式形式である(rustc自身の警告コメント`// WARNING: This output format is intended for human consumers only and is subject to change without notice.`が実際に出力に含まれることを確認済み)ため、本格的な統合には`-Zunpretty=mir`ではなく`rustc_middle::mir::Body`自体を扱う(コンパイラプラグイン/カスタムドライバとしての)経路が必要になる。節5.3の`rustc_driver`ベースの経路が、まさにこの「本格的な統合」の第一歩である。
- `SemanticFacts`の拡張(CFG+所有権タグ)を`unified-symbol-graph`の`declare_analyzed_symbol`/`require_symbol`パスへ実際に統合する作業(本書ではPoCとして独立モジュールに留め、既存の`AddressState`型定義自体はissue #67の後方互換のため変更しない)。
- x86_64以外の7つのtarget(ELF/ARM64、COFF x2、Mach-O/ARM64、WASM x3)への`TargetIr`→`CodeBody`変換規則の分岐は未着手。特にWASMは前回研究の`unified-symbol-graph`自身のdoc comment(節「WASMの実測」)が既に確認した通り、アドレス計算を伴わないインデックス置換モデルであり、本書のx86_64 PC相対分岐という前提が全く成立しない別設計が必要になる。
- QBEのように型を犠牲にする再エンコードを避けつつ、所有権タグをtarget固有表現の中でどこまで保持し続けるべきかの具体的な境界(全ての値にタグを付けるのか、noalias相当の最適化ヒントを出す箇所だけに限定するのか)は未設計。
- `BuilderMethods`のABI決定層(`rustc_target::callconv::FnAbi`)を、独自にどこまで再実装する必要があるか、あるいは`rustc_target`クレート自体を(LLVM非依存な形で)再利用できるかは未検証。

## 関連文書

- [意味論から「中間表現」を削除する](removing-intermediate-representation_ja.md) — 本書の前提となる判定(節7総合判定表)
- [全体設計 — なぜ「一つの計算システム」でなければならないか](../../01-foundations/joint-symbol-schedule-layout-design_ja.md) — 問題D・問題F
- issue #67 — `SharedSymbolGraph`/`AddressState`/`ElfX86_64PendingReloc`の確立
- issue #68 — 本書が対応するissue

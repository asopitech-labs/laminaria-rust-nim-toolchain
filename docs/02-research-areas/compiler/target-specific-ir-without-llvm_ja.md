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

## 6. まだ解けていないこと

- `mir_text::parse_mir_text`が対応するRust構文は「直線コード」と「2分岐+`switchInt`+共通`goto`合流点」の2パターンのみ。`match`式(3分岐以上)、ループ、関数呼び出し、構造体/参照型、`i32`以外の数値型は未対応であり、`rustc`の出力フォーマット自体も"human-readable"であり将来変更されうる非公式形式である(rustc自身の警告コメント`// WARNING: This output format is intended for human consumers only and is subject to change without notice.`が実際に出力に含まれることを確認済み)ため、本格的な統合には`-Zunpretty=mir`ではなく`rustc_middle::mir::Body`自体を扱う(コンパイラプラグイン/カスタムドライバとしての)経路が必要になる。
- `SemanticFacts`の拡張(CFG+所有権タグ)を`unified-symbol-graph`の`declare_analyzed_symbol`/`require_symbol`パスへ実際に統合する作業(本書ではPoCとして独立モジュールに留め、既存の`AddressState`型定義自体はissue #67の後方互換のため変更しない)。
- x86_64以外の7つのtarget(ELF/ARM64、COFF x2、Mach-O/ARM64、WASM x3)への`TargetIr`→`CodeBody`変換規則の分岐は未着手。特にWASMは前回研究の`unified-symbol-graph`自身のdoc comment(節「WASMの実測」)が既に確認した通り、アドレス計算を伴わないインデックス置換モデルであり、本書のx86_64 PC相対分岐という前提が全く成立しない別設計が必要になる。
- QBEのように型を犠牲にする再エンコードを避けつつ、所有権タグをtarget固有表現の中でどこまで保持し続けるべきかの具体的な境界(全ての値にタグを付けるのか、noalias相当の最適化ヒントを出す箇所だけに限定するのか)は未設計。
- `BuilderMethods`のABI決定層(`rustc_target::callconv::FnAbi`)を、独自にどこまで再実装する必要があるか、あるいは`rustc_target`クレート自体を(LLVM非依存な形で)再利用できるかは未検証。

## 関連文書

- [意味論から「中間表現」を削除する](removing-intermediate-representation_ja.md) — 本書の前提となる判定(節7総合判定表)
- [全体設計 — なぜ「一つの計算システム」でなければならないか](../../01-foundations/joint-symbol-schedule-layout-design_ja.md) — 問題D・問題F
- issue #67 — `SharedSymbolGraph`/`AddressState`/`ElfX86_64PendingReloc`の確立
- issue #68 — 本書が対応するissue

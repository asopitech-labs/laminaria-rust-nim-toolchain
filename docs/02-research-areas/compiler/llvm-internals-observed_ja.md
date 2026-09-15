# LLVM内部の実測記録 — IR生成からオブジェクトファイル書き出しまで

## 位置付け

本書は[LLVM再発見研究](llvm-rediscovery-research_ja.md)が求める "Prior art study"（LLVMが解決している問題と設計理由をsource/documentation/実験から調べる）と、[Backend Pipeline White-boxing研究方針](backend-pipeline-whiteboxing_ja.md)が求める "LLVM White-boxing"（LLVM内部を観測可能・説明可能なnested computation graphとして投影する）の、具体的な一次観測記録である。

両ドキュメントが方針・設計原則を定義するのに対し、本書は**実際に何を読み、何が分かったか**だけを記録する。LAMINARIAが同じ構造を採用すべきという主張はしない — その判断は上記2文書の枠組みで別途行う。

すべての記述は次のいずれかを根拠とする。

- LLVM公式ドキュメント（llvm.org/docs）、LLVM公式ブログ、rustc-dev-guideからのWeb一次資料調査
- `llvm/llvm-project`リポジトリの実ソースコード（GitHub raw contentから直接取得し読解）

推測・一般化は行わず、根拠を明示できない主張は含めない。

## 1. パイプライン全体像

LLVMバックエンドの処理は、大きく3段階に分かれる。

```text
LLVM IR
  → (1) IR最適化パス群（PassManager）
  → (2) 命令選択・レジスタ割付・命令スケジューリング（CodeGen層）
  → (3) 機械語エンコード・オブジェクトファイル書き出し（MC層）
```

### 1.1 IR最適化パス（PassManager）

現在のLLVMは新Pass Manager（NewPM）に統一されており、レガシーPass Managerは非推奨である[^newpm]。パスは階層構造を持つ。

- **Module Pass**: モジュール全体を対象
- **CGSCC Pass**: Call Graph SCC（強連結成分）単位、インライン化等に使用
- **Function Pass**: 関数単位（定数畳み込み、デッドコード除去等）
- **Loop Pass**: ループ単位

最適化パイプラインは`PassBuilder`が構築し、`-O0`〜`-O3`/`-Os`/`-Oz`ごとに事前定義された順序でパスを適用する[^passes]。パス間の依存は`AnalysisManager`がキャッシュ管理し、無効化された解析結果のみ再計算する。

### 1.2 命令選択・CodeGen層

IR最適化後、ターゲット固有の機械語命令へ変換する段階は2つの実装を持つ[^codegen][^writingbackend]。

- **SelectionDAG**: DAGベースの命令選択。最適化品質重視。
- **GlobalISel**: `-O0`向けの高速パス。コンパイル速度改善が目的で、SelectionDAGと住み分けている（2018年のLLVM Developers' Meetingで設計議論[^globalisel]）。

命令選択後、`MachineFunction`単位でレジスタ割付・命令スケジューリングが行われ、最終的に`MachineInstr`列としてMC層へ渡される。

### 1.3 MC（Machine Code）層

MC層の設計思想は、2010年のLLVM公式ブログ記事に明確に記されている[^mcblog]。

> MCプロジェクト以前、LLVMは`.s`（テキストアセンブリ）を出力し、外部の`as`（GNU asなど）を呼び出してオブジェクトファイルを生成していた。MC層導入後は「integrated assembler」により、**LLVM自身が外部プロセスを一切使わず、直接ELF/Mach-O/COFFバイナリを書き出せる**ようになった。

現在の既定動作はintegrated assemblerの使用であり、`-fno-integrated-as`相当のフラグで明示的に無効化しない限り、外部アセンブラは起動されない。

MC層の中核は`MCStreamer`という抽象インターフェースである。コード生成の最終段（`AsmPrinter`）は出力形式を意識せず`MCStreamer`のAPI（`emitInstruction`等）を呼ぶだけでよい。実装は出力先ごとに分かれる。

- `MCAsmStreamer`: テキストアセンブリ（`.s`）を出力
- `MCObjectStreamer`（の派生: `MCELFStreamer`、`MCMachOStreamer`、`MCWinCOFFStreamer`）: バイナリオブジェクトファイルを直接出力

## 2. `MCObjectStreamer` — 命令の蓄積と再配置の検出（実装コード確認済み）

`llvm/lib/MC/MCObjectStreamer.cpp`（817行、2026年時点の`main`ブランチ）を直接取得し読解した[^mcobjstreamer]。

### 2.1 フラグメント管理

命令・データは`MCFragment`という単位でバッファに蓄積される。

```cpp
constexpr size_t FragBlockSize = 16384;
```

16KB単位でメモリブロックを割り当て（`allocFragSpace`）、フラグメントを連結していく。これは1命令ごとに個別のヒープ確保を行わないための、パフォーマンス指向の設計である。

### 2.2 命令エンコードと再配置検出の統合

外部シンボル参照を含む命令の処理（`emitInstToData`）は次の形を取る。

```cpp
void MCObjectStreamer::emitInstToData(const MCInst &Inst,
                                      const MCSubtargetInfo &STI) {
  ...
  SmallVector<MCFixup, 1> Fixups;
  getAssembler().getEmitter().encodeInstruction(Inst, Content, Fixups, STI);
  appendContents(Content);
  ...
  F->appendFixups(Fixups);
}
```

`encodeInstruction`という**単一の呼び出し**が、機械語バイト列（`Content`）の生成と、その中で外部シンボル解決を要する箇所（`Fixups`、後述のELF再配置に相当する中間表現）の検出を**同時に**行う。命令エンコードと再配置検出は分離された2つの処理ではなく、1つのエンコーダの中で不可分に行われる。

### 2.3 Relaxable Fragment（緩和可能フラグメント）とlinker-relaxable

`emitInstToFragment`（`MCObjectStreamer.cpp` 489行目以降）は、最終的なジャンプ距離次第でより短いエンコーディングに置き換えられる可能性のある命令を`MCFragment::FT_Relaxable`として扱う。

さらに`Fixup.isLinkerRelaxable()`という概念が存在する（471行目、504行目）。これは「アセンブラ（LLVM自身）が確定させた再配置だが、**リンカ側がさらに命令を書き換える可能性がある**」ことを示すフラグである。コンパイラが再配置を確定して終わりではなく、リンカとの間により深い協調（緩和・最適化の余地を残す）が設計されている。

## 3. `ELFObjectWriter` — 再配置のバイナリ化（実装コード確認済み）

`llvm/lib/MC/ELFObjectWriter.cpp`（1412行）を直接取得し読解した[^elfobjwriter]。

### 3.1 `recordRelocation` — Fixupから最終的な再配置エントリへ

```cpp
void ELFObjectWriter::recordRelocation(const MCFragment &F,
                                       const MCFixup &Fixup, MCValue Target,
                                       uint64_t &FixedValue) {
  ...
  bool IsPCRel = Fixup.isPCRel();
  uint64_t FixupOffset = Asm->getFragmentOffset(F) + Fixup.getOffset();
  uint64_t Addend = Target.getConstant();
  ...
  unsigned Type = TargetObjectWriter->getRelocType(Fixup, Target, IsPCRel);
  ...
  Relocations[&Section].emplace_back(FixupOffset, SymA, Type, Addend);
}
```

`MCFixup`（セクション内オフセット・対象シンボル・PC相対フラグ）から、最終的なELF再配置エントリ`(offset, symbol, type, addend)`が構成される。この4つ組は、ELFフォーマット自体が定義する再配置レコードの構成要素と一致する。

このコードは単純な計算だけでなく、複数のエッジケースを扱っている。

- **ローカルシンボルのセクションシンボルへの変換**（`UseSectionSym`判定）: 特定条件下で、シンボル参照をそのシンボルが属するセクション自体への参照へ置き換える
- **2シンボル間の差分（サブトラクション式）**: `Target.getSubSym()`が存在する場合、2つのシンボル間のオフセット差分を再配置として扱う特殊経路
- **シンボルのリネーム**（`Renames`マップ）

これらは、単純な「アドレス計算式を1つ適用する」という粒度を超えた、多数の文脈依存処理を含む。

### 3.2 `writeRelocations` — バイナリへの実書き出し

```cpp
ELF::Elf64_Rela ERE;
ERE.setSymbolAndType(Symidx, Entry.Type);
write(ERE.r_info);
if (Rela)
  write(Entry.Addend);
```

`Relocations`マップに蓄積された再配置エントリは、最終的に`Elf64_Rela`（またはRelaを使わない場合は`Elf64_Rel`）構造体としてシリアライズされ、セクションへ書き出される。MIPSアーキテクチャ向けの特殊な多重リロケーション形式や、`Crel`（コンパクト再配置形式）向けの分岐も同じ関数内に存在する。

## 4. rustcにおけるCGU（コード生成ユニット）とLLVMへの呼び出し粒度（実装コード確認済み）

`compiler/rustc_monomorphize/src/partitioning.rs`（1398行）を直接取得し読解した[^partitioning]。これはLLVM自体の内部実装ではないが、「LLVMへ何を、どの粒度で渡すか」を決めるrustc側の実装であり、LLVM呼び出しの実際の粒度を確定するために必須の裏取りである。

### 4.1 CGUは`.rs`ファイル単位でもクレート全体単位でもない

モジュール冒頭のコメントに設計意図が明記されている。

> Since the unit of codegen and optimization for LLVM is "modules" or, how we call them "codegen units", the particulars of how much time can be saved by incremental compilation are tightly linked to how the output program is partitioned into these codegen units prior to passing it to LLVM.

CGUへの割り当ては2段階で行われる。

**第1段階（`place_mono_items`）**: モノモーフィ化済みアイテム（ジェネリックが具体的な型で実体化された後の、関数・静的変数の実体）1つずつを、そのアイテムが属する**ソースレベルの`mod`階層**（ファイル境界ではなくRustの論理的モジュール構造）を基準にCGUへ割り当てる。1モジュールにつき正確に2つのCGUを生成する。

> There are two codegen units for every source-level module: One for "stable", that is non-generic, code. One for more "volatile" code, i.e., monomorphized instances of functions defined in that module.

この粒度を選ぶ理由もコメントに明記されている。1モジュール=1CGUでは、そのモジュール内の1つのジェネリック関数への新しい参照が追加されるだけでモジュール全体のインクリメンタルキャッシュが無効化される。逆に1アイテム=1CGUまで細分化すると、LLVMのインライン化を含む手続き間最適化がモジュール境界を越えられないため実行時性能が劣化する。

**第2段階（`merge_codegen_units`）**: `-C codegen-units=N`（既定16、インクリメンタルビルドでは256）を超えるCGU数の場合、**インライン化アイテムの重複が最大の組を貪欲法で統合**していく。

```rust
// We use inlined item overlap to guide this merging because it minimizes
// duplication of inlined items, which makes LLVM be faster and generate
// better and smaller machine code.
```

### 4.2 この粒度が知らないこと

CGUメンバーシップは、その1クレート自身のモジュール構造とジェネリック実体化集合だけから決定される。**あるクレートのどのシンボルが、別のクレート（あるいは別のエコシステム）から実際にリンク時に参照されるか、という情報は一切考慮されない** — そもそも1回の`rustc`呼び出しは1クレートしか見えないため、考慮する手段が構造的にない。

## 5. rustcフロントエンド↔LLVMバックエンドの並列化（実装コード確認済み）

`compiler/rustc_codegen_ssa/src/base.rs`（1295行）、`compiler/rustc_codegen_ssa/src/back/write.rs`（2311行）を直接取得し読解した[^base][^write]。これもrustc側の実装だが、「LLVMへの呼び出しがどう並列にスケジューリングされるか」を扱うため、LLVM呼び出しの実際の挙動を理解するために必要な裏取りである。

### 5.1 LLVMContextは1スレッド1コンテキストが原則

LLVM公式メーリングリストの議論によれば、`LLVMContext`自体はロック保証を一切提供せず、1コンテキストにつき1スレッドが原則である[^llvmcontext]。別々のコンテキストであれば別々のスレッドで同時実行できる。

rustc側は、**CGUごとに独立した`CodegenCx`を持ち、各`CodegenCx`が自前の`llvm::Context`を持つ**ことでこの制約を満たしている（rustc-dev-guideに明記[^parallelrustc]）。

### 5.2 フロントエンドは直列、バックエンドは五月雨式並列（コメント原文で確認）

`base.rs`のコメントに、この制約が直接記されている。

```rust
// The non-parallel compiler can only translate codegen units to LLVM IR
// on a single thread, leading to a staircase effect where the N LLVM
// threads have to wait on the single codegen threads to generate work
// for them. The parallel compiler does not have this restriction, so
// we can pre-load the LLVM queue in parallel before handing off
// coordination to the OnGoingCodegen scheduler.
//
// This likely is a temporary measure. Once we don't have to support the
// non-parallel compiler anymore, we can compile CGUs end-to-end in
// parallel and get rid of the complicated scheduling logic.
```

フロントエンド（MIR→LLVM IR変換）はCGUを1つずつ直列に処理する。あるCGUの変換が完了した瞬間、そのCGU単体が即座にLLVMワーカーキューへ投入される（`submit_codegened_module_to_llvm`）。全CGUの変換完了を待つことはない。

対策として、`-Z threads`（parallel compiler）有効時は、複数CGUの先頭バッチを`par_map`で並列に事前コンパイルし、LLVMキューへ先読み投入する仕組みが実装されている。**rustc開発チーム自身がこの仕組みを「一時的な措置」と明記している**。

外部分析（Nicholas Nethercote、2023年）が指摘した「階段状効果」問題[^nethercote]は、このコメントと完全に一致する。

### 5.3 バックプレッシャー — 無制限の先読みではない

`back/write.rs`のコーディネータ設計コメントに、フロントエンドとバックエンドの並列度制御の詳細がある。

```rust
// So the actual goal is to always produce just enough LLVM WorkItems as
// not to starve our LLVM worker threads. That means, once we have enough
// WorkItems in our queue, we can block the main thread, so it does not
// produce more until we need them.
```

メモリ消費を抑えるため、フロントエンド（メインスレッド）はキューに十分な作業が溜まると意図的にブロックされる。もし無制限に先行させれば、コンパイル（codegen）の方がLLVM処理より速いため、未処理のLLVM WorkItemがキューに溜まり続け、それぞれが大量のメモリを保持したままになる。

3つの主体による生産者-消費者パイプラインとして設計されている。

- **メインスレッド**: CGUをLLVM作業パッケージへcodegenする（この処理ができるのはメインスレッドのみ）
- **コーディネータスレッド**: メッセージループを実行し、LLVM WorkItemの"Token"（並列度予算）が空いたらLLVMワーカースレッドを起動する
- **LLVMワーカースレッド**: 実際の最適化・コード生成を行い、完了したら破棄される（保持していたLLVMモジュールのメモリも解放される）

## 6. ThinLTO — サマリーの直列マージと本体の並列処理（Web一次資料確認済み）

Clang公式ドキュメント[^thinlto-clang]、LLVM公式ブログ（2016年）[^thinlto-blog]による。

ThinLTOモードでは、コンパイル段階でLLVM bitcodeに加えて**モジュールのコンパクトなsummary（要約）**を出力する。リンク時には**summaryだけを読み込んで結合indexにマージ**し、この結合index上で高速な全プログラム解析（インライン化候補の決定など）を行う。実際の変換（関数インポートを含む）はその後、**完全並列なバックエンドで各モジュールごとに**発生する。

> ThinLTO is designed to scale like a non-LTO build, while preserving most of the performance achievement of full LTO.

この「重い統合作業を先に済ませず、軽量な要約だけを直列にマージし、実際の重い処理は各モジュール単位で並列に行う」という設計は、2016年時点でLLVM自身が既に実装していたものである。

## Sources

[^newpm]: LLVM Project, "[The New Pass Manager](https://llvm.org/docs/NewPassManager.html)." Accessed 2026.
[^passes]: LLVM Project, "[LLVM's Analysis and Transform Passes](https://llvm.org/docs/Passes.html)." Accessed 2026.
[^codegen]: LLVM Project, "[The LLVM Target-Independent Code Generator](https://llvm.org/docs/CodeGenerator.html)." Accessed 2026.
[^writingbackend]: LLVM Project, "[Writing an LLVM Backend](https://llvm.org/docs/WritingAnLLVMBackend.html)." Accessed 2026.
[^globalisel]: LLVM Developers' Meeting 2018, "GlobalISel" session materials, llvm.org/devmtg/2018-10/. Accessed 2026.
[^mcblog]: Chris Lattner, "[Intro to the LLVM MC Project](https://blog.llvm.org/2010/04/intro-to-llvm-mc-project.html)," LLVM Blog, 2010-04.
[^mcobjstreamer]: LLVM Project, `llvm/lib/MC/MCObjectStreamer.cpp`, `llvm/llvm-project` repository, `main` branch. Fetched and read directly in this session.
[^elfobjwriter]: LLVM Project, `llvm/lib/MC/ELFObjectWriter.cpp`, `llvm/llvm-project` repository, `main` branch. Fetched and read directly in this session.
[^partitioning]: Rust Project, `compiler/rustc_monomorphize/src/partitioning.rs`, `rust-lang/rust` repository, `master` branch. Fetched and read directly in this session.
[^base]: Rust Project, `compiler/rustc_codegen_ssa/src/base.rs`, `rust-lang/rust` repository, `master` branch. Fetched and read directly in this session.
[^write]: Rust Project, `compiler/rustc_codegen_ssa/src/back/write.rs`, `rust-lang/rust` repository, `master` branch. Fetched and read directly in this session.
[^llvmcontext]: LLVM Developers Mailing List, "LLVMContext: Threads and Ownership," 2018-09, lists.llvm.org/pipermail/llvm-dev/2018-September/126132.html. Accessed 2026.
[^parallelrustc]: Rust Project, "[Parallel Compilation of a Single Crate](https://rustc-dev-guide.rust-lang.org/parallel-rustc.html)," rustc-dev-guide. Accessed 2026.
[^nethercote]: Nicholas Nethercote, "[Back-end parallelism in the Rust compiler](https://nnethercote.github.io/2023/07/11/back-end-parallelism-in-the-rust-compiler.html)," 2023-07-11.
[^thinlto-clang]: LLVM/Clang Project, "[ThinLTO](https://clang.llvm.org/docs/ThinLTO.html)," Clang documentation. Accessed 2026.
[^thinlto-blog]: LLVM Project, "[ThinLTO: Scalable and Incremental LTO](https://blog.llvm.org/2016/06/thinlto-scalable-and-incremental-lto.html)," LLVM Blog, 2016-06.

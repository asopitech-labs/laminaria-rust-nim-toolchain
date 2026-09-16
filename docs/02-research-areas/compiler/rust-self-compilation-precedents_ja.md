# Rustそのもののコンパイルを研究・実験した先行プロジェクト・事例

## 目的

[意味論から「中間表現」を削除する](removing-intermediate-representation_ja.md)が、Rust IR→LLVM IR→バイナリという既存パイプラインの内部構造を検証したのに対し、本書はさらに一段引いた問い — **「Rust自身のコンパイルを、既存のrustc/LLVM以外の経路で実現しようとした先行プロジェクトは何を発見したか」** — を扱う。LAMINARIAが「複数の専用コンパイラを束ねる」設計([全体設計ドキュメント](../../01-foundations/joint-symbol-schedule-layout-design_ja.md)の問題D)を採るなら、その先例が既に存在するかを確認する必要がある。

## 1. rustc自身のブートストラップ史 — 「単一言語・単一バックエンド」も歴史的必然ではなかった

Rustは2006年、Graydon Hoareの個人プロジェクトとして始まり、最初のコンパイラ`rustboot`(2010年)は**OCaml**で書かれていた。OCamlの代数的データ型・パターンマッチングが、Rustが必要とした実験的な型システム設計に適していたためである。この`rustboot`の役割は、Rust自身で書かれた第2のコンパイラ`rustc`をビルドし、セルフホスティングのブートストラップサイクルを開始することだった。`rustboot`が手書きのx86コード生成器を持っていたのに対し、`rustc`はバックエンドとしてLLVMを採用した。

移行は段階的に行われた。まず`rustboot`をRustで書き直しつつOCamlをコンパイルホストとして残し、そのRust-in-Rustコンパイラが自分自身をコンパイルできるようになった時点でOCaml依存を外した。**Rustが初めて自分自身をビルドしたのは2011年4月20日**で、1時間かかった(当時としては非常に遅いと認識されていた)。以降、新しいrustcリリースは常に前バージョンでコンパイルされる。

この史実が示すのは、「Rustは最初からRust+LLVMという単一の組み合わせで存在した」という前提が誤りだということである。**言語自身の実装言語もバックエンドも、両方とも途中で入れ替わった経緯**があり、これはLAMINARIAが「既存の専用コンパイラを束ねる、あるいはtarget固有の専用コンパイラへ段階的に移行する」という設計を取る際、Rust自身の歴史に直接の先例があることを意味する。

参照: rustboot(<https://github.com/iohub/rustboot>)、Language Lineage "What is Rust written in? OCaml first, Rust since 2011"、rust-dev mailing list "how is Rust bootstrapped?"(2014年6月、<https://mail.mozilla.org/pipermail/rust-dev/2014-June/010222.html>)。

## 2. 現行のstage0/1/2ブートストラップ構造 — 複数コンパイラ世代の共存を前提とした設計

`src/bootstrap/README.md`(rust-lang/rust、GitHub Contents APIで実ファイルを裏取り)を確認した結果、現行のブートストラップは次の3段階を経る。

1. `x`/`x.py`エントリポイントが、**stage0コンパイラ**(ダウンロード済みの既存rustcバイナリ)を取得する。
2. stage0コンパイラと標準ライブラリを使って、**stage1コンパイラ**をビルドする。stage1コンパイラはstage0の標準ライブラリとリンクされる。
3. stage1コンパイラを使ってstage1標準ライブラリをビルドし、それを使って**再度stage1コンパイラを実行し、stage2コンパイラ**を生成する。stage2コンパイラはstage1標準ライブラリとリンクされる。

各段階のビルド成果物は`build/<host-triple>/stageN-{std,rustc,tools,test}/`という個別ディレクトリに分離され、Cargoの通常のビルド機構(1つの`cargo`呼び出しが1段階を埋める)にできる限り委譲される。

**LAMINARIAへの示唆**: この構造は、「1つのコンパイラバイナリが常に唯一の真実である」という前提を最初から取っていない。**異なる世代のコンパイラが同時に存在し、互いにビルドし合う**という構成を、Rustコンパイラ自身の開発プロセスが恒常的に採用している。LAMINARIAが「target固有の専用コンパイラ群」を段階的に構築する場合も、ある段階では既存rustc(bootstrap用)を使い、別の段階ではLAMINARIA自身が生成したコンパイラを使う、という同型の多段階構造を取れる可能性がある。これは節1で確認したOCaml→Rust移行の構造(移行期に2つの実装が共存する)と同じパターンの、恒常化されたバージョンである。

参照: `src/bootstrap/README.md`(<https://raw.githubusercontent.com/rust-lang/rust/main/src/bootstrap/README.md>)、rustc-dev-guide bootstrapping章(<https://rustc-dev-guide.rust-lang.org/building/bootstrapping/intro.html>)。

## 3. バックエンド切り替え機構 — trait経由の完全にプラガブルな設計、実装コードで確定

`compiler/rustc_interface/src/util.rs`の`get_codegen_backend`関数(GitHub Contents APIでファイル構造を確認後、rawで裏取り)を読むと、rustc本体は次のロジックでコード生成バックエンドを選択する。

```rust
pub fn get_codegen_backend(
    early_dcx: &EarlyDiagCtxt,
    sysroot: &Sysroot,
    backend_name: Option<&str>,
    target: &Target,
) -> Box<dyn CodegenBackend> {
    // ...
    let backend = backend_name
        .or(target.default_codegen_backend.as_deref())
        .or(option_env!("CFG_DEFAULT_CODEGEN_BACKEND"))
        .unwrap_or("dummy");

    match backend {
        filename if filename.contains('.') => load_backend_from_dylib(early_dcx, filename.as_ref()),
        "dummy" => || Box::new(DummyCodegenBackend),
        #[cfg(feature = "llvm")]
        "llvm" => rustc_codegen_llvm::LlvmCodegenBackend::new,
        backend_name => get_codegen_sysroot(early_dcx, sysroot, backend_name),
    }
}
```

決定的な発見は3点:

1. **`"llvm"`は特別扱いされたビルトインの1つに過ぎない** — `#[cfg(feature = "llvm")]`というコンパイル時フィーチャーフラグの下にあり、LLVMバックエンドはrustc本体にとって「唯一の真実」ではなく「ビルド時に選択可能な複数の実装の1つ」として最初から設計されている。
2. **`.`を含む文字列は動的ライブラリのパスとして扱われる**(`load_backend_from_dylib`) — 任意の外部バックエンドを、rustc本体の再コンパイルなしに`.so`/`.dylib`としてロードできる。
3. **それ以外の名前(`"cranelift"`等)は`get_codegen_sysroot`経由で、sysroot内の`codegen-backends/`ディレクトリから対応する動的ライブラリを探索する** — これはrustc_codegen_craneliftが実際にrustcへ統合される方法そのものである。同関数(`util.rs` 522行〜)は`sysroot.all_paths()`を辿って`codegen-backends`ディレクトリを探し、そこにある動的ライブラリを`dlopen`する。

**LAMINARIAへの示唆**: rustc自身が既に「単一の中間表現(LLVM IR)を前提としたバックエンド」ではなく、**`CodegenBackend` traitを実装する任意の動的ライブラリを実行時に差し替えられる**という設計を持っている。これは節3(全体設計ドキュメントの問題D)が主張する「target固有の専用コンパイラを束ねる」というアーキテクチャの、**rustc自身による直接の先例**である。LAMINARIAがtarget固有バックエンドを実装する場合、この`CodegenBackend` trait相当の境界を自前で設計するのではなく、rustcが既に持つこの差し替え可能性を土台にできる可能性がある(ただし、trait自体はLLVM IR以外の低水準表現も許容する形で設計されているか、Rustのownership/borrow情報をどこまで運べるかは別途検証が必要)。

参照: `compiler/rustc_interface/src/util.rs`(345-609行、`load_backend_from_dylib`/`get_codegen_backend`/`get_codegen_sysroot`関数)。

## 4. GCC Rust frontend(gccrs) — 「別の専用コンパイラを最初から作る」実例、現在進行形

gccrsは、GCCのフロントエンドとしてRustを新規実装するプロジェクトであり、mrustc/rustc_codegen_gcc/Miriと並ぶRust代替実装の1つとして[意味論から「中間表現」を削除する — 節5.2](removing-intermediate-representation_ja.md#52-gccrs-gcc-rust--フロントエンドを独自実装しgimplertlという別の汎用中間表現へ接続する設計)で既に詳細比較を行った(LLVM IRを経由しないがGIMPLE/RTLという別の汎用中間表現を経由する設計、成熟度比較表を含む)。本節ではLAMINARIAの実務的な工数見積もりという観点だけを補足する。

2026年5月時点の進捗(公式月次報告、rust-gcc.github.io): Linuxカーネルコンパイル対応が35%、`core`クレートの早期名前解決が97%完了、`alloc`クレート対応はGSoC学生が2026年5月から着手開始したばかりで、Drop/デストラクタ実装も同時期に着手開始という段階にある。

**LAMINARIAへの示唆**: gccrsはGCCという既に完成した最適化・コード生成基盤の上にフロントエンドだけを新規実装しているにもかかわらず、2026年時点でまだ`alloc`クレートの基本対応に着手したばかりである。これは**「LLVM IR相当の汎用中間表現を経由しない完全な独自Rustフロントエンド」がどれほど巨大な工数を要するプロジェクトか**を示す直接の実測データであり、LAMINARIAが「target固有の専用コンパイラを最初から作る」道を選ぶ場合の工数見積もりに対する重要な参照点である。楽観的な見積もりへの抑止力として、節3(rustcのバックエンド差し替え可能性を土台にする、既存の専用コンパイラを束ねる道)の相対的な現実性を裏付ける。

参照: gccrs公式月次報告(<https://rust-gcc.github.io/2026/06/02/2026-05-monthly-report.html>、<https://rust-gcc.github.io/2025/08/05/2025-07-monthly-report.html>ほか)、"Progress toward compiling Linux with gccrs"(daily.dev)。

## 5. no_std/no_core — Rustコンパイルの最小構成、LAMINARIAが最初に対応すべきサブセットの参考

`no_std`は「`std`ではなく`core`クレートにリンクする」というクレートレベル属性であり、`core`は「プログラムが動作するシステムについて一切の仮定を置かない」`std`のプラットフォーム非依存サブセットである。`no_core`はさらに`core`クレート自体のプレリュード注入も止める、より根源的な属性である。

Embedonomicon(公式ドキュメント)が示す最小`no_std`/`no_main`プログラムの実装(実際に裏取り)は次の要素で完結する:

```rust
#![no_std]
#![no_main]

#[panic_handler]
#[inline(never)]
fn panic(_panic: &PanicInfo<'_>) -> ! {
    loop {}
}
```

加えて`panic = "abort"`をプロファイルに設定することで、`eh_personality`(例外処理ランドスケープ、[中間表現削除研究](removing-intermediate-representation_ja.md)の節3.4で確認したItanium `landingpad`/Windows SEH funcletの分岐そのもの)を丸ごと不要にできる。

**LAMINARIAへの示唆**: これはLAMINARIAが最初に対応すべき最小のRustサブセットを具体的に定義する材料になる。`no_std`+`panic = "abort"`という組み合わせは、標準ライブラリのOS依存機能(アロケータ、スレッド、I/O)だけでなく、**節3.4で問題視したtarget依存の例外処理意味論(landingpad/funclet)自体を最初から要求しない**という、LAMINARIAの実装難易度を大きく下げる境界線を提供する。「target固有の専用コンパイラを最初から作る」場合、まず`no_std`+`panic=abort`という最小サブセットに対応するコンパイラを構築し、その後段階的に`alloc`、`std`のOS依存機能へ拡張するという順序が、gccrsの実際の進捗順序(`core`→`alloc`→`std`)とも一致する自然な優先順位になる。

参照: RFC 1184 "Stabilize no_std"(<https://rust-lang.github.io/rfcs/1184-stabilize-no_std.html>)、Embedonomicon "The smallest #![no_std] program"(<https://docs.rust-embedded.org/embedonomicon/smallest-no-std.html>)、The Embedded Rust Book "no_std"(<https://docs.rust-embedded.org/book/intro/no-std.html>)、RFC 2480 "liballoc"(<https://github.com/rust-lang/rfcs/blob/master/text/2480-liballoc.md>)。

## 6. 総合: LAMINARIAが参照すべき優先順位

上記5件の先行事例を、LAMINARIAの問題D(target固有専用コンパイラを作る、または既存専用コンパイラを束ねる)への直接性で順位付けすると:

1. **rustc自身のバックエンド差し替え機構(節3)** — 最も直接的な先例。`CodegenBackend` trait + 動的ライブラリロードという境界が既に存在し、LAMINARIAが新規に設計する必要のある「複数専用コンパイラを束ねる境界」の土台になり得る。
2. **rustcのstage0/1/2ブートストラップ(節2)** — 「異なる世代・実装のコンパイラが互いをビルドし合う」という多段階構造の先例。LAMINARIA自身が段階的にセルフホスティングへ移行する際の直接のモデルになる。
3. **no_std/no_core(節5)** — LAMINARIAが最初に対応すべき最小Rustサブセットの具体的な境界線を提供する。target依存の例外処理意味論を最初から除外できる実用的な出発点。
4. **gccrs(節4)** — 「target固有の専用コンパイラを最初から作る」道の工数の現実的な下限を示す実測データ。楽観的な見積もりへの抑止力として参照する。
5. **rustcのOCaml→Rustブートストラップ史(節1)** — 「言語自身の実装もバックエンドも入れ替わり得る」という歴史的な安心材料だが、具体的な設計への示唆は節2・3ほど直接的ではない。

## 7. まだ解けていないこと

- `CodegenBackend` traitの実際の定義(`rustc_codegen_ssa::traits::CodegenBackend`)を、LAMINARIAのtarget固有バックエンドがどこまで実装可能か(LLVM IR以外の低水準表現を許容する設計になっているか)はまだ検証していない。
- gccrsが実際に遭遇している設計上の困難(名前解決の3namespace対応、メタデータ出力欠陥、Drop実装)が、LAMINARIA自身の独自フロントエンド実装でも同型で発生するかどうかはまだ検証していない。
- no_std+panic=abortという最小サブセットが、LAMINARIAの[8つの宣言済みtarget](../../01-foundations/joint-symbol-schedule-layout-design_ja.md#4-現時点の仮説アーキテクチャ-unified-symbol-graph)それぞれで、どこまでの実用的なプログラムをカバーできるかの具体的な棚卸しはまだ行っていない。

## 関連文書

- [全体設計 — なぜ「一つの計算システム」でなければならないか](../../01-foundations/joint-symbol-schedule-layout-design_ja.md) — 問題D(target固有専用コンパイラ、または既存専用コンパイラを束ねる)
- [意味論から「中間表現」を削除する](removing-intermediate-representation_ja.md) — Rust IR→LLVM IR→バイナリの詳細検証
- [LLVM内部の実測記録](llvm-internals-observed_ja.md) — Cranelift実測失敗の記録(issue #48)

# 上流早期枝刈りの先行研究調査 — 理論と実装の限界

調査日: 2026-09-15

## 位置づけ

本書は[Lane B基礎研究](lane-b-efficient-compiler-computation-foundations_ja.md)の§2先行研究地図を補強する。Lane Bの既存調査はビルドシステム・コンパイラ内部query系(Build Systems à la Carte、rustc query system、Salsa、ThinLTO)に厚く、「**意味解析やコンパイルそのものを始める前に、型システムや静的解析、パッケージ解決だけで不要と証明する**」という、より上流かつ理論寄りの系統が手薄だった。本書はその欠落を、学術研究(理論系)と既存ツール実装(実装系)の両面から埋める。

問いは単純である。[cross-layer-reachability-pruning_ja.md](../toolchains/cross-layer-reachability-pruning_ja.md)の表「枝刈りの層」がいう「resolution前」「source discovery時」という最も早い段階で、実際にどこまで計算コストを避けられるかは、既存の学術研究・実装のどちらにも、まだ完全な解はない。

## Part 1: 理論系 — 型システム／静的解析による早期到達不能判定

### 1.1 Program Slicing — 出力criterionから逆算する到達可能性の原型

Weiserの古典的定義は、スライシング基準`(p, v)`(プログラム位置`p`における変数`v`)を指定し、その値に影響しうる文の集合だけを、データフロー依存とコントロールフロー依存を**逆向きに**再帰的に辿って抽出する(backward slicing)。[^weiser]

「criterion(最終的に必要な出力)から逆算して影響を与えるコード片だけを抽出する」という構造は、LAMINARIAの「native executableのentry/export/undefined symbolをrootに、影響を与える上流のsource/module/packageだけを辿る」という設計と方向性が完全に同型である。ただしWeiserのスライシングは単一プログラム内の文単位の依存グラフが対象で、package選択・crate境界・ABI・link順序といったマルチレイヤー依存は扱わない。適用段階は**意味解析後(AST/CFG構築後)**——parse前には適用できない。

### 1.2 GHC Demand Analysis / Absence Analysis — 「使われない値の計算自体をしない」証明

strictness analysisは「ある引数が必ず(遅延評価されずに)forceされるか」を判定する。absence analysisはそれとは独立に「**ある引数がそもそも一切使われない**」ことを証明し、demand signature(`<S,U>`ペア形式でGHC Core上に表現される)として関数シグネチャに埋め込み、worker-wrapper変換によって未使用引数を呼び出し規約から完全に取り除く。「値を計算してから捨てる」のではなく「そもそも計算しない」ことを、型付きの中間表現(Core)上で保証する。[^ghc-demand][^ghc-demand-jfp][^ghc-cardinality]

適用段階は**Core-to-Core最適化パス**(型検査・脱糖後、コード生成前の中間表現に対する反復的変換)。これはLane BのB-H2(semantic early cutoff)と直接同型の設計思想を持つ、既に実運用されている実装である。GHCのdemand signatureは`ProvenDead`証明に相当する型付き証拠を、関数シグネチャという安定な単位にmemoizeしている点で、Lane B §6の「cache keyはsemantic input digestを含む必要がある」という要件をHaskell型システムの枠内で先行実装した事例と言える。ただし単一言語・単一コンパイラ内部に閉じており、LAMINARIAが必要とするcross-ecosystem(Cargo/Nimble/C/C++)の意味論境界を跨ぐ拡張はスコープ外。

### 1.3 Reachability Types — 用語の意味論的な誤接続に注意

学術的に確立された「reachability types」は、**メモリエイリアシング・変数の生存範囲追跡**(ある変数がどのヒープオブジェクトに到達しうるかを型でトラッキングし分離性を保証する理論)を指し、LAMINARIAが必要とする「呼び出しグラフ／シンボルの到達可能性」とは別概念である。[^reachability-types] 両者は「reachability」という語を共有するのみで、目的(前者はメモリ安全性の型検証、後者はコンパイル前の枝刈り)が異なる。LAMINARIAが「型で到達可能性を表現する」試みを探す場合、この名称の理論を追っても直接的な解にはならない——より近い先例は1.2のGHC demand signatureである。

### 1.4 Supercompilation / Partial Evaluation — 分岐そのものを消す特化技法

プログラムを(部分的に既知の)入力に対してシンボリックに実行し、実行不可能と判明した分岐(doomed branch)を特化の過程で除去する。「情報伝播による変数値・等価性の把握」と「決定的な評価ステップの先行計算」の2本柱で、コンパイル時に分岐そのものを刈る。[^supercompilation] 適用段階は意味解析後、コード生成前の中間表現に対する変換で、GHCのdemand analysisと近い段階だが、supercompilationは「値が使われるか」ではなく「制御フローが到達しうるか」をシンボリック実行で判定する点が異なる。B-H1(cross-layer demand)と技術的に近縁だが、変換コスト自体が高く(指数的に木が膨らみうる)、Lane B §11の反証条件「graph構築・hash・serialization costが避けたworkを上回る」に該当するリスクが高い技法として慎重な参照が必要。

### 理論系の総括

| 系統 | 適用段階 | B-H1/B-H2との対応 |
|---|---|---|
| Program slicing (Weiser 1981) | 意味解析後(AST/CFG) | B-H1の理論的起点(criterion逆算) |
| GHC demand/absence analysis | Core-to-Core(型検査後、コード生成前) | B-H2の実運用先行実装、最も具体的 |
| Reachability types | 型検査時 | 対応なし(別概念、誤引用注意) |
| Supercompilation | 意味解析後、コード生成前 | B-H1と近縁、コスト面で反証条件リスク |

いずれも単一言語・単一コンパイラ内部に閉じており、cross-ecosystem境界(Cargo/Nimble/C/C++)を跨いだ使用性解析の先行研究は、本調査の範囲では確認できなかった。これはLane Bの独自性を裏付けるlandscape claimである。

## Part 2: 実装系 — パッケージ解決・ビルドグラフ最上流での早期枝刈り

B-H1(「最終artifact/testのrootからpackage/source/IR/artifactへ需要を伝えることで、全候補・全code先行展開より正しいclosureのまま外部actionとpeak memoryを削減できる」)を、5つの既存実装系統で検証した。**「resolution前にmetadata fetch・candidate expansion自体を避ける」ことに完全に成功している実装は見当たらず、B-H1は既存ツールでは部分的にしか達成されていない未解決の主張であることが確認できた。**

### 2.1 Cargo — lock生成は全候補を先に展開する

[Cargo Book: Dependency Resolution](https://doc.rust-lang.org/cargo/reference/resolver.html)は「Cargo.lockを生成する際、workspace全メンバーの全featureが有効であるかのように依存グラフを構築する」と明記する。`cfg(windows)`限定の依存やoptional dependencyであっても、**lock生成フェーズでは条件を無視して依存グラフに含める**。これはB-H1への直接的な反証材料であり、Cargoの設計は「まずlockファイル生成時に最大限展開してから、実コンパイル時に縮減する」という2段階構成を取っている。`resolver = "2"`はtarget-specific依存の望まないunificationを緩和するが、lock生成時のregistry index照会自体は全候補に対して発生する構造は変わらない。[^cargo-resolver][^rfc2957][^rfc3692]

実際に避けられるのはregistry indexクエリ(軽量なメタデータ)の対象ではなく、`.crate`ファイル本体のダウンロード(選択版のみ)。つまり候補探索自体は避けられていない。

### 2.2 Bazel/Buck2 — loading/analysis/execution phaseの分離と、正確さとの引き換え

Bazelは明確に3段階に分かれる: **Loading phase**(BUILDファイルをparseし設定を適用しない生の対象グラフを構築)、**Analysis phase**(設定を適用し`select()`を評価、configured target graphを構築)、**Execution phase**(action実行)。[^bazel-query]

`bazel query`はloading phaseのみで完結し`select()`を全ての可能な解決として過保守的に扱う(分岐を刈れない)。`bazel cquery`は[analysis phaseの結果に対して動作する](https://bazel.build/query/cquery)ため`select()`を正確に評価できる代償として、analysis phaseまでの計算コスト(BUILDファイル全読み込み+configured target構築)を先に払う必要がある。公式ドキュメントは「cqueryはbuild actionが実行される直前までのビルドをトリガーする」と明記しており、action実行は避けるがその手前の全段階の計算は避けていない。[^bazel-cquery]

Bazelの3段階分離は、「loading(軽い)→ analysis(重い、しかし正確)」という順序として、resolution前で完全な枝刈りをすることの難しさを裏付ける。configuration依存のbranch判定には結局configured target相当の重い解析が必要、というのがBazelの現実解である。

### 2.3 Nix — evaluation/realization分離と、それを破壊するIFD

Nixは設計上、**evaluation**(Nix式評価、derivationの生成、ビルドグラフ構築)と**realization**(実際のビルド実行)を明確に分離する。この分離自体はB-H1が理想とする「resolution前」の純粋な段階に極めて近い——evaluation段階は原理的にファイルシステムI/Oやネットワークアクセスを要求せず、純粋にderivationのグラフだけを構築できる。

しかし**Import From Derivation (IFD)**はこの分離を破壊する。IFDは「あるderivationのビルド結果(output)の内容に依存するNix式」を許すため、評価を進めるためには**そのderivationを実際にビルド(realize)しなければならない**。公式ドキュメントは「evaluationとbuildingがinterleave(相互に絡み合う)してしまう」と明記し、解説記事は「これによりレイヤーごとの逐次的なフェーズを強制され、並列性を失う」と指摘する。[^nix-ifd][^nixcademy-ifd]

**これはB-H1・B-H2にとって決定的な反例パターンである。** 「resolution前の純粋なグラフ構築」という段階を設計上どれだけ綺麗に分離しても、**動的性(あるノードの構造が別ノードのビルド結果に依存する)が一箇所でも混入すると、その分離全体が崩れ、逐次実行に退化する**。これは[cross-layer-reachability-pruning_ja.md](../toolchains/cross-layer-reachability-pruning_ja.md)の表「枝刈りの層」が暗黙に仮定している「各層が独立に早期判定できる」という前提そのものへの反証であり、[cross-language-compiler-pruning-landscape_ja.md](../toolchains/cross-language-compiler-pruning-landscape_ja.md)で既に確認した「Go/Swiftの動的機構が保守化粒度全体を退化させる」現象、およびissue #62で実測した「cargo incrementalがworkspace crate全体のCGU命名粒度を退化させる」現象と、build-graph構築レベルでも同型の現象が起きることを示す。

### 2.4 npm/Node.js — conditional exportsは真の「resolution前」枝刈りに最も近い(が粒度限定)

Node.js公式ドキュメントによれば、`package.json`の`exports`フィールドのconditional resolutionは「JSON記述順で条件を評価し、最初にマッチしたものを採用する」設計で、「これはディスクアクセスを最小化し将来のネットワークベース環境での効率的な解決を保証するために設計されている」と明記されている。[^nodejs-packages]

調査した5系統の中で唯一、「候補を実際に開く前に静的な文字列マッチだけで解決先を1つに絞り込む」という、真の意味での「resolution前」枝刈りに近い設計である。ただしスコープはパッケージ内の単一エントリポイント選択に限定され、Cargo/Bazelのような依存グラフ全体のtransitive closureには適用されない、粒度の小さい枝刈りである。バンドラ間でも「どのconditionをデフォルト有効にするか」の差異があり(esbuild/Rollupは`module`を既定有効、webpackは既定無効)、これ自体が「解決前の枝刈り基準が完全にはツール非依存にならない」ことを示す実例である。

### 2.5 Gradle/Maven — 宣言的excludeは自動証明ではない

`exclude`・dependency constraintsは「この依存グラフパスは要らない」と明示できる機構だが、これは利用者が明示的に指定する条件付きrootであり、ツール自身が自動的に到達不能性を証明する機構ではない。Cargoのresolver v2と同様の「人間が保守的にexcludeを宣言する」パターンであり、B-H1が目指す「自動的なcross-layer demand伝播」とは性質が異なる。

### 実装系の総括

| 実装 | resolution前の候補展開を避けているか | 限界 |
|---|---|---|
| Cargo | ✗ lock生成時に全feature/全target候補のindexクエリを実施 | `.crate`本体DLは選択版のみという粒度の限定的節約のみ |
| Bazel query | ✓ loading phaseのみで完結 | `select()`を評価できず過保守的(全分岐を残す) |
| Bazel cquery | ✗ analysis phase相当の全計算を先に払う | 正確だが「action実行直前まで」の重い処理を伴う |
| Nix evaluation | ✓ 設計上は分離済み | IFDが混入すると分離が崩壊し逐次実行に退化 |
| npm exports | ✓ 文字列マッチで単一エントリに絞り込み | 依存グラフ全体でなく単一パッケージのエントリポイント選択に限定 |
| Gradle/Maven exclude | ✓(宣言的) | 自動証明ではなく人手のroot宣言 |

「候補展開自体を避ける」ことに部分的にでも成功しているのはNix(動的性がない限り)とnpm exports(粒度限定)のみであり、いずれも**動的性や粒度の制約下でのみ**成立する。Cargo・Bazelという、LAMINARIAが直接対象とするエコシステム自体が、resolution前の完全な枝刈りを構造的に達成できていない。これはLane B §11の反証条件「dynamic/implicit dependencyを保守的に扱うと全候補展開と同等になる」が、既存ツールの設計でも繰り返し現実化していることの強い裏付けである。

## 総合的な結論: Lane BのB-H1/B-H2への反映

1. **B-H2(semantic early cutoff)には具体的な既存実装の先行事例がある**(GHC demand/absence analysis)。型付き中間表現上でdemand signatureをmemoizeする設計は、Lane B §6のcache key要件を単一言語スコープで先行実装した参照実装として扱える。
2. **B-H1(cross-layer demand)は、既存の全ツールで未達成の主張として位置づけるべきである。** Nixのevaluation/realization分離が示す「動的性が一箇所でも混入すると層分離全体が崩壊する」現象は、LAMINARIAがB-H1を実証する際の最大の技術的障壁になる。**Nixが示した制約を、型システムかcontract化された条件付きroot(FFI export, dlopen等)の仕組みでどう克服するか**が、Lane B実験計画(E0〜E4)の核心的な設計課題である。
3. Program slicingという「criterionから逆算する」古典的理論は方向性としてLAMINARIAの設計と一致するが、cross-ecosystem境界を跨ぐ拡張は理論的先例がなく、LAMINARIA自身が埋めるべき研究ギャップである。

## Sources

[^weiser]: M. Weiser, "Program Slicing," *Proc. 5th ICSE*, IEEE, 1981, pp. 439–449. https://www.cse.msu.edu/~cse870/Public/Homework/SS2003/HW5/p439-weiser.pdf
[^ghc-demand]: S. Peyton Jones, P. Sestoft, J. Hughes, "Demand analysis," draft, Microsoft Research, 2006. https://www.microsoft.com/en-us/research/wp-content/uploads/2006/07/demand-1.pdf
[^ghc-demand-jfp]: I. Sergey, D. Vytiniotis, S. Peyton Jones, "Theory and Practice of Demand Analysis in Haskell," *JFP* 27:e11, 2017. https://www.microsoft.com/en-us/research/wp-content/uploads/2017/03/demand-jfp-draft.pdf
[^ghc-cardinality]: I. Sergey, D. Vytiniotis, S. Peyton Jones, J. Breitner, "Modular, Higher-Order Cardinality Analysis in Theory and Practice," *JFP*, 2017. https://www.cambridge.org/core/services/aop-cambridge-core/content/view/5D815BD54F43FD49146B2F4154565DE4/S0956796817000016a.pdf/modular_higher_order_cardinality_analysis_in_theory_and_practice.pdf
[^reachability-types]: C. Bao et al., "Polymorphic Reachability Types: Tracking Freshness, Aliasing, and Separation in Higher-Order Generic Programs," arXiv:2307.13844. https://arxiv.org/abs/2307.13844
[^supercompilation]: "Supercompilation: Techniques and Results." https://link.springer.com/chapter/10.1007/3-540-62064-8_20
[^cargo-resolver]: Cargo Book, "Dependency Resolution." https://doc.rust-lang.org/cargo/reference/resolver.html
[^rfc2957]: Rust RFCs, "2957-cargo-features2." https://rust-lang.github.io/rfcs/2957-cargo-features2.html
[^rfc3692]: Rust RFCs, "3692-feature-unification." https://rust-lang.github.io/rfcs/3692-feature-unification.html
[^bazel-query]: Bazel documentation, "Query reference." https://bazel.build/reference/query
[^bazel-cquery]: Bazel documentation, "cquery." https://bazel.build/query/cquery
[^nix-ifd]: Nix Reference Manual, "Import From Derivation." https://nix.dev/manual/nix/2.35/language/import-from-derivation
[^nixcademy-ifd]: "What is IFD? Ups and downs." https://nixcademy.com/posts/what-is-ifd-ups-and-downs/
[^nodejs-packages]: Node.js documentation, "Modules: Packages." https://nodejs.org/api/packages.html

## 未検証・スコープ外の注記

- 本調査はWeb検索・公式ドキュメント・学術論文PDFに基づく。占有的な網羅性は主張しない。
- occurrence typing / flow typingは調査したが本テーマとの強い関連が見出せず、詳細な扱いを見送った。
- Gradle/Mavenは簡潔な扱いに留めた。より詳細な依存解決戦略の比較は別調査が必要。

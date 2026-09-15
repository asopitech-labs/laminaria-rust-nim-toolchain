# 他言語コンパイラ／ビルドツールの枝刈り(dead code elimination)先行研究調査

調査日: 2026-09-15

## Executive summary

[cross-layer reachability pruning](cross-layer-reachability-pruning_ja.md)が定義する`LivenessState =
Unknown | Live | ProvenDead | RetainedConservatively`というモデルと、明示root／条件付きrootという
root set分類を、JVM(GraalVM Native Image, ProGuard/R8, jlink)、JavaScript/TypeScript(webpack, Rollup,
esbuild, Closure Compiler, V8)、Swift、Go、.NET(IL Trimming, Native AOT)の実装と対応づけて調査した。

結論は3点。

1. **動的性(reflection、eval、dlopen相当)がある言語ほど、静的到達可能性解析は縮退し、
   「解析できない箇所をout-of-band契約でユーザーに申告させる」設計に収束する。** JS の
   `sideEffects: false`/`/*#__PURE__*/`、GraalVMの`reflect-config.json`、.NETの
   `[DynamicallyAccessedMembers]`、Goの`reflect.Value.Method`検出はいずれも同型の問題への
   異なる解法であり、LAMINARIAが直面するRust/Nim/C/C++ FFI境界の`Ffi`/`DynamicLookup`条件付き
   rootと構造的に同じ問題である。
2. **「削除」と「生成しない」は異なる最適化ステージであり、これを明確に区別している実装は
   .NET Native AOTのみ**だった。.NET公式ドキュメントは「PublishAotが使うtrimmingはAOTコンパイラに
   組み込まれている……コンパイラは何かを削除しているのではなく、単に生成していないだけである」と
   明記しており、これはLAMINARIAの`cross-layer-reachability-pruning_ja.md`冒頭の主張(「LLVM DCEは
   既に払った計算コストを回収しない、必要なのは上流で不要な計算そのものを止めること」)と
   完全に同型の議論が既に決着している先行事例である。
3. **GraalVM Native Imageのclosed-world points-to analysisが、LAMINARIAの目標構造に最も近い
   既存実装**だが、動的性への対応を自動化せず「実行時トレースエージェントで経験的にconfigを
   生成する」という、静的健全性を放棄した現実的だが理論的に不完全な設計を採用している。

これらはLAMINARIAが独自に解くべき問題ではなく、**各エコシステムが個別に発明した部分解を、
type-safeなcross-ecosystem provenance付きgraphとして統合する**というLAMINARIAの立ち位置を
補強する landscape claim である。新しい反例が見つかれば更新する。

## 1. JVM / GraalVM Native Image — closed-world points-to analysis

GraalVM Native Imageは「ビルド時点でアプリケーション全体が確定しており実行時に新しいクラスが
ロードされない」という**closed world assumption**を前提とする。これはLAMINARIAの「native
executableから逆算する」ゴールと構造的に同一の前提である。

解析エンジンはcontext-insensitive/field-basedの**points-to analysis**で、`main`をrootに、
到達可能なメソッドから呼ばれるメソッド・アクセスされるフィールド・インスタンス化される型を
再帰的に収集する。virtual dispatchは「その時点で到達可能と判明している具象型の集合」に基づいて
解決先候補を絞り込み、新しい到達可能型の発見と解決先候補の拡大が相互再帰するため
**fixed-pointまで反復**する(§解決と枝刈りの協調の手順9と同型)。[^graalvm-static-analysis]

closed world assumptionの弱点はreflection/JNI/dynamic proxy/serializationであり、GraalVMは
これらを解析不能として扱い、`reflect-config.json`/`jni-config.json`/`proxy-config.json`/
`serialization-config.json`という**ユーザー明示のconfiguration file**で代替する。これらは
`native-image-agent`でアプリを実際に走らせてトレースし自動生成することが多い。つまり
**動的到達可能性を静的解析で導出することを諦め、実行時トレースによる経験的root集合の収集に
切り替えている**。GraalVMにはconfig漏れに対する自動的な保守的フォールバックがなく、漏れは
実行時`ClassNotFoundException`等の**silent miscompile相当の失敗**として現れる。これは
LAMINARIAの正しさの条件(「必要codeを消すfalse negativeはmiscompileである」)を機械的に
保証することの難しさを裏付ける実例である。[^graalvm-metadata]

`Feature`インタフェースは静的解析の各fixed-pointイテレーション中にフックされ、動的にroot
(reachable methods/classes)を追加登録できる。Spring/Quarkus/Micronautはこれを使いフレームワーク
固有のリフレクション使用箇所を自動登録する。[^graalvm-features]

ProGuard/R8(Android向けJVMバイトコードshrinker)はGraalVMより粗い**Class Hierarchy Analysis
(CHA)**でreachabilityを解く。`-keep class ...`というkeep rule DSLで、エントリポイント・
マニフェスト参照クラス・native修飾子メソッドを暗黙的にroot化する。reflectionパターンは
ヒューリスティック検出で**警告のみ**を出し自動保持しない——LAMINARIAの正しさの条件より緩い
立場である。[^r8][^proguard]

jlink(JPMS)はモジュール記述子の`requires`/`exports`という宣言的依存に基づく、package層のみの
枝刈りであり、semantic item層の精密な到達可能性は見ない。[^jlink]

points-to analysisの理論的背景はAndersen(1994、context-insensitive subset-based)と
Steensgaard(1996、unification-based高速版)に遡る。GraalVM固有の実装論文は Wimmer et al.,
"Initialize Once, Start Fast: Application Initialization at Build Time" (OOPSLA
2019)。[^graalvm-init]

## 2. JavaScript/TypeScript — ES modulesの静的性への依存

webpack/Rollup/esbuildのtree shakingは共通して**ES modulesの静的import/export構造**を
根拠にする。CommonJS(`require(variableName)`)は静的解析不能なため対象外——「semantic
query」層でのroot探索が言語仕様の静的性そのものに依存する点が、Rust/NimのFFI境界より
むしろ単純な制約になっている。[^webpack-tree-shaking]

esbuildはlinking段階で、エントリポイントの副作用付きパーツから出発しシンボル参照エッジと
副作用エッジを辿って到達したパーツのみ残す(LAMINARIAの`Live` edge到達探索と同型)。[^esbuild-arch]

**副作用の扱いがJS系全体の中心的な保守化ポイント**である。`package.json`の`sideEffects`は
デフォルト`true`(全モジュールに副作用ありと仮定)で、ユーザーが明示的に`false`または除外パスを
申告した場合のみサブツリーを除去できる。`/*#__PURE__*/`アノテーションは文レベルの副作用なし
宣言。Rollupは「副作用がないことを証明するのが不可能な場合が必ずある」という理由で、
**全モジュール列挙ではなく副作用ありモジュールだけを明示させる**(デフォルト安全側)設計を
選んでいる。[^rollup-issue][^webpack-tree-shaking]

Closure Compiler(ADVANCED_OPTIMIZATIONS)は制御フローグラフ上のreachability analysisに加え、
**externsファイル**(コンパイル対象外から呼ばれるシンボルの明示宣言、LAMINARIAの「明示root:
manifest／public contractで要求したexport」に対応)と`@export`/`@nocollapse`アノテーションで
動的性を扱う。決定的な違いとして、Closure Compilerの公式ドキュメントは`eval()`や文字列による
動的プロパティアクセスを「解析不能」と明記した上で、**わからなければユーザーの責任で壊れる**
側に倒している。これはLAMINARIAの「静的に精密なtarget setが得られない場合は over-approximate
して保持する」という規律と正反対の設計選択であり、対照事例として重要である。[^closure-limitations]

V8/TurboFan(JIT、AOTのLAMINARIAとは文脈が異なる参考比較)は、型が安定していると**証明ではなく
統計的推測**で最適化コードを生成し、前提が崩れたらinterpreterへdeoptimizeする。AOTには
実行時巻き戻しの機構がないため、この「たぶん正しい」楽観的枝刈りはLAMINARIAの正しさの条件
(削除は保守的規則で示せるものだけ)と両立しない。[^v8-turbofan]

## 3. Swift — witness table単位の粗粒度保守化

`-whole-module-optimization`はモジュール全体を単一コンパイル単位として扱い、非public関数の
未使用性を判定できるようにする。実装(`DeadFunctionElimination.cpp`)のroot判定
(`isAnchorFunction()`)は、外部から参照されうるlinkage・distributed関数(名前による実行時
ルックアップ)・dynamic replacement対象・Objective-Cメソッド(ランタイムmessage send経由)・
keypath参照関数などを常にLiveとする。[^swift-dfe]

**witness table/vtableは個々のメソッドでなく単位ごと丸ごと保守化**される。コメントには
「witness tableは"外側"から可視である。したがって全メソッドが呼ばれうる」とあり、protocol
conformanceの単位で粗く保守化する設計になっている。これはLAMINARIAの`RetainedConservatively`が
必ずしも最小粒度である必要はない、という設計上の先例である。`keepExternalWitnessTablesAlive`
フラグによる Early DFE(保守的) → Late DFE(積極的)の2段階実行は、LAMINARIAの「`Unknown ->
Live`を基本に単調に進め」という漸進的確信度引き上げモデルの実装例として参考になる。[^swift-dfe]

## 4. Go — interface変換に伴う保守化の爆発

`cmd/link`のdeadcode eliminationはエントリポイントからシンボルグラフを辿り非到達シンボルを
除去する。決定的な論点は**`reflect.Value.Method`/`MethodByName`が非定数引数で呼ばれている
場合、リンカは到達可能な全ての型のpublicメソッドを保持せざるを得ない**ことで、バイナリサイズが
約30%増加するケースが報告されている(golang/go issue #60221)。`-dumpdep`フラグで保守化理由を
診断できる仕組みはLAMINARIAの「各保持判断にroot、path、analysis version、toolchain identityを
provenanceとして付ける」という設計と同型である。[^go-deadcode][^go-issue-60221]

`golang.org/x/tools/cmd/deadcode`(リンカとは別のソースレベル静的解析ツール)の設計は明快:
「具体型がinterfaceへ変換された時点で、その型の全メソッドが動的呼び出しの対象候補になる」。
reflectを使うコードは「interface変換で登場した型全体 + reflect経由で派生可能な型全体」という
最も広い保守化範囲になる。これはissue #62 P0で観測した「動的解決可能性が一箇所でもあると
保守化範囲が爆発的に広がる」現象と同型であり、alopex-cliのworkspace member crateがすべて
incremental compilationでhash-only cguになった(P0実測)ことと構造的に類似する——**一つの
条件(動的機構/コンパイラオプション)が集合全体の解析粒度を退化させる**という現象がGoの
reflectとRustのincremental cguで独立に観測されている。[^go-deadcode-blog]

TinyGoは単一ステップコンパイルでLLVMの最適化パスをフルに活用し、Goのような関数ポインタ
テーブル事前計算をしない。「LLVMがinterface呼び出しを跨いで最適化できることが、追加のDCEで
得られる効果に見合う」という設計判断であり、組み込み向けゆえにreflect多用系ライブラリを避ける
文化を前提にできる。[^tinygo-interfaces]

## 5. .NET — 値の流れに沿った型注釈による動的到達可能性の明示

IL Trimming(`PublishTrimmed`)はエントリポイントからのwhole program analysisで到達可能な
メンバーのみを残す。`TrimmerRootAssembly`は明示root指定に相当する。[^dotnet-trimming]

**`[DynamicallyAccessedMembers]`が.NET trimmingの最重要機構であり、LAMINARIAのFFI/dynamic
lookup問題への直接的な参考になる。** リフレクションを使うメソッドのパラメータに「この型の
どんなメンバーが動的にアクセスされるか」を型システムで注釈し、trimmerは値が静的に追跡可能な
限りその型のメンバーを保持root候補として伝播させる。静的追跡が切れる箇所(フィールド経由、
条件分岐後など)では警告(IL2070/IL2077等)を出し、**注釈の伝播を呼び出し元へ要求する**——
呼び出し元が同じ注釈を持つまで警告が連鎖的に上流へ伝わる設計である。[^dotnet-dynamically-accessed]

役割分担は明確: `[DynamicallyAccessedMembers]`(値の流れに沿った到達可能性の型注釈)、
`[RequiresUnreferencedCode]`(注釈できないパターン用のエスケープハッチ、呼び出し元へ警告伝播)、
`[DynamicDependency]`(直接的なkeep指示、他の注釈で表現不可能なパターンの最終手段)、
`[UnconditionalSuppressMessage]`(開発者が不変条件により安全と保証して警告抑制)。公式
ドキュメントは「無効化の正当化は実際にreflectionの可視対象であったメンバーに対してのみ
許される」と明記しており、これはLAMINARIAの「retained nodeにはrootからのpathまたは
保守的保持理由がある」という正しさの条件と一致する。[^dotnet-prepare-trimming]

**Native AOT(ILCompiler)は「削除」ではなく「生成しない」という設計思想を明文化している。**
dotnet/runtime公式議論には「PublishAotが使うtrimmingはAOTコンパイラに組み込まれている。IL
levelのtrimming(ILLinkによるもの)は出力が再びILであることを前提にするため、しばしば
それほど刈り込めない。AOTコンパイラの仕事の一つはどのnative codeを生成するかを決めることであり
(genericsのためIL⇔native codeは1:1対応しない)、trimmingはこのプロセスの自然な副産物に
すぎない——コンパイラは何かを削除しているのではなく、単に生成していないだけである」と
ある。これはissue #62 P0の停止条件(「245,058個ではなく、実際にエントリポイントから到達可能な
最小集合だけが内部シンボルとして生成される」)が目指す理想形の、**既存実装での先行事例**である。
closed-world assumptionにより「あるinterfaceの実装が1つしかないとコンパイラが判定できれば、
interface呼び出しを直接呼び出しに置き換えられる」という最適化も可能になる。[^dotnet-aot-discussion][^dotnet-ilc-arch]

## LAMINARIA対応表(横断)

| LAMINARIA概念 | GraalVM | JS(webpack/Rollup) | Closure Compiler | Swift | Go | .NET |
|---|---|---|---|---|---|---|
| 明示root(Entry) | `main`メソッド | エントリバンドル | `main`/externs | public/open linkage | main+init | TrimmerRootAssembly |
| 条件付きroot(`Ffi`) | jni-config.json | — | externs | ObjCメソッド | cgo境界(未調査) | P/Invoke(未調査) |
| 条件付きroot(`DynamicLookup`) | reflect-config.json(手動) | sideEffects:false申告 | `@nocollapse` | dynamic replacement | reflect.Value.Method時に全型全メソッド保守化 | `[DynamicallyAccessedMembers]`(型注釈で精密化) |
| `RetainedConservatively`の自動フォールバック有無 | なし(config漏れ=実行時失敗) | あり(デフォルトsideEffects:true) | なし(わからなければ壊れる) | あり(witness table単位で丸ごと) | あり(型全体) | あり(警告付き、`Unknown`へ降格) |
| 段階的確信度引き上げ | Feature APIのfixed-point反復 | usedExports(AST)→minify(実削除)の2段階 | — | Early DFE→Late DFEの2段階 | — | IL trim→AOT生成の2段階 |
| 「削除」でなく「生成しない」 | 部分的(points-to analysisがcodegen前) | 部分的(usedExportsがminify前) | 該当せず | 該当せず | 該当せず | **Native AOTで明文化** |

## LAMINARIAへの示唆

1. 動的性への対応で「型注釈により値の流れに沿って保守化rootを精密化する」.NETの設計
   (`[DynamicallyAccessedMembers]`)は、LAMINARIAがRust/Nim/C/C++のFFI境界を`Ffi`/
   `DynamicLookup`という条件付きrootとして型システムレベルで扱おうとする際の直接的な
   設計参照になる。単なるon/off保守化ではなく、値の追跡が切れた地点で`Unknown`へ降格し
   警告を上流伝播する仕組みは、cross-layer-reachability-pruning_ja.mdの`LivenessReason::
   UnsupportedAnalysis`をより実装可能な形に具体化する候補である。
2. GraalVM Native Imageの「解析不能なら実行時トレースで経験的にconfigを生成する」設計は、
   LAMINARIAの正しさの条件(false negativeを許さない)を機械的に保証することの難しさを
   裏付ける反面教師である。LAMINARIAはこの妥協を取らない前提を保つべきである。
3. .NET Native AOTの「削除ではなく生成しない」という立場は、LAMINARIAが自身の研究の
   独自性を主張する際の最も直接的な比較対象であり、既に理論的な決着がついている論点として
   引用できる。一方でこの立場を実装レベルで達成しているのは.NETのみであり、他言語
   (Swift/Go/JVM/JS)はいずれも後段DCE/trimmingが主戦場である。
4. Swift/Goに共通する「一つの動的機構(witness table可視性、reflect.Value.Method)が
   集合全体の保守化粒度を退化させる」現象は、issue #62でLAMINARIA自身が観測した
   「cargo incrementalがworkspace member crate全体のCGU命名粒度を退化させる」現象と
   構造的に同型であり、他言語からの示唆というより、LAMINARIA自身の実測がこのクラスの
   現象の一般性を裏付けたと言える。

## Sources

[^graalvm-static-analysis]: Oracle, "[Native Image Basics — Static Analysis](https://www.graalvm.org/latest/reference-manual/native-image/basics/#static-analysis)." Accessed 2026-09-15.
[^graalvm-metadata]: Oracle, "[Reachability Metadata](https://www.graalvm.org/latest/reference-manual/native-image/metadata/)." Accessed 2026-09-15.
[^graalvm-features]: Oracle, "[Native Image Dynamic Features / Feature interface](https://www.graalvm.org/latest/reference-manual/native-image/dynamic-features/)." Accessed 2026-09-15.
[^graalvm-init]: C. Wimmer et al., "Initialize Once, Start Fast: Application Initialization at Build Time," OOPSLA 2019. https://dl.acm.org/doi/10.1145/3360610
[^r8]: Google, "[R8 documentation](https://r8.googlesource.com/r8/)." Accessed 2026-09-15.
[^proguard]: Guardsquare, "[ProGuard Manual — Keep Options](https://www.guardsquare.com/manual/configuration/usage#keepoverview)." Accessed 2026-09-15.
[^jlink]: Oracle, "[jlink documentation](https://docs.oracle.com/en/java/javase/17/docs/specs/man/jlink.html)"; OpenJDK, "[JEP 220: Modular Run-Time Images](https://openjdk.org/jeps/220)." Accessed 2026-09-15.
[^webpack-tree-shaking]: webpack, "[Tree Shaking guide](https://webpack.js.org/guides/tree-shaking/)." Accessed 2026-09-15.
[^esbuild-arch]: E. Wallace, "[esbuild architecture.md](https://github.com/evanw/esbuild/blob/main/docs/architecture.md)." Accessed 2026-09-15.
[^rollup-issue]: Rollup, "[Issue #5987 — sideEffects design discussion](https://github.com/rollup/rollup/issues/5987)." Accessed 2026-09-15.
[^closure-limitations]: Google, "[Closure Compiler — Advanced compilation limitations](https://developers.google.com/closure/compiler/docs/limitations)." Accessed 2026-09-15.
[^v8-turbofan]: "[V8 Engine: The Journey of JavaScript from Code to Execution](https://daily.dev/posts/v8-engine-the-journey-of-javascript-from-code-to-execution-junca4frw)." Accessed 2026-09-15.
[^swift-dfe]: Swift Project, "[lib/SILOptimizer/IPO/DeadFunctionElimination.cpp](https://github.com/swiftlang/swift/blob/main/lib/SILOptimizer/IPO/DeadFunctionElimination.cpp)"; "[Whole-Module Optimization in Swift 3](https://www.swift.org/blog/whole-module-optimizations/)." Accessed 2026-09-15.
[^go-deadcode]: A. Obregon, "[Dead Code Elimination in Go Builds](https://medium.com/@AlexanderObregon/dead-code-elimination-in-go-builds-119555fad1fd)." Accessed 2026-09-15.
[^go-issue-60221]: golang/go, "[Issue #60221 — cmd/link: way to determine why deadcode elimination was not performed](https://github.com/golang/go/issues/60221)." Accessed 2026-09-15.
[^go-deadcode-blog]: Go Project, "[Finding unreachable functions with deadcode](https://go.dev/blog/deadcode)." Accessed 2026-09-15.
[^tinygo-interfaces]: A. van Laethem, "[Interfaces in TinyGo](https://aykevl.nl/2018/12/tinygo-interface/)." Accessed 2026-09-15.
[^dotnet-trimming]: Microsoft, "[Trimming options - .NET](https://learn.microsoft.com/en-us/dotnet/core/deploying/trimming/trimming-options)." Accessed 2026-09-15.
[^dotnet-dynamically-accessed]: Microsoft, "[Prepare .NET libraries for trimming](https://learn.microsoft.com/en-us/dotnet/core/deploying/trimming/prepare-libraries-for-trimming)." Accessed 2026-09-15.
[^dotnet-prepare-trimming]: 同上。
[^dotnet-aot-discussion]: dotnet/runtime, "[Discussion #97288 — AOT: analyze trimmed assemblies pre native compilation](https://github.com/dotnet/runtime/discussions/97288)." Accessed 2026-09-15.
[^dotnet-ilc-arch]: dotnet/runtime, "[ILC Compiler Architecture](https://github.com/dotnet/runtime/blob/main/docs/design/coreclr/botr/ilc-architecture.md)." Accessed 2026-09-15.

## 未検証・スコープ外の注記

- Deno/Bunバンドラの独自DCE設計は、既存ツール(esbuild)と異なる一次設計文書を本調査では
  確認できなかった。
- GoのCGO境界、.NETのP/Invoke境界における`Ffi`相当の枝刈り扱いは本調査でカバーしていない。
- 本調査はWeb検索・公式ドキュメント・GitHub一次ソースに基づく。学術論文の網羅的サーベイは
  行っていない(Andersen 1994、Steensgaard 1996は言及のみ)。

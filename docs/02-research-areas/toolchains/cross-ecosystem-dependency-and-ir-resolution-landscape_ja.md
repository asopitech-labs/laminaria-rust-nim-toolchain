# 複数ecosystem依存とcompiler IRを結合して解く先行研究調査

調査日: 2026-09-13

## Executive summary

「Cargo、Nimble、C、C++の依存関係を一つの表現へ正規化して解く仕組みは存在しない」という仮説は、そのままでは成立しない。2026年の *Package Managers à la Carte* は、30超のpackage managerを調査し、ecosystem固有のdependency expressionを共通のPackage Calculusへreductionして、polyglot projectを一つのresolutionとして解く形式的基盤を示した。Package Calculusはpackage間のcross-ecosystem resolution IRという、LAMINARIAが必要とする層の有力な直接先行研究である。[^package-calculus]

一方、調査したproduction tool、実験実装、一次研究の中に、次の全てを一つの説明可能な増分解決過程として結合したsystemは確認できなかった。

1. Cargo、Nimble、C/C++等の異なるpackage semantics
2. sourceから発見されるmodule、name、type、FFI、generated-source関係
3. HIR/MIR/MLIR/LLVM IR等の複数levelにおけるlowering可能性と最適化判断
4. object、archive、shared library、required/provided symbol、ABI、link order
5. native executableをrootとするdemand-driven closure
6. 変更時の増分再解決と、時間・peak memory・探索stateを含む資源目的

既存系はこのうち隣接する二、三層を強く解くが、境界にopaque compiler、recipe、build script、linker、または事前生成済みbitcodeを残す。したがってLAMINARIAの代替不可能な研究テーマは「cross-ecosystem package solver」単独ではない。**Package Calculus等で得られるpackage-level共通表現を起点に、source semantics、compiler IR、native artifact／ABI／link closureまでをprovenance付きtyped graphで結び、各層の専用solverを増分fixed pointとして協調させること**にある。

この結論は不在の絶対証明ではない。「本書で確認した一次資料と現行実装の範囲では、上の連言を満たすsystemを確認できなかった」というlandscape claimであり、新しい反例が見つかれば更新する。

### 調査範囲と方法

2026-09-13時点の公式documentation、公開source、原著論文、付属artifactを優先した。対象を、言語固有package manager、cross-ecosystem/formal package solver、C/C++/HPC package manager、polyglot build system、functional/incremental build research、compiler query system、module dependency scanner、multi-level/compiler IR、link-time IR、polyglot runtimeに分けた。

各systemについて、「複数言語をbuildできる」という表面的な機能ではなく、どのnode/constraintが第一級で、どの判断がrecipe、compiler process、build script、prebuilt bitcodeへ渡されるかを確認した。引用した名称すべてを同粒度で羅列するのではなく、各設計空間の最も強い代表例と、LAMINARIAへの接続面がある実験を選んだ。

## 1. 問いを分解する

### 1.1 「圏」の扱い

本書では「Cargo圏」「Nimble圏」「C圏」「C++圏」を、各ecosystemが固有に持つidentity、constraint、artifact、toolchain、resolution ruleの領域という意味で使う。対象、射、合成、恒等射を定めていない段階でcategory theory上の圏であるとは主張しない。まずtyped graph／constraint domainとして実装し、後に有用なら各domain間の意味保存写像をfunctorとして形式化する。

### 1.2 同じ「dependency」ではない

| 層 | 典型node／edge | 問い | 適する解法 |
|---|---|---|---|
| package | package、version、feature、variant、provider | 同時に選べる構成はどれか | SAT、ASP、CDCL、constraint solving |
| source semantics | module、definition、type、macro、FFI declaration | 名前／型／生成単位が何を要求するか | compiler query、scope／type solver |
| compiler IR | operation、value、region、dialect、lowering | どの表現変換が妥当か | rewrite、dataflow、pass／cost model |
| native artifact | object、archive、shared library、ABI、symbol | 実行物に必要な実体が揃うか | compatibility、symbol／link closure |
| action | compiler、generator、archiver、linker invocation | 何をどの順で実行するか | DAG／dynamic dependency scheduling |
| placement | memory、local disk、remote cache、worker | どこで保持・実行するか | resource scheduling／cache policy |

package resolutionは候補を選ぶ非単調なglobal searchである。type resolutionはscopeとsemantic ruleに従う。IRのuse-def graphはversion selectionではない。action graphはproducer／consumerの実行順であり、linkerはsymbol、archive scan、link orderを扱う。全てをnode/edgeと呼べても、一種類のsolverで解くのは適切ではない。

### 1.3 本書でいう「同時解決」

全層を一回のalgorithm呼出しで解くことではない。次を満たす**coupled resolution**を意味する。

- 各層の判断と根拠が同じstable identity／provenance空間へ記録される。
- package featureがsource／IRを変え、semantic解析で発見したnative requirementがpackage／toolchain選択を狭める。
- lowering結果がrequired symbol、runtime、link inputを増やし、それが上流の候補を無効化できる。
- 新事実により必要部分だけを再解決し、矛盾はどの層を跨いだか説明できる。
- 終了条件は「最終native executableの全producerが閉じた」または「閉じられない制約coreが得られた」である。

これは直線pipelineではなく、層別solverが収束するまで事実と選択を交換するincremental fixed pointである。

## 2. 現在のRust/Cargo境界

RustからC/C++を利用する通常のbuildでは、`rustc`はC/C++ sourceをcompileしない。Cargoがhost向け`build.rs`をcompile・実行し、`build.rs`が`cc`、Clang/GCC/MSVC、CMake等を起動してobject/archiveを作るかsystem libraryを探索する。その後Cargoが`cargo::rustc-link-*` instructionを`rustc`の`-L`、`-l`、`-C link-arg`へ渡し、`rustc`がsystem linker／compiler driverを起動して最終成果物をlinkする。詳細は[Rust/CargoにおけるC/C++ native依存のbuild model](rust-c-cpp-native-build-model_ja.md)に保存した。[^cargo-build-scripts] [^rustc-linker]

重要なのは、Cargoのpackage graph、`build.rs`内のnative build graph、linkerのsymbol closureが別であることだ。`rules_rust`のCargo build-script対応もscriptをcompile・実行し、その出力をconsumerへ渡すため、Cargo互換性は高いがscript内部をsemantic graphへ展開するわけではない。[^rules-rust-cargo]

Nimは別の境界を持つ。Nim compilerはNim module graphを解析してC/C++等を生成し、platform C/C++ compilerでnative binaryまで作れる。`{.compile.}`は外部C sourceをcompile対象へ加え、`importc`／`importcpp`はforeign symbolを宣言できる。Nimbleはpackage version requirementをSAT solverで選び、backendやpath等をNim invocationへ渡すが、Nimble package resolutionとNim compiler内部のmodule／packed AST／generated C graphは同じsolverではない。[^nim-backend] [^nimble-reference]

## 3. 先行方式の比較

凡例: `強`は対象層を第一級にmodel化、`部`は限定的またはplugin／rule任せ、`境界`は外部toolへのopaque handoff、`—`は主対象外。

| 系統／system | ecosystem横断package | source semantics | multi-level IR | native ABI／artifact | 増分／需要駆動 | 状態 |
|---|---:|---:|---:|---:|---:|---|
| Package Calculus | 強 | — | package IRのみ | — | — | 形式モデル、Lean mechanization |
| Spack | 部（HPC packages） | 境界 | — | 強（compiler／arch／variant、一部ABI） | solver cache／reuse | production |
| Conan 2 | C/C++内で強 | 境界 | — | 強（binary、host/build context） | lock／cache | production |
| Cargo／Nimble | 各ecosystem内で強 | compilerへ境界 | — | 部 | package単位 | production |
| Bazel／Buck2 | ruleで部 | compiler actionへ境界 | artifact IR | 強 | 強 | production |
| Nix／Guix | derivation間で強 | builderへ境界 | derivation IR | artifact/store単位 | cache／substitution | production |
| Pluto／PIE | — | builder次第で部〜強 | computation IR | file／builder単位 | 強 | research implementation |
| rustc query／Salsa | — | 強 | HIR→MIR→LLVM IR query | crate内で部 | 強 | production／library |
| Swift explicit modules／clang-scan-deps | packageとは分離 | module依存を強く抽出 | module artifact | module／object | 強 | production |
| MLIR | — | frontend dialect次第 | 強 | lowering後に部 | pass infrastructure | production/research infra |
| LLVM LTO／ThinLTO | — | 低level IRへ消失 | LLVM IRで強 | linkerと強く接続 | ThinLTOは並列・cache | production |
| GraalVM／Truffle | — | language implementation内 | runtime common protocol／LLVM bitcode | runtime interop | JIT内 | production |

この表で空いている列を一つのsystemに足し合わせても自動的に統合にはならない。層間のidentity、constraint feedback、invalidations、diagnosticを意味保存して接続するprotocol自体が研究対象である。

## 4. Package resolutionの先行研究

### 4.1 Package Managers à la Carte／Package Calculus

最も直接的な反例である。Package Calculusのcoreはroot inclusion、dependency closure、version uniquenessでresolutionを定義し、dependency resolutionがNP-completeであることを示す。conflict、concurrent version、peer dependency、feature、package formula、variable formula、virtual package等をextensionとして定義し、coreへのsound／complete reductionを与える。複数extensionのcompositionにより、各ecosystem frontendがdependencyをcoreへcompileし、一つのpolyglot resolutionを返す構想まで明示している。[^package-calculus]

ただし2026年7月の成果は35ページの形式モデルとLean 4 mechanizationであり、配布artifactはproof scriptで「実行可能resolverではない」と明記される。source semantic IR、build action、compiler lowering、ABI、symbol、linkerはcalculusの対象ではない。[^package-calculus-artifact]

LAMINARIAはこれを無視して独自のpackage calculusを再発明すべきではない。まずCargo／Nimble／C/C++ metadataがPackage Calculusのどのextensionへ写るかを評価し、足りないartifact／ABI／semantic constraintだけを明示的に拡張するのがよい。

### 4.2 Spack

Spackのabstract specはversionだけでなくvariant、compilerとversion、architecture、compiler flag、dependency constraintを表し、concretizerが全選択を埋めた具体的build DAGへする。[^spack-spec] 現行concretizerはinstalled／binary cache再利用、build toolの重複、compiler mixing、virtual provider、target、ABI-compatible packageのexperimental automatic splicingまで扱う。[^spack-concretizer]

SpackのASP concretizer研究は、GPU runtime、flag、build option等を含むHPC構成をAnswer Set Programmingで表し、新規buildとpreinstalled binaryを混ぜ、数万package規模でも実用的なsolver性能を報告した。[^spack-asp] これはtoolchain／binary／ABIをpackage choiceへ入れる最も強いproduction先例である。

限界は、package recipeとcompiler invocationより内側のname/type resolutionやHIR→MIR loweringがsolver factではないこと、明示spliceではABI compatibilityの責任が利用者に残る場合があることだ。LAMINARIAのbenchmark baselineとして非常に重要だが、semantic/compiler graphの統合例ではない。

### 4.3 Conan 2

Conanはsettings、options、profiles、conditional／transitive requirementからC/C++ dependency graphを計算し、各nodeについてprebuilt binaryを使うかsourceからbuildするかを評価し、正しい順でdownload／buildする。host contextとbuild contextを区別し、compiler、architecture、`libcxx`、build type等をpackage identityへ反映する。[^conan-install] [^conan-graph]

これはC/C++ native artifact graphの実用的な先例だが、recipeの`build()`内ではCMake等が実行される。translation unit、AST、template instantiation、required/provided symbolはgraph nodeではない。他ecosystemのsemanticsを共通calculusへ翻訳するsystemでもない。

### 4.4 Cargo、Nimble、Nix、Guix

CargoとNimbleはそれぞれのmanifest semanticsをよく解くが、ecosystem外の作業はbuild script、task、compiler optionへ渡す。NixとGuixはlanguage-neutralなpackage recipeをderivation、すなわちbuilder、input、environment、outputのconcrete build actionへ落とすため、異種toolchainを同じstore graphへ配置できる。[^nix-derivation] [^guix]

これらの強みはreproducible artifact identityとcacheである。限界はbuilder内部がsemanticにopaqueな点であり、「同じgraphにRustとC artifactが存在する」ことと「Rust/C semanticsをjointly solveする」ことは異なる。

## 5. Polyglot build／action graph

### 5.1 Bazel

Bazel ruleは入力から出力を作るaction列を定義し、providerでtyped informationをdependentへ渡す。C++ ruleならsource、compiler、standard libraryも入力として宣言し、compileとlink actionをartifact graphへ登録する。custom ruleにより多言語を一つのtarget／action graphでbuildできる。[^bazel-rules]

ただしanalysis phaseはfile contentを読まず、actionの全input/outputはexecution前に既知である必要がある。compilerが実行中に発見する型やlowering constraintは通常depfile、generated manifest、dynamic execution等の限定protocolへ要約される。Cargo manifestは`crate_universe`でBazel targetへ変換できるが、Cargo build script互換機構はscriptを実行する。[^rules-rust-universe] したがってBazelはpolyglot artifact/action graphの強力なbaselineであり、cross-language semantic solverではない。

### 5.2 Buck2／DICE

Buck2はC++、Rust、Swift、OCaml等を含む大規模multi-language monorepo向けで、unconfigured target、configured target、action等を一つのDICE-backed dependency computationへ置く。configuration constraintを適用し、rule analysisからaction graphを生成し、incrementalityとdeferred materializationを提供する。[^buck2-architecture]

Buck2の価値は「graphを全展開してから実行」ではなく、demandとdynamic dependencyをdependency computation自体へ入れた点にある。しかし各compilerのsource semanticsはruleが起動するaction内に残る。LAMINARIAはDICE型のincremental computationを参考にできるが、semantic factのschemaとcross-solver feedbackを追加する必要がある。

### 5.3 Build Systems à la Carte、Shake、Pluto、PIE

*Build Systems à la Carte* はMake、Shake、Bazel、Buck、Nix等を、task description、scheduler、rebuild strategyの組合せとして比較する実行可能なformal frameworkを示す。[^build-systems-calculus] 「何をbuildするか」と「いつ再実行するか」を分ける上で重要だが、task内部のlanguage semanticsはparameterである。

Shakeは実行中に必要fileを発見できる。Plutoはbuilderがrequired／produced fileと他builderを動的登録し、dependency analysisとexecutionをinterleaveして、前回graphに対するsound／optimal incremental rebuildを証明した。[^pluto] PIEはこの系譜をpersistentなinteractive development pipelineのDSL／runtimeへ拡張した。[^pie]

さらにStratego compilerを対象に、Pluto系のinternal build systemをcompiler内へ組み込み、cross-module compiler componentをincrementalに接続した実験がある。[^hybrid-incremental-compiler] これは「compiler semantic computationとbuild graphを近づける」最も近い先例である。ただし単一language compiler内のcross-module extensibilityが対象で、cross-ecosystem package solverやnative ABI closureは扱わない。

## 6. Semantic queryとmodule discovery

### 6.1 rustc query system／Salsa

`rustc`はtype query、optimized MIR、LLVM IR生成等の主要段階をqueryとして構成する。query invocationをnode、参照をedgeとするdependency graphを記録し、fingerprintとred-green algorithmで変更の影響を受けないresultを再利用する。[^rustc-query] Salsaはこの考えを汎用のon-demand incremental computation frameworkとして提供する。[^salsa]

これはsource semanticsからIRまでの増分解決の直接先例である。しかしCargo resolverや`build.rs`内部のnative graphは別process／別identity spaceにある。LAMINARIAが借りるべきなのはquery memoizationとprecise invalidationであり、crate外package choiceまでrustc queryとみなすことではない。

### 6.2 Clang dependency scanner／Swift explicit modules

C++20 moduleはcompile順序を要求するため、`clang-scan-deps`は実際のcompile commandを基にP1689形式の`provides`／`requires`を抽出する。headerの推移dependencyもdepfileとして取得できる。[^clang-scan-deps] Swift driverもdependency scannerの結果をexplicit inter-module dependency graphとして保持し、Swift／Clang module artifactを事前buildできる。[^swift-driver]

これはsource-derived dependencyをaction graphへ渡す実用例であり、LAMINARIAの`DiscoveredRequirement` protocolに近い。ただし抽出結果はmodule build orderであり、package version、全languageのtype relation、IR lowering、final symbol closureを一括解決しない。

## 7. Compiler／runtime IRの先行方式

### 7.1 MLIR

MLIRはdialectにより異なるabstraction levelとdomainを同じextensible infrastructureに保持し、既存compiler、hardware target、execution environment間をつなぐ。高level表現を早くLLVM IRへ潰さず、段階的loweringとdomain-specific optimizationを可能にする。[^mlir]

したがって「言語表現IRから複数の中間表現IRへ」という部分の最重要先例である。しかしMLIR operationはCargo package candidateではなく、dialect conversionはpackage version selectionではない。external libraryの取得、compiler選択、native ABI、archive member、link orderは別build systemが担う。

### 7.2 LLVM LTO／ThinLTO

LLVM LTOはlinkerがLLVM bitcodeをnative objectと同様に認識し、symbol resolutionとintermodular optimizationを密結合する。これはartifact graphとlow-level compiler IRを実際に接続した重要なproduction例である。[^llvm-lto]

ThinLTOは各moduleのsummary indexをlink時に結合し、backendを並列実行し、cacheによるincremental buildを可能にしてmonolithic LTOの時間・memory問題を緩和する。[^thin-lto] ただし入力artifactをどうpackage managerから選び、source languageの型／FFI制約がどうbitcodeへ至ったかは対象外である。全dependencyが既にbitcodeであるとも限らず、ABI不整合をpackage solverへ戻さない。

### 7.3 GraalVM／Truffle

GraalVM polyglot APIはJavaScript、Python、Ruby、R、Java、LLVM等の値をruntime interop protocolで接続できる。LLVM runtimeはC/C++等を事前にClangでembedded bitcodeへcompileしてから実行する。[^graal-polyglot] [^graal-llvm]

これはruntime-level polyglot executionであり、source/package/build closureのsolverではない。事前compile済みbitcodeとruntime availabilityが前提なので、native executableを要求rootとするLAMINARIAの問題を置き換えない。

### 7.4 Cross-layer reachability pruning

LLVM/MLIR DCE、Rust/Nimの到達可能item選別、linker section GCはそれぞれ強力だが、後段でcodeを捨てても上流のpackage取得、parse、typecheck、monomorphization、codegenの費用は戻らない。LAMINARIAはNativeExecutableからのreachabilityをpackage、source/module、semantic item、IR、artifact、symbol/section、runtimeまで伝播し、早期のwork回避と後期のoutput eliminationを同じprovenanceで説明する。詳細は[Native executableをrootとするcross-layer枝刈り](cross-layer-reachability-pruning_ja.md)に定義する。

この協調解決が生む利用者向け価値は、枝刈りやclosure配布そのものとは別に定義する必要がある。package選択、source semantics、language/intermediate IR、ABI、symbol、linkの各依存義務をbuild時の変換でdischargeし、元のecosystem graphを再resolution対象ではなくprovenanceへ変えることである。Nixのclosure deploymentはruntime到達物を運ぶ直接的先行例だが、package/store objectより内側のsemantic・IR・link義務まで成果物へ変換する仮説は[異種依存義務をbuild時にdischargeするartifact contract](dependency-resolved-artifact-closure_ja.md)に分離して定義する。

## 8. 何が既に解かれ、何が残るか

### 8.1 既に独自性を主張できない部分

- cross-ecosystem package semanticsの共通IRと形式的reduction: Package Calculusが直接先行する。
- compiler／architecture／variant／binary reuseを含む大規模dependency solving: Spackがproductionで実証する。
- 多言語artifact/action graphとremote／incremental execution: Bazel、Buck2、Nix等が成熟している。
- source semantic queryのdemand-driven incrementalization: rustc、Salsa、Pluto／PIEが先行する。
- multi-level IR: MLIRが汎用基盤を提供する。
- low-level IRとlinkerの結合: LTO／ThinLTOがproductionで実現する。

### 8.2 調査範囲で残ったgap

次のfeedback loopが一つのcorrectness contractの下で閉じていない。

```text
NativeExecutable requirement
  -> package candidates / versions / features / providers
  -> source and generated-unit discovery
  -> name / type / FFI / representation constraints
  -> HIR / MIR / dialect lowering choices
  -> object / archive / shared-runtime requirements
  -> ABI / required-symbol / provided-symbol / link-order closure
  -> executable producer
              |
              +-- discovered incompatibility or new requirement
                    -> invalidate and re-solve the minimum upstream slice
```

特に未解決なのは次である。

- source semanticsを読むまで判明しないnative requirementを、package solverへ安全に戻す方法
- package feature／toolchain choiceによりsemantic graph自体が変わるときのterminationとcache identity
- C++ template、inline、exception、RTTI、standard library ABIをartifact-level factへ要約する境界
- package solverの非単調なchoiceと、semantic queryの単調なfact accumulationを混同せず協調させる方法
- lowering／fusion／partition choiceとpeak memory／compile timeを、correctnessを壊さず同時最適化する方法
- package候補からobject sectionまで、同じroot-setに基づき不要workをどこまで早期枝刈りできるか
- opaque build scriptを禁止したとき、未知generator／probeを構造化されたgapとして止める方法
- layerを跨ぐminimal unsat explanationとprovenance

## 9. LAMINARIAの研究仮説

### 9.1 一つの万能IRではなく、共通envelopeを持つtyped hypergraph

最低限、次のnode kindを別identityとして持つ。

```text
Requirement / PackageCandidate / Feature / Provider
SourceUnit / Module / Definition / SemanticFact / GeneratedUnit
IRUnit / Dialect / Lowering / RepresentationConstraint
Toolchain / Target / AbiConstraint
Object / Archive / SharedLibrary / Runtime / Symbol
Action / Placement / NativeExecutable
```

edgeは`requires`だけでなく、`provides`、`conflicts`、`generated-by`、`lowers-to`、`compiled-by`、`compatible-with`、`requires-symbol`、`provides-symbol`、`ordered-before`を区別する。各factにはproducer、input digest、host/target、toolchain identity、reasonを付ける。

Package Calculus、Cargo/Nimble adapter、compiler semantic IR、MLIR/LLVM IRを一形式へ平坦化しない。各IRを保持し、共通envelopeにprojectionとprovenanceを登録する。意味保存できない情報は削らず、ecosystem-specific payloadまたはunsupported gapとして残す。

### 9.2 協調solver

```text
1. demand rootからpackage候補をlazy expansion
2. Package Calculus／SAT／ASP層で暫定candidate setを選択
3. 必要なsourceだけをparse・semantic queryし、新constraintを発見
4. version／feature／toolchain／ABI solverへconstraintを反映
5. 成立部分だけloweringし、artifact／symbol requirementを抽出
6. final-link closureを検査
7. 新事実があれば影響sliceだけinvalidateして2へ戻る
8. closureまたはunsat coreで停止
```

fact discoveryは可能な限り単調にし、candidate選択や撤回は明示的なepoch／decision trailに隔離する。これによりSalsa/DICE型memoizationとSAT/ASP型backtrackingを混同しない。cycleはSCCとして凝縮し、許されるsemantic recursionと不正なproducer cycleを別ruleで扱う。

### 9.3 効率仮説

比較すべき技術は、naiveな直積生成に対する次の組合せである。

- demand-driven candidate／source expansion
- constraint propagationとearly ABI／target pruning
- canonical identityとequivalent-state merging
- compiler query result、package subproblem、IR summaryの層別memoization
- SCC condensationとstable summary
- incremental SAT／ASP solvingとassumption literal
- ThinLTO型summary indexによる全IR materialization回避
- peak-memory budgetに応じたIR eviction／reconstruction
- dominance pruning。ただし性能が良いという理由でvalid solutionを落とさない証明またはoracle比較を要求する

## 10. 最初の反証可能な実験

### 10.1 Workload

固定projectに次を一つずつ含める。

- Cargo crate: featureと`build.rs`相当native requirementを持つ
- Nimble package: Nim moduleとgenerated Cまたは`{.compile.}` requirementを持つ
- C library: header、translation unit、static archiveを持つ
- C++ library: adapter、standard library／ABI constraint、明示的symbolを持つ
- root: 通常起動できるnative executable

最初から任意のCargo／Nimble／CMake projectを処理しない。opaque `build.rs`／taskを実行して成功扱いにせず、対応したsubsetをadapterでgraphへ展開し、未対応operationはstructured gapとして拒否する。

### 10.2 比較対象

1. eager baseline: 全候補、全source、全artifact combinationを展開する。
2. staged baseline: package resolve → compiler → linkを一方向に実行し、後段矛盾時はclean restartする。
3. coupled candidate: lazy expansion、semantic feedback、incremental re-solveを使う。

package-onlyの正解はPackage Calculus相当のexhaustive oracleまたは独立SAT encodingと比較する。artifact closureは実際にproduction pathからnative executableを生成・起動するdirect executable testで検証する。手書きYAML、fixture専用validator、そのvalidator testの三重管理を正本にしない。

### 10.3 Positive／negative cases

- positive: 唯一のversion／feature／toolchain／ABI closureからbinaryが生成され、期待値を出力する。
- version conflict: package解決時にcompile前拒否する。
- semantic feedback: source解析でのみ判明するFFI requirementが別native providerを選ばせる。
- ABI conflict: package versionは成立するがC++ standard library／target ABIが不一致でcompile前拒否する。
- symbol conflict: archiveは存在するがrequired symbolを提供せずfinal action登録前に拒否する。
- incremental: leaf、feature、target predicate変更ごとに再解決sliceを記録する。

### 10.4 Measurement

- cold／warm wall-clock
- peak RSSと層別live node／IR bytes
- enumerated、expanded、pruned、merged、recomputed state数
- SAT／ASP decision、conflict、restart数
- parse／semantic／lowering query hit率
- materializeしたsource／IR／artifact数
- 起動したexternal compiler／archiver／linker数
- invalidation fan-out
- diagnosticが示すdecision trailとminimal conflicting facts

correctnessが一致するresolverだけを性能比較する。container計測をnative host性能と混同しない。

## 11. 現時点の判断

1. **Package layerの研究起点を更新する。** Package Calculusを主要prior art／potential foundationとしてIssueとdesignへ組み込む。
2. **Spackを最重要implementation baselineにする。** ASP concretization、compiler／architecture／variant、binary reuse／ABI splicingを比較対象とする。
3. **Bazel/Buck2をaction graph baselineにする。** ただしcompiler actionのopaque境界を明記する。
4. **rustc/Salsa/Pluto/PIEをincremental semantic graph baselineにする。** package solverへそのまま拡張しない。
5. **MLIRとThinLTOをIR／summary設計のbaselineにする。** package resolutionの代替とは扱わない。
6. **LAMINARIAのnovelty claimを狭く強くする。** 「多言語build」や「共通IR」ではなく、cross-ecosystem package choice、source-derived semantic requirement、multi-level IR、native ABI／symbol closureを、native executable rootの下で増分協調解決する点を検証する。
7. **WASMをgoalへ戻さない。** 最初のcompletion artifactは通常実行できるnative binaryである。

## Sources

[^package-calculus]: Ryan Gibb et al., “[Package Managers à la Carte: A Formal Model of Dependency Resolution](https://arxiv.org/abs/2602.18602),” Proc. ACM Program. Lang. 10, ICFP, Article 301, 2026. v5, 2026-07-17.
[^package-calculus-artifact]: Ryan Gibb et al., “[Package Managers à la Carte: Lean 4 Mechanisation](https://zenodo.org/records/21115421),” Zenodo, 2026.
[^cargo-build-scripts]: Rust Project, “[Build Scripts — The Cargo Book](https://doc.rust-lang.org/cargo/reference/build-scripts.html).” Accessed 2026-09-13.
[^rustc-linker]: Rust Project, “[Codegen Options — linker and linker-flavor](https://doc.rust-lang.org/rustc/codegen-options/index.html#linker).” Accessed 2026-09-13.
[^rules-rust-cargo]: Bazel rules_rust, “[Cargo Build Scripts](https://bazelbuild.github.io/rules_rust/cargo.html).” Accessed 2026-09-13.
[^nim-backend]: Nim Project, “[Nim Backend Integration](https://nim-lang.org/docs/backends.html).” Accessed 2026-09-13.
[^nimble-reference]: Nimble Project, “[`.nimble` file reference](https://nim-lang.github.io/nimble/nimble-reference.html).” Accessed 2026-09-13.
[^spack-spec]: Spack Project, “[Spec Syntax](https://spack.readthedocs.io/en/latest/spec_syntax.html).” Accessed 2026-09-13.
[^spack-concretizer]: Spack Project, “[Concretization Settings](https://spack.readthedocs.io/en/latest/build_settings.html).” Accessed 2026-09-13.
[^spack-asp]: Todd Gamblin et al., “[Using Answer Set Programming for HPC Dependency Solving](https://arxiv.org/abs/2210.08404),” SC22, 2022.
[^conan-install]: Conan Project, “[conan install](https://docs.conan.io/2/reference/commands/install.html).” Accessed 2026-09-13.
[^conan-graph]: Conan Project, “[conan graph](https://docs.conan.io/2/reference/commands/graph.html).” Accessed 2026-09-13.
[^nix-derivation]: Nix Project, “[Building a package](https://github.com/NixOS/nix/blob/master/doc/manual/source/store/building.md).” Accessed 2026-09-13.
[^guix]: GNU Project, “[GNU Guix Reference Manual](https://guix.gnu.org/manual/en/guix.pdf).” Accessed 2026-09-13.
[^bazel-rules]: Bazel Project, “[Rules](https://bazel.build/extending/rules).” Accessed 2026-09-13.
[^rules-rust-universe]: Bazel rules_rust, “[crate_universe](https://bazelbuild.github.io/rules_rust/crate_universe_bzlmod.html).” Accessed 2026-09-13.
[^buck2-architecture]: Meta, “[Buck2 Architectural Model](https://buck2.build/docs/concepts/architecture/).” Accessed 2026-09-13.
[^build-systems-calculus]: Andrey Mokhov, Neil Mitchell, Simon Peyton Jones, “[Build systems à la carte: theory and practice](https://www.microsoft.com/en-us/research/publication/build-systems-a-la-carte/),” Journal of Functional Programming 30, 2020.
[^pluto]: Sebastian Erdweg, Moritz Lichter, Manuel Weiel, “[A Sound and Optimal Incremental Build System with Dynamic Dependencies](https://doi.org/10.1145/2814270.2814316),” OOPSLA 2015.
[^pie]: Gabriël Konat et al., “[PIE: A Domain-Specific Language for Interactive Software Development Pipelines](https://arxiv.org/abs/1803.10197),” 2018.
[^hybrid-incremental-compiler]: Daniël A. A. Pelsmaeker et al., “[Constructing Hybrid Incremental Compilers for Cross-Module Extensibility with an Internal Build System](https://arxiv.org/abs/2002.06183),” 2020.
[^rustc-query]: Rust Project, “[Incremental compilation in detail — Rust Compiler Development Guide](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html).” Accessed 2026-09-13.
[^salsa]: Salsa Project, “[salsa: A generic framework for on-demand, incrementalized computation](https://github.com/salsa-rs/salsa).” Accessed 2026-09-13.
[^clang-scan-deps]: LLVM Project, “[Standard C++ Modules — Discovering Dependencies](https://clang.llvm.org/docs/StandardCPlusPlusModules.html#discovering-dependencies).” Accessed 2026-09-13.
[^swift-driver]: Swift Project, “[Swift Driver implementation](https://github.com/swiftlang/swift-driver/blob/main/Sources/SwiftDriver/Driver/Driver.swift).” Accessed 2026-09-13.
[^mlir]: Chris Lattner et al., “[MLIR: A Compiler Infrastructure for the End of Moore's Law](https://arxiv.org/abs/2002.11054),” 2020; MLIR Project, “[MLIR Rationale](https://mlir.llvm.org/docs/Rationale/Rationale/).” Accessed 2026-09-13.
[^llvm-lto]: LLVM Project, “[LLVM Link Time Optimization: Design and Implementation](https://llvm.org/docs/LinkTimeOptimization.html).” Accessed 2026-09-13.
[^thin-lto]: LLVM Project, “[ThinLTO](https://clang.llvm.org/docs/ThinLTO.html).” Accessed 2026-09-13.
[^graal-polyglot]: Oracle, “[GraalVM Polyglot Programming](https://www.graalvm.org/latest/reference-manual/polyglot-programming/).” Accessed 2026-09-13.
[^graal-llvm]: Oracle, “[GraalVM LLVM Runtime](https://www.graalvm.org/latest/reference-manual/llvm/).” Accessed 2026-09-13.

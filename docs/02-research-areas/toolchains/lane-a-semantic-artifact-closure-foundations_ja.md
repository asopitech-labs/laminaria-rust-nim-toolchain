# Lane A 基礎研究 — Semantic and Artifact Closure

## 位置づけ

本書は[当面の研究ゴール](../../near-term-research-program_ja.md)から派生するLane Aの基礎研究である。調査日は2026-09-13。目的は「複数package managerを一つにすること」でも「最終linkが成功すること」でもない。要求されたnative executableから逆向きに、Cargo、Nimble、C、C++にまたがるpackage、source semantics、language/intermediate IR、artifact、toolchain、ABI、symbol、link、runtimeの義務を確定し、各義務をbuild時に充足して、利用者が元のbuild graphを再解決せず実行できる成果物へ閉じることである。

ここでの結論は、既存研究が不足しているという単純な主張ではない。package resolution、multi-level IR、native dependency discovery、link-time optimization、deployment closureにはそれぞれ強い先行研究がある。LAMINARIAの研究仮説は、それらの境界で失われる情報を一つのtyped obligation graphへ保ち、下流のsemantic/ABI/link事実を上流の候補選択へ戻せるか、という統合問題にある。

## 1. 問題の分解

要求成果物を (R)、選択候補を (P)、source/semantic factsを (S)、IR/lowering factsを (I)、native artifact/ABI/link factsを (A)、runtime contractを (E) とする。Lane Aが求めるものは一方向pipelineではなく、次の制約を同時に満たす閉包である。

```text
R -> P <-> S <-> I <-> A -> E
     ^                 |
     +--- rejection ---+
```

- `R`: executable、target triple、entry、export、dynamic-loading root、許容する外部runtime。
- `P`: package/version/feature/provider/source revision、host/target/build dependency role。
- `S`: module/import、型、generic instantiation、generated source、FFI declaration、conditional compilation。
- `I`: dialect/representation、legalization前提、calling convention、data layout、lowering capability。
- `A`: object/archive/shared object、symbol definition/reference、link order、visibility、relocation、loader dependency。
- `E`: kernel、system library、driver、resource、environment等、成果物へ内包できない明示契約。

packageのversionが解けても、選ばれたC++ libraryのABIがtargetやRust/Nim側FFIと両立しなければ閉包ではない。逆に、linkerが偶然host上のlibraryを発見して成功しても、package identity、producer、runtime contractが不明なら再現可能な解決ではない。

## 2. 先行研究の地図

| 系統 | 強い点 | 境界 / LAMINARIAに残る問い |
| --- | --- | --- |
| Package Calculus | 異なるpackage managerの依存表現を共通IRへ写し、ecosystem間解決を形式化する[1] | source/IR lowering、ABI、symbol、link、実行成果物のdischargeまでは研究対象ではない |
| Spack ASP concretizer | version、variant、compiler、GPU等をASPで宣言的に扱い、品質基準を含む大規模探索を行う[2] | package concretization後のlanguage semanticsやlink-time reachabilityを同じ固定点へ戻すモデルではない |
| Cargo | feature/version/target/build dependencyを解き、unit graphをcompiler invocation単位で構成する[3][4] | build scriptは任意processでnative情報を注入する。Cargo metadataだけでは全unit関係を表現できず、C/C++をrustc自身がcompileするわけではない[3][5] |
| Nimble | version constraint、lock、SAT resolver、local dependency、Nim versionを扱う[6][7] | NimScript taskやcompiler backend、生成C/C++、system discoveryをpackage resolutionと同じ意味モデルには閉じない |
| Conan | C/C++のsettings/options/requirementsからbinary package graphとlockfileを扱う[8] | source-language IRや他ecosystem resolverとの意味的feedbackは外部統合に委ねる |
| CMake | imported targetがinclude path、define、link requirementを伝播し、providerが外部package managerを接続できる[9] | `find_package` moduleはheuristicで陳腐化し得る。host探索結果はそれ自体ではcontent-addressedな解決証明にならない |
| Bazel/Buck2 | 複数言語をrule/action graphに正規化し、hermetic actionを構成する[10][11] | rule authorが既に知っているaction/input/outputを表す層であり、language/ABI意味からpackage候補を共同探索するsolverではない |
| MLIR | 複数dialectを同居させ、意味を保ったprogressive loweringとlegalizationを提供する[12][13] | package/version/providerの制約解決やnative deployment closureを目的にしない。低水準machine code生成も非目標[14] |
| LLVM LTO / linker GC | global symbol resolutionをLLVM optimizationへ伝え、module/sectionをまたぐdead strippingを行う[15][16] | linkへ到達済みのobject/bitcodeが対象。package/source/action候補を早期に削る仕組みではない |
| Nix closure | store path中の参照からbuild-time closureとruntime closureを明示する[17] | content reference単位のdeployment completenessは強いが、source/IR/ABI義務を解くcompiler semanticsではない |
| SLSA provenance | artifact digestをsubjectとしてproducer、parameters、materialsを検証可能に結び付ける[18] | provenanceは解決・compile・testの正しさそのものを証明しない |

## 3. ecosystemごとに異なる「依存」の意味

### 3.1 Cargo / Rust

Cargo resolverはcrate versionとfeatureを選ぶが、build対象はpackage graphそのものではなくtarget/profile/kind/feature等で特殊化されたunitである。nightlyの`--unit-graph`が、`cargo metadata`では表せないdependency kindごとのfeature関係、build script、test unitを出すのは、この差を明示している[4]。

C/C++依存では`build.rs`が`cc`等の外部toolを起動し、`cargo::rustc-link-lib`、`cargo::rustc-link-search`、`cargo::metadata`をCargo/rustcへ返す。`package.links`は同じnative libraryを複数crateが重複linkすることを抑えるが、scriptの任意副作用を意味的に理解するものではない[3]。Rust Referenceも、混成binaryの最終linkは、native library/objectを渡して`rustc`に行わせるか、Rustを`staticlib`にしてforeign linkerへ渡す二方式としている[5]。したがって「rustcがC/C++をcompileする」のではなく、通常は別compilerがobject/archiveを作り、最終linkで合流する。

### 3.2 Nimble / Nim

Nimbleはversion requirement、lockfile、checksum、local path、SAT resolverのmax/min方針を持つ[6][7]。同時にNim compilerはC/C++等のbackendへ生成でき、package taskはNimScriptとして振る舞う。つまりpackage choice、Nim module discovery、generated C/C++、native compiler invocationは異なる意味層である。lockfileが同一revisionの取得を助けても、生成されたtranslation unit、compiler flags、linked system objectsまで同一であることは自動的には意味しない。

### 3.3 C / C++

C/C++には単一の言語標準package universeがない。Conan recipe、CMake config/find-module、pkg-config、system package、vendored source等がproviderになり、header usage requirement、macro、compiler/stdlib/ABI、binary package ID、link optionを伝える。CMake自身もconfig modeをmodule modeより信頼できるとし、別保守のfind-moduleはrelease差で古くなり得ると明記する[9]。したがって`foo`という論理名だけでなく、provider、target、compiler/stdlib ABI、configuration、artifact digestをidentityへ含める必要がある。

## 4. IRとpackage graphを単純に一つへ潰してはいけない

共通graphは、全nodeを同じ型にすることではない。Package Calculusのpackage term、Cargo unit、Nim module、C translation unit、MLIR operation、object section、runtime libraryはそれぞれ異なる同値性とinvalidity条件を持つ。必要なのはtyped node/edgeと、層をまたぐ明示的なobligationである。

例:

```text
PackageCandidate(libfoo, 2.1, provider=Conan)
  --provides-header--> Header(foo.h, digest=...)
  --produces--> Archive(libfoo.a, target=..., abi=...)
FFIDecl(foo_call, abi=C)
  --requires-symbol--> Symbol(foo_call)
Lowering(RustCall)
  --requires-calling-convention--> ABI(C, target=...)
LinkAction(app)
  --consumes--> Archive(libfoo.a)
  --must-resolve--> Symbol(foo_call)
```

MLIRが示す重要な教訓は、loweringを早くしすぎると高水準意味を再構成できず、最適化や合法性判断が弱くなることである[12][13]。一方、package metadataをIR operationへ無理に変換すると、version rangeやprovider preferenceの探索意味を失う。したがって「共通IR」は単一syntaxではなく、異種domainを保ったtyped relation substrateでなければならない。

## 5. artifact closureとdischarge

各obligationは少なくとも次の状態を持つ。

| 状態 | 意味 |
| --- | --- |
| `Unresolved` | providerまたは充足方法が未確定 |
| `Selected` | 候補が選ばれたが成果物で未検証 |
| `Satisfied` | 中間action/事実が要求を満たした |
| `Discharged` | 最終成果物へ内包・変換され、consumerの再解決が不要 |
| `Externalized` | OS/library/driver等として残り、version/loader条件と検証方法が明示 |
| `ProvenIrrelevant` | rootから到達不能で、保守的条件の下で不要と証明 |
| `Rejected` | conflict/unsupported/ambiguityを原因付きで拒否 |

Nixはoutput pathのclosureをruntime dependenciesと捉え、完全なclosureをdeployしなければruntime file欠落が起きるとする[17]。LAMINARIAはこの考えをpath参照より手前へ拡張する。ただし「依存から解放」とは、物理的外部依存ゼロではない。静的に内包した義務はdischargeし、不可避なdynamic/system dependencyはexternalizeし、その両方をexact artifact identityへ結ぶ。

## 6. 枝刈りの位置

枝刈りはartifact closureを得る手段であり、目的の全体ではない。

- package候補枝刈り: constraint propagation、conflict learning、dominance。
- source枝刈り: conditional module/import、feature、target predicate。
- semantic枝刈り: monomorphization root、reflection/dynamic loadingの保守的root。
- IR枝刈り: side-effect/visibilityを考慮したDCE。
- link枝刈り: entry/export/undefined symbolからrelocationを再帰的に辿るsection GC[16]。
- runtime枝刈り: 実際に残るloader/resource requirementだけをclosureへ含める。

linker GCは入力sectionが既に生成された後の到達可能性であり[16]、Package Calculus/ASPの候補削減はcode reachabilityではない。LAMINARIAが検証すべき価値は、下流で判明するlive symbolやlowering capabilityを上流へ伝え、compile/download以前のworkを安全に避けられるかである。

## 7. 研究ギャップと非主張

### 確認できたギャップ

調査したsystemには、次の全てを一つの需要駆動固定点として公開するものは確認できなかった。

1. 複数ecosystem固有semanticsを保持したpackage candidate resolution。
2. source/module/type/FFIとmulti-level loweringからの制約feedback。
3. native artifact、ABI、symbol、link order、runtime closureのproof obligation。
4. retained/pruned/rejected decisionの因果provenance。
5. exact runnable artifactへのdischargeとtest subject identity。

これは存在しないことの証明でも、個々の機構の発明を主張するものでもない。LAMINARIAの候補新規性は、既知のsolver、IR、build graph、linker、closure、provenance機構の間に明示的なsemantic feedbackとobligation lifecycleを置く構成にある。

### 主張しないこと

- Package Calculusより一般的なpackage formalismを最初から作ること。
- Cargo/Nimble/Conan/CMakeの全semanticsをM1で再実装すること。
- C++ ABIをtarget横断で統一すること。
- MLIRそのものを採用すれば問題が解けること。
- static linkによってOS/kernel/driver依存まで消えること。
- link成功だけでsemantic correctnessやtestabilityが証明されること。

## 8. M0/M1へ与える実験仕様

### 最小workload

- Cargo crate、Nimble package、C static library、C++ libraryを各1つ。
- Rust→C、Nim→C++ adapterを通る観測可能な値。
- host/build/target dependencyを区別する1条件。
- featureまたはtarget条件で不要候補が生じる1分岐。
- ABI/symbol/toolchainの不整合をcompile前に拒否できる1 negative case。
- dynamic system dependencyを1つだけ明示externalizeするprofile。

### 比較baseline

1. 各package manager/build toolへのopaque delegation。
2. package choiceを先に固定し、後段failureだけを返すstaged pipeline。
3. typed obligationsを通じてfeedbackするLAMINARIA candidate。

### 必須観測

- 選択・棄却したcandidateとreason。
- download/parse/compile/linkしたwork、および避けたwork。
- final link inputs、symbol resolution、section retention。
- artifact digest、producer action、external runtime closure。
- `Unresolved`が0であること。残存義務は`Externalized`のみであること。
- clean environmentでexact artifactを実行したLane C evidence。

### 反証条件

- package解決後のABI/link failureを上流へ戻せない。
- arbitrary build scriptをopaque実行しなければgraphが確定しない。
- semantic feedbackが候補数・compile work・診断精度のどれも改善しない。
- graph統合の時間/メモリoverheadが、得られる早期棄却/枝刈り利益を恒常的に上回る。
- 同じartifact demandから非決定的に異なるclosureが生じ、理由を説明できない。

## 9. 採用判断

M1では、Package Calculusをpackage意味の比較基準、Spack ASPを制約探索の比較基準、Cargo unit graph/CMake imported target/Conan graphをingestion比較基準、MLIR dialect conversionをmulti-level representationの比較基準、LLVM LTO/linker GCをlate reachabilityの比較基準、Nix/SLSAをclosure/provenanceの比較基準にする。ただし、どれか一つを全体architectureとして先に採用しない。固定workloadで、typed feedbackが実際に一つ以上のpre-build rejectionまたはavoided workを生むことを先に確認する。

## 参考文献

1. Gibb et al., “Package Managers à la Carte: A Formal Model of Dependency Resolution,” ICFP 2026, [arXiv:2602.18602](https://arxiv.org/abs/2602.18602).
2. Gamblin et al., “Using Answer Set Programming for HPC Dependency Solving,” [arXiv:2210.08404](https://arxiv.org/abs/2210.08404).
3. Cargo Book, [Build Scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html).
4. Cargo Book, [Dependency Resolution](https://doc.rust-lang.org/cargo/reference/resolver.html) and [`--unit-graph`](https://doc.rust-lang.org/cargo/reference/unstable.html#unit-graph).
5. Rust Reference, [Linkage — Mixed Rust and foreign codebases](https://doc.rust-lang.org/reference/linkage.html#mixed-rust-and-foreign-codebases).
6. Nimble User Guide, [Nimble develop workflow and lock file](https://nim-lang.github.io/nimble/workflow.html).
7. Nimble User Guide, [`.nimble` file reference](https://nim-lang.github.io/nimble/nimble-reference.html).
8. Conan 2 documentation, [`conan graph`](https://docs.conan.io/2/reference/commands/graph.html) and [lockfiles](https://docs.conan.io/2/tutorial/versioning/lockfiles.html).
9. CMake documentation, [Using Dependencies Guide](https://cmake.org/cmake/help/latest/guide/using-dependencies/index.html).
10. Bazel documentation, [Rules](https://bazel.build/extending/rules).
11. Buck2 documentation, [Architecture](https://buck2.build/docs/concepts/architecture/).
12. MLIR, [Rationale](https://mlir.llvm.org/docs/Rationale/Rationale/).
13. MLIR, [Dialect conversion and lowering glossary](https://mlir.llvm.org/getting_started/Glossary/).
14. MLIR, [Overview and non-goals](https://mlir.llvm.org/).
15. LLVM, [Link Time Optimization: Design and Implementation](https://llvm.org/docs/LinkTimeOptimization.html).
16. GNU Binutils, [`ld --gc-sections`](https://sourceware.org/binutils/docs/ld/Options.html#index-_002d_002dgc_002dsections).
17. Nix Reference Manual, [closure glossary](https://nix.dev/manual/nix/2.25/glossary.html#gloss-closure).
18. SLSA v1.2, [Build provenance](https://slsa.dev/spec/v1.2/build-provenance) and [verifying artifacts](https://slsa.dev/spec/v1.2/verifying-artifacts).


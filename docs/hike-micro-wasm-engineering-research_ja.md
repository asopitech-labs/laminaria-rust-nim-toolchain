# LAMINARIA — Hike極小Wasmコンパイラのエンジニアリング研究

## 位置付け

本書は、Hikeの現行コードをLAMINARIAへ採用できるかを評価する文書ではない。

研究対象は、Hikeがどの問題を選び、どの性質を同時達成するゴールを置き、そのゴールからcompiler・runtime・ABI・toolingをどう一体設計し、最終的に極小のWebAssembly artifactを生成するところまで到達したか、というcompiler engineeringのプロセスである。

Hikeの記事は、文字列処理、JavaScript/DOM連携、再帰計算を含む例について、生成されたWasm binaryが2.56KBになったと報告している。重要なのは、これは将来構想だけではなく、目標とした性質が実際のcompilerと生成artifactへ到達した結果だという点である。

LAMINARIAはこの結果を無視したり、Hikeの成熟度や直接採用可能性へ論点を移したりしない。2.56KBという値は、機能範囲、toolchain、測定条件、JavaScript bridgeを含む配布単位を固定してLAMINARIAの証拠基盤上で独立再現する必要があるが、再現前であることはHikeの工学的成果を否定する理由ではない。

参照資料:

- [うっかり世界最強のWasmコンパイラを作ってしまった件](https://qiita.com/kanryu/items/95147e22ed5ac542ba58)
- [kanryu/hike-lang](https://github.com/kanryu/hike-lang)
- 調査時の参照revision: `6402155652fa61692818fe7985193f0fd97513f5`（2026-09-11）
- [LLVM lld WebAssembly port](https://lld.llvm.org/WebAssembly.html)
- [Rust `wasm32-unknown-unknown` target](https://doc.rust-lang.org/stable/rustc/platform-support/wasm32-unknown-unknown.html)
- [WebAssembly JavaScript API](https://webassembly.github.io/spec/js-api/)

## 1. Hikeが選んだ問題

Hikeの出発点は「既存のどのWasm toolchainが総合的に優れているか」ではない。Web frontendの比較的小さな計算へWasmを導入するとき、既存の選択肢が別々の価値を実現する一方で、利用者に過剰なtrade-offを要求することを問題としている。

記事が示す対立は概ね次である。

| 欲しい性質 | 既存経路で生じると捉えたcost |
| --- | --- |
| Goに近い記述性 | Go runtime、GC、schedulerを含むbinary size |
| C/C++の性能と制御性 | libc/POSIX互換層、仮想filesystem、glue code |
| Rustの小ささと性能 | ownership学習、複数toolによるbuild/interop pipeline |
| Web APIとの連携 | linear memory、pointer/length、型変換を扱う手書きbridge |
| 小さなhot pathだけへの導入 | toolchainとruntimeの固定費が処理本体より大きくなる可能性 |

ここでHikeが行った重要な判断は、このtrade-offの中から一つを選ばなかったことである。「書きやすさを取るなら数MBを受け入れる」「小ささを取るなら低水準の境界処理を手書きする」という前提そのものを研究対象にした。

## 2. 複合ゴール

Hikeのゴールは単に「小さいWasmを生成する」ではない。少なくとも次の性質を同時に成立させることである。

```text
Go-like language ergonomics
  × extremely small Wasm artifact
  × no bundled general-purpose language runtime
  × no mandatory libc/POSIX compatibility layer
  × explicit JavaScript/Wasm interoperability
  × compiler-generated bridge code
  × one-command build experience
```

この複合ゴールが重要である。個別の各性質には既存解があっても、同時達成を要求するとlanguage semantics、data representation、runtime surface、host ABI、linker option、generated support code、CLI UXを別々の問題として扱えなくなる。

Hikeはこの同時達成を、極小の最終artifactという観測可能な結果まで押し通した。2.56KBという結果は単なる宣伝用指標ではなく、この垂直統合が少なくとも提示されたworkloadで成立したことを示す工学的outputである。

## 3. ゴールから導かれた設計原則

### 3.1 Runtimeを不可避の固定費とみなさない

Hikeは、言語の使いやすさと巨大なruntimeを同一視しない。GC、background scheduler、汎用I/O、OS abstraction、POSIX emulationをすべて最初から成果物へ入れるのではなく、対象workloadが必要とするsupportだけを残す。

これは単なるlink-time dead-code eliminationより強い問題設定である。最終段階で不要コードを捨てるだけでなく、言語機能とruntime capabilityの関係をcompilerが知り、不要なruntime obligationを最初から発生させない方向を示している。

### 3.2 Languageからdeploymentまでを一つの設計面として扱う

Hikeはparserやcode generatorだけを作って終わらない。

```text
source syntax
→ semantic analysis
→ specialization / lowering
→ HIR
→ LLVM IR
→ target-specific compilation and linking
→ Core Wasm
→ JavaScript runtime bridge
→ browser-visible execution
```

極小Wasmというゴールは、上流の型表現から下流のbrowser integrationまで貫かれている。最終artifactの大きさや利用体験に責任を持つには、compiler frontendだけでなくruntime、ABI、link、loaderを含むend-to-end ownershipが必要だという実例になっている。

### 3.3 Support routineをoptimizerから見える位置へ置く

Hikeはmemory/string操作等をLLVM IRの内部実装として供給し、可能な範囲でinlining、constant propagation、vectorization、dead-code eliminationの対象にする方針を取る。

重要なのはLLVM IRという形式そのものではない。support routineを不透明なprecompiled libraryへ早期に閉じ込めず、call siteと一緒に変換できる表現としてcompiler pipelineへ置くという原則である。

LAMINARIAでは、同じ必要性が独自IR上で再現されるかを検証する。LLVMの`define internal`を写経するのではなく、次を問う。

- helperの意味をcall siteと同時に解析すると、どのworkを除去できるか。
- helper specializationに必要なsize、alignment、alias、effect factは何か。
- helperをmaterializeする最も遅い段階はどこか。
- native、Wasm、componentで同じsupport definitionを使うべきか。

### 3.4 Target widthを意味処理へ伝播させる

Hikeの実装はwasm32を単なる最終backend flagとして扱わず、pointer、`int`、`uintptr`、slice layout等の幅へ反映し、semantic analysisとloweringへ伝える。

ここから得られる原則は、target contractがcodegen直前だけの入力ではないことである。data layout、integer width、ABI、allocation size、aggregate layoutは意味表現、合法な変換、cache identity、diagnosticにも影響する。

### 3.5 Host boundaryを言語とcompilerの責任にする

`extern func`と`cfunc`は、JavaScript/Wasm境界を偶発的な手書きコードではなく、source上の宣言された契約として扱う。さらにbridgeをcompilerが生成し、linear memory、string decoding、integer conversion、allocation、DOM callback等を利用者から隠蔽する。

ここでの工学的価値は「glue codeが存在しない」ことではない。必要なglueを認識し、生成し、Wasm本体と整合するversioned artifactとして管理できることである。

### 3.6 Build UXもarchitectureの一部にする

Hikeは`hikec build -target wasm32 ...`からWasmとbridgeを同時生成する。内部に複数stageがあっても、それを利用者へ手作業として漏らさない。

これはpipelineの可視化と単純なUXが対立しないことを示す。内部では各artifact、tool、boundaryを識別しながら、外部には一つの意図指向commandを提供できる。

### 3.7 「マイクロWasm」をdeployment unitとして定義する

Hikeは巨大application全体をWasmへ移植する発想から離れ、validation、parser、geometry、binary processing等の限定されたhot pathへWasmを差し込む利用形態を提示した。

この再定義により、評価対象も変わる。

- application全体の移植可能性ではなく、機能単位の導入cost
- peak throughputだけでなく、download、instantiate、bridge、call overhead
- 汎用runtimeの機能数ではなく、必要能力あたりの限界cost
- 大規模互換性ではなく、小さなsemantic kernelを確実に生成する能力

## 4. Hikeの開発プロセスから学べること

公開記事とrepository historyからは、次のような垂直slice志向を読み取れる。

1. Web frontendでのWasm導入costを具体的な問題として選ぶ。
2. 記述性、binary size、interop、build UXを同時達成するゴールにする。
3. そのゴールを阻むruntime、libc、data representation、host boundaryを特定する。
4. `extern`/export、browser example、runtime generation、wasm32 width handlingを短いend-to-end経路として接続する。
5. 文字列、DOM callback、再帰計算を含むvisible workloadで生成と実行を示す。
6. binary sizeという最終artifact metricで設計結果を評価する。
7. 得られた能力から「マイクロWasm」という適用モデルを提案する。

ここで注目すべきなのは、language featureを横に広く実装してからWasmへ到達したのではなく、価値を証明できるend-to-end sliceを先に作った点である。これはLAMINARIAのowned compiler sliceにも直接関係する。

## 5. LAMINARIAへの主要な示唆

### 5.1 独自compiler ownershipを最終artifactで証明する

LAMINARIAの独自IR、変換、planner、schedulerは、それぞれ存在するだけでは垂直統合を証明しない。小さくてもよいので、Rust/Nim sourceからLAMINARIA-owned target generationを通り、実行可能な極小artifactへ到達する必要がある。

```text
Rust/Nim source-derived semantics
→ LAMINARIA-owned IR and transformations
→ demand-derived runtime capabilities
→ LAMINARIA-owned target lowering
→ minimal executable Core Wasm
→ generated host contract / adapter
```

Hikeは、最終artifactがcompiler architectureに対する強い検査になることを示している。不要なruntime、曖昧なABI、過剰なexport、重複support、隠れたfallbackはすべて生成物へ現れる。

### 5.2 「最小対応言語」ではなく「最小で完全な価値経路」を作る

LAMINARIAの最初のowned target sliceは、単に`i32 add`をbinaryへ変換するだけでは不十分である。Hikeの事例を踏まえると、小さくても次を同時に含むvertical workloadが望ましい。

- RustとNimから導出された同じ意味契約
- local computationとcall
- bounded memoryまたはbyte sequence
- host importとexport
- target-specific layout
- generated adapterまたはmachine-readable ABI contract
- artifact sizeとruntime capability attribution
- 実際のinstantiateとexecution

### 5.3 Runtime Capability Graphを導入候補として研究する

Runtimeを一枚のlibraryとして扱わず、source semanticsとtarget demandから必要能力を導く候補を研究する。

```text
source operation
→ semantic/runtime obligation
→ required capability
→ support implementation variant
→ generated/imported symbol
→ artifact bytes and runtime cost
```

能力候補にはallocation、memory copy、string decode、panic、bounds failure、host callback、clock、thread、atomic、WASI I/O等がある。

このgraphにより、次を説明できる可能性がある。

- なぜこのhelperがartifactへ入ったか。
- どのsource constructがruntime costを発生させたか。
- capabilityを削除すると何byte、何call、何import減るか。
- Rust/Nim間でどのsupportを共有できるか。
- host importとmodule内実装のどちらを選んだか。

### 5.4 Bridgeを正式なartifactとして扱う

Wasm本体だけを生成物とみなさず、JavaScript loader、WIT、canonical ABI adapter、import manifest、TypeScript declaration等を同じartifact graphに置く。

Wasmが小さくてもbridgeへ複雑性を移しただけなら、system全体のcostは小さくなっていない可能性がある。一方、bridge生成により人間の手作業、ABI drift、integration failureを除去できるなら、その効果もcompiler outputの価値である。

したがってLAMINARIAは次を分けて記録する。

```text
core_module_bytes
generated_bridge_bytes
runtime_support_bytes
metadata_and_adapter_bytes
compressed_delivery_bytes
manual_integration_surface
```

### 5.5 Sizeをprovenance付きmetricにする

単一の`.wasm` file sizeだけでなく、section、symbol、capability、producer、source featureへ帰属させる。

```text
ArtifactSizeEvidence {
  artifact_identity
  producer_revision
  target_contract
  optimization_pipeline
  section_sizes
  user_semantic_payload_bytes
  runtime_support_bytes
  adapter_metadata_bytes
  generated_bridge_bytes
  compression_variant
  provenance
}
```

これにより、サイズ増減を単なる数字ではなくcompiler decisionとして説明できる。

### 5.6 Work eliminationを生成前へ押し上げる

LAMINARIAの既存方針は、不要なworkの高速化よりeliminationを優先する。Hikeの極小artifactは、この原則をcompile workだけでなくruntime/support generationにも適用する動機になる。

- 未使用helperをlinker GCで落とす。
- 未使用helperをIRへ生成しない。
- 必要capabilityをsemantic demandから限定する。
- Rust/Nimで重複するsupportを一つの共有実装へする。
- bridgeで不要な型変換をABI contractから除去する。

これらを別variantとして比較し、どの段階のeliminationがcompile latency、artifact size、incrementalityに有利かを測る。

## 6. 中心研究仮説

- **H1 — Runtime demand derivation:** Source semanticsからruntime capabilityを導出すれば、汎用runtimeをlinkして後から削る経路より小さく説明可能なartifactを生成できる。
- **H2 — Visible support optimization:** Support routineをcall siteと同時に解析可能な表現へ置けば、不透明なruntime libraryよりspecializationとwork eliminationが増える。
- **H3 — Cross-language support sharing:** Rust/Nimの意味契約から共通support obligationを導けば、言語ごとのruntime重複を減らせる。
- **H4 — Generated boundary correctness:** Host ABIとbridgeをcompiler artifactとして生成すれば、手書きglueより統合作業とABI driftを減らせる。
- **H5 — Target facts are upstream inputs:** Pointer width、memory model、feature set、host contractを意味処理から保持すると、codegen直前にtargetを選ぶ設計より正確なlayout、invalidation、diagnosticが得られる。
- **H6 — Tiny artifact as architectural evidence:** 極小で実行可能なartifactは、runtime ownership、target lowering、support reachability、fallback不使用を検査する有効な統合証拠になる。
- **H7 — Micro-Wasm changes the optimum:** 小さなhot pathではsteady-state throughputよりdownload、instantiate、bridge、call、memory fixed costの比重が高く、application-scale Wasmとは異なる最適設計になる。
- **H8 — Simple UX can preserve internal evidence:** 単一commandのUXを保ちながら、内部のIR、link、post-link、adapter、runtime capabilityを個別artifact/actionとして観測できる。

## 7. 最初の実験計画

### Phase 0 — Hike成果の再現

Hikeの参照revisionとtoolchainを固定し、記事のworkloadまたは同等の公開workloadについて次を保存する。

- source、compiler revision、Go/Clang/LLD version
- compile/link commandと全option
- generated LLVM IR、Wasm、bridge
- Wasm imports/exports、section size、raw/compressed size
- browser/Nodeでのinstantiateとexecution result
- cold/warm execution、memory growth、allocation behavior

目的はHikeを合否判定することではなく、2.56KBへ至るartifact pathとcost移動を再構成することである。

### Phase 1 — Capability差分実験

同一compiler・targetで機能を一つずつ追加し、artifact deltaを測る。

| Variant | 追加する能力 | 主な観測 |
| --- | --- | --- |
| V0 | scalar add/exportのみ | 最小module固定費 |
| V1 | internal call/branch | code reachability |
| V2 | byte sequenceとbounds | memory/support cost |
| V3 | allocation | allocator本体またはhost import cost |
| V4 | string/UTF-8 bridge | representationとbridge cost |
| V5 | DOM/host callback | import/adapter cost |
| V6 | panic/failure | failure runtime cost |
| V7 | concurrency/atomic | memory/thread capability cost |

各deltaをsource construct、semantic obligation、generated symbol、artifact bytesへ関連付ける。

### Phase 2 — LAMINARIA owned micro-Wasm slice

LAMINARIAの対応済みRust/Nim subsetから、V0〜V2相当の一つを外部compiler fallbackなしで生成する。

最初からHikeの2.56KBを下回ることを完了条件にはしない。最初の目的は、生成された全byteと全importについて、LAMINARIAが「なぜ必要か」を説明できることである。

### Phase 3 — Runtime materialization variant

同じsupport capabilityについて次を比較する。

1. module内へ常に埋め込む。
2. module内へdemand-drivenに生成する。
3. host importとして供給する。
4. 複数Rust/Nim unitで共有する。
5. component/adapter境界へ置く。

比較軸はWasm sizeだけでなく、bridge size、startup、call cost、memory、cache identity、invalidation、portability、説明可能性を含む。

### Phase 4 — Micro-Wasm topology

同じ処理を次の構成で比較する。

- 単一Core Wasm module
- 複数の小module
- Rust/Nim別moduleとgenerated adapter
- Rust/Nim統合module
- WebAssembly Component

module分割による重複runtime、download/instantiate並列性、call boundary、partial invalidation、artifact reuseを測る。

## 8. 必須測定

### Artifact

- raw `.wasm` bytes
- gzip/Brotli後のbytes
- code/data/custom section別bytes
- user logic、runtime support、adapter、metadataの帰属
- JavaScript/TypeScript bridge bytes
- import/export数とsignature
- total delivery bytes

### Compilation

- parse、semantic analysis、IR construction、transform、target lowering
- support capability resolution
- codegen、link、post-link optimization、bridge generation
- wall/CPU time、peak memory、I/O
- executed/skipped actionとfallback/delegationの有無

### Runtime

- downloadを除くinstantiate time
- first callとwarm call
- representative computation time
- linear-memory initial/peak/growth
- allocation count/bytesとreclamation policy
- host boundary call countと変換bytes

### Reproducibility

- source、compiler、runtime、linker、optimizer revision
- target/features/data layout
- exact command/options
- environment fingerprint
- workload/input identity
- sample count、分布、warmup

## 9. 初期完了条件

- Hikeの極小Wasm経路を固定revisionと完全なtoolchain identityで再現する。
- 2.56KBの構成をsection、symbol、runtime、bridgeの観点から説明する。値が環境差で変わる場合は差のproducerを特定する。
- V0〜V5のうち4種類以上で、source featureからartifact deltaまでを追跡する。
- Rust/Nim双方のsource-derived workloadから、少なくとも一つのLAMINARIA-owned executable Core Wasmを生成する。
- 生成物が正しく実行され、外部source compiler fallbackが使われていないことを証明する。
- すべてのimportとsupport symbolにsemantic/runtime obligationを対応付ける。
- Wasm本体だけでなくbridge、adapter、metadataを含むsystem delivery costを報告する。
- module内support、host import、共有supportのうち2方式以上を比較する。
- 一つのsource/capability変更について、link/post-link/adapterの正確なinvalidation setを示す。
- 結果がHikeより大きい場合も、どの意味契約またはsupport obligationが差を生んだかを説明する。

## 10. 達成と主張を混同しないための境界

Hikeの成果から学ぶことと、すべての説明を無条件に一般化することは別である。

- 2.56KBは、提示されたworkloadにおける重要な実証結果として扱う。
- その値を全Hike program、全browser、全toolchainへ一般化しない。
- Wasm本体からhost/bridgeへ移ったcostも記録するが、それを理由に本体の極小化を無価値としない。
- 現行repositoryの変更や文書との差異は、再現revisionと達成範囲を決める情報として扱う。
- HikeのLLVM利用をLAMINARIAのarchitectureとして採用しないが、極小artifactを可能にした必要条件は独立に再導出する。
- Hike、Rust、TinyGo、Emscripten等の総合順位を目的にせず、差を生むruntime、ABI、representation、link、tooling decisionを特定する。

## 11. 本研究の核心

Hikeから得るべき中心的な知見は、単に「runtimeを削ればWasmが小さくなる」ことではない。

> 高級な記述性、極小artifact、host integration、単純なbuild UXを同時にゴールへ置くと、compilerはsyntax translatorではいられず、semantic representation、runtime capability、ABI、adapter、link、deploymentを一つの設計問題として所有する必要がある。

Hikeはこの考えを、極小Wasmを生成する実際のcompilerという結果まで進めた。

LAMINARIAはそのコードを採用するのではなく、そのエンジニアリング上の到達を出発点にする。Rust/Nimの意味を共有する独自compilerというLAMINARIA固有の条件から、必要runtime、support representation、target contract、artifact boundaryを再導出し、最終artifactの正しさ、小ささ、生成過程、全byteの理由によって検証する。

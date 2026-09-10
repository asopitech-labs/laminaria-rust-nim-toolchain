# LAMINARIA — LLVM上流の性能最適化研究

## 位置付け

この文書は、命令選択、レジスタ割付、命令スケジューリングといったtarget backendの改善ではなく、**低水準IRへloweringする前に保持されている意味を使って、計算量、データ移動、局所性、並列性、特殊化可能性を改善する方法**を研究する。

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的と完了判定の基準とする。LLVM、MLIR、Polly、Pluto、Halide、ISPC、既存C/C++ compiler等はprior art、比較対象、実験用projectionとして扱う。これらへ本コンパイルを委譲することをLAMINARIAの成果とはしない。

本研究は[LLVM再発見研究](llvm-rediscovery-research_ja.md)を、特に次の問いについて具体化する。

- RustとNimのfrontendが知っている意味のうち、低水準IRで失われるものは何か。
- その意味を保持すれば、既存frontendが別々に生成したLLVM IRを後からmergeするより強い変換が可能か。
- target-independentな変換と、target・resource・workloadに依存するschedule選択をどこで分離すべきか。
- 変換、解析、探索、計測をLAMINARIAの計算グラフ上でどう表現し、再利用・無効化・分散できるか。

## 「LLVM上流」の定義

本書でいう上流は、単にLLVM pass pipelineの前半を意味しない。

```text
Rust source / Nim source / restricted kernel or domain description
  → language semantics and effects
  → preserved semantic facts and computational relations
  → legal high-level transformations
  → specialization and schedule search
  → progressively lowered representations
  → target-independent low-level IR
  → target lowering and machine code
```

中心対象は、低水準IRへ落とすと復元が困難になる以下の情報である。

- shape、rank、extent、stride、layout
- alias、ownership、borrow、escape
- purity、effect、例外・panic・unwind、allocation
- iteration domain、dependence、reduction、associativity
- producer/consumer relation、pipeline stage
- generic parameter、compile-time value、runtime-stable value
- branch probability、value distribution、representative input class
- parallel independence、determinism、ordering contract
- numerical contract、overflow、rounding、fast-mathを許す範囲

## 研究目標

1. 上流意味情報を保持した場合に初めて可能になる最適化を、Rust/Nim両方のsource-derived workloadで示す。
2. 変換の合法性を、backendの成功や出力一致だけでなく、言語意味・effect・依存関係から説明する。
3. algorithm、transformation、schedule、target loweringを混同せず、各決定の入力と責任主体を明示する。
4. 静的cost model、profile feedback、empirical autotuningの適用範囲を比較する。
5. 性能向上だけでなく、compile latency、memory、code size、探索仕事量、incrementality、説明可能性を測る。
6. 成功例だけでなく、変換を拒否すべき条件と性能退行を再現可能な証拠として残す。

## 中心仮説

- **H1 — 意味保持は低水準IRからの再発見より有利である。** Source由来のshape、alias、effect、reduction factsは、load/store/branchからの再構築より多くの合法変換を安定して可能にする。
- **H2 — 万能IRより段階的な複数表現が適する。** 言語固有facts、計算関係、structured compute、低水準IRを必要に応じて共存させ、由来を追跡する方が早期正規化より情報損失を抑えられる。
- **H3 — algorithmとscheduleの分離は探索可能性を高める。** 何を計算するかと、順序・粒度・layout・並列度を分離すれば、意味を壊さず複数scheduleを比較できる。
- **H4 — specializationは型だけでなく値と利用文脈を必要とする。** Shape、stride、alignment、value range、hot call context、effectも特殊化dimensionになる。
- **H5 — cost model単独では十分でない。** 静的modelは候補削減に有効だが、cache、入力分布、分岐coherence、runtime libraryを含む順位付けには代表workloadでの計測が必要である。
- **H6 — 高速化は生成命令数だけでは評価できない。** データ移動、allocation、中間materialization、cache miss、parallel overhead、code size、startupを分けて測る必要がある。
- **H7 — 変換探索もincremental computationである。** Source、semantic fact、profile、target constraintの一部が変わっても、無関係な解析・候補生成・benchmarkを再実行しない設計が可能である。
- **H8 — 汎用言語の曖昧さを減らすほど強い最適化が可能になる。** 任意pointer、観測可能な副作用、dynamic dispatch、例外、未知の外部callを許すほど合法性証明は弱くなる。

## Prior artと再発見対象

### 1. Polyhedral compilation

Polyhedral modelは、affineなloop boundとarray subscriptを持つiteration domainおよびdependenceを整数集合・写像として扱い、loop interchange、fusion/fission、tiling、skewing、parallelizationを探索する。

- **Pluto**はCのloop nestを解析し、局所性とparallelismを目的にC/OpenMPへsource-to-source変換する。
- **Polly**はLLVM上のpolyhedral optimization infrastructureで、tiling、fusion、OpenMP parallelism、SIMD opportunityの露出を扱う。
- **MLIR Affine/Linalg**はloop、多次元data、layout、structured operationを低水準化前に保持し、段階的な変換を構成する。

LAMINARIAで再発見すべきものはツールのAPIではなく、次である。

- iteration domainとdependenceをどのsemantic factsから構成できるか。
- 非affine条件、pointer、effect、panicを含むとき、どの範囲を安全なregionとして切り出せるか。
- fusionによる中間削減と、parallelism低下・working set増加のtrade-offをどう表現するか。
- tile sizeをsemantic transformationとschedule parameterのどちらとして扱うか。

### 2. Multi-level IRとprogressive lowering

MLIRの重要な原則は、**変換に必要な抽象度を、その変換が終わるまで失わない**ことである。LAMINARIAでは次を比較する。

```text
early normalization to one low-level IR
vs.
language facts + relational IR + structured compute IR + low-level IR
```

比較軸は、合法変換数、情報の重複、整合性維持cost、serialization cost、incremental invalidation、diagnostic provenanceである。

### 3. Algorithm / schedule separationとDSL

Halideは画像・array pipelineについてalgorithmとscheduleを分離し、fusion、tiling、parallelization、vectorization、中間buffer配置を独立に探索可能にする。ISPCは`uniform`/`varying`とSPMD execution modelを言語意味として与え、通常のC/C++から推測しにくいSIMD mappingを明示する。

LAMINARIAでは完全な新言語を前提にせず、次の三方式を比較する。

1. 通常のRust/Nim sourceから自動抽出する。
2. annotation、type、macro等で追加契約を与える。
3. 限定されたkernel/domain表現を埋め込み、通常コードと明示境界で接続する。

誤った契約の検出、fallback禁止、debuggability、通常コードとのdata movementも評価する。

### 4. Partial evaluation、staging、specialization

既知の型だけでなく、compile-time value、runtime中に安定する値、call context、shape、stride、effect、value rangeを使い、generic computationから専用variantを生成する。

候補には、固定shape loop、schema別parser/serializer、callback target、alignment/alias contract、input size別algorithm、ahead-of-timeまたはJIT multi-versioningを含む。利益とともにcode size、instruction-cache、compile work、cache identity、variant selection costを測定する。

### 5. Equality saturationと探索的rewrite

貪欲なpass orderingでは、早い局所判断が後の候補を消すことがある。Equality saturationは等価な表現を一定期間保持し、cost modelまたは計測に基づいて抽出する。

全面採用は仮定せず、限定されたpure expression、address calculation、algebraic simplification、fusion/fission candidateに対し、greedy rewriteとの品質差、overflow・numerical・effect semantics、探索budget、proof/explanationの保存を検証する。

### 6. Profile-guided specializationとautotuning

Profileは低水準branch countだけでなく、input shape distribution、value range、sparsity、branch coherence、allocation lifetime、call-context frequency、candidate scheduleの実測値をsemantic nodeへ関連付ける。

静的modelで不適格候補を除外し、残りを代表workloadで測るhybrid方式を中心候補とする。収集環境、入力集合、version、target、信頼区間、鮮度をidentityに含め、holdout workloadで過学習を検出する。

### 7. Source contractsとoptimizer-enabling information

No-alias、alignment、purity、`final`、range、loop transformation directive等は、変換が合法であることをcompilerへ伝える。

LAMINARIAでは契約にsourceまたは解析結果への由来、scope、verifier、依存する変換、invalidation条件、違反時のdiagnosticまたはruntime checkを要求する。Unsafeな仮定で速くなった結果を証明済み最適化と混同しない。

### 8. Algorithmとdata representationの選択

AoS/SoA、dense/sparse、eager/fused、comparison/radix、scalar/batch、repeated lookup/precomputed index等の選択は命令置換より大きな差を生む場合がある。一方でmemory、ordering、stability、numerical behavior、API observable behaviorを変え得るため、明示されたsemantic contractとcost boundaryを持つ計画問題として扱う。

## Candidate LAMINARIA architecture

次は採用済み構造ではなく、実験対象となるcandidateである。

```text
Rust/Nim source
  → syntax + source provenance
  → language-specific semantic facts
  → effects / alias / ownership / numeric contracts
  → computational relation graph
  → structured regions (loop / pipeline / reduction / kernel)
  → legality analysis
  → transformation space
  → static pruning
  → specialization and schedule variants
  → profile-guided / empirical selection
  → progressively lowered LAMINARIA IR(s)
  → LAMINARIA-owned target generation
```

各変換候補は少なくとも次を持つ。

```text
TransformationCandidate {
  input_semantic_identity
  preconditions
  legality_evidence
  affected_relations
  produced_variant_identity
  invalidated_analyses
  estimated_cost_delta
  measured_evidence
  provenance
  rejection_reason
}
```

Pass名だけを記録して説明可能性としない。「どのfactが何を許可し、どの候補と比較し、なぜ選択または拒否したか」を追跡可能にする。

## Rust/Nim cross-language研究質問

1. Rust ownership/borrow informationをalias・escape・lifetime factとしてどこまで言語横断利用できるか。
2. Nimのeffect tracking、compile-time execution、generic instantiationから何を共通表現へ保持できるか。
3. 一方の言語の強い契約を、C ABIへ早期loweringせず他方から呼ばれるregionの最適化へ利用できるか。
4. Panic、exception、unwind、destructor/finalizer、GC safepointを跨ぐfusionやmotionはどこまで合法か。
5. Rust iteratorまたはNim iteratorを共通pipeline relationへ変換し、中間allocationやdispatchを除去できるか。
6. Cross-language callbackをspecialize/devirtualizeする条件をsource semanticsから構築できるか。
7. 同じ意味のRust/Nim実装が異なる高水準構造を生成した場合、正規化すべきか別variantとして探索すべきか。

## 最初の実験workload

- **W1 — Map/filter/reduce pipeline:** Rust/Nim iteratorについてmaterialization、allocation、fusion、reduction、parallel reductionを比較する。
- **W2 — Affine stencil:** 2D/3D stencilでloop interchange、fusion、tiling、parallelizationを比較する。
- **W3 — Shape-specialized matrix kernel:** dynamic shape、profile-dominant shape、fixed shapeを比較し、variant/code-size costも測る。
- **W4 — AoS / SoA layout:** Scan、filter、update、変換、FFI境界のdata movementを含めて比較する。
- **W5 — Alias-sensitive loop:** Alias不明、解析済みno-alias、source contract、runtime overlap checkの各variantとnegative testを比較する。
- **W6 — Context specialization:** Hot contextだけを特殊化し、code size、compile work、cold-path regressionを測る。
- **W7 — Irregular rejection case:** Pointer chasing、observable effect、non-affine bound、panic/unwindを含め、適用しない理由を説明する。
- **W8 — Autotuned schedule:** Loop order、tile size、fusion boundaryについて静的、profile、実測選択をholdout inputで比較する。

正しさには順序、overflow、floating-point、panic/effect、並行性契約を含める。

## 比較対象

| 分類 | 候補 | 比較するもの |
| --- | --- | --- |
| 汎用compiler | Clang/LLVM、GCC、rustc、Nim compiler | 通常optimizationのbaseline |
| Polyhedral | Pluto、Polly | dependence、tiling、fusion、parallelization |
| Multi-level IR | MLIR Affine/Linalg | 意味保持とprogressive lowering |
| Schedule DSL | Halide | algorithm/schedule分離とautoscheduling |
| Parallel language | ISPC | 明示的SPMD semantics |
| LAMINARIA | 独自facts、IR、変換、探索 | 仮説を検証する本経路 |

既存toolの総合順位だけを結論にせず、どの情報、変換、cost model、runtime/libraryが差を生んだかを分解する。

## 測定契約

### Correctness

- semantic oracleまたはproperty-based comparison
- overflow、rounding、NaN、ordering
- panic/exception/unwind、allocation、observable effects
- race、determinism、reduction contract
- negative legality tests

### Runtime performance

- wall timeと分散、throughput / latency
- allocation count / bytes、peak resident memory
- 利用可能な場合のcache miss、branch、instructions
- cold、warm、startup、steady stateの分離

### Compiler economics

- parse、analysis、candidate generation、lowering、codegen、linkの時間とpeak memory
- intermediate size、variant数、code size
- autotuning試行数と総CPU時間
- source/profile/target変更時の再実行・再利用範囲

### Optimization quality and explainability

- 検討した候補と適用・拒否理由
- 使用したsemantic factsと由来
- invalidated analysis
- static estimateとmeasurementの差
- baselineとの差を生んだ変換

性能値にはtoolchain、commit、target、flags、input identity、measurement environment、sample countを付ける。異なる意味契約やfast-math条件を同一結果として比較しない。

## 実験の段階

1. **Phase 0 — Prior-art reproduction:** 小さなworkloadで代表変換、入力制約、失敗条件、計測costを再現する。
2. **Phase 1 — Semantic fact inventory:** Rust/Nim sourceからshape、alias、effect、iteration、reduction、numeric contractを導出し、保持・変形・消失を追跡する。
3. **Phase 2 — One owned transformation slice:** 一つのworkloadをsource-derived facts、独自representation、合法性、変換、target生成までLAMINARIA自身で実行する。
4. **Phase 3 — Variant and empirical selection:** 複数の合法variantを生成し、training inputとholdout inputを分けて静的modelと実測選択を比較する。
5. **Phase 4 — Incremental and distributed search:** Fact、profile、target、schedule変更時のinvalidationと解析・候補・measurement artifactの再利用を検証する。
6. **Phase 5 — Cross-language transformation:** Rust/Nim境界を跨ぐfusion、specialization、allocation eliminationを一つ以上成立または証拠付きで棄却する。

## 初期完了条件

- 8 workloadのうち5つ以上をsource-derived Rust/Nim pairとして実装する。
- 3系統以上のprior artについて代表変換と失敗条件を再現する。
- 2種類以上の高水準semantic factが、低水準推測では得られない、または不安定な最適化機会を生むことを示す。
- 合法な変換と拒否する変換をLAMINARIA所有のlegality evidenceで説明する。
- 1つ以上のowned transformationをsourceからtarget artifactまで本経路で実行する。
- Static scheduleとempirically selected scheduleをholdout inputで比較する。
- Runtimeだけでなくcompiler economicsとcode sizeを報告する。
- Source、profile、targetの各変更についてinvalidation/reuseを測る。
- Rust/Nim境界を跨ぐ最適化を成立させるか、意味上不可能な理由を再現可能な証拠で示す。
- 選択・拒否・退行を人が追跡できるdiagnosticを生成する。

`-O3`、LTO、PGO、Polly flagの有効化、MLIR/LLVM IRの生成、Halide/ISPC/vendor libraryの呼出し、単発benchmarkの高速化、unchecked unsafe assumptionは、この完了条件を満たさない。

## 非目標

- あらゆるC/C++、Rust、Nim programの自動algorithm改善を保証すること。
- LLVM、MLIR、Halide等のAPIを同じ形で再実装すること。
- 単一benchmarkまたは単一CPUへの過学習を製品性能として扱うこと。
- Floating-point、overflow、effectの意味を黙って弱めること。
- 全探索を行い、探索costを性能結果から除外すること。
- 既存toolへの委譲をLAMINARIA独自コンパイラの証拠とすること。

## 主要資料

### Multi-level IR

- MLIR, “Rationale”: <https://mlir.llvm.org/docs/Rationale/Rationale/>
- MLIR, “Affine Dialect”: <https://mlir.llvm.org/docs/Dialects/Affine/>
- MLIR, “Linalg Dialect”: <https://mlir.llvm.org/docs/Dialects/Linalg/>
- Lattner et al., “MLIR: Scaling Compiler Infrastructure for Domain Specific Computation”: <https://arxiv.org/abs/2002.11054>

### Polyhedral compilation

- Pluto project: <https://github.com/bondhugula/pluto>
- Polly project: <https://polly.llvm.org/>
- Polyhedral compilation resources: <https://polyhedral.info/>

### Algorithm / schedule separation and explicit parallel semantics

- Halide project and publications: <https://halide-lang.org/>
- Halide autoscheduler tutorial: <https://halide-lang.org/docs/tutorial/lesson_21_auto_scheduler_generate.html>
- Intel ISPC: <https://ispc.github.io/>
- Intel ISPC Performance Guide: <https://ispc.github.io/perfguide.html>
- OpenMP specifications: <https://www.openmp.org/specifications/>

### Specialization and equality saturation

- Jones, Gomard, Sestoft, “Partial Evaluation and Automatic Program Generation”: <https://studwww.itu.dk/people/sestoft/pebook/>
- Willsey et al., “egg: Fast and Extensible Equality Saturation”: <https://doi.org/10.1145/3434304>
- `egg` documentation and tutorials: <https://docs.rs/egg/latest/egg/>

### Feedback and conventional optimization baseline

- Clang Profile Guided Optimization: <https://clang.llvm.org/docs/UsersManual.html#profile-guided-optimization>
- Clang ThinLTO: <https://clang.llvm.org/docs/ThinLTO.html>
- GCC Optimize Options: <https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html>

資料の記述やbenchmark結果をLAMINARIAへの適用証拠とみなさない。各主張は固定した意味契約、再現可能なworkload、計測、negative testから独立に検証する。

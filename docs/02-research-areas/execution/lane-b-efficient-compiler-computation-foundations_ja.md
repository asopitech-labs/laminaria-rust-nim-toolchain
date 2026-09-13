# Lane B 基礎研究 — Efficient Compiler Computation

## 位置づけ

本書は[当面の研究ゴール](../../near-term-research-program_ja.md)から派生するLane Bの基礎研究である。調査日は2026-09-13。Lane Bの問いは「並列buildを作ること」ではない。Lane Aが定義した正しいartifact closureを得るために、どの計算を発見し、避け、再利用し、再計算し、どの順序と場所で実行し、どの中間値を保持・破棄するかを、時間・CPU・peak memory・I/O・転送量の制約下で決めることである。

最重要の区別は、依存関係の意味と、依存関係を評価する算法を混同しないことである。Cargo/Nimble/C/C++の関係を一つのgraphへ載せても、それだけでは高速でも省メモリでもない。逆に高速なaction schedulerを持っても、graphがpackage/source/IR/ABIの真の依存を欠けば誤った結果を速く作るだけである。

## 1. 計算問題の軸

Build Systems à la Carteはbuild systemを、taskの実行順序を決めるschedulerと、再実行要否を決めるrebuilderへ分離し、static/dynamic dependencies、dirty bit、verifying/constructive trace等を比較可能にした[1]。LAMINARIAではさらに次を別の判断軸として扱う。

| 軸 | 問い | 失敗例 |
| --- | --- | --- |
| discovery | 必要なnode/edgeをいつ知るか | 全候補・全sourceを先に展開する |
| correctness | 変更・動的依存を漏らさないか | stale result、false green |
| pruning | 計算前に不要を証明できるか | linker GCまで全compileを払う |
| reuse | 同一結果をmemory/disk/remoteから再利用できるか | cache key不足、誤共有 |
| granularity | query/function/module/crate/actionのどこで切るか | 細粒度overheadまたは粗粒度再計算 |
| scheduling | ready workの順序・並列度をどう選ぶか | critical path悪化、oversubscription |
| retention | 中間値をいつ保持・spill・dropするか | peak RSS超過、再計算過多 |
| placement | local/remote/heterogeneous nodeのどこで動かすか | 転送が計算利益を超える |
| publication | resultをいつatomicに可視化するか | partial/stale artifact混入 |
| explanation | 実行・省略・invalidated理由を説明できるか | tuning不能、証拠不能 |

## 2. 先行研究の地図

| 系統 | 得られる原理 | LAMINARIAとの差 |
| --- | --- | --- |
| Build Systems à la Carte | schedulerとrebuilderの直交性、correctness/minimality、dynamic dependency分類[1] | task semanticsを所与とし、package/IR/ABIを同時に解くものではない |
| Pluto | dynamic/cyclic dependencyを含むincremental buildについてsoundnessとoptimalityを扱う[2] | compiler IRやheterogeneous artifact obligationは対象外 |
| PIE | filesystem interactionを明示したprecise incremental build script[3] | graphの意味はbuild-script levelで、compiler queryやpackage candidate feedbackとは別 |
| Forward build/Rattle | process tracingで「完全な依存宣言」を要求せず、parallel/early cutoffのcorrectnessを形式化[4][5] | 観測したfile/process dependencyより高いsource/ABI意味は復元しない |
| rustc query system | demand-driven memoized query、runtime dependency discovery、red-green incremental、projection firewall[6][7] | 主に1 compiler/crate内部。Cargo/native package selectionやlink/runtime closureは別系統 |
| Salsa | revision間memoizationとtracked dependencyをlibrary化[8] | query authorが入力/同値性/副作用境界を正しく定義する必要がある |
| Bazel Skyframe | immutable valueのparallel incremental evaluation[9] | configured target/action graphの評価であり、source-to-package feedback solverではない |
| Buck2 DICE | build daemon内の汎用incremental computation、multi-language rule graph[10] | compiler semantic queryとpackage constraintの共同固定点はrule実装側に残る |
| ThinLTO | compact module summary、serial thin-link、parallel backend、incremental cacheでwhole-program最適化をscaleさせる[11] | linkに届いたbitcode/moduleが対象。package/sourceの早期pruningではない |
| Remote execution/CAS | hermetic actionをdigestで共有し、placementとcacheを分離する[12] | cache hitは不要計算を意味的に証明したことではない。転送・trust・platform identityが必要 |
| memory-aware DAG scheduling | makespanとpeak memoryが衝突し、一般のparallel peak-memory最小化は困難である[13][14] | compiler/buildの値寿命、再計算可能性、spill costをmodelへ写す必要がある |

## 3. 正しさ、minimality、速さは別である

Build Systems à la Carteのminimalityは「1 build中にtaskを高々一度、前回から変更されたinputへ推移的に依存する場合だけ実行」と定義される[1]。これは重要だが、LAMINARIAにはその前に「task集合を過不足なく発見したか」がある。

```text
semantic correctness
  -> dependency correctness
    -> invalidation correctness
      -> work avoidance / reuse
        -> schedule / memory / placement optimization
```

下位の最適化は上位の正しさを仮定する。例えばcache keyがtarget ABIを含まなければhit率は高くても誤りである。dynamic `dlopen` rootを無視すればpruning率は高くても不健全である。全testを省略すればtest時間は最小だがLane Cの選択soundnessを満たさない。

## 4. demand-driven discoveryとdynamic dependency

rustc query systemはtop-level demandから必要な情報を遡り、query invocation時にdependency edgeを発見し、memoizeする[6]。これはLAMINARIAに近いが、実装上の教訓がある。

- queryは同一inputに同一resultを返す純粋性が基本。file/global state/side effectを読むqueryは別扱いが必要[7]。
- stable identityがなければrevision間cacheの値を対応付けられない[7]。
- hashing自体にもcostがあり、全値を細粒度fingerprintするのが最適とは限らない[7]。
- 大きく変わるaggregateから小さなprojection queryを作り、change propagation firewallにできる[7]。
- greenと判定した中間結果をdiskからloadしなかった場合、次revisionのcacheから失うためcache promotion等のretention政策が要る[7]。

Pluto、PIE、forward buildは、dependencyを事前に完全宣言する方式だけが選択肢でないことを示す[2][3][4]。しかしtraceだけでは「fileを読んだ」事実は得られても、なぜそのheader/feature/symbolが必要か、別候補なら不要かは分からない。LAMINARIAでは、declared semantic edgeとobserved process/file edgeを同じ種類に潰さず、相互検証する。

## 5. 枝刈りは複数時点にある

| 時点 | root / evidence | 避けられるcost |
| --- | --- | --- |
| resolution前 | target/feature/provider/ABI constraint | metadata fetch、candidate expansion |
| source discovery時 | conditional import、module/type/FFI fact | parse、macro/generation、semantic analysis |
| monomorphization/lowering前 | reachable generic/call/export root | IR生成、optimization |
| IR optimization時 | use-def、effect、visibility | instruction/codegen |
| link時 | entry/export/undefined symbol、relocation | retained section、binary size |
| runtime closure時 | loader/resource reference | deployment content |

LLVM LTOではlinkerのglobal symbol tableとlive symbolをoptimizerへ返し、intermodular optimization後に再びsymbol tableを更新する[15]。ThinLTOは全module本体を一つへmergeせずsummary indexを作り、parallel backendへ分ける[11]。これは「下流のlink情報をoptimizationへ戻す」「summaryで全体判断し、本体処理を分割する」先例である。ただしLAMINARIAの仮説は、このfeedbackをpackage/source/IR生成前まで早める点にある。

early pruning、compiler DCE、linker GCは代替関係ではない。早期層はcostを避け、後期層は早期近似が保守的に残した不要物を回収する。比較では最終binary sizeだけでなく、各層で避けたparse/compile/IR bytes/action数を測る。

## 6. incremental identityとinvalidation

cache keyは少なくとも次のlogical identityを含む必要がある。

```text
operation kind
+ semantic input digests
+ target / host role
+ toolchain and ABI identity
+ relevant configuration/features
+ environment contract
+ algorithm/schema version
```

path、timestamp、process-local sequential IDだけではrevision間同一性にならない。逆に全environmentをkeyへ含めるとreuseが消える。各operationが読むexternal stateを明示し、declared/observed dependencyを照合する必要がある。

変更時の証拠は単なるcache hit率では不十分である。

- invalidated node数と理由。
- recomputed node数、loaded node数、結果同値によりearly-cutoffしたnode数。
- downstreamへ変化を伝播しなかったprojection/canonicalization。
- stale reuseを検出するclean rebuildとのartifact/behavior比較。
- graph schema/toolchain変更による意図的全失効。

## 7. 粒度、並列性、peak memory

細粒度化はincremental reuseと並列性を増やす一方、node/edge、hash、serialization、scheduling overheadを増やす。粗粒度化は管理costを減らす一方、leaf editで大きな再計算を起こす。したがって「関数単位が常に最善」「crate/action単位が十分」という先験的結論を置かない。

ThinLTOはcompact summaryによる全体分析とmodule単位parallel backendという二段構成で、monolithic LTOのtime/memory/incremental問題へ対処する[11]。LAMINARIAでも、package/source/IR/symbol summaryを常駐させ、重いbodyを必要時にmaterializeする設計が候補になる。

最大並列度は最小時間を保証しない。ready taskが大きなintermediateを同時保持すればpeak RSSが増える。task graphのpeak-memory-aware schedulingは一般に計算困難で、並列度とcritical pathのtrade-offを持つ[13][14]。各nodeへ次を観測・推定する必要がある。

- execution time、CPU、I/O、network。
- resident input/result bytesと一時working set。
- fan-out、last consumer、critical-path slack。
- spill/load/recompute cost。
- deterministic/hermetic/remote-eligible属性。

初期schedulerは最適化solverから始めず、memory budget付きready queue、last-consumer後の解放、oversized taskの並列抑制という説明可能なheuristicと、unbounded parallel baselineを比較する。

## 8. cache、永続化、分散は後続の物理化である

CAS/remote executionは論理graphを速くする有力手段だが、最初の研究対象を隠してはならない。

- cache hit: 必要な計算結果を再利用した。
- pruning: 計算自体が意味的に不要と証明された。
- early cutoff: 入力側が変わっても結果が不変なので下流伝播を止めた。
- fusion: 中間materialization/transferを消した。
- distribution: 計算場所を変えた。

これらは別metricである。Buck2はBazel Remote Execution specを並列化/cacheに使い、idempotencyとhermeticityを重視する[10][12]。LAMINARIAでは、logical operation identityとphysical attempt identityを分け、retry/speculationしても成果物producer lineageが曖昧にならないようにする。

## 9. LAMINARIA固有の仮説

### B-H1 — cross-layer demand

final artifact/test rootからpackage/source/IR/artifactへ需要を伝えることで、全候補・全code先行展開より、正しいclosureのまま外部actionとpeak memoryを削減できる。

### B-H2 — semantic early cutoff

text/file digestが変わっても、canonical semantic summaryが同じなら下流package/lowering/link/test invalidationを止められる。

### B-H3 — summary/body分離

常駐するsmall summary graphと、必要時だけmaterializeするsource/IR/object bodyを分けることで、探索能力を保ったままpeak RSSを抑えられる。

### B-H4 — obligation-aware scheduling

critical pathだけでなく、未解決obligationを最も減らす情報獲得価値とmemory releaseを優先すると、単純FIFO/最大並列よりtime-to-rejectionまたはtime-to-artifactを改善できる。

### B-H5 — shared graph retest

build invalidationとLane Cのretest selectionが同じcausal edgesを使えば、別のcoverage catalogより保守的soundnessを説明しやすい。

## 10. 実験計画

### workload changes

1. cold build。
2. no-op rebuild。
3. 到達するleaf implementation変更。
4. 到達不能source変更。
5. public type/ABI変更。
6. feature/target predicate変更。
7. root/export/test demand変更。
8. toolchain/schema version変更。

### strategies

- E0: eager candidate/source expansion + coarse action graph。
- E1: demand-driven discoveryのみ。
- E2: E1 + cross-layer pruning/canonical early cutoff。
- E3: E2 + persistent memoization。
- E4: E3 + memory-budget scheduling/spill/recompute。

### metrics

- correctness: clean buildとのartifact digest/behavior/obligation state一致。
- latency: resolution、first rejection、first runnable artifact、total wall-clock。
- work: expanded/pruned/merged/recomputed node、external process、IR/codegen unit。
- resource: CPU time、peak RSS、bytes read/written/serialized/transferred。
- reuse: memo hit、loaded result、early cutoff、recomputed equivalent resultを別集計。
- quality: artifact size、runtime、診断説明可能性。

### 反証条件

- graph構築・hash・serialization costが、代表変更で避けたworkを上回る。
- clean buildとincremental resultが一致しない。
- dynamic/implicit dependencyを保守的に扱うと全候補展開と同等になる。
- 細粒度化でpeak memoryまたはwall-clockが一貫して悪化し、意味feedbackにも寄与しない。
- memory budget schedulerがOOMを避けても許容不能なcritical-path悪化を生む。
- semantic summaryの同値判定が本体再計算より高価または不健全である。

## 11. M1での採用範囲

M1ではdistributed executionやglobal optimal schedulerを要求しない。まずE0〜E2を同一process/local machineで比較し、次を満たす一つのarchitecture判断を得る。

1. exact native artifactとLane C結果がbaselineと同値。
2. pre-compilation rejectionまたは到達不能branchで、外部actionを一つ以上回避。
3. expanded state、wall-clock、peak RSSを同時に記録。
4. 改善しない場合も、どのoverhead/semantic barrierが原因かをoperation eventで説明。

## 参考文献

1. Mokhov, Mitchell, Peyton Jones, “Build Systems à la Carte,” ICFP 2018, [DOI:10.1145/3236774](https://doi.org/10.1145/3236774), [author PDF](https://simon.peytonjones.org/assets/pdfs/build-systems-original.pdf).
2. Erdweg et al., “A Sound and Optimal Incremental Build System with Dynamic Dependencies,” OOPSLA 2015, [PDF](https://www.mathematik.uni-marburg.de/~seba/publications/pluto-incremental-build.pdf).
3. Konat et al., “Precise, Efficient, and Expressive Incremental Build Scripts with PIE,” [PDF](https://gkonat.github.io/assets/publication/pie-ic19.pdf).
4. Spall, Mitchell, Tobin-Hochstadt, “Build Scripts with Perfect Dependencies,” [arXiv:2007.12737](https://arxiv.org/abs/2007.12737).
5. Spall, Mitchell, Tobin-Hochstadt, “Forward Build Systems, Formally,” [arXiv:2202.05328](https://arxiv.org/abs/2202.05328).
6. Rust Compiler Development Guide, [Queries: demand-driven compilation](https://rustc-dev-guide.rust-lang.org/query.html).
7. Rust Compiler Development Guide, [Incremental compilation in detail](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html).
8. Salsa project, [Overview](https://salsa-rs.github.io/salsa/).
9. Bazel documentation, [Skyframe](https://bazel.build/versions/7.0.0/reference/skyframe).
10. Buck2 documentation, [Modern DICE](https://buck2.build/docs/insights_and_knowledge/modern_dice/) and [Architecture](https://buck2.build/docs/concepts/architecture/).
11. Clang documentation, [ThinLTO](https://clang.llvm.org/docs/ThinLTO.html).
12. Bazel documentation, [Remote execution overview](https://bazel.build/remote/rbe).
13. Marchal et al., “Limiting the Memory Footprint when Dynamically Scheduling DAGs on Shared-Memory Platforms,” JPDC 2019, [DOI](https://doi.org/10.1016/j.jpdc.2018.10.003).
14. Jin et al., “New Tools for Peak Memory Scheduling,” [arXiv:2312.13526](https://arxiv.org/abs/2312.13526).
15. LLVM, [Link Time Optimization: Design and Implementation](https://llvm.org/docs/LinkTimeOptimization.html).


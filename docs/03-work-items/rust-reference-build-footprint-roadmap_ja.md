# Rust build pathの代替不可能なコア価値：省資源研究ロードマップ

## 目的・現在地・次の判断

**目的**は、LAMINARIA自身がCargo、rustc、LLVMのRust buildにおける責務を置換する最終目標へ向け、既存systemを組み合わせても代替できない最小コア価値を先に反証することである。その価値は、要求native artifactからpackage/unit、Rust semantics、IR、symbol/linkまでを一つの需要駆動typed graphで閉じ、下流で得た事実を上流へ戻して**実行前に不要workを消せるか**にある。CPU時間、peak RSS、disk、I/O、測定可能なenergyは、この因果が正しいかを判定する成果指標である。

**現在地**では、Cargo/Nimble/C/C++のclosureを同じtyped graphで解き、LAMINARIAのsource/IR/planner/target pathからnative executableへ至るM1が主gateである。Rustに限る本trackはその縮小版ではない。Cargoのwork discovery、rustcのsemantic incremental computation、LLVMのoptimization/target computationを横断して結ぶ仮説を、Rust-only workloadで最初に検証する。

**次の判断**は、Cargo互換inputを受け取るLAMINARIAのRust pathが、どの最小semantic contractまでを所有すれば、下流のsemantic/ABI/symbol/liveness事実によって上流のpackage/unit/source/IR workを安全に変えられるか、である。外部Cargo/rustc/LLVMはreference oracleであり、外部toolの候補比較や一般的な再実装のfeasibilityを本研究のゴールにしない。

## P0：代替不可能な最小コア

Cargo resolver、rustc query、LLVM analysis/LTO、Bazel/Buck2のaction graph、generic cache/schedulerは、それぞれの層では既に強い先行実装がある。したがって「resolverを実装できる」「queryをcacheできる」「native objectを作れる」「memory budgetを設ける」だけでは、LAMINARIA固有の研究仮説を支持しない。

```text
requested native artifact
  <-> package / version / feature / build unit
  <-> Rust source semantics / macro effect / generic reachability
  <-> LAMINARIA IR / lowering capability
  <-> object / symbol / link / runtime closure
```

P0で問うのは、上の閉ループを一つのtyped graphとして解き、**source semanticsまたはlink livenessで初めて得た事実が、package candidateまたはbuild unitの選択・棄却を変え、parse/typecheck/IR/codegenの実行前にworkを消すか**である。Cargo→rustc→LLVMを直列に走らせ、最後にcacheやlinker GCをかける方式ではこの因果を示せない。

| 優先度 | 検証対象 | 完了と数えないもの |
| --- | --- | --- |
| P0 | cross-layer feedbackでのearly pruning、compile前reject、同じgraphでの正しさと資源因果 | Cargo/rustc/LLVM相当の単体機能、cache hit、link成功 |
| P1 | P0を成立させる最小Rust parse/semantic IR、unit model、target lowering、identity、direct executable test | 全Rust互換、全platform、汎用memory/disk管理 |
| P2 | spill/remote/cache service、全macro/build-script、独自linker、広いtoolchain/OS matrix | P0の代わりとなる「完成度」 |

## 置換の境界

「Cargoを置換する」は`Cargo.toml`/`Cargo.lock`を読めなくすることではない。互換input、registry/source取得、lockfileというecosystem data formatを受理してもよい。一方、Cargo processによるresolution、feature activation、unit graph構築、build script/proc macro実行、rustc invocationの組立てを本経路に残さない。同じく「LLVMを置換する」はLLVM IRを出せることではなく、LAMINARIAのIR、analysis、変換、target lowering、object production、link-plan/livenessを計算主体として所有することである。

```text
Cargo-compatible manifest / lock / source closure
  -> LAMINARIA Rust package + feature + target + unit graph
  -> LAMINARIA macro / generated-input effect contract
  -> LAMINARIA Rust parsing, name/type/ownership semantics, source-derived IR
  -> LAMINARIA demand-driven analysis, transformation, monomorphization
  -> LAMINARIA target IR, optimization, native object production
  -> LAMINARIA link plan, symbol/section/runtime closure
  -> requested native executable
```

初期native sliceでplatform linkerまたは明示されたforeign C/C++ compilerをActionとして使う余地はある。ただしRust target unitをrustc/LLVMへ渡すfallbackではない。linkerは入力・symbol・runtime obligationをLAMINARIAのgraphで受け取る外部境界であり、link成功をLLVM置換の証明にしない。

| 置換対象 | LAMINARIAが所有する責務 | resource上の中心仮説 | 外部実装の位置 |
| --- | --- | --- | --- |
| Cargo | dependency/version/feature/host-target/build roleの解決、unit/action graph、invalidity、実行順序 | 不要candidate、target、build actionをparse/download/compile前に除ける | manifest/lock/registry semanticsのreference。Cargo processはbaselineのみ |
| rustc | Rust syntaxからのsemantic facts、macro/effect contract、name/type/ownership、monomorphization、query DAG、incremental invalidation | semantic changeを小さなprojectionで止め、無関係なcrate/item/IRを再計算しない | compiler behavior/oracleと観測baseline。rustc queryを実装依存にしない |
| LLVM | target-independent IR、analysis/transform、analysis preservation/invalidation、target lowering、object generation、link-time liveness feedback | whole-program情報はcompact summaryで扱い、body/analysis/codegenを必要時だけmaterializeする | prior-art/reference。LLVM IR/bitcode/backendへの委譲は本経路にしない |

## 最小仮説検証契約

- **優先度:** P0。Rust-onlyのowned pathをM1 mixed closureと同じ原則で反証する。外部toolの運用改善は成果物ではない。
- **最小仮説:** 一つのRust workloadで、source semanticsまたはlink livenessから得た事実をpackage/unit選択へfeedbackすれば、外部compile前に一つ以上のcandidate/unit/source/IR/codegen workを安全に省略しつつ、同じnative artifact behaviorまたは構造化rejectを得られる。
- **最小実験:** 一つのlock済みRust-only workloadに、到達不能または意味/ABI上不適格なcandidateを一つだけ含める。positive artifact、compile前reject、そしてfeedbackを切ったeager baselineとの実行集合比較を同じEnvironmentFingerprintで行う。
- **停止条件:** feedbackが選択・棄却・実行集合を実際に変え、direct executable testとraw resource evidenceがその正しさとcostを支持する、または変えられない理由を得た時点。外部compilerを呼ばなければ成立しない要求は対応範囲外として構造化rejectする。
- **非ゴール:** Cargo/rustc/LLVMの全互換性、全macro/build-scriptの実装、全target/OS、最初からの独自linker、または一指標だけの最速化。

## 資源を減らす設計順序

各段で次の順序を崩さない。並列化、cache、速いbackendを先に置くと、本来不要な計算を速く・大容量で実行するだけになり得る。

```text
1. work elimination       要求artifactに不要な候補・source・IR・symbolを発見前に除く
2. valid reuse            同じ意味結果だけをmemory/diskから再利用する
3. narrow invalidation    変更をprojectionで止め、必要なquery/actionだけをredにする
4. memory lifetime        last consumer後にdropし、spill/reload/recomputeを比較する
5. bounded parallelism    CPU/RSS/I/O budget下でready workを選ぶ
6. residual speedup       残ったparse/typecheck/opt/codegen/linkを速くする
```

energyはwall-clockやCPU時間から自動的には結論しない。[Linux powercap/RAPLの`energy_uj`のような信頼できるenergy counter](https://docs.kernel.org/power/powercap/powercap.html)が同一EnvironmentFingerprintで取れる場合だけjouleを記録する。取れない環境ではCPU時間、RSS、I/Oをenergy proxyと明記し、energy削減と断定しない。

## Cargo/rustc/LLVM責務はP0の従属enabler

次の三節は完了順序ではない。P0のfeedback loopを成立させる最小範囲だけを実装し、どの責務を広げるかはP0の反例が決める。

## Cargo責務：必要workを構成する前に減らす

Cargo resolverは[version/backtracking、feature、target、normal/build/dev dependencyを扱い](https://doc.rust-lang.org/cargo/reference/resolver.html)、[unit graphはcompiler実行単位までspecializeする](https://doc.rust-lang.org/cargo/reference/unstable.html#unit-graph)。LAMINARIAはこの意味をpackage名の平坦なDAGへ落とさず、少なくとも次をtyped node/edgeとして保持する。

```text
PackageCandidate(version, source, checksum)
FeatureSet / cfg / target / host role
BuildUnit(kind=lib|bin|build-script|proc-macro|test, profile)
GeneratedInput / NativeLinkRequirement / RuntimeRequirement
RequestedArtifact -> demanded units -> semantic and artifact obligations
```

- version/feature/target conflictはcompile前にrejectし、全候補をdownload/parseしない。
- feature、target、requested artifactから到達しないpackage/unitは展開しない。
- build scriptとproc macroは任意processを黙って実行しない。対応済みのeffect/input/output contractとしてgraphへ取り込むか、未対応としてrejectする。
- Cargo `--unit-graph` はreference traceとして使えるが、production graphの生成器ではない。

**測定:** explored/pruned/merged package/feature/unit state、metadata/source bytes、実行を避けたbuild/proc-macro action、resolver CPU/RSS、negative rejectionがcompile開始前だったこと。

## rustc責務：semantic computationを需要駆動・増分にする

[rustcのquery/red-green model](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html)は、依存を持つ純粋計算をDAGとして記録し、入力がgreenなら値をloadせず再実行を避けることを示す。LAMINARIAはquery名やrustc内部IRを模倣せず、Rust意味に必要な独自queryとsource-derived IRを定義する。

```text
source / tokens / syntax
  -> macro-expansion contract -> module/import discovery
  -> name resolution -> type/ownership/effect facts
  -> reachable generic instances -> LAMINARIA semantic / target IR
```

- syntax再利用を許す場合も、型付きAST/HIR/MIRを外部compilerから取得しない。
- query identityはsource digestだけでなく、feature/cfg/target、macro/effect input、language/IR revision、ABIを含める。
- large aggregateをそのまま下流へ渡さず、意味的に小さいprojectionを作り、同値ならdependent queryをgreenにする。
- queryの副作用、file/environment read、macro生成物は宣言・観測し、pure query cacheと混ぜない。
- monomorphization root、export、reflection/dynamic rootを明示し、到達不能generic/IR/codegenを早い時点で除く。

**測定:** parse/expand/resolve/typecheck/lower query数、red/green/loaded/recomputed数と理由、semantic/IR bytes、peak live value bytes、clean rebuildとのartifact/behavior同値性、誤reuse/hidden external compilerがないこと。

## LLVM責務：analysis、変換、target生成をbodyの前に制御する

[LLVM New Pass Manager](https://llvm.org/docs/NewPassManager.html)と[ThinLTO](https://clang.llvm.org/docs/ThinLTO.html)は、analysisの保持/無効化、module summary、whole-program判断とparallel backendの分離を示す重要なreferenceである。LAMINARIAが採用するのは原理であり、LLVM pass pipelineやbitcodeを本経路へ持ち込むことではない。

```text
LAMINARIA semantic IR
  -> target-independent analyses / transformations
  -> compact package/source/IR/symbol summary
  -> demand + liveness + target decision
  -> target-specific lowering / native object
  -> link-plan and symbol/section/runtime closure
```

- analysisはIR unitと依存を明示し、transformはpreserve/update/invalidateを返す。変更のたびに全analysisを捨てないが、証明できないreuseはしない。
- 全module bodyをmerge/常駐させず、summaryでglobal reachability・import/inline候補・link livenessを判断して必要bodyだけmaterializeする。
- logical stage、observation boundary、checkpoint/artifact boundary、execution boundaryを分離する。passごとのprocess/CAS化を既定にしない。
- target loweringとobject生成はLAMINARIA自身の責務とし、LLVM IR/bitcode/Cranelift/GCC backendへのfallbackをdenyする。
- linkで初めて分かるlive symbol/retention情報をsummary/IR側へfeedbackし、早期pruningを保守的に改善する。

**測定:** analysis reuse/invalidation、summary/body bytes、materialized checkpoint数とserialization I/O、executed/skipped transform/codegen action、target object/final artifact size、link liveness、CPU/RSS/I/O、checkpoint benefit対cost、生成artifactの実行結果。

## P0から始める段階的ロードマップ

| 段階 | 最小作業 | P0への寄与 | 完了の証拠 |
| --- | --- | --- | --- |
| R0 — oracle固定 | 一つのRust workload、artifact/reject contract、eager reference execution集合、EnvironmentFingerprintを固定する | 何をfeedbackが変えるべきかを曖昧にしない | direct test、external reference trace、observer overhead |
| R1 — closed-loop proof | requested artifact→unit→semantic/IR→symbol/liveness→unitのfeedbackを一つ実装する | 代替不可能な中心仮説を直接反証する | positive artifact、compile前reject、feedback有無で異なる選択/実行集合 |
| R2 — causality and cost | eager baselineと同じbehaviorを確認し、避けたworkとCPU/RSS/disk/I/Oを対応付ける | 「速そう」ではなく、feedbackが資源差を生んだ因果を示す | raw samples、avoided node/action理由、correctness-equivalent behavior |
| R3 — controlled edit | leaf editまたはroot/feature変更で、semantic projectionが変わらない範囲をgreenに保つ | cross-layer identity/invalidationが一過性のpruningでないかを検証する | clean rebuild比較、recomputed/loaded/dropped理由、no stale reuse |
| R4 — evidence-triggered expansion | R1–R3を壊した最小反例だけをCargo/rustc/LLVM責務へ戻して広げる | 汎用再実装を避け、次の非代替gapだけへ投資する | 反例、追加contract、採用/棄却判断 |

Cargo/rustc/LLVMの個別sliceはR1を成立させるために必要な範囲だけを取る。外部baselineはR0/R2に置き、PFE、Cranelift、sccache、mold、Bazel等をLAMINARIAの候補実装として列挙しない。

## 最小workloadと拒否ケース

新しいfixtureを先に増やさない。既存のlock済みRust workloadから、次の段階を反証できる最小部分を選ぶ。

1. **positive:** Rust-only executable。crate dependency、feature/cfg分岐、reachable genericまたはexportを一つ含み、直接実行できる。
2. **Cargo拒否:** version/feature/targetの一つが両立せず、external Cargoやrustcを起動する前に理由付きでrejectする。
3. **semantic edit:** 実装のleaf変更で、無関係module/IR/objectが再計算されないかをclean rebuildと比較する。
4. **effect拒否:** 未対応build script/proc macroまたは未宣言environment/file readを検出し、外部実行へfallbackせずrejectする。
5. **liveness:** root/feature/export変更で必要なcodegen/link inputだけが変わり、保守的rootは誤ってpruneされない。

## 共通の測定・正しさ契約

[#11](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/11)のRun schema、[#22](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/22)のToolchainFingerprint、Lane Cのexact artifact test contractを再利用し、専用benchmark storeを作らない。

| 観測 | Cargo / rustc / LLVM相当の因果を分ける方法 |
| --- | --- |
| elapsed, user/system CPU, peak RSS, read/write I/O | resolver、semantic query、transform/codegen、link-plan/actionを別eventとして同一clockへ記録する |
| disk footprint | source/metadata cache、semantic/IR checkpoint、object/final artifactを別identity・別quotaで記録する |
| avoided work | candidate、unit、query、IR body、transform、codegen、symbol/sectionの各数と理由を記録する |
| reuse/invalidation | red/green/recomputed/loaded/droppedと、feature/cfg/target/ABI/IR revisionによる理由を記録する |
| correctness | exact production subjectを直接実行し、artifact/ABI/symbol/runtime observationを比較する。byte equalityは必要な場合だけ要求する |
| no fallback | process/action provenanceにCargo/rustc/LLVM backendがRust target unitのproducerとして現れない否定検査を置く |

## Issueと参照

この研究は[#50](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/50)で追跡する。#50はCargo/rustc/LLVMの全置換を一括で完了するissueではなく、R1のclosed-loop proofを最初のclose conditionとする。M1のmixed Cargo/Nimble/C/C++ closure、Lane Bの効率研究、Lane Cのartifact qualificationを置き換えず、Rust専用の反証と設計判断をそこへ戻す。

## 参考資料

1. Cargo Book, [Dependency Resolution](https://doc.rust-lang.org/cargo/reference/resolver.html), [Build Scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html), [unit graph](https://doc.rust-lang.org/cargo/reference/unstable.html#unit-graph).
2. Rust Reference, [Procedural Macros](https://doc.rust-lang.org/reference/procedural-macros.html).
3. Rust Compiler Development Guide, [Compiler Overview](https://rustc-dev-guide.rust-lang.org/overview.html), [Queries](https://rustc-dev-guide.rust-lang.org/query.html), [Incremental Compilation](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html).
4. LLVM, [New Pass Manager](https://llvm.org/docs/NewPassManager.html), [Link Time Optimization](https://llvm.org/docs/LinkTimeOptimization.html), [ThinLTO](https://clang.llvm.org/docs/ThinLTO.html).
5. Repository research: [compiler ownership contract](../01-foundations/compiler-ownership-contract_ja.md), [Lane B](../02-research-areas/execution/lane-b-efficient-compiler-computation-foundations_ja.md), [LLVM rediscovery](../02-research-areas/compiler/llvm-rediscovery-research_ja.md), [measurement foundation](../02-research-areas/measurement/measurement-foundation.md).

# Rust build path（Cargo/rustc/LLVM置換）の省資源研究ロードマップ

## 目的・現在地・次の判断

**目的**は、LAMINARIA自身がCargo、rustc、LLVMのRust buildにおける責務を段階的に置換し、正しいnative artifactを作るためのwork、CPU時間、peak RSS、disk、I/O、そして測定可能な場合のenergyを最小化することである。外部Cargo/rustc/LLVMは、意味・資源・生成artifactを比較するreference/bootstrapping oracleであって、完成経路のprofile候補ではない。

**現在地**では、Cargo/Nimble/C/C++のclosureを同じtyped graphで解き、LAMINARIAのsource/IR/planner/target pathからnative executableへ至るM1が主gateである。Rustに限る本trackはその縮小版ではない。Cargoのwork discovery、rustcのsemantic incremental computation、LLVMのoptimization/target computationを、Rust-only workloadで最初に再発見し、省資源設計を後からのtuningではなく各責務のacceptance conditionにする。

**次の判断**は、Cargo互換inputを受け取るLAMINARIAのRust pathが、どの最小semantic contractまでを外部processなしで所有すれば、同じrequested artifactをより少ない計算で成立させられるか、である。まずexternal baselineを測ってcostの所在を明らかにし、その後はCargo/rustc/LLVMの候補を比較するのではなく、LAMINARIAの候補設計を実装・反証する。

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

- **優先度:** P0/P1。Rust-onlyのowned pathをM1 mixed closureと同じ原則で前進させる。外部toolの運用改善は成果物ではない。
- **最小仮説:** Rust buildのpackage選択、semantic computation、target computationを一つのdemand-driven graphとして所有すれば、同じ正しいnative artifactに必要なworkを早期に除去・再利用・限定再計算でき、external Cargo/rustc/LLVM baselineより少ない資源で成立するsliceを一つ示せる。
- **最小実験:** 一つのlock済みRust-only workloadで、positive artifactとcompile前rejectを各一つ用意する。cold、true no-op、leaf semantic edit、root/feature/target changeを同じEnvironmentFingerprintで測る。
- **停止条件:** 各段で、採用するgraph/IR/invalidation/retention設計、または不成立理由を、direct executable testと資源証拠で決める。外部compilerを呼ばなければ成立しない要求は対応範囲外として構造化rejectする。
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

## Cargo置換：必要workを構成する前に減らす

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

## rustc置換：semantic computationを需要駆動・増分にする

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

## LLVM置換：analysis、変換、target生成をbodyの前に制御する

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

## 段階的ロードマップ

| 段階 | owned slice | 資源判断 | 完了の証拠 |
| --- | --- | --- | --- |
| R0 — contract and oracle | Rust workload、requested artifact、対応するmanifest/lock input、semantic/artifact/reject contractを固定する | 測定対象とcache pre-state、external baselineとの差分、energy計測可否を固定する | exact fingerprints、direct test、observer overhead、external reference trace |
| R1 — Cargo responsibility | LAMINARIA resolverがpackage/feature/target/unit graphを構成し、一つのconflictをcompile前にrejectする | candidate/unit展開をいつ止めればmetadata/source/build workを避けられるか | Cargo processなしのowned graph、選択/棄却理由、避けたworkの測定 |
| R2 — rustc responsibility | Rust syntaxからsemantic factsと独自IRを導き、demand-driven query/invalidationを実装する | no-opとleaf editでどのsemantic workをgreen/reuse/dropできるか | rustc processなしのpositive artifactまたは明示reject、query/action資源証拠 |
| R3 — LLVM responsibility | 独自IRのanalysis/transform/summary/target loweringでnative objectとlink planを作る | summary、checkpoint、retention、bounded parallelismのどれがCPU/RSS/disk/I/Oを下げるか | LLVM backendなしのnative object/closure、direct executable test、採否判断 |
| R4 — end-to-end evidence | R1–R3を同一Rust workloadで結合し、cold/no-op/edit/root changeを反復する | external Cargo/rustc/LLVM baselineとのresource frontier、および残るcost center | no-fallback trace、raw samples、correctness-equivalent behavior、次の設計決定 |

R1–R3は直列の完成品フェーズではない。R2/R3が必要とする最小contractをR1へfeedbackし、R4で同じtyped graph上の因果を検証する。外部baselineはR0/R4だけに置き、PFE、Cranelift、sccache、mold、Bazel等をLAMINARIAの候補実装として列挙しない。

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

この研究は[#50](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/50)で追跡する。#50はCargo/rustc/LLVMの全置換を一括で完了するissueではなく、上記R0–R4から次の一つのowned sliceと資源判断を選ぶ台帳である。M1のmixed Cargo/Nimble/C/C++ closure、Lane Bの効率研究、Lane Cのartifact qualificationを置き換えず、Rust専用の反証と設計判断をそこへ戻す。

## 参考資料

1. Cargo Book, [Dependency Resolution](https://doc.rust-lang.org/cargo/reference/resolver.html), [Build Scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html), [unit graph](https://doc.rust-lang.org/cargo/reference/unstable.html#unit-graph).
2. Rust Reference, [Procedural Macros](https://doc.rust-lang.org/reference/procedural-macros.html).
3. Rust Compiler Development Guide, [Compiler Overview](https://rustc-dev-guide.rust-lang.org/overview.html), [Queries](https://rustc-dev-guide.rust-lang.org/query.html), [Incremental Compilation](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html).
4. LLVM, [New Pass Manager](https://llvm.org/docs/NewPassManager.html), [Link Time Optimization](https://llvm.org/docs/LinkTimeOptimization.html), [ThinLTO](https://clang.llvm.org/docs/ThinLTO.html).
5. Repository research: [compiler ownership contract](../01-foundations/compiler-ownership-contract_ja.md), [Lane B](../02-research-areas/execution/lane-b-efficient-compiler-computation-foundations_ja.md), [LLVM rediscovery](../02-research-areas/compiler/llvm-rediscovery-research_ja.md), [measurement foundation](../02-research-areas/measurement/measurement-foundation.md).

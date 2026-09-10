# Backend Pipeline White-boxing 研究方針

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

本書のLLVM、Nim、rustc、native-link、Wasm経路は比較・観測実験である。図・adapter API・checkpoint条件をLAMINARIA独自コンパイラの規定にしない。#25/#3が独自実装するsource/IR経路を#5/#13で実証し、将来variantとして許容するだけにしない。外部backendによる成果は独自コンパイルの認定にならない。

## 本経路で必要な独自pipelineの実証

#25/#3のsource-derived IRを使い、LAMINARIA自身の解析・変換・target生成を#6/#8へ接続する。論理stage・観測・checkpoint・実行境界を区別し、少なくとも一つの独自計算partitionとcheckpoint採否を意味・資源の証拠で検証する。以下のLLVM/Wasm実験の完了だけではこの条件を満たさない。

## 以下の既存backend実験の適用範囲

以下のprovider、LLVM/ThinLTO/Wasm pipeline、外部fallbackと完了条件は比較・観測用である。独自compiler内のgroupingやcheckpoint経済性に知見を戻すが、既存backendを本経路へ自動的に採用しない。

## 目的

LAMINARIAは、RustとNimのfrontendやcodegen unitを細粒度化する一方で、選択されたbackend、linker、WASM toolchainを再びopaqueな単一Actionとして扱ってはならない。

本研究では、LLVM、ThinLTO/DTLTO、LLD、WebAssembly code generation、`wasm-ld`、Binaryen、WIT/Component Model等の内部処理を、可能な限り観測可能・説明可能なnested computation graphとしてLAMINARIAへ投影し、そのうち性能上意味のある境界だけをcache/checkpoint/schedulingの実行境界として採用する。

中心原則は次の通りである。

> White-boxing does not mean one process per compiler pass.

backend内部を見えるようにすることと、全passを独立processや独立CAS artifactへ分割することは別問題である。LAMINARIAは内部構造をlogical graphとして理解しつつ、実行分割によるserialization、reload、analysis再計算、cache locality低下、optimization quality低下を測定し、利益がコストを上回る境界だけをmaterializeする。

## 1. 現行Backend Graphの不足

現行のBackend Graphは主として次を扱う。

```text
Language representation
  → backend selection
  → backend lowering
  → backend optimization
  → machine artifact
```

これはLLVM、Cranelift、GCC等を固定前提から外し、variantとして選択するには十分だが、backend選択後の計算を一つの大きな箱として扱う危険がある。

LAMINARIAの研究ポリシーは、outer build commandをopaqueな実行単位として扱わず、不要なworkの除去、artifact reuse、invalidation縮小、parallelism露出、global schedulingをcompiler-stage単位へ適用することである。同じ原則はbackend、linker、post-link optimizer、componentizationにも適用されなければならない。

したがってBackend Graphを次の二段階へ分離する。

```text
Backend Route Selection
  ↓
Backend Pipeline Expansion
```

Backend Route Selectionは、backend family、target、optimization、LTO、linker、artifact kind等のconstraintを解く。

Backend Pipeline Expansionは、選択されたrouteが内部に持つlowering、optimization、LTO、target codegen、link、post-link等をnested graphとして展開する。

## 2. Backend Pipeline Provider

backendは単なるenumではなく、LAMINARIAへpipeline graphを提供するcomponentとして扱う。

概念的なcontractは次の形を取る。

```text
expand_backend(
  input_artifacts,
  backend_route,
  target,
  optimization,
  lto_mode,
  debug_profile,
  toolchain_identity,
  constraints
) -> BackendPipelineGraph
```

`BackendPipelineGraph`は少なくとも以下を含む。

- logical stage;
- artifact producer/consumer edge;
- observation point;
- candidate checkpoint;
- executable action;
- dynamic graph-expansion point;
- invalidation dependency;
- resource profile;
- toolchain/backend-specific metadata。

backend固有の詳細を共通IRへ無理に平坦化しない。LLVM pass manager、Binaryen pass runner、Cranelift pipeline等の固有構造はtyped metadataまたはnested graphとして保持する。

## 3. 境界を4種類に分ける

### 3.1 Logical Stage

compiler/backend内部で意味のある計算段階。LAMINARIAの説明・可視化・分析には現れるが、独立processとは限らない。

例:

- LLVM inlining;
- loop optimization;
- vectorization;
- dead-code elimination;
- Binaryen function optimization;
- Wasm post-link cleanup。

### 3.2 Observation Boundary

時間、CPU、memory、IR/module size、optimization remark、変換前後のdigest等を取得できる境界。

観測可能であることは、cache artifactとして保存することを意味しない。

### 3.3 Checkpoint / Artifact Boundary

再利用・invalidation縮小・cross-machine transfer等の利益が見込めるmaterialized artifact境界。

候補:

- LLVM IR / bitcode;
- pre-link bitcode;
- ThinLTO module summary;
- ThinLTO per-module index;
- ThinLTO backend output;
- native object;
- relocatable Wasm object;
- linked Core Wasm module;
- optimized Core Wasm module;
- WIT/component metadata;
- final WebAssembly Component。

### 3.4 Execution Boundary

LAMINARIA schedulerが独立してready/running/blocked/completed状態を管理するAction境界。

Logical StageやObservation Boundaryを自動的にExecution Boundaryへ昇格させてはならない。

## 4. Checkpoint Economics

backend内部を細分化する価値は測定によって決定する。

概念的には次を比較する。

```text
checkpoint benefit =
  eliminated work
+ reusable work
+ reduced invalidation
+ scheduling gain
+ remote/distributed execution gain

checkpoint cost =
  serialization
+ deserialization/reload
+ hashing
+ process/IPC overhead
+ lost analysis state
+ lost cache locality
+ increased memory traffic
+ optimization-quality risk
```

候補境界ごとにbenefitとcostを実測し、単に「細かいほど良い」とはしない。

LLVM New Pass ManagerではModule / CGSCC / Function / Loopの階層があり、同じIR unit上のpassをまとめて実行することがcache localityやoptimization qualityに影響する。そのためpass単位の外部process化を既定設計にしない。

## 5. LLVM Pipeline Graph

LLVM routeでは少なくとも次の層を区別する。

```text
Language / Codegen Unit
  ↓
LLVM IR generation
  ↓
IR preparation / canonicalization
  ↓
Middle-end optimization pipeline
  ├─ Module
  ├─ CGSCC
  ├─ Function
  └─ Loop pass groups
  ↓
Pre-link optimization
  ↓
LTO strategy
  ├─ none
  ├─ ThinLTO
  └─ Full LTO
  ↓
Target-dependent code generation
  ↓
Object / relocatable target artifact
  ↓
Link
```

LAMINARIAはpass pipelineの存在、順序、所要時間、optimization remarks、分析再利用/invalidation等を観測可能にする。一方、各passを必ず独立Actionへすることは要求しない。

### 5.1 LLVMの観測面

研究では少なくとも以下を利用・比較する。

- New Pass ManagerのModule / CGSCC / Function / Loop構造;
- `PassBuilder`で構築されるdefault pipelineとcustom pipeline;
- pass execution timing;
- optimization remarks (`Passed` / `Missed` / `Analysis`);
- IR/bitcode size and digest before/after selected groups;
- rustc codegen-unit、LTO、linker-plugin-LTOとの接続;
- pass/plugin/configurationを含むpipeline fingerprint。

### 5.2 Pipeline Identity

LLVM checkpointのidentityは入力bitcodeだけでは不十分である。少なくとも次を検討する。

- LLVM version/build identity;
- target triple / data layout / target features;
- optimization level;
- pass pipeline fingerprint;
- codegen options;
- LTO mode;
- PGO/profile input;
- debug configuration;
- external/plugin passes;
- relevant environment/toolchain inputs。

## 6. ThinLTO / DTLTOをAction Graphとして扱う

ThinLTOはbackend white-boxingの主要reference implementationとする。

概念構造:

```text
bitcode modules
  ↓
thin-link / combined summary analysis
  ↓
per-module summary index
  ↓
independent ThinLTO backend jobs
  ↓
native object outputs
  ↓
final link
```

DTLTOではLLDが各backend compilationについてinput、output、index、compiler commandをJSON job descriptionとして外部distributorへ渡せる。この構造はLAMINARIAのAction Graphへ直接写像できる可能性が高い。

LAMINARIAでは次の3経路を比較する。

1. opaque in-process ThinLTO;
2. explicit thin-link/index-only + per-module backend jobs;
3. DTLTO distributor経由でLAMINARIA schedulerへbackend jobsを取り込む経路。

研究上の重点:

- link時に発見されるbackend jobをdynamic subgraphとして安全にmaterializeできるか;
- per-module indexをartifact dependencyとして扱えるか;
- LAMINARIAのglobal CPU/memory/I/O budgetでbackend jobsをscheduleできるか;
- LLVM native ThinLTO cacheとLAMINARIA CAS/action cacheの責務をどう分離・統合するか;
- controlled editで実際に必要なbackend jobだけを再実行できるか;
- critical pathとqueue/resource waitを説明できるか。

## 7. Rust / NimからLLVMへの収束

LLVM pipeline white-boxingはRustのみを対象にしない。

候補経路:

```text
Rust
  → rustc_codegen_ssa / LLVM bitcode

Nim 2
  → nlvm → LLVM IR
  or
  → generated C → Clang → LLVM IR/bitcode

Nim 3 / Nimony
  → Leng / lengc → LLVM IR
```

これらが同じLLVM IR/bitcode/LTO経路へ参加できる範囲を測定する。

これは任意のRust/Nim型やruntime semanticsが互換であるという仮定ではない。target triple、data layout、symbol visibility、calling convention、runtime initialization、allocator、panic/exception、TLS、ownership等の互換性を別途検証する。

成功すれば、RustとNimのcross-language optimization、ThinLTO backend scheduling、cache identityを同じBackend Pipeline Graph上で評価できる。

## 8. WebAssemblyはbackendではなくTarget Pipeline

WebAssemblyはLLVM等と同列のbackend familyとして扱わない。

LAMINARIAでは少なくとも次のdimensionを分離する。

```text
Backend Engine
× Target ISA / Object Model
× Link Model
× Post-link Optimizer
× Composition Model
```

例:

```text
backend = LLVM
target = wasm32
linker = wasm-ld
post_link = Binaryen/wasm-opt
composition = CoreModule | ComponentModel
```

## 9. WebAssembly Target Pipeline

LLVM系routeでは概念的に次を扱う。

```text
Language IR
  ↓
LLVM IR
  ↓
LLVM optimization
  ↓
WebAssembly target codegen
  ↓
relocatable Wasm object
  ↓
wasm-ld
  ↓
Core WebAssembly Module
  ↓
Binaryen / wasm-opt
  ↓
Optimized Core Module
  ↓
WIT metadata / adapter processing
  ↓
componentization
  ↓
WebAssembly Component
```

`wasm-ld`、Binaryen、componentizationを一つの`WASM backend` actionへ畳み込まない。

### 9.1 wasm-ld

relocatable WebAssembly objectとfinal core moduleの間の独立stageとして扱う。symbol resolution、link options、link input identity、GC/LTO関連の挙動を観測対象とする。

### 9.2 Binaryen

`wasm-opt`には独自のpass pipelineがある。LLVMと同様に、passの存在・所要時間・module metrics・変化量は観測可能にしつつ、すべてを独立processへ分割することは前提にしない。

候補checkpointは、linked core moduleとpost-link optimized core moduleをまず比較する。

### 9.3 WIT / Component Model

WIT metadata embedding、adapter selection、core moduleからcomponentへの変換を独立artifact/action候補として扱う。

`wasm-tools component embed`と`component new`が別操作であることを利用し、core module生成とcomponentizationのinvalidation境界を測定する。

例えばWITのみの変更でcore module本体の再生成が不要なケースが成立するかを検証する。ただしgenerated bindings、exports/imports、Canonical ABI要求が変わる場合はcompiler側までinvalidateされ得るため、常にpost-linkだけで済むとは仮定しない。

## 10. Dynamic Graph Expansion

DTLTOのように、上流Actionの実行後に初めて具体的なbackend job集合が得られるpipelineが存在する。

LAMINARIAはこれをhidden nested schedulerに任せるのではなく、明示的なgraph expansionとして表現することを研究する。

候補モデル:

```text
GraphExpansionAction
  inputs: planning/link artifacts
  output: ExpansionManifest
  expands-to: child actions + artifact edges
```

必要条件:

- expansion resultにcontent identityがある;
-同一入力からdeterministicなchild graphが得られるか、非決定要因を明示する;
- child actionsが通常のresource budget、cache、critical-path accountingへ参加する;
- fallbackとしてopaque executionを選んだ場合はtraceで明示する;
- graph expansion overhead自体を測定する。

## 11. Work Eliminationをbackend内部まで適用する

LAMINARIAの最適化順序はbackend内部でも変えない。

1. 不要なbackend stageを除去する;
2. 有効なcheckpoint artifactを再利用する;
3. invalidation範囲を狭める;
4. parallelismを露出する;
5. global schedulingする;
6. 残ったindividual stageを高速化する。

評価例:

- semantic/codegen artifact不変時にLLVM IR生成を省略できるか;
- unchanged bitcodeに対してThinLTO backendを再実行しないか;
-一部module変更でunaffected ThinLTO backend outputを再利用できるか;
- linked Core Wasm不変時に不要な`wasm-ld`を省略できるか;
- post-link inputs不変時に`wasm-opt`を省略できるか;
- WIT/component metadataだけの変更時にcompile/linkを避けられるか。

## 12. 必須メトリクス

backend white-boxing研究では少なくとも以下を記録する。

- logical stage count;
- observation boundary count;
- materialized checkpoint count;
- executed/skipped backend stage count;
- pass group / backend job wall and CPU time;
- queue/dependency/resource wait;
- peak/time-weighted memory;
- bytes serialized/deserialized/hashed/read/written;
- IR/bitcode/object/module/component size;
- optimization remarks and pass-level change evidence where available;
- analysis/cache reuse where observable;
- ThinLTO backend job count and invalidation set;
- linker input/output and symbol inventory;
- Binaryen pass timing/module metrics where available;
- componentization/adaptation time and artifacts;
- graph-expansion overhead;
- reference baseline ratio;
- checkpoint benefit versus checkpoint cost。

## 13. 完了条件

この研究は、backend内部を単に可視化した時点では完了しない。

最低限、次を実証する。

1. backend route selectionとpipeline expansionが別概念として表現される;
2.少なくともLLVM pipelineがlogical/observable/checkpoint/execution境界へ分類される;
3. pass-per-process化を避けた上でpass/pipeline内部の実行証拠を取得できる;
4. ThinLTO/DTLTOのbackend jobsをLAMINARIA graphへ写像する実験が再現可能である;
5. WASMが`backend`ではなくlink/post-link/componentizationを含むTarget Pipelineとして記録される;
6.少なくとも一つのbackend checkpointでwork eliminationまたはartifact reuseの利益を実測する;
7. checkpointを増やしたことで逆に遅くなるケースも測定し、境界選択へ反映する;
8. opaque fallbackが使用された場合に隠さず説明できる。

## 14. 主要参考資料

- LLVM New Pass Manager: https://llvm.org/docs/NewPassManager.html
- LLVM Optimization Remarks: https://llvm.org/docs/Remarks.html
- Clang ThinLTO: https://clang.llvm.org/docs/ThinLTO.html
- LLVM DTLTO: https://llvm.org/docs/DTLTO.html
- rustc codegen options: https://doc.rust-lang.org/rustc/codegen-options/
- LLD WebAssembly port: https://lld.llvm.org/WebAssembly.html
- Binaryen / wasm-opt: https://github.com/WebAssembly/binaryen
- wasm-tools: https://github.com/bytecodealliance/wasm-tools
- nlvm: https://github.com/arnetheduck/nlvm
- Nimony: https://github.com/nim-lang/nimony

これらは単なる実装候補ではなく、pipeline境界、外部化可能性、cache/scheduling、WASM compositionを検証するreference implementationとして扱う。

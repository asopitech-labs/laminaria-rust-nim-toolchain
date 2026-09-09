# LAMINARIA — 水平分散・分散compiler研究

## 位置付け

LAMINARIAでは、水平分散を後から付け足すdeployment featureではなく、compiler graphそのものの研究対象として扱う。

中心となる問いは、単にcompiler commandを別machineへ送れるかではない。

> RustとNimのsemantic factsから出発し、correctness、optimization legality、incrementality、resource accounting、explainabilityを保ったまま、どの計算をmachine間へ分割できるか。

本書は #6、#7、#13–#17、#25を横断する研究charterである。Kbuild、LLVM DTLTO、Bazel、distcc、icecreamをLAMINARIAのarchitectureとして採用するものではない。

## 1. 分散の三種類

### 1.1 Source / build graphの分散

独立したsource fileまたはgenerated translation unitを異なるworkerでcompileする。

```text
source / header / config graph
        ↓
translation unit
        ↓
object file × N
        ↓
archive / final link
```

これは通常のLinux Kbuildやdistcc/icecreamの型である。translation unitが十分独立していればよくスケールするが、compiler invocationの境界を越えた豊富なcross-language semantic modelは保持しない。

### 1.2 Global analysis後のbackend分散

globalまたはcross-module analysisでsummary/indexを作り、その結果を使って独立backend jobを実行する。

```text
lowered module
        ↓
global summary / thin-link
        ↓
partition index
        ↓
backend job × N
        ↓
native object / final link
```

LLVM ThinLTO/DTLTOが代表例である。workerの並列性だけでなく、global barrierとpartition contractが重要になる。

### 1.3 Action単位のremote execution

build systemがAction Graphを構築し、宣言されたActionをremote execution serviceへ送る。

```text
declared Action Graph
        ↓
CAS / Action Cache
        ↓
remote worker × N
        ↓
Action result
```

Bazel Remote Executionが代表例である。このモデルではcompilerは通常Actionから呼び出されるtoolであり、分散semantic modelの所有者ではない。

## 2. 既存例の比較

| System | 分散単位 | Global情報 | 主な境界 | まだ答えないこと |
| --- | --- | --- | --- | --- |
| Linux Kbuild | source/object compile | `.config`、generated header、dependency file | object、archive、final link | language横断semantic partition |
| distcc | compiler invocation | command line / environment contract | preprocessed sourceまたはcompiler input | optimization legality / semantic provenance |
| icecream | 中央scheduler付きcompiler invocation | compiler environmentとworker load | compiler invocation | Rust/Nim共通の計算表現 |
| LLVM ThinLTO | moduleごとのbackend compile | combined summary / module index | thin-link、backend object、final link | LLVMのpartitionがLAMINARIAにも適切か |
| LLVM DTLTO | JSONで記述されたThinLTO backend job | LLD生成job manifest | external distributor / final link | bitcode以前に失われたsemantic情報 |
| Bazel Remote Execution | declared build/test Action | input、command、environment、platform | Action / CAS / result | compiler内部境界と言語semantic |

compiler-levelの水平分散に最も近いのはLLVM DTLTOである。heterogeneousなbuild graph全体の分散に最も近いのはBazel Remote Executionである。どちらもLAMINARIAのsubstrate問題の代替ではない。

## 3. Linux kernelを分散のcase studyとして見る

Linuxは通常、一つのglobal optimized program representationとしてbuildしない。Kbuildは`.config`を読み、configurationで選ばれたdirectoryへ降り、`obj-y`/`obj-m`をcompileし、directory単位の`built-in.a` archiveへまとめ、最終的に`vmlinux`とmoduleをlinkする。

```text
.config + generated header
        ↓
Kbuild directory / object selection
        ↓
C / assembly / Rust translation unit
        ↓
object file（parallel）
        ↓
directory built-in.a
        ↓
vmlinux / module
        ↓
architecture-specific post-processing
```

`make -jN`が独立Action間のparallelismを露出する。kernelのjobserverはhelper programへ利用可能なparallelism budgetを渡し、nested invocationが親の上限を超えないようにする。

ここから二つの研究上の観察が得られる。

1. object compileは有効なcoarse execution boundaryである。
2. archiveの所属とlink orderはsemanticまたはoperational constraintであり、workerの完了順だけではfinal artifactを定義できない。

Kbuildは`make LLVM=1`でClang/LLVM utilitiesを選択できる。また現在のLinux treeには実験的な`LTO_CLANG_THIN_DIST`があり、ThinLTO index生成とbackend compilationを明示化し、ThinLTO backend後のnative objectを生成する。これは通常のKbuild分散と、明示的なdistributed ThinLTOを比較する材料になる。

したがってLinuxは、一つの万能distributed compilerではなく、次のlayered designを示している。

```text
Kbuild source DAG
    + optional compiler invocation distribution
    + optional ThinLTO global-summary / backend distribution
    + final architecture-specific link / post-processing
```

## 4. LLVM ThinLTO/DTLTOをprior artとして扱う

LLVM ThinLTOはglobal thin-linkとparallel backend compilationを分ける。DTLTOはこれをlink stepへ統合し、LLDにbackend jobのJSON descriptionを作らせる。各jobにはmodule bitcode input、individual index、output pathがあり、common compiler arguments/inputは分離して表現される。

重要なのはJSON形式ではなく、次の明示的contractである。

```text
global analysis
  → explicit partition / index
  → independently executable backend job
  → native object
  → final linker integration
```

DTLTOはdistribution systemの詳細をLLVMの内部へ持ち込まない。またmatching compiler/linker versionと、version-specific schemaを理解するdistributorを要求する。remote jobはsource fileだけではidentityできないという直接的な注意である。

LAMINARIAではDTLTOをbaselineとして測定し、canonical answerとはしない。次を問う必要がある。

- 何のglobal factがsummaryを必要にしたのか。
- summaryのどのfactがcorrectness用で、どれがprofitability用か。
- moduleがRust/Nimにとって適切なpartition単位か。
- language-specific semantic factsをdistribution boundaryの向こうへ渡すべきか。
- semantic、toolchain、target、profile変更時に何がinvalidationされるか。
- final linkがnested schedulerを隠さずresultを消費できるか。
- incrementalityまたはresource localityのために別partitionが有利か。

## 5. Candidate LAMINARIA distributed model

LAMINARIAはmerged LLVM IRをcanonical distributed inputとして開始してはならない。

```text
Rust semantic facts ─┐
                     ├→ preserved facts + provenance
Nim semantic facts  ─┘
                              ↓
                    global requirements / summaries
                              ↓
                    candidate partition manifest
                              ↓
                 remote semantic/backend action × N
                              ↓
                    backend-specific projection
                              ↓
                     object / archive / final link
```

candidate partition manifestには少なくとも次を持たせる。

- semantic workloadとdemandされたartifact
- partition identityとprovenance
- partition間で必要なfact
- legality assumptionと未解決obligation
- language/compiler/backend/toolchainのexact identity
- target、data layout、ABI、feature contract
- declared input/output artifact
- resource requirementとcost予測
- cache/invalidation identity
- scheduler placement constraint
- なぜこのpartitionがvalidなのかの説明

表現が一つのIR、複数IR、typed fact set、graph relation、analysis database、hybridのいずれになるかは、workload evidenceから決める。

## 6. Persistenceとmaterializationもpartition判断に含める

永続化を「すべてのintermediateをlocal diskへ書くこと」とモデル化しない。logical artifactとphysical replicaは別のobjectである。候補storage tierは次の通り。

- memoryまたはprocess-local state
- local SSD/NVMe
- peerまたはworker-local cache
- remote CAS/object storage
- durable archive storage

schedulerはintermediateを保持、materialize、replicate、transfer、recomputeのどれにするかを選ぶ。判断要因はreuse probability、recovery value、data locality、capacity、serialization/hash cost、transfer bandwidth/latency、storage bandwidth/latency、consistency/commit cost、failure riskである。大きなtransferではnetwork pathが古いlocal HDDより速い場合があるが、小さなI/Oのlatency、availability、coordinationによって逆転することもある。したがってlocalityはprefer ruleではなく、測定対象である。

persistence recordでは次を分離する。

- logical artifact identity、semantic/provenance identity、compatibility
- physical replicaのlocationとworker/storage capability
- replica lineage、complete/commit state、retention/GC、recovery status
- serialize、hash、write、read、upload、download、recomputeしたbytes
- persistence、transfer、recomputeを選んだ理由の説明

## 7. Heterogeneous nodeの参加とcross-compilation

execution hostとcompilation targetを分離する。

```text
execution host: OS × ISA × ABI × resources × toolchain environment
compilation target: OS × ISA × ABI × object format × sysroot/SDK × features
```

Windows、macOS、Raspberry Pi nodeは、native builder、cross-compiler、test runner、performance measurement node、evidence-only observerのいずれにもなりうる。nodeのOSやCPU名だけからroleを推測してはいけない。

node capability fingerprintには少なくとも次を持たせる。

- host OS、kernel/runtime、ISA
- physical/logical core、core class、memory、I/O/network topology
- endianness、pointer width、ABI、libc/runtime、object format
- compiler、linker、sysroot/SDK、exact revision
- supported target tripleとtarget feature
- virtualization/container/emulation capability
- trust、qualification、availability、measurement status

各actionには`execute-on`と`produces-for`の両方のconstraintを持たせる。input、toolchain/sysroot、target contract、outputが独立していれば、cross-compilation actionは異種node上で同時実行できる。一方、native execution、target-specific linking、performance measurement、runtime testは対応target nodeを要求することがある。cross-compiled artifactが生成成功しただけで、host-executable test artifactとして消費してはいけない。

混在nodeの結果を次の3つに分ける。

1. portable target artifact: exact contractの下で、qualified hostなら生成できる。
2. target-bound artifact: productionはportableだが、link/runtime/testにはtarget-compatible nodeが必要。
3. host/target-coupled action: producer自身が特定hostまたはtoolchain environmentを要求する。

architecture、ABI、sysroot、linker、target feature、artifact format、runtime obligation、reproducibility constraintが明示されている場合だけmixed-node schedulingをvalidとする。速いnodeでも、要求target contractを生成または検証できないならeligibleではない。

## 8. 研究上の問い

1. 独立したRust/Nim backend workを可能にする最小global fact setは何か。
2. semantic informationをbackend-specific IRへlowerする前にpartitionを定義できるか。
3. workerへ複製すべきsemantic factとsummaryで十分なfactは何か。
4. module partitionはtotal timeを最小化するのか、それともload balanceとinvalidationを悪化させるのか。
5. communication costがremote executionの利益を上回る条件は何か。
6. source edit、worktree、machine、compiler versionをまたいでremote resultを再利用できるか。
7. worker不足・異種worker・worker failureをcorrectnessと再現性を壊さず扱えるか。
8. nested compiler/backend schedulerがglobal schedulerと競合しないようにできるか。
9. どの境界がlogical、observation、checkpoint、execution、dynamic expansion boundaryなのか。
10. LLVM/Kbuild/Bazelのどの設計判断がLAMINARIAにも必要で、どれがbackend固有・歴史的選択なのか。
11. remote persistenceがlocal persistenceまたはrecomputationより安く有用になる条件は何か。
12. どのintermediateをvertical integrationの中でephemeralに保ち、どれをhorizontal materializationするべきか。
13. どのhost/target pairが同一action graphへ参加でき、OS、ISA、ABI、sysroot、runtime obligationのどこでboundaryが必要になるか。
14. 複数target向けの独立したcross-compilation actionを同時実行しつつ、target-specific testを正しいnodeへ配置できるか。

## 9. 反証可能な仮説

### H1 — coarse translation-unit分散はbaselineとして有効

十分独立したRust、Nim-generated native、C translation unitでは、object-level分散がfinal artifactを変えずwall timeを短縮する。transfer、environment preparation、load imbalanceがlocal CPU削減を上回れば反証される。

### H2 — global analysisは本物のbarrierである

cross-module optimizationではlocal inputだけで全判断は安全にできない。必要なglobal factを削ると、optimization miss、unsafe transformation、または明示的なconservative fallbackが発生するはずである。

### H3 — backend partitionはsemantic partitionとは限らない

LLVMがlowering後に使うpartitionは、LAMINARIAのcross-language planningやprecise invalidationに必要なfactを保持しない可能性がある。source/semantic partitionが有用な情報を残す、またはbitcode-module partitionよりinvalidationを狭めるなら支持される。

### H4 — remote executionにはsource content以上のidentityが必要

remote resultの再利用にはproducer、compiler/backend version、target/data layout/features、relevant flags、semantic provenance、environment contractが必要である。identity変更後の再利用は拒否するか、同値性を証明しなければならない。

### H5 — 分散を細かくすれば常によいわけではない

LLVM pass、細かなsemantic relation、tiny artifactまで分割すると、locality低下、serialization/scheduler overhead、optimization quality低下が起きる。少なくとも一つのover-fine candidateを測定して却下する。

## 10. Workloadと実験

### Experiment 0 — Kbuild-shaped object DAG

configurationで選択される多数translation unitのworkloadで、object compile parallelism、archive/link barrier、load balance、generated-header/config invalidation、local/remote transfer costを測定する。

### Experiment 1 — LLVM ThinLTO/DTLTO baseline

thin-link、index、backend job、final linkを観測する。exact job manifest、compiler/linker identity、cache、job duration、input/output size、final artifactを保存する。

### Experiment 2 — Rust/Nim semantic partition candidate

merged LLVM IRではなくsource contractからpaired Rust/Nim workloadを定義する。preserved-fact/provenance representation、partition manifestを試作し、少なくとも一つのpartitionをLLVMへ投影する。他backendへの余地を残す。

### Experiment 3 — Partition comparison

同一workloadでsource/object、LLVM module、ThinLTO backend、candidate semantic partitionを比較する。成功buildでは不十分で、retained information、legality evidence、invalidation scopeも比較する。

### Experiment 4 — Incremental / no-op distribution

local semantic edit、Rust/Nim boundary変更、generated-header/config変更、target/feature変更、compiler/backend変更、unchanged rebuildを繰り返す。回避・再利用・invalidationされたremote jobと、その判定に必要なmetadata/transfer workを測る。

### Experiment 5 — Failure / heterogeneity

worker削除、worker速度差、compiler identity変更、missing input、backend job interruptionを試す。fail closedまたは明示的fallbackにする。黙ったlocal rebuildはdistributed execution成功とみなさない。

## 11. 必須証拠

- exact source revisionとsemantic workload contract
- Rust/Nim/frontend/backend/compiler/linker/toolchain identity
- machine/worker EnvironmentFingerprint
- graph/partition manifest
- declared input/outputとcontent digest
- global-summary/index provenance
- scheduler placement、queue wait、execution time、resource use
- transfer、serialization、hashing、CAS cost
- cache hit/missとinvalidation reason
- expected/actual execution set
- final artifact correctnessとruntime check
- semantic informationのretained/transformed/lost記録
- negative result、fallback、未解決obligation

## 12. 完了条件

Linux kernelまたはLLVM ThinLTOのbuildが複数machineで動くだけでは完了としない。

1. Kbuild-shapedまたは同等のobject-DAG distribution baseline
2. ThinLTO/DTLTO型のglobal-summary→backend-job実験
3. merged LLVM IRではなくpreserved Rust/Nim semantic factsから始まるcandidate partition
4. 少なくとも二つのpartition粒度のcommunication/locality cost比較
5. avoided/executed job setを示すcontrolled incremental実験
6. unsafeなcross-toolchain reuseを防ぐidentity/failure policy
7. LAMINARIA evidenceから独立に支持されたLLVM/Kbuild設計判断を少なくとも一つ
8. evidenceに基づきreject、再構成、またはunresolvedとした分散境界を少なくとも一つ
9. hidden nested compiler schedulerではなくglobal schedulerがworker resource useを計上した証拠
10. reproducible command、fixture、committed machine-readable evidence

## 13. 既存研究trackとの関係

- **#6 Scheduler:** global resource-aware placementとexecution accounting
- **#7 CAS/invalidation:** remote input/outputのidentityとreuse policy
- **#13 Backend graph:** logical/observation/checkpoint/execution boundaryのeconomics
- **#14 LLVM white-boxing:** LLVM pass、analysis、backend evidenceを供給するがLAMINARIA partitionは決めない
- **#15 ThinLTO/DTLTO:** global-summaryとdynamic backend-jobのbaseline
- **#17 Rust/Nim LLVM convergence:** lowered artifact compatibility evidenceを供給するがfinal common substrateではない
- **#25 LLVM rediscovery:** distributed partitionをLLVM-derived、semantic-fact-derived、hybridのどれにするか決める

## 14. 非目標

- 全compiler passをremote processにすること
- remote speedupをcommon semantic substrateの証明とすること
- successful linkをcross-language optimization compatibilityの証明とすること
- environment、toolchain、target差をgeneric worker labelの下に隠すこと
- LLVM module partitionやBazel Action shapeを必要性の検証なしに採用すること
- measurement spineと互換性のない別evidence storeを作ること

## 参考資料

- [Linux Kernel Makefiles](https://www.kernel.org/doc/html/latest/kbuild/makefiles.html)
- [Building Linux with Clang/LLVM](https://kernel.org/doc/html/next/kbuild/llvm.html)
- [Linux kernel jobserver module](https://www.kernel.org/doc/html/latest/tools/jobserver.html)
- [Linux kernel `arch/Kconfig` — LTO / distributed ThinLTO options](https://github.com/torvalds/linux/blob/master/arch/Kconfig)
- [LLVM DTLTO](https://www.llvm.org/docs/DTLTO.html)
- [Clang ThinLTO](https://clang.llvm.org/docs/ThinLTO.html)
- [Bazel Remote Execution](https://bazel.build/remote/rbe)
- [Bazel Remote Caching](https://bazel.build/remote/caching)
- [distcc](https://github.com/distcc/distcc)
- [icecream](https://github.com/icecc/icecream)

# Rust/C++ test tool landscapeとLAMINARIAへの適用

## 位置づけと結論

本書は[当面の研究ゴール](../../near-term-research-program_ja.md)から派生するLane Cの詳細調査である。調査日は2026-09-13。RustとC++のtest toolを、人気や記法ではなく、**どのartifactを実行し、どの故障クラスを反証し、どの証拠を残せるか**で比較する。

結論は次のとおりである。

1. Rustの`rustc --test`/`cargo test`も、C++のGoogleTest/Catch2/doctestも、原則としてproduction executableとは別の**test executable**を作る。component testには不可欠だが、それだけで出荷binaryを認定できない。
2. runner、assertion framework、property test、fuzzer、sanitizer、model checker、coverage、mutation、ABI inspector、benchmarkは相互代替ではない。一つの「標準test tool」に統合すると、subject identityと検出能力を失う。
3. LAMINARIAはframeworkを再実装せず、既存test executableを列挙・実行するadapterと、exact production artifactをprocessとして直接制御・観測するqualification harnessを分ける。
4. Rust/C++境界ではsource-level unit testだけでなく、実際のcompiler/linker組合せで作ったobject/archive/shared library、export/import symbol、calling convention、layout、unwind、loader closureを検査する。
5. test用YAML、fixture専用validator、validator自身のtestを別々に正本化しない。正本はproduction implementationへ直接作用する実行可能testであり、manifestはそのtest invocationと期待propertyを表す入力に限定する。

## 1. 比較軸

各toolを次の軸で評価する。

| 軸 | 問うこと |
| --- | --- |
| subject | production binary、test binary、library、source、MIR/IR、object/ABIのどれを検査するか |
| oracle | exact value、property、snapshot、differential、metamorphic、UB detector、proofのどれか |
| isolation | process、thread、address space、target environmentを分離できるか |
| discovery | test一覧を機械可読に取得できるか、cross targetで実行が必要か |
| evidence | exit/signal/output、seed、counterexample、trace、coverage、ABI diffを保存できるか |
| perturbation | instrumentation、test cfg、mock、runtime差がsubjectをどれだけ変えるか |
| cost | build time、run time、memory、state explosion、platform制限 |
| M1 role | exact artifact認定、component反証、診断、補助計測のどれを担うか |

「testが多い」ではなく、fault modelに対して独立なoracleがあり、結果がexact subjectへ結び付くことを評価する。

## 2. Rust標準test path

### 2.1 `rustc --test`とlibtest

`rustc --test`はcrateをtest modeでcompileし、`cfg(test)`を有効にしてtest関数を収集し、libtestへlinkした専用executableを作る。既存の`main`はcompile対象にはなってもentry pointではなく、compilerがtest harness用`main`を生成する[1]。したがって次が重要である。

- private itemへ直接到達できるunit testには強い。
- test filter、capture、ignored test等の標準protocolを得られる。
- `cfg(test)`、test-only dependency、生成`main`、link内容がproduction binaryと異なる。
- 「同じsourceをtestした」は言えても「同じartifactを実行した」とは言えない。

### 2.2 `cargo test`

`cargo test`はlibrary、binary、example、integration test、doctest等をbuildし、それぞれのtest executableを実行する。integration testは`tests/`ごとに別crate/executableとなり、通常はlibraryのpublic APIを使う[2][3]。`harness = false`ならlibtestを外せるが、targetがproduction executableと同一になることを自動保証しない。

LAMINARIAがCargo adapterから保存すべきものは、単なる最終exitではなく、package/target/profile/features/target triple、実行したtest executable digest、test filter、raw resultである。`cargo test`の成功をrelease binary digestへ転記してはならない。

### 2.3 rustdoc doctest

rustdocはdocumentation code blockを抽出し、test programへ変換してcompile・実行する[4]。public APIの例が陳腐化していないことには強いが、抽出されたprogramとproduction binaryは別subjectである。documentation contract testとして保存し、artifact qualificationとは分ける。

## 3. Rustのtool landscape

| tool/系統 | 主なsubjectと強み | 限界とidentity上の注意 | LAMINARIAでの位置 |
| --- | --- | --- | --- |
| libtest / `cargo test` | unit、integration、test executable。標準的なassertion、filter、capture | generated harnessと`cfg(test)`を含む別artifact | Rust componentの既定adapter |
| rustdoc doctest | public API例をcompile/run | 抽出programでありproduction pathではない | documentation/API contract |
| rustc compiletest | UI diagnostic、compile-fail、run-pass、codegen、MIR、incremental、run-makeをmode別に検証[5] | rustc repository固有の強いinfrastructure | owned frontend/loweringの比較設計 |
| trybuild | pass/compile-fail crateを`rustc`でcompileし、diagnosticを`.stderr`と比較[6] | compiler versionやpath表現でsnapshot churn。runtime artifactは対象外 | FFI/type/unsupported syntaxのnegative test |
| cargo-nextest | testごとに別process、timeout、retry、優先度、test group、partition、archive実行[7][8] | libtest形式のtest binaryを動かすrunner。production binary認定ではない | 大規模Rust testのexecution adapter候補 |
| `assert_cmd` | 任意pathまたはCargo binaryをprocess起動し、exit/stdout/stderr/stdin/env/timeoutをassert[9] | `cargo_bin`が指すbuild/profileとrelease candidate digestの同一性を別途固定する必要 | exact CLI harnessの参考実装 |
| trycmd / snapbox | 多数のCLI caseとstdout/stderr snapshot、README例を実行[10] | snapshot更新が容易なため、意味propertyをreviewなしに上書きし得る | CLI互換性の補助oracle |
| proptest | strategyから値を生成し、failureをshrinkして最小反例へ寄せる[11] | generator分布とpropertyの弱さを超えられない | graph/ABI/IR不変条件とcounterexample生成 |
| cargo-fuzz / libFuzzer | coverage-guided fuzzingでpanic/crash/UB探索、corpusと再現inputを保存[12][27] | harness entryとinstrumented build。到達しないsemantic errorは見つからない | parser、resolver、FFI boundaryの継続探索 |
| Miri | MIR interpreterでout-of-bounds、use-after-free、uninitialized、alignment、data race等のUBを検出[13] | 全Rust挙動を実装せず、native ABI/loader/実機timingを再現しない | unsafe Rust/FFI手前のcomponent gate |
| Loom | concurrent executionのinterleavingをmodelし、atomic/thread algorithmを探索[14] | Loom型への置換が必要で、memory modelと探索範囲に公表された制限 | scheduler/cache state machineの小型model |
| rustc sanitizers | ASan/LSan/MSan/TSan等のinstrumented native execution[15] | nightly/target制限、実行時overhead、別artifact。C/C++も同じsanitizer runtimeでcompileする配慮が必要 | mixed-language memory/race gate |
| Kani | bit-precise model checkingでassertion、安全性、contractをbounded proof[16] | harness/assumptionの健全性、未対応機能、state/resource exhaustion | 小さいunsafe/graph transitionのproof補助 |
| cargo-llvm-cov | LLVM source-based coverageをCargoへ統合[17] | coverageはoracleでもcorrectnessでもなく、instrumentationがcfg/artifactを変える | test gapの観測値 |
| cargo-mutants | source mutationをcompile/testし、生存mutantからtest感度不足を検出[18] | equivalent mutant、build/test cost、source-level限定 | direct testが意味あるfaultを捕捉するかの監査 |
| Insta | value/structured/CLI snapshotのreview workflow[19] | nondeterministic fieldや過大snapshot、無批判なacceptでfalse confidence | diagnostic/evidence formatの限定利用 |
| Criterion.rs | statistical measurement、warm-up、sampling、comparisonを行うbenchmark harness[20] | correctness testではなく、production workloadとも限らない | Lane B microbenchmark補助。release gateから分離 |

### 3.1 nextestが解く問題と解かない問題

nextestが各testを個別processで動かす理由は、process lifecycleの制御、正確なper-test timing、timeout後のkill、retry、stdout/stderr分離である[7]。これはin-process libtestよりLane Cのfailure isolationに近い。build artifactをarchiveし別machineで実行する機能もあり、host/target分離の参考になる[8]。

ただしnextest archiveに含まれるのはtest executableと実行依存であり、それをproduction closureと同一視しない。retry後の成功も通常PASSへ畳まず、attempt列と`Flaky`を保存する。source tree fixtureが必要なtestはarchiveだけで完結しない場合があるため、declared harness dependencyとして記録する。

### 3.2 Miri、sanitizer、Valgrindは補完関係

- MiriはRust MIR semantics上のUBを深く検査するが、foreign C/C++ codeやnative loaderの現実を直接走らせない。
- sanitizerはcompiler instrumentationを伴うnative executionでmixed-language boundaryを観測できるが、対応compiler/runtime/targetが必要である[15][25]。
- Valgrind Memcheckはsynthetic CPU上でbinaryを動的instrumentし、invalid access、uninitialized value、mismatched allocation/free、leak等を検出する[26]。production binary pathを入力にできる一方、native timingではなく大きなslowdownとplatform制限がある。

三者のPASSは互いを代替しない。少なくとも`exact-uninstrumented`、`sanitized`、`Miri/model`を別variantとして記録する。

## 4. C++のtool landscape

### 4.1 assertion frameworkとrunnerを分ける

C++ではGoogleTest、Catch2、doctest、Boost.Testが主に**test executable内の登録・assertion・reporting**を担い、CTestやlitが**複数testの発見・選択・実行**を担う。この二層を混同しない。

| tool/系統 | 主なsubjectと強み | 限界とidentity上の注意 | LAMINARIAでの位置 |
| --- | --- | --- | --- |
| GoogleTest | assertion、fixture、value/type parameterization、death test、XML等[21] | production `main`とは別のtest executableが標準 | C++ component adapterの第一候補 |
| gMock | call expectation、matcher、actionでcollaborator interactionを検証[22] | real library/loaderではなくmockとの契約をtestしやすい | 局所protocol test。artifact qualificationには使わない |
| Catch2 | assertion、section、fixture、generator、tag/report、benchmark[23] | test executableを生成。section再実行やsnapshot的出力の意味を理解する必要 | 既存projectが採用済みなら保持するadapter |
| doctest | lightweight C++ test framework、testをproduction source近傍へ配置可能[24] | compile-time利点はproject自己評価を含む。test registry/mainはproductionと別 | 小規模libraryの選択肢。全体標準にはしない |
| Boost.Test | fixture、data/template/parameterized case、複数link形態 | Boost dependencyとtest executableを伴う | Boost ecosystem入力の受容対象 |
| CTest | CMake build treeの`add_test`を実行し、filter/label/parallel/resource/fixture/repeat/timeout/JUnitを提供[29] | assertion frameworkではない。既定では0 testsも成功し得るため`--no-tests=error`が必要 | CMake/C++ runner adapterの第一候補 |
| `gtest_discover_tests` | compiled executableへ`--gtest_list_tests`し、parameterized instanceも発見[30] | discovery自体がtarget binary実行を要し、cross compileではemulator/target実行が必要 | dynamic discovery adapter |
| `catch_discover_tests` | Catch2 executableを実行してtestを列挙しCTestへ登録[31] | code signing/cross executionのため`PRE_TEST`等が必要な場合 | Catch2 adapter |
| LLVM lit | portable discovery、parallel、feature/target条件、status、timeout、selection[32] | suite configurationとshell-like pipelineが強力な分、hermetic inputを別途宣言する必要 | compiler/tool output regressionの参考 |
| FileCheck | ordered/negative/regex patternでtool outputを照合[33] | patternが広すぎればfalse pass、狭すぎればformat churn。意味同値ではない | IR/object/diagnosticの局所oracle |
| RapidCheck | QuickCheck型property generation/shrinking、stateful testing[34] | upstream選定・保守状態とC++ standard対応をprojectごとに確認 | C++ graph/ABI propertyの候補、必須依存ではない |
| libFuzzer | in-process coverage-guided fuzz target、SanitizerCoverageとcorpus[27] | upstream文書上、主要開発はCentipedeへ移りbug/security fix中心。test function用artifact | C/C++ parser、FFI adapter、archive reader |
| Clang sanitizers | ASan、UBSan、TSan、MSan等でmemory/UB/raceを動的検出[25] | instrumented artifact、target制限、TSan等の高いtime/memory cost | Rust/C++を同一sanitizer configurationで横断test |
| Valgrind Memcheck | binary translationでmemory access/value/leakを検出[26] | 10–50倍級slowdownの説明、最適化によるfalse positive/negative可能性 | uninstrumented candidateの補助動的検査 |
| Csmith | UBを避けたrandom C programのdifferential compiler testing[35] | C++固有feature、共通compiler bug、generator外を覆わない | C/native lowering differential corpus |
| `llvm-reduce` | interestingness testを保ちながらIR/MIR testcaseをdelta reduction[36] | oracleではなく失敗縮小器。flaky predicateでは結果が不安定 | failing IR/compile caseのfirst-class reducer |
| `llvm-readobj`/`llvm-nm`/`readelf` | header、section、symbol、relocation、dynamic/unwind等をobject/binaryから検査[37][38] | presenceはABI動作正しさの十分条件でない。format/platform差あり | exact artifactの構造・link evidence |
| Libabigail `abidiff` | ELF shared libraryのexported function/variable/type ABIをDWARF/CTF/BTFまたはsymbolで比較[39] | ELF中心、debug info品質とsuppressionに依存 | C/C++ shared ABIの差分gate |
| Google Benchmark | microbenchmarkの反復、counter、統計report[40] | functional oracleではなく、test binary/workloadもproductionと別 | Lane BのC++ microbenchmark補助 |

### 4.2 GoogleTestのdeath testもproduction process testではない

GoogleTest death testはstatementをchild processで実行し、終了状態とstderr patternを検査する[21]。assertion failureは必ずしもprocess deathではなく、threadと`fork`の相互作用にも注意が必要である。これはlibrary内部のfatal contractを反証する機構であり、packaged production executableのstartup、loader、files、signals、relocationを検査するend-to-end process harnessとは異なる。

### 4.3 CTest discoveryのcross-target問題

`gtest_discover_tests`と`catch_discover_tests`はsourceを解析せず、build済みtest executableを実行して一覧を得る[30][31]。parameterized testを正確に列挙できる反面、hostでtarget binaryを起動できないcross compileではemulatorまたはtarget-side discoveryが必要になる。LAMINARIAのtest graphには次を明示する。

- `discovery_runs_subject: true/false`
- `execute_on`と`produces_for`
- emulator/runner identity
- discovery phase (`post-build`または`pre-test`)
- discovery timeoutとraw list output

CTestはtestが0件でもcommand-line既定では正常扱いになり得る[29]。adapterは必ずempty selectionを`NoTests`またはerrorとして区別し、偽の成功を防ぐ。

## 5. RustとC++を能力別に対応付ける

| 故障クラス | Rust側の代表 | C++側の代表 | exact production artifactへの残課題 |
| --- | --- | --- | --- |
| 局所logic | libtest | GoogleTest/Catch2/doctest | test-only main/cfg/linkを除いた外部behavior確認 |
| compile rejection/diagnostic | compiletest、trybuild | lit + FileCheck、compiler invocation | diagnosticだけでなく「compile開始前のgraph rejection」と区別 |
| CLI behavior | `assert_cmd`、trycmd | CTest/custom process test | runnerが受け取ったpathのdigest固定 |
| property/counterexample | proptest | RapidCheck | mixed graph generatorとshrink provenance |
| memory/UB | Miri、sanitizer | sanitizer、Valgrind | production variantとの対応、FFI全体のinstrumentation整合 |
| concurrency | Loom、TSan | TSan、Valgrind DRD/Helgrind等 | schedule coverageと実機timingの限界 |
| fuzz | cargo-fuzz/libFuzzer | libFuzzer | corpus/seed/toolchain identity、failure reduction |
| bounded proof | Kani | project固有model checker/SMT | foreign library/runtime assumptions |
| test adequacy | cargo-llvm-cov、cargo-mutants | LLVM coverage、mutation tool | coverageをcorrectnessへ読み替えない |
| ABI/object/link | rustc/LLVM tools、runtime test | readobj/nm/readelf、Libabigail | Rust ABIは原則stable contractでなく、C ABI/明示contractを境界にする |
| performance | Criterion.rs | Google Benchmark | Lane Bのproduction-scale計測と分離して相関確認 |

この表は「Rust toolをC++ toolへ翻訳する」ものではない。mixed executableの一つのfaultが複数層に跨るため、各oracleの観測境界を明示するための表である。

## 6. LAMINARIAで採用するtest architecture

```text
shared production graph
  ├─ production artifact ── exact-artifact qualification harness
  │                            ├─ process behavior
  │                            ├─ loader/resource/closure
  │                            └─ object/symbol/ABI inspection
  ├─ Rust test artifacts ─── libtest/nextest adapter
  ├─ C++ test artifacts ──── CTest + framework discovery adapter
  ├─ instrumented variants ─ sanitizer/fuzz/coverage adapter
  ├─ semantic variants ───── Miri/Loom/Kani/property/differential
  └─ failure artifacts ───── seed/corpus/reduced input/raw evidence
```

### 6.1 exact-artifact qualification harness

M1の正本はframework内testではなく、G2が生成したproduction artifact digestを入力として直接起動するharnessである。最低限、次を扱う。

- argv、stdin、cwd、環境変数allowlist、filesystem image、resource limits。
- stdout/stderr byte列、exit code、signal/exception、timeout、生成/変更file。
- loader dependency、opened resource、spawned process、networkの観測。
- expected valueだけでなく、negative dependency、relocation、pruned/eager equivalence。
- subject、harness、environment、oracle、raw evidenceのdigest。

Rustでharnessを実装する場合、`std::process::Command`と`assert_cmd`のcontrol/assertion設計は参考になる。しかしCargoがtest用に生成したbinary pathを暗黙取得せず、production graphが発行したimmutable artifact identityを渡す。

### 6.2 framework adapter

adapterは各frameworkの意味を共通最小protocolへ写像する。

```text
TestInvocation
  framework: libtest | nextest | ctest | gtest | catch2 | lit | external
  subject_artifact
  discovery_artifact?
  selector
  execute_on / produces_for
  controls

TestResult
  status: pass | fail | flaky | timeout | crash | unsupported |
          no-tests | harness-error | inconclusive
  attempts[]
  raw_stdout / raw_stderr / structured_report?
  observations[]
  subject_digest / harness_digest / environment_digest
```

framework固有のXML/JSONを新しい正本へ変換しない。raw reportを保存し、共通statusとprovenanceをindexとして派生させる。再解析可能であることを優先する。

### 6.3 test dependencyをrelease dependencyから隔離する

GoogleTest、Catch2、libtest、sanitizer runtime、fuzzer engine、snapshot、fixture generatorはtest graphのdependencyである。production closureへ到達しないことをgraph reachabilityとexact binary inspectionの両方で確認する。

mockだけで成立するtestは、実際のC/C++ archive、Rust-C ABI shim、loaderとのintegration testを置換しない。fixture専用validatorがproduction parser/solverとは別logicを持つことも禁止し、可能な限りproduction entry pointへfixtureを入力する。

## 7. M0/M1のtool選択

すべてを導入するのではなく、fault classが重ならない最小集合を採る。

### M0で固定するもの

1. exact-artifact process harnessのsubject/evidence schema。
2. Rust `cargo test`/libtest result adapter。
3. C++ CTest adapter。fixture projectがGoogleTestなら`gtest_discover_tests`を使用し、Catch2を二重導入しない。
4. object/symbol inspectionのportable interface。backendはplatformごとに`llvm-readobj`/`llvm-nm`、`readelf`等を選ぶ。
5. instrumented/test/production variantを別identityにする規則。

### M1で必須の実行

1. exact production binaryのpositive behaviorとclean-environment execution。
2. Rust component testとC++ component testを各native frameworkから収集。ただし両者の成功をM1 passの代替にしない。
3. Rust→C ABI→C++ adapter→C ABI→Rust/Nim observable resultの実行。
4. export/import symbol、relocation、loader closureのinspection。
5. ABI/symbol mismatchを含むnegative graphのpre-build rejection。
6. ASanまたは同等のmixed-language instrumented variant。Rust/C++双方のruntime整合を確認する。
7. eager/pruned、clean/incrementalのexact behavior比較。
8. 意図的faultを少なくともlogic、ABI/link、runtime dependencyの三層へ注入し、対応testが検出すること。

Miri、Loom、Kani、fuzz、mutation、coverage、Libabigailは、固定workloadのfault modelに応じて追加する。tool導入数をmilestone evidenceにしない。

## 8. 反証可能な実験

### E-C-T1 — test binaryとproduction binaryの差

同じRust/C++ sourceからproduction、framework-test、sanitizedの三variantを作り、digest、entry point、linked dependency、section、sizeを比較する。差が観測できなければidentity modelが不足している。

### E-C-T2 — adapterのlossiness

libtest/nextest/CTest/litから、pass、fail、skip/ignored、timeout、crash、zero-tests、flaky相当を入力し、raw evidenceを失わず共通statusへ写像できるか調べる。写像不能な状態は`Inconclusive`として保存し、PASSへ畳まない。

### E-C-T3 — mixed sanitizer

RustとC++を同一compatible sanitizer configurationでbuildし、FFI境界を跨ぐheap ownership、out-of-bounds、use-after-free、raceのseeded faultを検出する。片側のみinstrumentした場合との差も保存する。

### E-C-T4 — ABI evidence

C++ libraryのlayout、name mangling、standard library/ABI設定、exception/unwind条件を一つ変え、graph rejection、symbol inspection、Libabigail、runtime testのどの段階で検出できるか比較する。C ABI shimでexternalizedした条件と直接C++ ABIを使う条件を分ける。

### E-C-T5 — test adequacy

production implementationへmutationまたは既知faultを注入し、どのtestが選択・失敗するかを記録する。fixture validatorだけが失敗しproduction path testが通る場合、そのvalidatorは正本として不適格である。

### E-C-T6 — cross-target discovery

hostで起動できないtest executableについて、static registration、emulator discovery、target-side pre-test discoveryを比較し、discovery時間、失敗分類、target dependency、再現性を測る。

## 9. 非採用・禁止事項

- RustとC++へ同じassertion frameworkを強制しない。
- `cargo test`またはCTestが通っただけでproduction artifactをqualifiedにしない。
- instrumented binaryのPASSをuninstrumented release candidateへ転記しない。
- coverage率をcorrectnessまたはtest adequacyの単独指標にしない。
- retryで通った結果を通常PASSにしない。
- snapshotの一括更新をsemantic承認にしない。
- mock expectationをactual ABI、symbol、loader、filesystem behaviorの代替にしない。
- framework出力を変換したYAMLと、そのYAML専用validatorと、そのvalidator testを三重保守しない。
- config/lockのproject名、件数、SHA、属性をtestへ再列挙して第二の正本にしない。
- zero-testsを成功として扱わない。
- benchmarkの改善をcorrectness evidenceにしない。

## 10. 未解決研究課題

1. production executable内にtest hookを入れず、内部semantic obligationとexternal observationをどこまで対応付けられるか。
2. Rustの不安定なnative ABIとC++ ABI variationを、C ABI shim、generated adapter、explicit externalizationのどこで境界化するか。
3. framework固有test selectionとshared graphのaffected-test計算を、false negativeなしにどう合成するか。
4. sanitizer/Miri/model checkerで得たfailureを、uninstrumented production behaviorへどの強さで帰属できるか。
5. cross-target test discoveryをsubject実行なしで行う場合、登録情報の正本性をどう保証するか。
6. object/ABI structure oracleとruntime behavior oracleが不一致のとき、どのobligationを再openするか。
7. fuzz corpus、property counterexample、reduced IRをgraph nodeとして保持し、compiler/toolchain更新後に効率的に再選択する方法。

## 11. 参考実装と再現可能性

本調査を文献一覧だけで終わらせないため、architecture上の代表実装を
[`reference-projects.lock.json`](../../../reference-projects.lock.json)へ完全commit
SHA付きで登録した。Rust標準test/compiletestは既存の`rust`、LLVM系toolは既存の
`llvm-project`を参照し、新たに`cargo-nextest`、`googletest`、`Catch2`、`CMake`、
`miri`、`kani`、`libabigail`を追加した。

取得、offline再検証、読む実装境界、完了条件は
[参考プロジェクト再現手順](../../04-guides/reference-projects.md)
を正本とする。調査上の主張を更新するときは、参照したproject名、lock SHA、source path、
観察した境界を残す。moving branchの最新実装を暗黙に根拠へ混ぜない。

## Sources

1. Rust Project, [The `#[test]` attribute / `rustc --test`](https://doc.rust-lang.org/rustc/tests/).
2. Cargo, [`cargo test`](https://doc.rust-lang.org/cargo/commands/cargo-test.html).
3. Cargo, [Cargo Targets: tests](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#tests).
4. Rustdoc, [Documentation tests](https://doc.rust-lang.org/rustdoc/documentation-tests.html).
5. Rust Compiler Development Guide, [Compiletest](https://rustc-dev-guide.rust-lang.org/tests/compiletest.html).
6. trybuild, [Compile-fail tests for Rust](https://docs.rs/trybuild/latest/trybuild/).
7. cargo-nextest, [Why process-per-test?](https://nexte.st/docs/design/why-process-per-test/) and [Retries](https://nexte.st/docs/features/retries/).
8. cargo-nextest, [Archiving and reusing builds](https://nexte.st/docs/ci-features/archiving/) and [Target runners](https://nexte.st/docs/features/target-runners/).
9. assert_cmd, [CLI integration testing](https://docs.rs/assert_cmd/latest/assert_cmd/).
10. trycmd, [Snapshot testing for CLI commands](https://docs.rs/trycmd/latest/trycmd/).
11. proptest, [`Strategy` and shrinking](https://docs.rs/proptest/latest/proptest/strategy/trait.Strategy.html).
12. Rust Fuzz Book, [Introduction](https://rust-fuzz.github.io/book/).
13. Rust Project, [Miri](https://github.com/rust-lang/miri).
14. Tokio project, [Loom](https://github.com/tokio-rs/loom).
15. Rust Project, [Sanitizer support](https://doc.rust-lang.org/beta/unstable-book/compiler-flags/sanitizer.html).
16. Kani, [Kani Rust Verifier](https://model-checking.github.io/kani/).
17. cargo-llvm-cov, [README](https://github.com/taiki-e/cargo-llvm-cov/blob/main/README.md).
18. cargo-mutants, [How it works](https://mutants.rs/how-it-works.html).
19. Insta, [Snapshot testing documentation](https://insta.rs/docs/).
20. Criterion.rs, [Criterion.rs documentation](https://docs.rs/criterion/latest/criterion/).
21. GoogleTest, [User guide](https://google.github.io/googletest/) and [Advanced topics](https://google.github.io/googletest/advanced.html).
22. GoogleTest, [gMock cookbook](https://google.github.io/googletest/gmock_cook_book.html).
23. Catch2, [Documentation index](https://github.com/catchorg/Catch2/blob/devel/docs/Readme.md).
24. doctest, [Project documentation](https://github.com/doctest/doctest).
25. LLVM/Clang, [AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html), [UndefinedBehaviorSanitizer](https://clang.llvm.org/docs/UndefinedBehaviorSanitizer.html), and [ThreadSanitizer](https://clang.llvm.org/docs/ThreadSanitizer.html).
26. Valgrind, [Memcheck manual](https://valgrind.org/docs/manual/mc-manual.html) and [Valgrind core](https://valgrind.org/docs/manual/manual-core.html).
27. LLVM, [libFuzzer](https://llvm.org/docs/LibFuzzer.html).
28. Boost, [Boost.Test introduction](https://www.boost.org/latest/libs/test/doc/html/boost_test/intro.html).
29. CMake, [`ctest(1)`](https://cmake.org/cmake/help/latest/manual/ctest.1.html).
30. CMake, [`GoogleTest` module and `gtest_discover_tests`](https://cmake.org/cmake/help/latest/module/GoogleTest.html).
31. Catch2, [`catch_discover_tests` and CMake integration](https://github.com/catchorg/Catch2/blob/devel/docs/cmake-integration.md).
32. LLVM, [`lit` — LLVM Integrated Tester](https://llvm.org/docs/CommandGuide/lit.html).
33. LLVM, [`FileCheck`](https://llvm.org/docs/CommandGuide/FileCheck.html) and [Testing Infrastructure Guide](https://llvm.org/docs/TestingGuide.html).
34. RapidCheck, [QuickCheck-style property testing for C++](https://github.com/emil-e/rapidcheck).
35. Yang et al., [Finding and Understanding Bugs in C Compilers](https://users.cs.utah.edu/~regehr/papers/pldi11-preprint.pdf), PLDI 2011.
36. LLVM, [`llvm-reduce`](https://llvm.org/docs/CommandGuide/llvm-reduce.html).
37. LLVM, [`llvm-readobj`](https://llvm.org/docs/CommandGuide/llvm-readobj.html) and [`llvm-nm`](https://llvm.org/docs/CommandGuide/llvm-nm.html).
38. GNU Binutils, [`readelf`](https://sourceware.org/binutils/docs/binutils/readelf.html).
39. Libabigail, [Overview](https://sourceware.org/libabigail/manual/libabigail-overview.html) and [`abidiff`](https://sourceware.org/libabigail/manual/abidiff.html).
40. Google Benchmark, [User guide](https://google.github.io/benchmark/).

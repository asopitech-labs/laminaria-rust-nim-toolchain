# Lane C 基礎研究 — Executable Verification and Testability

## 位置づけ

本書は[当面の研究ゴール](../../near-term-research-program_ja.md)から派生するLane Cの基礎研究である。調査日は2026-09-13。「binaryが生成された」と「そのbinaryが価値を持つ」は同じではない。exact production artifactを指定し、そのartifactを対象環境で制御・観測し、期待する性質を反証可能に判定し、結果をartifact identityと環境へ結び付けられて初めて、M1の成果物は完成する。

Lane CはCI設定やtest runnerの付属作業ではない。Lane Aが閉じた依存義務が実行時にも成立するか、Lane Bの枝刈り・増分再利用が意味を変えていないかを判定する、独立した研究レーンである。

Rust/C++の個別tool、test executableとproduction executableのidentity差、mixed-language sanitizer、CTest/nextest adapter、ABI inspectionの詳細は[「Rust/C++ test tool landscapeとLAMINARIAへの適用」](rust-cpp-testing-tool-landscape_ja.md)に分離した。本書の`TestContract`とM0/M1要件が上位契約であり、個別toolはfault classに応じて選ぶ実装候補である。

## 1. 検証対象を固定する

最低限の`TestContract`は次を分離して持つ。

```text
TestContract
  subject        exact production artifact digest + variant
  harness        runner/tools/scripts digest + identity
  environment    target platform, runtime closure, isolation profile
  controls       argv, stdin, env, files, time, signals, resources, faults
  observations   exit, signal, stdout/stderr, files, ABI/symbol/loader events
  oracle         property or expected relation
  evidence       raw observations + provenance + timestamps
  verdict        pass/fail/timeout/unsupported/flaky/inconclusive
```

instrumentation、sanitizer、coverage、test cfgを含むbinaryは、production binaryと別のsubject identityである。instrumented buildでしか通らないtestは有用だが、それだけでrelease candidateを認定できない。逆にproduction binaryだけでは内部状態を観測できない場合、二つのartifactの対応関係を明示し、外部behavior testを残す。

## 2. 先行研究の地図

| 系統 | 強い点 | 残る境界 |
| --- | --- | --- |
| Bazel Test Encyclopedia | hermetic testのruntime environment、declared inputs、runner責務をnormativeに定義する[1] | compiler semantic oracleやexact release artifactのclosure検証はrule/test側に残る |
| LLVM lit / FileCheck | test discovery、parallelism、selection、status、target feature、tool outputの順序/否定patternを軽量に扱う[2][3] | textual patternは意味同値性の一般oracleではなく、広すぎるpatternはbugを隠し得る |
| rustc compiletest | compile-fail、run-pass、codegen、MIR diff、incremental query cleanliness、run-make等をcompiler固有に統合する[4] | package/runtime closureや外部ecosystemのartifact identityを一般化したharnessではない |
| Csmith | undefined/unspecified behaviorを避けたrandom C programを生成し、compiler間differential testingで多数のwrong-code bugを発見[5] | oracleは複数compilerの合意に依存し、共通bugやgenerator範囲外を保証しない |
| EMI | test input上で等価なprogram variantを作り、optimizerの静的処理を差分検証する[6] | input集合外のvariant意味は同値でなく、全semantic correctnessの証明ではない |
| QuickCheck | 性質をgeneratorとshrink可能なrandom testとして記述する[7] | generator分布とpropertyの強さがcoverageを支配する |
| Alive2 | LLVM transformationをUB-aware refinementとしてSMTでtranslation validateする[8] | interprocedural transformation等に制限があり、package/ABI/runtime全体は扱わない |
| CompCert | compiler pass correctnessをmechanized proofで結び、testingより強い保証を与える[9] | supported language/target/assumption範囲があり、foreign library/runtime/environmentは別契約 |
| fuzzing / sanitizers | crash、UB、memory safety等を大量探索・動的検出する[10] | passは不在の証明ではなく、instrumentationがartifact identity/behaviorを変える |
| regression test selection | observed file dependency等から影響testだけを選び時間を削減する[11] | 観測されなかったdynamic dependencyやABI/semantic edgeでfalse negativeが起こり得る |
| reproducible builds / SLSA |同一input/environment/instructionからbit-identical artifactを目指し、digestとproducer provenanceを結ぶ[12][13] | 再現性/provenanceは機能正しさを証明しない |

## 3. 「何をtestするか」の層

一種類のend-to-end testで全問題を覆わない。共通graphから異なるoracleを導出する。

| 層 | subject | 代表oracle |
| --- | --- | --- |
| resolver | selected/rejected graph | constraint充足、一意性、説明、negative rejection |
| source semantics | typed module/function | reference interpreter、property、diagnostic |
| lowering | before/after IR | legality、refinement、differential behavior |
| ABI/symbol | object/archive/shared object | exported/imported symbol、layout、calling convention、unwind |
| link | final executable | undefined/duplicate、retained roots、loader closure |
| behavior | exact binary process | exit/signal/output/file/protocol/property |
| closure | clean target environment | undeclared file/library/tool/sourceへアクセスしない |
| computation | baseline/candidate artifacts | behavioral equivalence、artifact relation、work/metric invariants |
| release | packaged exact candidate | install、relocate、run、update、rollback、provenance |
| self-host | stage1/stage2 artifacts | producer lineage、conformance、外部copy排除 |

unit testは局所logicを速く反証するが、production pathを通らないmock/fixtureだけでは成果物を認定しない。end-to-end testは統合を反証するが、失敗局所化と入力空間coverageが弱い。direct production testを正本とし、その下にproperty/differential/translation-validation/unit testを重ねる。

## 4. oracle problem

compiler/toolchain testの中心的難題は、任意programの正しい出力を安価に知れないことである。そこでoracleを複線化する。

### 4.1 explicit example oracle

固定入力に対するexit/stdout/file result等。M1のcross-language値とnegative caseに適する。狭いが説明しやすい。

### 4.2 differential oracle

reference compiler、interpreter、別optimization level、eager/pruned pathを同じ入力で比較する。Csmithはvalid Cを生成して複数compilerの出力差からwrong-codeを発見した[5]。ただし全実装が同じbugを持つ場合や、未定義動作を含む場合は成立しない。

### 4.3 metamorphic / EMI oracle

期待値そのものではなく、入力・program変換の前後で保つべき関係を検査する。EMIは既知test inputで実行されないcodeを変形してcompilerを差分検証した[6]。LAMINARIAでは、到達不能branch追加、依存順序変更、同値canonicalization、pruning on/off等が候補である。

### 4.4 property oracle

型、ABI、graph、実行結果の不変条件を生成入力で検査し、失敗caseをshrinkする[7]。例: 「全`requires-symbol`は一つのcompatible definitionへ解決」「`Discharged`義務はconsumer側toolを要求しない」。generatorが現実のCargo/Nimble/C/C++ corner caseを含むかを測る必要がある。

### 4.5 translation validation / proof

各optimization/lowering instanceがsourceをrefineするかを検証する。Alive2はLLVM IR semanticsとUBを考慮したrefinement checkerを提供するが、interprocedural変換等の制限を公表している[8]。全compilerを証明するCompCert型のapproach[9]と、各実行instanceを検証するtranslation validationは異なる。M1では全証明を目指さず、restricted lowering/propertyとdifferential executionを組み合わせる。

## 5. hermeticityとdependency-dischargeの検査

Bazelはtest resultがdeclared source、declared build products、runnerが保証するresourceだけへ依存すべきとし、非hermetic testは再現性、culprit finding、auditability、resource isolationを損なうと説明する[1]。LAMINARIAではさらに、artifact closureの負の主張をtestする。

- build tool、source tree、package cacheをclean environmentから除く。
- 許可したruntime library/resourceだけを配置する。
- loader dependency、opened file、spawned process、network accessを観測する。
- undeclared dependencyを意図的に欠落/破損/別versionへ置換する。
- static/discharged dependencyをhostへ置いてもartifactが偶然拾わないことを確認する。
- externalized dependencyが欠けた場合は、構造化された予期可能なfailureになることを確認する。

「実行できた」だけではhost contaminationを排除できない。negative dependency testとaccess traceを併用し、declared closureとのset差を証拠化する。

## 6. exact artifact identityとprovenance

SLSAはprovenance statementの`subject` digestが検査対象artifact digestと一致することをverificationの基本に置く[13]。LAMINARIAのtest resultも同じ原則を使う。

```text
artifact digest -> producer operation/inputs/toolchain
               -> test contract digest
               -> harness/environment identities
               -> raw observation digest
               -> verdict
```

pathやfilenameだけでsubjectを識別しない。test後に同じpathへ別binaryを上書きした場合、過去のpassは新しいbinaryへ移らない。artifactが再現可能でも機能correctnessは別であり、testがpassしてもbuild provenanceの真正性は別である。再現性、provenance、verification verdictを別propertyとして結合する。

## 7. test-only dependencyとinstrumentation bias

test harness、oracle、fixture、sanitizer runtime、coverage runtimeはtest executionには必要だが、release artifactのruntime closureへ混ぜない。

- `subject dependency`: production artifactの意味/実行に必要。
- `test-build dependency`: instrumented/test variant生成だけに必要。
- `harness dependency`: testを制御・観測する側に必要。
- `environment dependency`: target sandbox/emulator/runnerに必要。
- `oracle dependency`: reference implementation/SMT/checkerに必要。

同一logical sourceからproduction/test variantを作る場合も、compiler flags、cfg、linked runtime、digestは別identityにする。sanitizerでのみ発生/消失するrace、timing、layout差を考慮し、production exact-binary smoke/behavior/closure testを必須にする。

## 8. failure modelと判定

`pass/fail`だけでは原因を失う。最低限次を区別する。

- `BuildRejected`: Lane Aが正しくpre-build rejection。
- `HarnessError`: subject実行前のtest infrastructure failure。
- `Unsupported`: target capabilityが契約外。
- `Timeout`: deadline超過。kill sequenceとpartial outputを保存。
- `Crash`: signal/exception/abnormal exit。
- `OracleMismatch`: 正常終了したが性質不一致。
- `DependencyLeak`: undeclared file/library/process/network access。
- `ResourceViolation`: memory/CPU/disk等のcontract超過。
- `Flaky`: 同一identity/contractで結果が変動。
- `Inconclusive`: oracleまたは観測が判定に不足。

LLVM litもPASS/FAILだけでなくXFAIL/XPASS/UNRESOLVED/UNSUPPORTED/TIMEOUT/FLAKYPASSを区別し、並列実行、retry、sharding、subset selectionを提供する[2]。LAMINARIAはこれを採用候補のstatus vocabularyとしつつ、artifact/obligation固有のcauseを追加する。

## 9. retest selectionはLane Bと共有する

Ekstaziはtestが実行時にaccessしたfile dependencyを記録し、回帰test選択で実行時間を削減した[11]。しかしdynamic observationだけでは未実行path、callback、ABI、toolchain影響を漏らし得る。LAMINARIAでは次のunionを保守的なaffected setとする。

```text
affected tests =
  static typed graph reachability
  union observed runtime dependencies
  union changed environment/toolchain contracts
  union uncertainty fallback
```

unknown edge、reflection/dynamic loading、generator変更、graph schema変更ではfull retestへfallbackする。選択効率だけでなく、mutation/known-failureを注入し、選択したtestが必ず失敗を捕捉するかを測る。build invalidationとretest selectionは同じedgeを読むが、artifact producer edgeとtest oracle edgeを同一種類にはしない。

## 10. flakiness、非決定性、再現性

同じtest identityで結果が揺れる原因を少なくとも次へ分類する。

- product nondeterminism。
- harness race/timeout。
- environment contamination。
- unordered outputを過剰に固定したoracle。
- external service/network/time/randomness。
- undefined behaviorまたはABI mismatch。

retryでpassした結果を通常PASSへ畳まない。seed、attempt、schedule/resource limits、raw outputを保存し、`FLAKYPASS`相当としてrelease gateを別扱いにする。random/property/fuzz testはseed replayとshrink後caseをfirst-class artifactにする。

## 11. LAMINARIA固有の仮説

### C-H1 — exact subject binding

artifact digest、producer、contract、environment、raw evidenceをgraphで結ぶと、「違うbinaryをtestした」「test dependencyがreleaseへ漏れた」というfalse qualificationを機械的に拒否できる。

### C-H2 — obligation-derived tests

Lane AのABI/symbol/runtime/externalization obligationからtestを導出すると、手書きend-to-end exampleだけよりdependency leakと境界failureを発見できる。

### C-H3 — pruning/incremental equivalence

Lane Bのeager/pruned、clean/incremental経路を同一controls/oracleで比較すると、性能最適化によるsemantic driftやstale reuseを直接反証できる。

### C-H4 — shared-graph retest selection

static semantic edges、runtime observations、uncertainty fallbackをunionすれば、別管理のtest catalogなしに安全なretest削減ができる。

### C-H5 — artifact testability as design pressure

制御点・観測点・failure classificationを成果物設計時に要求すると、暗黙runtime dependency、説明不能なlink、非決定的actionをM1以前に露出できる。

## 12. M0/M1実験仕様

### required contracts

- production artifact digestとproducer lineage。
- harness binary/script digest。
- target environment/allowed closure identity。
- deterministic controlsとraw observations。
- explicit/metamorphic/differential oracleの少なくとも二系統。

### required tests

1. exact production binaryのclean-environment positive execution。
2. Rust/Nim/C/C++を横断した観測値。
3. ABIまたはsymbol mismatchのpre-build rejection。
4. externalized runtime dependency欠落時のnegative execution。
5. undeclared build tool/source/package cacheが存在しないこと。
6. eager vs pruned binaryのbehavioral equivalence。
7. clean vs incremental resultのbehavioral equivalence。
8. unused branch追加のmetamorphic relation。
9. timeout/crash時のraw evidence回収。
10. test-only dependencyがrelease closureへ入らないこと。

### metrics

- detection: seeded faults caught / total、層別failure localization。
- soundness: known affected testsの選択漏れ、clean baselineとのfalse pass。
- cost: test discovery/execution wall-clock、peak RSS、selected/total tests。
- reproducibility: identical contractのverdict/raw observation一致率。
- isolation: declared/observed dependency set差。
- traceability: verdictからsubject/producer/inputへ辿れる割合。

### 反証条件

- digestを結んでもrunnerが別path/loader dependencyを実行し得る。
- obligation-derived testが手書きtestより欠陥検出や説明を改善しない。
- graph-based selectionがknown faultを取り逃がす。
- isolation/trace overheadがM1の実行を実用不能にする。
- production/test variant差が大きく、instrumented evidenceを対応付けられない。
- oracle間不一致を`Inconclusive`として保存できず、誤ったPASSへ畳む。

## 13. M1での採用範囲

M1では万能compiler verifierを作らない。Bazelのhermetic environment contract、lit/compiletestのstatusとtest mode、Csmith/EMI/QuickCheckのoracle多様化、Alive2のinstance validation、SLSAのsubject digest bindingを比較基準として、固定mixed workloadのexact production artifactを直接testする。成功はtest数ではなく、上記10 testがproduction graphから実行され、raw evidenceとartifact identityを結び、少なくとも一つの意図的faultを正しい層で検出することで判定する。

Rust側はlibtest/`cargo test`をcomponent adapterとして受け入れ、必要に応じnextest、trybuild、proptest、Miri、sanitizer等を追加する。C++側はfixture projectが既に使うGoogleTestまたはCatch2をCTestから実行し、frameworkを二重導入しない。いずれもproduction executableとは別subjectであるため、exact artifactのprocess/loader/closure test、object/symbol/ABI inspectionを独立した必須gateとする。選定根拠と制約は[tool landscape](rust-cpp-testing-tool-landscape_ja.md)に従う。

## 参考文献

1. Bazel, [Test Encyclopedia](https://bazel.build/reference/test-encyclopedia) and [Hermeticity](https://bazel.build/concepts/hermeticity).
2. LLVM, [`lit` — LLVM Integrated Tester](https://llvm.org/docs/CommandGuide/lit.html).
3. LLVM, [`FileCheck`](https://llvm.org/docs/CommandGuide/FileCheck.html) and [Testing Infrastructure Guide](https://llvm.org/docs/TestingGuide.html).
4. Rust Compiler Development Guide, [Compiletest](https://rustc-dev-guide.rust-lang.org/tests/compiletest.html).
5. Yang et al., “Finding and Understanding Bugs in C Compilers,” PLDI 2011, [author PDF](https://users.cs.utah.edu/~regehr/papers/pldi11-preprint.pdf).
6. Le, Afshari, Su, “Compiler Validation via Equivalence Modulo Inputs,” PLDI 2014, [author PDF](https://www.vuminhle.com/pdf/pldi14-emi.pdf).
7. Claessen, Hughes, “QuickCheck: A Lightweight Tool for Random Testing of Haskell Programs,” ICFP 2000, [DOI:10.1145/351240.351266](https://doi.org/10.1145/351240.351266).
8. Lopes et al., “Alive2: Bounded Translation Validation for LLVM,” PLDI 2021, [project and paper](https://github.com/AliveToolkit/alive2).
9. Leroy, “Formal Verification of a Realistic Compiler,” CACM 2009, [CompCert publications](https://compcert.org/publi.html).
10. LLVM, [libFuzzer](https://llvm.org/docs/LibFuzzer.html) and [AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html).
11. Gligoric et al., “Practical Regression Test Selection with Dynamic File Dependencies,” ISSTA 2015, [author PDF](https://users.ece.utexas.edu/~gligoric/papers/GligoricETAL15Ekstazi.pdf).
12. Reproducible Builds, [Definition](https://reproducible-builds.org/docs/definition/).
13. SLSA v1.2, [Provenance](https://slsa.dev/spec/v1.2/provenance) and [Verifying artifacts](https://slsa.dev/spec/v1.2/verifying-artifacts).

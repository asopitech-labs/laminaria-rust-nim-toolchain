# Testable Native Artifactと第一級Test Harness

## 結論

LAMINARIAがnative binaryを生成して一度起動できても、そのbinaryを反復可能に検証できなければ成果物として不十分である。test harnessはrepository外側のCI scriptや人間の手順ではなく、要求artifact、dependency graph、compiler computation、runtime contract、evidenceを接続する第一級のgraph contractでなければならない。この独立した研究領域を**Lane C — Executable Verification and Testability**とする。

```text
TestContract
  + exact TestSubject artifact identity
  + inputs / environment / isolation / controls
  + observations / oracle / tolerances
  + target execution capability
  + harness and test-dependency identities
                         |
                         v
                   TestExecution
                         |
                         v
  pass / fail / unsupported + raw evidence + provenance
```

「test用buildが通る」ことと、「利用者へ渡すproduction binaryをtestした」ことを区別する。instrumentation、mock、test-only export、assertion、sanitizer等を含むbinaryは重要な証拠を作れるが、別identityのartifactである。production artifactの合格には、そのexact digestを対象とするblack-box／runtime／loader testが必要である。

Rustのlibtest/nextest/`assert_cmd`/Miri/Kani等と、C++のGoogleTest/Catch2/CTest/lit/sanitizer/ABI tool等の役割比較、adapter設計、M0/M1での採用範囲は[「Rust/C++ test tool landscapeとLAMINARIAへの適用」](rust-cpp-testing-tool-landscape_ja.md)を参照する。既存frameworkはcomponent evidenceとして取り込み、framework固有test executableをproduction artifactと同一視しない。

## Lane Cの責務と他laneへのfeedback

### Lane Aへのfeedback

Lane Aでは、何をtest subjectとし、どの振る舞い・ABI・symbol・runtime・dependency-discharge条件を満たすべきかを定義する。

- `NativeExecutable`、`Library`、`TestExecutable`、`HarnessExecutable`を異なるartifact demandとして表す。
- production artifactとinstrumented/test artifactのsemantic relationとidentity差分を記録する。
- test-only package、source、symbol、runtimeを明示し、release artifactへ漏らさない。
- externalized runtime contractのpositive／negative environmentを生成可能にする。
- Cargo/Nimble/C/C++をまたぐcall、callback、layout、ownership、error、constructor、dynamic-loading条件をtest contractへ落とす。

### Lane Bへのfeedback

Lane Bでは、harness buildとtest executionを通常のcompiler computationとして計画し、再利用・invalidate・schedule・計測する。

- test selectionをroot demandとして必要なtest／subject／fixtureだけを展開する。
- test subject build、harness build、environment preparation、execution、observationを別actionにする。
- source／IR／ABI／runtime変更から、再buildすべきartifactと再実行すべきtestを導出する。
- target-compatible nodeへnative executionを配置し、cross compile成功をtest成功とみなさない。
- timeout、crash、signal、resource exhaustion、nondeterminismを成功／通常failureと混同しない。

Lane Cは単にLane A/Bの結果を受動的に検査しない。testから発見したmissing observation、uncontrollable dependency、ABI ambiguity、nondeterministic behavior、過剰invalidationを、artifact obligationまたはcompiler-computation constraintとして同じgraphへ返す。三laneは同じartifact identity、semantic facts、runtime contract、provenanceを読む。Lane A用の期待manifest、Lane B用のfixture validator、Lane C用のoracle catalogを別々の正本として保守しない。

## Test artifact model

```text
ArtifactDemand = ProductionExecutable
               | TestSubject
               | TestExecutable
               | HarnessExecutable
               | TestData

TestContract
  subject_artifact_id
  subject_profile
  test_kind
  input_contract
  environment_contract
  isolation_contract
  control_capabilities
  observation_contract
  oracle
  tolerance_policy
  timeout_and_resource_limits
  target_execution_requirements
  required_test_dependencies

TestResult
  contract_id
  exact_subject_digest
  harness_digest
  environment_fingerprint
  started_at / duration / resource_usage
  exit / signal / timeout / crash
  stdout / stderr / structured observations
  pass / fail / unsupported
  evidence_locations
```

test contract、harness、test data、environmentも依存を持つ。それらはproduction dependencyと混ぜず、test executionのための依存義務として同じresolverで解決・discharge／externalize／rejectする。

## 必須test層

### 1. Exact production artifact test

利用者へ渡すexact binary／bundle digestを、clean target environmentで起動する。exit、signal、stdout/stderr、file/network/process side effect、loader dependency、resource、runtime contractを観測する。instrumented binaryだけの成功では代替できない。

### 2. Semantic and cross-language conformance

対応source semanticsとIR変換が、Rust/Nim/C/C++境界を含め期待するobservable behaviorを保持することを検証する。必要ならreference implementationと比較するが、reference compilerの成功をLAMINARIA artifactの成功とはしない。

### 3. ABI and artifact contract test

symbol、calling convention、layout、alignment、ownership、callback、exception/unwind、constructor/destructor、dynamic loader、resource locationを実artifactから検査し実行する。manifestだけの一致では合格にしない。

### 4. Negative and fault test

version、feature、ABI、symbol、runtime、permission、resource、timeout等を意図的に一つ壊し、compile前 rejection、runtime preflight rejection、または分類済みexecution failureを確認する。欠落依存を偶然hostから拾う場合は失敗である。

### 5. Incremental test selection

変更したsemantic／IR／artifact／runtime nodeから、再buildと再testの集合をproduction graphで導出する。全test再実行はcorrectness baselineにはなっても、増分性の証拠にはならない。

### 6. Release qualification test

対象profileごとにsupported platform、minimum OS、runtime外部契約、install／relocation、upgrade、rollback、provenance、signature／digest、uninstallをexact release candidateで検証する。

## Controlとobservability

test可能性には少なくとも次が必要である。

- 明示input、seed、clock、locale、timezone、environment、filesystem、network policy
- stdin、signal、callback、dynamic dependency、failure、resource limitの制御
- exit status、signal、stdout/stderr、structured event、file／process／network side effectの観測
- timeout、deadlock、crash、partial output、cleanup failureの分類
- target environmentとharness自体のidentity
- nondeterministic testの反復、分布、許容差、flake判定

全てをproduct APIへ露出する必要はない。外部controller、debug/test profile、link-time hook、generated adapterを使い分ける。ただしtest-only surfaceがproduction behaviorを変える場合、そのtestはproduction artifactの証拠として範囲を限定する。

## 枝刈りとtest root

release binaryではtest codeは通常deadである。しかしtest buildではtest entry、harness callback、coverage hook、fault injection pointがrootになる。profileごとにroot setを分け、release rootから到達不能なtest-only依存を除去する一方、test profileの必要codeをproduction pruningで消さない。

```text
Production roots -> production artifact
Test roots ------> subject relation + harness/test artifacts
                      |
                      +-> exact production artifact black-box execution
```

## M0〜M4への適用

- **M0:** `TestContract`、subject identity、control／observation、target execution、raw evidenceのschemaを固定する。
- **M1:** mixed Cargo/Nimble/C/C++のexact production binaryをclean環境でtestし、cross-language conformance、ABI、negative dependency、pruning equivalenceを同じharness contractで検証する。
- **M2:** representative projectから反例を増やし、test selectionとenvironment matrixをboundedにする。
- **M3:** stage0／stage1／stage2それぞれのexact producer lineageとconformanceをtestし、外部compiler生成物の混入を拒否する。
- **M4:** release candidateのinstall、relocation、upgrade、rollback、runtime contract、provenanceを対象profileでtestする。

Lane C内部の進行は`C0 test contract`、`C1 exact native artifact harness`、`C2 semantic/ABI/failure coverage`、`C3 incremental/distributed test selection`、`C4 self-host/release qualification`とする。各C milestoneは対応する共有M gateの必要条件だが、それだけでM gateを通過しない。

## 現在の実装との差

repositoryにはRust unit／integration test、Nim planning-kernel test、CLI process execution、exit/stdout/stderr capture、Run evidenceがある。これらは有用な部品だが、現時点では次が欠ける。

- requested artifactとしての`TestContract`／`TestSubject`／`HarnessExecutable`
- production artifact digestとtest resultの必須binding
- test-only dependencyとproduction dependencyの型付き分離
- source／IR／ABI／runtime変更からのtest selection
- cross-target execution capabilityとunsupported結果
- release qualificationまで共通に使えるartifact test evidence

## 直接的な受入条件

1. production resolver／planner／compiler／linkerがsubjectとharnessを計画し、test専用validatorを正本にしない。
2. exact production artifact digestを対象にしたtestが少なくとも一つ成功する。
3. instrumented artifactの結果は別identityとして記録され、production合格へ自動昇格しない。
4. Rust/Nim/C/C++境界を実行し、ABI／symbol／runtime obligationとobservable behaviorを照合する。
5. test-only dependencyはrelease artifact closureへ漏れない。
6. 意図的に壊した外部依存を、hostから偶然取得せず期待段階で拒否する。
7. subject変更とharness変更のinvalidationを区別し、再build／再test集合を説明する。
8. target上で実行できない場合は`unsupported`とし、compile成功やemulationをnative test成功と偽らない。

## 現時点の主張

> Testability is an artifact property. LAMINARIAの成果物は、exact artifact identityに結び付いたtest contract、harness、target environment、raw evidenceとともに初めて価値を持つ。

まだ「全てのbinaryが完全にwhite-box test可能」「test harnessがcorrectnessを証明する」とは主張しない。testできる範囲、観測不能領域、保守的仮定、未対応environmentを構造化して示す。

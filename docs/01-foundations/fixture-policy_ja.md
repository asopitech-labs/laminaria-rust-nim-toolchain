# Fixture Policy

## 1. 目的と権威

本ポリシーは、LAMINARIAでfixtureが何を固定してよいか、何を固定してはいけないか、
fixtureを使うtestが何を証明できるかを定める。実装挙動については、production
implementationを直接実行するtestが正本である。fixtureはそのtestへ与える入力、
初期状態、反例、またはworkloadであり、独立した仕様実装ではない。

本ポリシーは[研究プログラム](research-program_ja.md)、
[独自コンパイラ責務契約](compiler-ownership-contract_ja.md)、`AGENTS.md`と併せて適用する。

## 2. 用語を分離する

| 種類 | 責務 | 正本 |
| --- | --- | --- |
| production implementation | parser、resolver、compiler、planner、executor等の実挙動 | production codeとその直接実行結果 |
| configuration / lock | 選択、version、SHA、feature、policy input等の宣言 | 当該config/lock file |
| documentation | 意図、契約、手順、制約の説明 | 当該文書。実装挙動の証明ではない |
| fixture | production pathへ与える固定入力、初期状態、反例、workload | fixture自身は入力の正本だけを担う |
| oracle | 観測結果を判定する、fixtureから独立した理由付きproperty | direct testのassertionまたは外部標準 |
| evidence | 実行から得たraw observation、artifact、measurement | subject/run/environment identity付き記録 |
| reference project | 外部実装を読むため固定したsource revision | `reference-projects.lock.json` |

configや文書はfixtureではない。configの宣言値をtestへ再列挙して「固定」すると、
二つ目の正本ができる。文書の文章を機械的に複製した期待値も実装のcorrectnessを証明しない。

## 3. Fixtureで固定してよいもの

fixtureは、次のいずれかの独立した目的があり、production consumerが直接読む場合に限る。

### 3.1 最小入力

- parser、resolver、frontend、compiler、linker、CLIへ渡すsource、manifest、byte列。
- 境界条件を起こす最小project構造、symbol、ABI shape、filesystem layout。
- 実際のuser inputまたは外部protocolを縮小した再現入力。

固定するのは**入力**であり、production implementationの内部graphや中間結果の手書きcopyではない。

### 3.2 初期状態とscenario control

- cold/warm/no-op、編集前後、欠落dependency、resource limit等を再現する初期状態。
- random/property/fuzz failureのseed、縮小済みcounterexample、必要な操作列。
- clock、environment、target、toolchain等、結果へ意味的に影響する制御値。

状態は再作成可能であり、どの操作が状態を作るかを明示する。cache directoryや生成物を
理由なく丸ごとcommitしない。

### 3.3 外部に由来するconformance data

- 標準仕様が定めたtest vector、wire bytes、diagnostic category、ABI定数。
- upstreamが公開した互換性corpusや、実障害から得た入力。

出典、version、license、取得または生成方法を残す。同じproduction implementationが
出力した値を、そのまま正解として取り込まない。

### 3.4 研究workload

- wide graph、critical path、boundary-heavy、incremental edit等、測りたい構造を持つproject。
- 比較する全方式へ同じ意味的需要を与えるsource/workload。

workloadは性能値そのものを固定しない。measurement resultはenvironmentとrun identityを持つ
evidenceであり、fixtureとは分離する。

## 4. Fixtureで固定してはいけないもの

以下をfixtureまたはtest内へ複製して第二の正本にしてはならない。

1. config/lockに既にあるproject名、件数、SHA、feature一覧、個別属性の完全なcopy。
2. production implementationから機械的に得たgraph、plan、IR、JSONを、独立oracleなしに
   golden resultとして固定したもの。
3. production codeと同じalgorithmや判定表を別validatorへ書き直したもの。
4. 文書のchecklistをそのままmachine-readable catalogへ転記し、そのcatalogだけを検査するtest。
5. mockだけが消費し、実際のproduction boundaryへ一度も到達しない入力。
6. path、timestamp、unordered ID、compiler wording等、本質でない揺れを含む巨大snapshot。
7. target/cache/generated binaryを、「再生成が面倒」という理由だけでcommitしたもの。
8. fixtureの内部整合性しか検査しないfixture-only validatorと、そのvalidatorだけのunit test。

「fixtureと実装を同じ変更で更新したらtestが通る」は独立保証ではない。変更前の実装に対して
新fixtureが失敗する、または既知faultを注入するとdirect testが失敗することを確認できなければ、
そのfixtureは回帰検出能力を持たない可能性が高い。

## 5. Oracleとexpected value

expected valueを固定できるのは、その正しさがproduction implementationから独立して説明できる場合だけである。

許容例:

- 標準仕様のtest vector。
- 小さく手計算でき、計算根拠をtest内で説明できる値。
- 独立したreference implementationとのdifferential relation。
- 「二方式の結果が等しい」「未要求actionは0」「symbolが一意に解決する」等のproperty。
- 実障害のfailure inputと、修正後に守るべき外部observable behavior。

不許容例:

- 現在の実装を一度実行して得た出力を、そのまま`expected.json`へ保存する。
- snapshot差分を読まずにoverwriteする。
- 同じagentがfixture、validator、validator testを同時更新し、整合したことだけを成功とする。

format全体のsnapshotより、意味のあるpropertyを直接assertする。protocol互換性などbyte-for-byte
identity自体が要求の場合だけ、最小snapshot/goldenを使い、変動fieldを除外し、出典を記録する。

## 6. Manifest、schema、validator

fixture manifestは、production runtimeに独立したconsumerがある場合、またはscenario inputを
宣言する必要がある場合に限り置ける。そのschemaをproduction parserが読むなら、testはその
production parser/CLIを直接実行する。

- schema validationはproduction parserの責務とする。
- parser testはtest内で最小のvalid/invalid inputを構築し、genericな挙動を検査する。
- checked-in configの全値をtestへ再列挙しない。
- fixture専用validatorを正本にしない。
- lintはformattingや危険なpath等の補助検査には使えるが、production correctnessの証拠にしない。

## 7. Fixture category

追加時に次のcategoryを一つ指定する。複数目的を一つのfixtureへ隠さない。

| category | 固定するもの | 主な判定 |
| --- | --- | --- |
| `conformance-input` | 外部仕様由来の入力 | production outputが仕様propertyを満たす |
| `regression-counterexample` | 過去failureの最小入力/操作 | production pathで再発しない |
| `scenario-state` | cold/warm/edit/missing dependency等の状態 | action/result/side effectのproperty |
| `fuzz-corpus` | seed/corpus/reduced case | crash/UB/oracle mismatchの再現 |
| `benchmark-workload` | 意味とgraph shapeが固定された仕事 | correctnessを保った測定値。閾値は別policy |
| `artifact-subject` | 検査対象そのものとなるbinary/object | digest、producer、target、provenance付きの直接検査 |

reference projectのsource pinやtoolchain lockはfixture categoryへ入れず、それぞれのlockを正本にする。

## 8. 追加・更新・削除の手順

fixtureを追加または変更するcommitは、少なくとも次を説明する。

1. categoryと、反証したいproduction claim。
2. fixtureを直接消費するproduction entry point/test command。
3. oracleの独立した根拠。
4. source、generator、seed、toolchain、license等のprovenance。
5. 最小化した範囲と、固定しなかったderived/unstable field。
6. 変更前またはseeded faultに対してtestが失敗する理由。

期待値変更はfixture更新へ埋め込まず、behavior contractを変えるdecisionとしてreviewする。
consumerのないfixture、同じclaimを重複するfixture、production pathを通らないfixtureは削除候補とする。

## 9. Review checklist

- これは入力か、それとも実装が生成した答えのcopyか。
- 独立したruntime consumerがあるか。
- production entry pointを直接通るか。
- oracleは実装から独立して説明できるか。
- config、文書、別fixtureとの二重管理になっていないか。
- 最小の反例/workloadか。
- test artifactとproduction artifactのidentityを混同していないか。
- updateを自動承認していないか。
- fixtureを削除しても同じclaimを直接testできるなら、fixtureは本当に必要か。

一つでも説明できない場合、fixtureを追加する前にdirect testまたはproduction interfaceを改善する。

## 10. `reference-projects.lock.json`への適用

`reference-projects.lock.json`はfixtureではなく、reference source identityの宣言である。
project名、件数、URL、SHA、filesystem属性はlockだけを正本とする。

`scripts/reference_projects.py`のtestは、一時的なlocal Git repositoryと一時lockを使い、任意の
宣言を正確・安全に消費するproduction utilityの挙動を検査する。checked-in lockの19件を
testへ再列挙してはならない。lockの変更理由と現在の内容は文書で説明できるが、その説明を
第二の実行仕様にはしない。

## 11. 既存fixtureの扱い

本ポリシーの導入は、既存`fixtures/`がすべて適合済みであることを意味しない。各fixtureは
触れる時点、またはmilestone evidenceへ採用する時点でcategory、consumer、oracle、provenanceを
監査する。不適合fixtureの存在を、新しいfixtureの重複管理を正当化するprecedentにしない。

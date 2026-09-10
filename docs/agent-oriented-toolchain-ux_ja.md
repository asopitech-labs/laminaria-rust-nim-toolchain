# Agent-Oriented Toolchain UX / エージェント指向ツールチェーンUX研究方針

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

通常のtarget profileはLAMINARIA compiler revision、対応Rust/Nim言語契約、IR/変換revision、target/runtime要件、scheduler・資源方針を選ぶ。以下の既存コンパイラversion行列と外部build試行は、明示的に別の比較/bootstrap profileの研究である。その成功で本コンパイルprofileを認定しない。LAMINARIA未対応は診断して停止し、rustc/Nimへのfallbackにしない。

## 独自コンパイラprofileの本経路と完了条件

本経路のcandidateは `LAMINARIA revision × Rust/Nim言語契約 × IR/変換revision × target/runtime × resource policy` である。Cargo/Nimble resolverのidentityは依存入力として別に記録する。既存compilerを選ぶために本経路を分岐しない。

Rust-only・Nim-only・混成を同じ独自compilerで処理し、未対応構文・target・依存は外部compile前に構造化診断で拒否する。bootstrap/referenceの認定を本経路のrecommendedへ昇格できないこと、profile/IR revision変更が正しく無効化されることをテストする。全機能の認定が揃うまで#25/#3/#6/#8の小さな実装を待たせない。

## 以下の既存ツール行列の適用範囲：比較・bootstrap profile

以下にあるrustc/Nim/LLVMのversion・release・compatibility行列、設定例、外部build試行とその成功条件は**比較・bootstrap profileに限定する**。共通のpruning・説明・探索budget原則は独自profileでも使うが、既存コンパイラを本経路のexecution engineへ昇格させない。例は実装済み／認定済みprofile一覧ではない。

## 目的

LAMINARIAでは、性能、cache、scheduler、compiler white-boxingだけでなく、**toolchain selectionとconfigurationのUXそのものを研究・実装対象**とする。

Rust/Nim compiler version、backend、target、linker、LTO、WASM composition等の内部組合せ空間は広く受け入れる。一方、その広い空間を人間やコーディングエージェントへそのまま露出し、失敗する組合せを一つずつ実行して探索させてはならない。

中心原則:

> Resolve before retrying. Explain before experimenting.

LAMINARIAの価値は「組合せをたくさん試せること」ではなく、既知の制約、検証済みprofile、compatibility evidence、negative knowledgeを使って**実行前に探索空間を縮め、最も妥当なplanを直接提示できること**にある。

本方針を評価可能な研究仮説・探索budget・UX metrics・失敗条件へ落とした正式なResearch Contractは `toolchain-ux-research-contract_ja.md` を参照する。

## 1. UXを独立した成功条件として扱う

通常のbuild toolでは、内部機能が正しくてもユーザーが大量のflag、compiler version、linker、target設定を手で組み合わせなければ使えない場合がある。

LAMINARIAではそれを不十分とみなす。

成功条件には少なくとも次を含める。

- 通常ユーザーはexact compiler/backend/linker組合せを手動解決しなくてよい;
- coding agentは大量のbuild失敗を通じて正解を発見しなくてよい;
- invalid combinationは可能な限りexecution前にreject/pruneされる;
- recommended pathは構造化された根拠を持つ;
- advanced userは必要なときだけ下位dimensionへ降りられる;
- custom configurationでも既知の非互換条件は説明付きで早期rejectする。

## 2. Agent Trial-and-Error Explosionを研究対象にする

コーディングエージェントにとって次のような探索は高コストである。

```text
Rust A + Nim X + LLVM P -> fail
Rust A + Nim X + LLVM Q -> fail
Rust A + Nim Y + LLVM P -> fail
Rust B + Nim X + LLVM P -> fail
...
```

この方法は、build時間だけでなく、log parsing、context消費、仮説生成、再編集、再実行を繰り返すためagent session全体のコストを増幅する。

LAMINARIAは次の順序へ変える。

```text
Project Requirements
  + Host / Target
  + Requested Intent
  + Package Constraints
  + Toolchain Capability Matrix
  + Known Compatibility / Incompatibility
  + Validated Profile Evidence
        ↓
Constraint Resolution / Pruning
        ↓
Ranked Viable Plans
        ↓
Recommended Plan + Explanation
        ↓
Execution
```

実行は探索の主要手段ではなく、**選定済みplanの検証・実行手段**とする。

## 3. Three Surfaces: Simple / Guided / Expert

### Simple

通常利用者と通常のcoding agent向け。

```text
recommended
latest-validated
long-term
preview
```

profileとintentだけでplanを決定できる。

### Guided

一部の要求だけを指定する。

例:

```text
profile = recommended
target = wasm-component
priority = fast-iteration
```

LAMINARIAは残りのdimensionを検証済み候補から解決する。

### Expert

compiler version、backend route、LTO、linker等を直接constraintする。

ただしexpert modeでも既知の非互換性を無視して無限try-and-errorへ移行しない。

## 4. ProfileはAgent Search PriorではなくEvidence-backed Constraintである

`recommended` profileは「最初に試す候補」というだけではない。

それはMeasurement Spineで検証されたbundle集合であり、agentに対して次を与える。

- exact resolved bundle;
- qualification scope;
- known limitations;
- compatible target/backend routes;
- measured performance/resource characteristics;
- known failure signatures;
- promotion/demotion history。

したがってagentは、根拠のない組合せ探索を始める前にprofile evidenceを利用できる。

## 5. Negative Knowledgeを保存する

成功した組合せだけでなく、**失敗・非互換・未対応の知識**を再利用する。

例:

```text
candidate rejected because:
  rustc capability missing
  target unsupported
  linker/object model incompatible
  Nim runtime obligation unresolved
  profile qualification failed on this host/target
  artifact compatibility unknown
```

この情報はstructured reason codeとして保存し、同一条件を別agent/sessionが再試行しないようにする。

ただしtemporary/environment-specific failureとsemantic incompatibilityを区別する。過去の一時的失敗を永続的non-compatible ruleへ誤昇格させない。

## 6. Static Resolution → Cheap Probe → Execution

探索コストを段階化する。

### Stage 1 — Static/Recorded Resolution

processを起動せずに判断可能なものを先に処理する。

- `rust-version` / edition;
- compiler/toolchain capability;
- known target/backend support;
- known profile qualification;
- known artifact compatibility;
- required component availability;
- known incompatible constraints。

### Stage 2 — Cheap Capability Probe

必要な場合のみ、version query、target listing、linker capability等の低コストprobeを行う。

### Stage 3 — Qualification Lookup

Measurement Spineに既存evidenceがあるか確認する。

### Stage 4 — Execution

十分に絞り込まれたplanだけを実行する。

未知領域の研究では複数candidate実行を許すが、通常ユーザー/agent pathと明確に区別する。

## 7. Ranked Planを返す

resolverは単に`success/failure`を返すのではなく、必要に応じて少数のranked planを返す。

例:

```text
1. recommended / fully validated
2. latest-validated / fully validated, newer Rust
3. preview / validated-with-limitations, Nimony
```

ranking要素候補:

- qualification level;
- constraint satisfaction;
- known failure risk;
- host/target coverage;
- performance/resource evidence;
- freshness;
- migration cost;
- requested intent。

ranking policyは説明可能でなければならない。

## 8. Agent-facing Structured Interface

coding agentがhuman-readable logを解析して状態を推測しないよう、machine-readable interfaceを第一級とする。

候補:

```text
laminaria toolchain resolve --json
laminaria toolchain profiles --json
laminaria explain-toolchain-selection --json
laminaria explain-profile-qualification --json
laminaria explain-candidate-rejection --json
laminaria plan --json
```

出力には最低限次を含める。

```text
requested intent/constraints
selected profile + revision
exact resolved toolchains
selected backend/target route
qualification status/scope
rejected candidate count
pruned candidate count
reason codes
known limitations
fallbacks
confidence/evidence class
whether execution/probe is still required
```

## 9. Bounded Exploration Policy

通常のagent-oriented pathでは探索budgetを持つ。

例:

- fully validated candidateがある場合はunvalidated candidateを自動試行しない;
- known-incompatible candidateをprocess executionしない;
- 同一reasonで失敗したequivalent candidateをまとめてpruneする;
- fallbackは明示されたpolicyに従う;
- 探索budget超過時は「さらに試行」ではなく未解決constraintと次の選択肢を返す。

研究modeでは広範囲探索を許可できるが、通常UXとは別modeとする。

## 10. Agent Context Economy

coding agentにとってbuild log量もresourceである。

LAMINARIAは、失敗ごとに長大なcompiler outputを返すだけでなく、構造化summaryを提供する。

例:

```text
selection_failed:
  reason = target_backend_incompatible
  rejected = 47 equivalent variants
  nearest_validated = profile:recommended@rev
  required_change = target -> wasm32-core
```

これによりagentは数十回の試行履歴をcontextへ保持せずに済む。

詳細log/raw evidenceは必要なときだけ参照できる。

## 11. UX Metrics

UXを目的とする以上、成功を主観だけで評価しない。

少なくとも次を計測候補とする。

- first viable planまでに実行したbuild/process回数;
- failed toolchain attempt数;
- staticにpruneできたcandidate数/割合;
- known negative evidenceにより回避した再試行数;
- profile選択からsuccessful artifactまでのwall time;
- resolver/planner自身のlatency;
- candidate explored/pruned/merged counts;
- agentへ返したlog/structured output size;
- fallback回数;
- validated profileからの成功率;
- custom/preview pathでのfailure分類率;
- 同一問題を別sessionで再探索した回数。

目標は「solverが高速」だけではなく、**不要なexternal compilation attemptsを減らすこと**である。

## 12. Coding Agent Acceptance Scenarios

最低限、次をfixture化する。

1. `recommended`だけでmixed Rust/Nim native buildが成功する;
2. latest upstreamの組合せが未検証の場合、agentがblind retryせず`latest-validated`を選べる;
3. `rust-version`を満たさない候補をbuild前にrejectする;
4. unsupported backend/target組合せをbuild前にpruneする;
5. overrideでprofile validationが失われたことをstructured outputで説明する;
6. 50以上のsynthetic candidateがある条件で、少数candidateへpruneしてから実行する;
7. known incompatible combinationを別Run/sessionで再実行しない;
8. fully validated candidateが無い場合、validation gapとranked alternativesを返し、無限retryしない;
9. research modeでは広い探索を明示的に有効化できる。

## 13. 成功条件

LAMINARIAのUX研究は、CLIが短いことだけを意味しない。

成功条件:

1. 通常ユーザーがtoolchain組合せを手動探索せずに利用できる;
2. coding agentが組合せ爆発をexternal buildのtrial-and-errorで解かない;
3. validated profile、constraints、negative knowledgeによりexecution前に大半の無効候補をpruneできる;
4. resolverが選択・棄却理由をmachine-readableに説明できる;
5. expert userは必要なら全dimensionへ降りられる;
6. 探索を絞ることが未知の研究組合せを禁止することにはならない;
7. UX metricsでfailed attempt、time-to-plan、pruned variants、log/context量を継続評価できる。

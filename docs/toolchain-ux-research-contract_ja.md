# Toolchain UX Research Contract / ツールチェーンUX研究契約

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

通常のtarget profileはLAMINARIA compiler revision、対応Rust/Nim言語契約、IR/変換revision、target/runtime要件、scheduler・資源方針を選ぶ。以下の既存コンパイラversion行列と外部build試行は、明示的に別の比較/bootstrap profileの研究である。その成功で本コンパイルprofileを認定しない。LAMINARIA未対応は診断して停止し、rustc/Nimへのfallbackにしない。

## 独自コンパイラprofileの本経路と完了条件

本経路のcandidateは `LAMINARIA revision × Rust/Nim言語契約 × IR/変換revision × target/runtime × resource policy` である。Cargo/Nimble resolverのidentityは依存入力として別に記録する。既存compilerを選ぶために本経路を分岐しない。

Rust-only・Nim-only・混成を同じ独自compilerで処理し、未対応構文・target・依存は外部compile前に構造化診断で拒否する。bootstrap/referenceの認定を本経路のrecommendedへ昇格できないこと、profile/IR revision変更が正しく無効化されることをテストする。全機能の認定が揃うまで#25/#3/#6/#8の小さな実装を待たせない。

## 以下の既存ツール行列の適用範囲：比較・bootstrap profile

以下にあるrustc/Nim/LLVMのversion・release・compatibility行列、設定例、外部build試行とその成功条件は**比較・bootstrap profileに限定する**。共通のpruning・説明・探索budget原則は独自profileでも使うが、既存コンパイラを本経路のexecution engineへ昇格させない。例は実装済み／認定済みprofile一覧ではない。

## 目的

LAMINARIAでは、compiler pipeline、cache、scheduler、backend white-boxing、性能だけでなく、**複雑なtoolchain空間を人間とコーディングエージェントが安全かつ少ない探索コストで扱えること自体**を研究成果として扱う。

LAMINARIA内部はRust/Nim compiler version、backend、target、linker、LTO、WASM composition、runtime等の広い組合せを受け入れる。一方、その自由度を利用者へそのまま露出し、正しい組合せを外部compiler/buildのtrial-and-errorで発見させる設計は失敗とみなす。

この文書は `agent-oriented-toolchain-ux_ja.md` と `validated-toolchain-profiles_ja.md` を、検証可能な研究仮説・評価指標・失敗条件へ落とすResearch Contractである。

中心原則:

> Resolve before retrying. Explain before experimenting.

> Broad internal freedom must not become external trial-and-error cost.

## 1. 研究対象としてのUX

LAMINARIAにおけるUXは、CLIの短さやflag数の少なさだけではない。

研究対象は次の4層である。

1. **Selection UX** — 適切なtoolchain/profile/routeをどう選ぶか。
2. **Configuration UX** — Simple / Guided / Advanced / Expertへどう段階的に詳細化するか。
3. **Failure UX** — 非互換・未検証・fallback・opaque regionをどう説明し、次の選択肢へ導くか。
4. **Agent Search UX** — coding agentが組合せ爆発をbuild失敗の反復で解かずに済むか。

したがって、最終artifactが正しく生成できても、そこへ到達するために人間またはagentが大量のcandidateを手動/自動でtrial-and-errorした場合、UX研究としては成功とみなさない。

## 2. 内部自由度と外部探索を分離する

内部モデルは広いcandidate spaceを維持する。

```text
Rust versions
× Nim versions / Nim 2 / Nimony
× backend engine
× target
× linker
× LTO
× post-link optimizer
× Wasm composition
× runtime
× feature/profile
× artifact boundary
...
```

通常UXはこの直積を直接露出しない。

```text
Project Requirements
+ Host / Target
+ Requested Intent
+ Package Constraints
+ Toolchain Capability Matrix
+ Validated Profile Evidence
+ Known Compatibility / Incompatibility
+ Negative Knowledge
        ↓
Constraint Resolution / Pruning
        ↓
Ranked Viable Plans
        ↓
Recommended Plan + Explanation
        ↓
Execution
```

**executionは探索の主要手段ではない。**

未知の組合せを広く試す行為はResearch Modeとして明示的に分離する。

## 3. 研究仮説

### HUX-1 — Validated Profile Reduction

検証済みprofileとqualification evidenceを先に利用することで、同じrequested artifactへ到達するためのexternal compiler/build試行数を、blind trial-and-errorより大幅に削減できる。

### HUX-2 — Constraint-first Pruning

`rust-version`、edition、target/backend capability、artifact compatibility、known incompatibility等をexecution前に評価することで、無効candidateの大半をcompiler process起動前に除去できる。

### HUX-3 — Negative Knowledge Reuse

過去に証明された非互換・unsupported・qualification failureをidentity付きstructured evidenceとして保存すれば、別Run・別session・別coding agentが同じ失敗を再発見する割合を削減できる。

### HUX-4 — Progressive Disclosure

`recommended`等のprofile → intent preset → advanced override → expert constraintsという段階的設定により、通常利用では設定負担を小さくしつつ、expert/research利用では内部variant spaceへの制御能力を失わずに済む。

### HUX-5 — Structured Explanation / Context Economy

選択・棄却・ranking理由をmachine-readable summaryとして返すことで、coding agentが大量のcompiler logと試行履歴をcontextへ保持する必要を減らせる。

### HUX-6 — Bounded Exploration

通常agent modeに探索budgetとstop conditionを持たせれば、解決不能/未検証条件でも無限retryへ入らず、未解決constraintとranked alternativesを返して終了できる。

### HUX-7 — Recommendation Without Capability Loss

通常UXをvalidated profile中心へ絞っても、Custom/Expert/Research Modeを通じて新しいcompiler/backend/target組合せを研究・利用する能力を維持できる。

## 4. Exploration Budgetを明示する

通常のcoding-agent pathは「成功するまで試す」ではなく、明示的なbudgetを持つ。

budgetは少なくとも次を独立して扱えるようにする。

```text
max external build attempts
max expensive capability probes
max unvalidated candidate executions
max fallback transitions
max planner wall time
max candidate expansion count
```

ただし固定値を全環境へ一律適用することを目的としない。profile qualification、requested intent、candidate cost、environment classに応じてpolicy化できることが重要である。

### Stop Conditions

次の場合は追加buildを続けず、説明へ切り替える。

- fully validated candidateが既に存在する;
- candidateがknown-incompatibleである;
- equivalent candidate群が同一reasonでrejectできる;
- remaining candidateがすべてunvalidatedで通常modeのpolicy外である;
- exploration budgetを超過した;
- artifact compatibilityがunknownでfail-closed policyに該当する;
- required capabilityを満たすtoolchainが存在しない。

終了結果は単なる`failed`ではなく、少なくとも次を含む。

```text
unresolved constraints
best validated alternative
ranked next candidates
validation gaps
required user/agent decision
research-mode option
```

## 5. Negative Knowledge Contract

negative knowledgeは単なる失敗ログではなく、再利用可能な判定材料として保存する。

最低限:

```text
reason_code
reason_class
input/toolchain/profile/host/target identity scope
evidence reference
first observed / last confirmed
confidence / evidence class
transient-or-semantic classification
invalidated-by conditions
```

reason class例:

```text
semantic_incompatible
capability_missing
target_unsupported
backend_linker_incompatible
runtime_contract_unresolved
artifact_compatibility_unknown
qualification_failed
transient_environment_failure
tool_failure_unknown
```

### Fail-closed と過剰pruningの両方を防ぐ

- semantic incompatibilityやunknown ABI/artifact compatibilityは安全側にrejectする。
- 一時的network/process/host failureを永久的incompatibilityへ昇格させない。
- toolchain/profile更新で前提が変わったnegative evidenceは再評価可能にする。

## 6. Profileは探索順ではなくEvidence Contract

`recommended`は「最初に試すcandidate」という意味ではない。

profile revisionは少なくとも次を持つ。

```text
exact ToolchainFingerprint bundle
qualification scope
host/target coverage
known limitations
known failures
performance/resource evidence
supported backend/target routes
promotion/demotion history
```

したがってfully validated profileがrequested constraintsを満たす場合、通常modeではそのprofileを直接選び、より不確実なcandidateを先に試さない。

profileに対してoverrideが入った場合は、元profileのqualificationをそのまま継承せず、変更dimensionに応じてqualification statusを再評価する。

## 7. Human UXとAgent UXは同じresolverを共有する

人間向けとcoding agent向けに別々のcompatibility logicを実装しない。

共通resolver / plannerが同じselection evidenceを生成し、presentation layerだけを変える。

### Human-facing

```text
Recommended
Latest Validated
Long-Term
Preview
Custom
```

必要に応じて理由・exact versions・known limitationsを展開する。

### Agent-facing

```text
laminaria toolchain resolve --json
laminaria plan --json
laminaria explain-toolchain-selection --json
laminaria explain-candidate-rejection --json
```

同じdecision graphからstructured evidenceを取得する。

## 8. UX Metrics

UXは継続計測する。

### Exploration Cost

- first viable planまでのexternal build/process attempt数;
- failed toolchain attempt数;
- expensive probe数;
- unvalidated execution数;
- fallback transition数;
- time-to-first-viable-plan;
- request-to-successful-artifact wall time。

### Search Reduction

- theoretical candidate count;
- explored candidate count;
- statically pruned count;
- negative-evidence-pruned count;
- profile qualificationで除外/選択できたcount;
- merged equivalent-state count;
- real compiler executionまで到達したcandidate数;
- pruning ratio。

### Agent Context Economy

- agentへ返したstructured output bytes/tokens;
- raw compiler log参照量;
- rejected candidateごとのlog生成量;
-同一incompatibilityを別sessionで再探索した回数。

### Recommendation Quality

- `recommended`からのsuccess rate;
- `latest-validated`からのsuccess rate;
- validated profile内でのunexpected failure rate;
- profile rollback/demotion数;
- recommended選択からfallbackした割合;
- validation gap説明後の再試行回数。

## 9. Baseline比較

UX改善はLAMINARIA単独値だけで評価しない。

最低限、同じfixtureについて次を比較する。

### Baseline A — Manual / Blind Agent

version/backend設定を知らないagentが通常のbuild errorを読み、設定変更→buildを繰り返す。

### Baseline B — Static Constraint Only

package metadataとtoolchain capabilityのみでpruneするが、profile qualification/negative knowledgeを使わない。

### LAMINARIA

Validated Profile + constraints + capability matrix + negative knowledge + bounded explorationを利用する。

比較指標は成功までのexternal attempts、wall time、CPU/I/O、log/context量、候補探索数とする。

## 10. 失敗条件

次はUX研究として失敗である。

- 最終的にはbuild成功するが、agentが多数のcandidateをexternal buildして発見した;
-内部plannerはlazyだが、未解決candidateを全部agentへ返して外部でtryさせた;
-`recommended`が実質的に「最初に試すだけ」で、失敗後の探索を抑制しない;
-known incompatibilityがあるのに同一identityで再buildした;
-validation gapがあるのにvalidated badgeを維持した;
-探索budget超過後も自動retryを継続した;
-human UIは簡単だがagent APIには構造化selection/rejection reasonがない;
-agent APIは存在するが、内部ではhuman-readable compiler logの解析に依存する;
-設定を簡単にするためCustom/Research Modeの能力を削除した。

## 11. Acceptance Workloads

最低限、次をversioned fixtureとして持つ。

1. mixed Rust/Nim native buildを`recommended`だけで解決する。
2. latest upstreamが未検証の場合、blind executionせず`latest-validated`へ解決する。
3. `rust-version`不適合candidateをprocess起動前にrejectする。
4. backend/target/linker不適合をprocess起動前にrejectする。
5. 50以上のsynthetic candidateを少数のviable planへ縮約する。
6. 一度証明した非互換candidateを別Run/sessionで再実行しない。
7. transient failureは再試行可能だがsemantic incompatibilityは再試行しない。
8. profile overrideによるqualification低下を説明する。
9. fully validated candidateが無い場合、budget内で停止してvalidation gapとranked alternativesを返す。
10. Research Modeでは同じcandidate spaceを明示的に広く探索できる。

## 12. 関連研究トラック

この研究は独立solverを作るものではない。

- #8 Variant Explosion Control — candidate expansion / pruning / merging。
- #10 Explainability — selection/rejection/ranking reason schema。
- #11 / #18–#21 Measurement Spine — profile qualificationとUX metricsの計測基盤。
- #22 Multi-version Toolchains — broad internal candidate / compatibility model。
- #23 Validated Toolchain Profiles — user-facing known-good bundle policy。
- #24 Agent-Oriented Planning UX — bounded explorationとcoding-agent acceptance。

同じNim Planning KernelとRust Runtimeの上に統合する。

## 13. 完了条件

本研究は、次が再現可能なevidenceとして示された場合に成立したとみなす。

1. 通常ユーザーはexact toolchain組合せを手動探索せずにartifactへ到達できる。
2. coding agentは組合せ爆発をexternal build failureの反復で解かない。
3. large candidate spaceの大半をexecution前にprune/mergeできる。
4. known negative knowledgeが別Run/sessionで再利用される。
5. normal modeの探索はboundedであり、未解決時に説明とalternativesを返して停止する。
6. selection/rejection/rankingはmachine-readableかつevidence-backedである。
7. profile/intent中心の簡単なUXとexpert/researchの詳細制御を同じresolver上で両立できる。
8. baseline trial-and-errorに対してexternal attempts、wall time、resource消費、agent context量の少なくとも一部で測定可能な改善を示す。
9. UX上の失敗が性能regressionと同様に設計修正を要求するquality gateとして扱われる。

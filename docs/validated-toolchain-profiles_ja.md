# Validated Toolchain Profiles / 検証済みツールチェーンプロファイル

## 目的

LAMINARIA内部はRust/Nim compiler version、backend、target、linker、post-link tool等の広い組合せ空間を受け入れ、constraintで解決する。一方、通常のユーザーへその組合せ空間を直接露出してはいけない。

ユーザー向けには、LAMINARIAが実際に構築・実行・計測し、互換性と実行経路を確認した **Validated Toolchain Profile** を第一級の入口として提供する。

中心原則:

> Broad candidate space internally; narrow known-good paths by default.

## 1. Candidate と Recommendation を分離する

LAMINARIAが解決可能なtoolchain combinationと、ユーザーへ推奨するcombinationは別の集合である。

```text
Candidate Variant Space
  Rust versions × Nim versions × backend × target × linker × optimizer × runtime ...
        ↓ constraints
Compatible Candidates
        ↓ qualification / evidence
Validated Profiles
        ↓ user-facing selection
Resolved Exact Toolchain Bundle
```

「constraint上は成立する」ことを「LAMINARIAとして推奨できる」ことと同一視しない。

## 2. Freshness と Validation Level を別軸にする

`latest`、`stable`、`nightly`、`LTS`等はupstream freshness/support semanticsであり、LAMINARIAがその組合せを十分検証したかどうかとは別問題である。

LAMINARIAは少なくとも次を分離する。

```text
Upstream Selection / Freshness
  latest stable
  older stable
  beta/nightly/devel
  upstream LTS where actually provided

LAMINARIA Qualification
  fully validated
  validated with limitations
  smoke-tested
  unvalidated
  rejected
```

`latest upstream`を自動的に`recommended`へ昇格させない。

Rustはstable/beta/nightlyのrelease trainを持つが、LAMINARIAが横断bundleとして使える公式Rust LTS channelを前提にしない。長期安定profileを提供する場合は **LAMINARIA Long-Term Validated** のようにLAMINARIA自身のsupport policyとして明示する。

NimについてもupstreamがLTSを明示するversionではその事実をmetadataとして利用できるが、Rust/Nim/LLVM等を組み合わせたbundle全体のlong-term保証はLAMINARIA側のqualificationである。

## 3. User-facing profile hierarchy

通常ユーザーはまずprofileだけを選べる。

### `recommended`

default。LAMINARIAが現在もっとも広く検証し、通常用途で失敗しにくいと判断したexact bundle。

優先順位:

1. compatibility/functional qualification;
2. known failureの少なさ;
3. performance regressionが許容範囲;
4. security/critical fixes;
5. upstream freshness。

必ずしもupstream最新versionとは限らない。

### `latest-validated`

Rust/Nim等のupstream stable releaseをできるだけ新しく保ちつつ、LAMINARIA qualification suiteを通過した最も新しいbundle。

`latest upstream`と異なり、未検証releaseへ自動追従しない。

### `long-term`

更新頻度を抑え、再現性と移行コストの低さを優先するLAMINARIA管理profile。

Rustに公式LTSがない場合でも使用できるが、その場合はUI/documentationで「LAMINARIA-maintained long-term profile」であることを明示する。

profileはexact toolchain bundleへ固定され、security/critical fixまたは明示したqualification policyに基づいて更新する。

### `preview`

Nimony/devel、Rust beta/nightly、新backend route等を含む可能性がある。新機能検証向け。

通常の「失敗しない」選択肢としては表示せず、validation levelと既知制限を必ず表示する。

### `custom`

詳細設定を有効にする。candidate spaceを直接constraintできる。

例:

```text
Rust exact version / channel / revision
Cargo version
Rust edition / MSRV policy
Nim generation (2 / 3-Nimony) and exact version/revision
backend engine
LLVM/Cranelift/GCC route
LTO mode
native compiler/linker
Wasm target/link/post-link/component model
runtime/memory model
optimization/debug profile
```

## 4. 段階的UI

設定面は progressive disclosure とする。

### Level 1 — Profile only

```text
Recommended
Latest Validated
Long-Term
Preview
Custom
```

通常ユーザーはここで完結できる。

### Level 2 — Intent presets

profileの中で用途を選ぶ。

候補:

```text
Default / General Development
Fast Iteration
Release / Maximum Optimization
Native
WebAssembly Core
WebAssembly Component
Compatibility-oriented
```

Intent presetはversion bundleだけでなくbackend/LTO/link/post-link等のroute selectionにconstraintを追加する。

### Level 3 — Advanced overrides

各dimensionを個別指定する。

profileを選んだ後、一部だけoverrideできる。

```text
base = recommended
rust = <exact rustc>
lto = thin
```

この場合LAMINARIAはもとのprofile qualificationをそのまま表示してはいけない。override後の組合せについてqualification evidenceを再評価し、状態を `validated`, `partially-validated`, `unvalidated` 等へ変更する。

### Level 4 — Expert graph constraints

CLI/configからvariant dimension、artifact boundary、backend route等を直接指定できる。

これは通常UIのdefault経路ではない。

## 5. Profile はmoving alias、Runはexact identity

`recommended`や`latest-validated`はmoving profileである。

実行時には必ずprofile revisionとexact bundleへ解決する。

```text
requested_profile = recommended
profile_revision = 2026-09-xx.N
resolved_bundle = {
  rustc = exact-version/revision
  cargo = exact-version
  nim = exact-version/revision
  llvm = exact-build
  linker = exact-build
  ...
}
```

Run/Action/Artifact/cache identityは`recommended`という文字列ではなくresolved exact identityを使う。

再現のためRunにはrequested profile、profile revision、resolved bundleをすべて保存する。

## 6. Qualification Evidence

Validated Profileは人手で「たぶん動く」と指定してはいけない。

profile qualificationは#11/#18–#21のMeasurement Spineを利用し、少なくとも次を証拠として持つ。

- environment classes;
- toolchain exact fingerprints;
- bootstrap success;
- representative Rust-only / Nim-only / mixed Rust+Nim builds;
- native link / C ABI baseline where relevant;
- check/build/test scenario results;
- cold/warm/no-op/incremental execution correctness;
- backend route actually executed;
- target-specific tests;
- CPU/memory/I/O/performance regressions;
- artifact/code size where relevant;
- known unsupported capabilities;
- known failure signatures。

Profile qualificationはbinary pass/failだけでなくcapability coverageを持つ。

## 7. Qualification Matrix

profileごとにscopeを持つ。

例:

```text
Profile: recommended@revision

host:
  linux-x86_64: validated
  windows-wsl2-x86_64: validated-with-limitations
  macos-arm64: validated

target:
  native: validated
  wasm32-core: validated
  wasm-component: experimental

features:
  Rust stable frontend: validated
  Nim 2 frontend: validated
  Nimony frontend: partial
  ThinLTO: validated
  DTLTO: research
```

「profile全体がvalidated」という一語でsupport範囲を曖昧にしない。

## 8. Promotion / Demotion

新しいupstream releaseが出ても即座に`recommended`を書き換えない。

概念pipeline:

```text
Upstream release discovered
  ↓
Candidate generated
  ↓
Bootstrap / smoke qualification
  ↓
Full qualification suite
  ↓
Performance/resource comparison
  ↓
Profile promotion decision
```

promotion候補:

```text
unvalidated
→ smoke-tested
→ validated-with-limitations
→ fully-validated
→ latest-validated
→ recommended (policy decision)
```

regressionやcritical issueが見つかった場合はprofileをdemoteできる。

既存Runのprofile revisionは変更しない。

## 9. `long-term` のpolicy

LAMINARIA Long-Termはupstream LTS labelの単純なunionではない。

長期profileでは少なくとも次を固定する。

- exact compiler/tool versions;
- qualification scope;
- minimum support windowまたはretirement criteria;
- permitted patch/security updates;
- compatibility break時のmigration policy;
- profile revisioning policy。

Rust upstreamに公式LTS channelがない場合は、LAMINARIAが選定したstable Rust releaseを一定期間検証・固定する。

Nim upstreamが公式LTSを提供する場合でも、Rust/LLVM/linkerとのbundle qualificationはLAMINARIAが別途行う。

## 10. Failure behavior

`recommended`等のvalidated profileで既知のqualification範囲内のbuildが失敗した場合、単なるuser errorとして扱わずLAMINARIA profile regression候補として記録できる必要がある。

`custom` / `preview` / unvalidated combinationでは、resolverは可能な限り実行を許可するが、次を明示する。

```text
validation status
missing qualification evidence
known incompatible edges
opaque compiler/backend regions
fallbacks
```

安全性・artifact compatibilityを証明できないedgeは「詳細設定だから」という理由でfail-openしない。

## 11. Explainability

ユーザーは少なくとも以下を問い合わせられる。

```text
laminaria toolchain profiles
laminaria toolchain profile recommended
laminaria explain-toolchain-selection
laminaria explain-profile-qualification
```

説明内容:

- なぜこのprofileがdefaultなのか;
- exactに何が選ばれたか;
- latest upstreamとの差;
- qualification coverage;
- known limitations;
- overrideによって何の保証が失われるか;
- なぜcandidate combinationがrejectされたか。

## 12. 成功条件

1. internal candidate spaceとuser-facing validated profileを分離できる。
2. default操作ではexact versionの組合せをユーザーが手動解決しなくてよい。
3. `recommended`, `latest-validated`, `long-term`, `preview`, `custom`を異なるpolicyとして表現できる。
4. upstream freshnessとLAMINARIA validation levelを別軸で保持する。
5. profileはexact versioned bundleへ解決され、Run/Action/Artifact identityへ伝播する。
6. profile qualificationがMeasurement Spineの再現可能なevidenceに基づく。
7. profileのoverride時にvalidation statusを再評価する。
8. expert userはprofileを越えて個別dimensionを細かくconstraintできる。
9. Rustに公式LTSがないことを隠してLAMINARIA long-term profileをupstream保証のように表示しない。
10. validated default pathでの失敗をprofile regressionとして検知・追跡できる。

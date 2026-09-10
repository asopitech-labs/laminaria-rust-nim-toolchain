# Multi-version Toolchains / 複数コンパイラバージョン対応方針

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

通常のtarget profileはLAMINARIA compiler revision、対応Rust/Nim言語契約、IR/変換revision、target/runtime要件、scheduler・資源方針を選ぶ。以下の既存コンパイラversion行列と外部build試行は、明示的に別の比較/bootstrap profileの研究である。その成功で本コンパイルprofileを認定しない。LAMINARIA未対応は診断して停止し、rustc/Nimへのfallbackにしない。

## 独自コンパイラprofileの本経路と完了条件

本経路のcandidateは `LAMINARIA revision × Rust/Nim言語契約 × IR/変換revision × target/runtime × resource policy` である。Cargo/Nimble resolverのidentityは依存入力として別に記録する。既存compilerを選ぶために本経路を分岐しない。

Rust-only・Nim-only・混成を同じ独自compilerで処理し、未対応構文・target・依存は外部compile前に構造化診断で拒否する。bootstrap/referenceの認定を本経路のrecommendedへ昇格できないこと、profile/IR revision変更が正しく無効化されることをテストする。全機能の認定が揃うまで#25/#3/#6/#8の小さな実装を待たせない。

## 以下の既存ツール行列の適用範囲：比較・bootstrap profile

以下にあるrustc/Nim/LLVMのversion・release・compatibility行列、設定例、外部build試行とその成功条件は**比較・bootstrap profileに限定する**。共通のpruning・説明・探索budget原則は独自profileでも使うが、既存コンパイラを本経路のexecution engineへ昇格させない。例は実装済み／認定済みprofile一覧ではない。

## 目的

LAMINARIAはNim 2 / Nim 3だけでなく、Rustについても複数のcompiler/toolchain versionをfirst-classに扱う。

複数version対応は単なるinstaller機能ではない。compiler versionはfrontend semantics、内部IR、backend route、bundled LLVM、metadata format、telemetry capability、cache identity、artifact compatibilityへ影響するため、LAMINARIAのVariant GraphとArtifact Graphの一部として扱う。

中心原則:

> Compiler version is a graph dimension, not an ambient machine setting.

## 1. Versionは入口だが、identityから消してはいけない

Nim 2 / Nim 3、Rustの複数versionはいずれもsource programをLAMINARIAの共通graph contractへ投影する入口である。

```text
Rust source ─→ Rust Toolchain Adapter(version/build) ─┐
                                                     ├→ Unified Program / Artifact / Action Graph
Nim source  ─→ Nim Toolchain Adapter(version/build) ─┘
```

ただし共通graphへ入った後も、producing toolchain identityはtyped metadataとして保持する。異なるcompiler versionが同じ意味・同じartifact compatibilityを持つと仮定しない。

## 2. Variant Graphへcompiler toolchain dimensionを追加する

従来のvariant spaceへ次を明示的に追加する。

```text
package
× language
× compiler toolchain
× target
× profile
× feature set
× edition/language mode
× backend route
× native compiler/linker
× artifact kind
× cross-language boundary
```

`compiler toolchain`は少なくとも次を区別する。

### Rust

- exact stable release;
- exact beta/nightly date or revision where used;
- custom/source-built rustc revision where experiments require it;
- Cargo version/build identity;
- installed component set;
- sysroot / standard library identity;
- bundled or selected LLVM/codegen backend identity;
- host triple and target components.

### Nim

- Nim 2 exact version/revision;
- Nimble identity;
- Nimony/Nim 3 exact source revision/build;
- nlvm revision where the LLVM route is used;
- selected backend/native compiler identity.

`stable`、`nightly`、`latest`のようなmoving labelだけをartifact/cache identityに使用しない。Run開始時にresolved toolchainをexact identityへ解決する。

## 3. Rust `rust-version` / edition / compiler versionを分離する

Cargo manifestの`rust-version`はpackageがサポートするminimum Rust versionを表す。LAMINARIAはこれをtoolchain selection constraintとして利用できる。

一方、次は別dimensionである。

- `rust-version`: packageの最低対応Rust version;
- Rust edition: source/language mode;
- selected rustc toolchain: 実際にcompileするcompiler;
- Cargo resolver behavior: workspace dependency resolution semantics;
- nightly feature requirement: stable toolchainでは満たせないcapability constraint。

これらを一つの`rust_version`フィールドへ潰さない。

参考: https://doc.rust-lang.org/cargo/reference/rust-version.html

## 4. Rust複数versionのdefault execution model

LAMINARIAは複数Rust toolchainを同時にinstall・discover・measureできるが、通常のCargo/Rust crate dependency graphでは、**一つのconnected Rust compilation graphに一つのselected Rust toolchain**をdefaultとする。

```text
Rust workspace / connected crate graph
  ↓
Toolchain Constraint Resolution
  ↓
selected rustc/cargo toolchain
  ↓
compiler pipeline
```

理由は、Rust compiler metadataや内部artifactをcross-version stable ABIとして扱えないためである。

異なるrustc versionで生成されたcrate metadata/`rlib`/`rmeta`を、互換性確認なしに通常のRust dependency edgeで接続しない。

rustc自身のmetadata formatにはversion識別が含まれ、compiler側でversion/format compatibilityを検査する実装になっている。このためLAMINARIAのcache/CASもcross-version reuseをdefaultで許可しない。

参考: https://doc.rust-lang.org/stable/nightly-rustc/rustc_metadata/rmeta/index.html

## 5. Cross-version compositionは明示的artifact boundaryで研究する

複数rustc versionが一つのfinal artifactへ参加する可能性自体は排除しない。ただし通常のRust metadata dependencyではなく、明示したartifact/ABI boundaryとして扱う。

候補:

```text
Rust(toolchain A) → native object/staticlib ─┐
                                             ├→ Link → Final Artifact
Rust(toolchain B) → native object/staticlib ─┘
```

または、

```text
Rust A → C ABI / explicit native contract ← Rust B
```

この場合も以下を検証する。

- object format / target triple;
- symbol/calling convention;
- allocator/runtime ownership;
- panic/unwind behavior;
- standard-library/runtime duplication;
- LTO compatibility;
- debug/unwind metadata;
- link compatibility。

「linkできた」ことを「Rust language ABI互換」と同一視しない。

## 6. Toolchain Adapter Capability Model

compiler versionごとに利用可能なintegration boundaryが異なるため、LAMINARIAはversionごとのadapter capabilityを持つ。

例:

```text
ToolchainCapability
  frontend_observation
  semantic_boundary
  mir_or_equivalent_access
  codegen_unit_visibility
  llvm_ir_or_bitcode_emission
  self_profile_support
  optimization_remark_support
  lto_modes
  backend_routes
  wasm_targets
  artifact_formats
```

同じRustでもversion/buildによりnightly flag、self-profile、internal API、LLVM version、backend supportが異なる。LAMINARIAは未対応capabilityをopaque/coarse-grained fallbackとして明示する。

## 7. Cache / Artifact Identity

artifact identityにはproducing toolchain identityを含める。

最低限:

```text
language
compiler family
compiler exact version/revision/build
cargo/nimble identity where semantically relevant
frontend/adapter version
standard library/sysroot identity
backend engine/version
bundled LLVM or backend revision where relevant
target/profile/features
pass/LTO configuration
input/dependency artifact identities
```

異なるcompiler version間でcontent digestが偶然一致しても、semantic compatibilityが証明されない限りcompiler-semantic artifactを共有しない。

一方、最終的なplain native objectやgenerated source等、artifact kindごとに安全なreuse可能性を個別研究する。

## 8. Measurement Foundationとの関係

`toolchains.lock.toml`は単一のRust stable + nightlyを固定するためのfileではなく、**named toolchain set**を定義できる必要がある。

概念例:

```toml
[rust.toolchains.release_a]
selector = "<exact release>"
components = ["rustc", "cargo", "rust-std"]

[rust.toolchains.nightly_a]
selector = "<exact nightly date or revision>"
components = ["rustc", "cargo", "rust-std", "rust-src"]

[nim.toolchains.nim2_a]
selector = "<exact version or revision>"

[nim.toolchains.nimony_a]
revision = "<exact source revision>"
```

この例の名前やversionはschema例であり、support policyを固定するものではない。

各Runは「requested toolchain selector」と「resolved ToolchainFingerprint」を両方記録する。

## 9. Version Matrixを性能・互換性研究に利用する

複数version対応はcompatibilityだけでなくLAMINARIA自身の研究にも利用する。

同一workloadを複数compiler versionで実行し、以下を比較できるようにする。

- total build latency;
- frontend/semantic/codegen/backend time;
- codegen-unit構造;
- LLVM/pass pipeline差分;
- incremental invalidation;
- peak memory / CPU / I/O;
- artifact size;
- generated-code runtime/size;
- cache identity/reuse opportunities;
- telemetry availability。

version間比較は同一EnvironmentFingerprint・同一scenario/cache-stateを基本条件とし、compiler version差以外を可能な限り固定する。

## 10. 成功条件

LAMINARIAの「複数version対応」は、複数compilerをinstallできることだけでは完了しない。

少なくとも次を満たす。

1. Rust/Nim toolchain versionをVariant Graph上で表現できる。
2. package/workspace requirementからcompatible toolchain候補をconstraintとして評価できる。
3. selected/resolved toolchainがRun/Action/Artifact identityへ伝播する。
4. compiler version別のpipeline capability/opaque boundaryを説明できる。
5. cross-version artifact reuseはcompatibility evidenceなしに行われない。
6. 同一workloadを複数Rust versionで再現可能に測定できる。
7. Nim 2/Nim 3とRust複数versionが同じtoolchain-version abstractionで扱える。

# Rust参照ビルド・チェーンの最小フットプリント研究ロードマップ

## 目的・現在地・次の判断

**目的**は、固定したRust workloadを既存のCargo/rustcでbuildする際、正しさを保ったままwall-clock、CPU時間、peak RSS、永続disk使用量、read/write I/Oを小さくできる**外部参照profile**を実測で選ぶことである。これはLAMINARIAの独自Rust/Nim compiler、IR、schedulerを置換する計画ではない。外部profileはreference/bootstrap evidenceであり、成功してもowned target-compilation pathへ昇格しない。[責務契約](../01-foundations/compiler-ownership-contract_ja.md)が優先する。

**現在地**は、共通Run/資源計測を担う[#11](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/11)、exact ToolchainFingerprintと互換性を担う[#22](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/22)、owned pathの現在gateである[#47](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/47)がある一方、外部Rust profileのPareto比較は未実施である。`rust-lld`/LLDを「新しい高速化候補」と数えない。対象targetで実際に選ばれたlinkerをfingerprintし、それをbaselineにする。

**次の判断**は、同一環境・同一source closure・同一artifact要求において、基準Cargo/rustc profileより資源面でPareto改善する候補があるか、または候補を棄却すべきか、である。総合点は作らない。速度、CPU、memory、disk、I/Oのどれを犠牲にしたかを隠さない。

## 境界：何を比較し、何を置換しないか

Cargoとrustcは一枚の「build tool」ではない。Cargoはmanifest/lockfileから依存を解決し、target・feature・host/target・build script・proc macroを含むunitを構成する。rustcはfrontend、型検査、monomorphization、MIR/codegen、backend、linkを担う。従って候補は置換層ごとに比較する。

```text
Cargo manifest + lockfile + fixed source closure
  -> Cargo resolver / unit graph / build-script and proc-macro execution
  -> rustc frontend, semantic analysis, monomorphization
  -> codegen backend
  -> linker
  -> requested native artifact
```

| 層 | 候補 | 解こうとすること | 本研究での扱い |
| --- | --- | --- | --- |
| Cargo resolver / unit graph | Cargo | package/version/feature/target/build-dependency解決とrustc invocationの構成 | 基準。Cargoの代替を最初から導入しない。 |
| rustc frontend | parallel frontend (`-Z threads`) | frontend内部の並列化 | nightly capabilityがfingerprintされた場合だけ比較する。CPU/RSS増を速度改善と混同しない。 |
| rustc backend | `rustc_codegen_cranelift` | rustc frontendを保ったままdebug向けcodegenを速くする可能性 | nightlyかつsupported workloadのみ。backend置換であり独立compilerではない。panic、target、生成artifactの契約が違えば比較から除外する。 |
| compilation cache | sccache | rustc invocation結果の再利用 | incrementalと同時に比較しない。cache byte、read/write、daemonを総footprintへ入れる。最終linkされる`bin`等をcacheできない制約も記録する。 |
| final linker | mold等 | final linkの残余時間/CPUを減らす | baselineでlinkが支配的と判明した場合だけ比較する。`rust-lld`が既に使われているならLLD再指定は候補ではない。 |
| action graph framework | Bazel/Buck2/Crane | action graph、hermeticity、remote/cache運用 | Cargo/rustcを置換しないため、初期の実測候補ではない。G1–G3後のsource-studyに留める。 |
| test/Docker cache tool | cargo-nextest/cargo-chef | test実行またはimage layer reuse | compile chainの比較対象から除外する。 |
| independent compiler | gccrs | rustc非依存のRust frontend/compiler | Cargo/rustc互換profileではない。現段階ではprior-art調査だけで、baseline比較に混ぜない。 |

この分離は「Cargo/rustcの代替が存在しない」という主張ではない。各候補が解く問題の境界が違うため、一つのベンチマーク名で「代替」と呼ぶと、cache hit、backend時間、link時間、package resolutionを誤って同じ効果として数えるからである。

## 最小仮説検証契約

- **優先度:** P1。現在のM1 native artifactを置き換えず、その判断に使うreference baselineを資源面で明確にする。
- **最小仮説:** 固定Cargo workloadについて、基準profileと比較可能な一つ以上の資格を満たす候補から、wall-clock、CPU時間、peak RSS、disk、I/Oの少なくとも一つを悪化させずに別の一つを改善するprofile、または採用候補なし、を再現可能に判断できる。
- **最小実験:** 一つの既存・lock済みRust workloadを選び、cold、true no-op、leaf implementation editを同じEnvironmentFingerprintで反復する。基準profileの後、資格を満たす候補を一つずつ比較する。
- **停止条件:** 各比較で採用、棄却、条件付き利用、または測定不能を理由とともに決め、Pareto frontierと次の作業を記録した時点。全候補・全platformの制覇は求めない。
- **非ゴール:** Cargo resolver、rustc frontend、LAMINARIAのowned compiler pathの置換。全OS/全target、remote build、最速の単一指標、あるいはcache容量だけを最小化すること。

## 比較契約

### 不変条件

比較ごとに、次を固定または明示的に差分として記録する。

1. Git revision、lockfile、features、profile、target、requested artifact、入力source closure。
2. EnvironmentFingerprintと、rustc、Cargo、sysroot、backend、linker、candidate wrapperを含む解決済みToolchainFingerprint。
3. cache pre-state。Cargo target、incremental、sccache、linker cache、registry/source cacheを「warm」と一語で混ぜない。
4. observable behavior。backendが異なりbyte-identical artifactにならない場合も、同じdirect executable test、exit/stdout/stderr、必要なABI/link/runtime contractで資格を確認する。
5. 計測modeとobserver overhead。container/WSLのcorrectness結果をnative performance基準へ混ぜない。

### 観測する資源

Run schemaは[#11](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/11)の共通形式を使い、新たなbenchmark storeを作らない。root processだけでなくchild process treeを集計する。

| 観測 | 意味 | 分離が必要なもの |
| --- | --- | --- |
| elapsed wall-clock | 利用者の待ち時間 | preparationとtimed build、observer overhead |
| user + system CPU time | 実際に消費したCPU | parallel化で速く見えるだけのCPU増加 |
| peak RSS | 必要なmemory上限 | root processだけでなくCargo/rustc/linker/wrapperのtree |
| target/artifact bytes | build出力と最終artifactの永続disk | final artifact、intermediate、incremental state |
| cache bytes | reuseのために保持するdisk | target dir、sccache等のcandidate cache、registry/source cache |
| read/write bytes | disk I/O負荷 | cache read、hash、compression、linker I/O |
| executed/skipped processes | どの仕事を避けたか | Cargo resolution、rustc、linkを一つの時間へ潰さない |

結果は「最速」を一つ選ばず、正しさを満たすprofileだけのPareto frontierとして示す。例えばsccacheがno-op/editを短縮しても、cold build、disk、CPUを悪化させるなら、そのtrade-offを条件付き利用として残す。

## 段階的ロードマップ

| 段階 | 最小作業 | 判断・成果 | 依存 |
| --- | --- | --- | --- |
| R0 — 比較可能性を固定 | 既存のlock済みRust workloadを一つ選び、artifact contract、fingerprint、cache pre-state、observer-on/offを記録する | 比較してよい基準profileと、比較不能な環境差を明文化する | #11, #22 |
| R1 — 基準を測る | 既定Cargo/rustcでcold、no-op、leaf editを反復し、実際のlinkerを記録する | linkが支配的か、frontend/backendが支配的か、または測定ノイズが大きいかを判定する | R0 |
| R2 — 層別候補を一つずつ比較 | 資格がある候補だけをPFE、Cranelift backend、sccache、必要時だけ別linkerの順で比較する | 各候補について採用/棄却/条件付き利用を決める。候補間を同じ「代替」として足し合わせない | R1 |
| R3 — handoff | Pareto frontier、raw evidence、資格外理由、採用可能なreference profileを#11/#22へリンクする | LAMINARIA main pathへ影響させず、G3等が参照できる外部baselineを確定する | R2 |

R2で「別linker」を試す入口はR1のtraceでfinal linkが意味のある資源/critical pathを占めることとする。`rust-lld`を既に選んでいるtargetでは、LLDを再指定するだけの試行を行わない。sccacheはincrementalを無効化した別profileなので、通常incremental Cargo profileに対する無条件の上位互換とは扱わない。

## Issue と完了時の記録

このロードマップは外部tracking用の[#50](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/50)に投影する。issueはR0–R3の実施台帳であって、project優先順位のauthorityではない。実装/証拠は次を残す。

1. workloadとrun matrix、exact fingerprints、cache pre-state、observer mode。
2. 各raw sampleと再計算可能なsummary。中央値だけで結論を出さない。
3. direct executable verification、artifact/runtime/ABI観測、資格外またはopaque region。
4. 各candidateの層、実際に減った/増えた資源、採否、適用条件。
5. #11のmeasurement spine、#22のtoolchain compatibility、必要なら#47のexternal baselineへのリンク。

## 参考資料

- Cargo: [Dependency Resolution](https://doc.rust-lang.org/cargo/reference/resolver.html)、[Build Scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html)、[unstable unit graph](https://doc.rust-lang.org/cargo/reference/unstable.html#unit-graph)
- rustc: [query system](https://rustc-dev-guide.rust-lang.org/query.html)、[incremental compilation](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html)、[linker options](https://doc.rust-lang.org/rustc/codegen-options/index.html)
- rustc backend: [`rustc_codegen_cranelift`](https://github.com/rust-lang/rustc_codegen_cranelift)
- compilation cache: [sccache Rust caveats](https://github.com/mozilla/sccache/blob/main/docs/Rust.md)
- measurement discipline: [rustc-perf](https://rustc-dev-guide.rust-lang.org/profiling/with-rustc-perf.html)、[Measurement Foundation](../02-research-areas/measurement/measurement-foundation.md)

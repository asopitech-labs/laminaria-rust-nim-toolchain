# LAMINARIA 研究基盤

## Rust Nim Unified Toolchain

### 文書の位置づけ

本書は、LAMINARIA の現在の方向性を形づくった設計研究を統合した研究ノートである。記載したすべてのコンパイラ境界が、すでに安定した公開 API として利用できると主張するものではなく、研究課題とアーキテクチャ仮説を整理する。

LAMINARIA は、Rust と Nim の開発を、依存解決、コンパイラ段階、バックエンド選択、成果物、FFI 境界、キャッシュ、実行までを横断する、説明可能な一つの計算グラフとして表現できるかを研究する。

プロジェクトステートメントは次のとおりである。

> LAMINARIA は、Rust と Nim のツールチェーンを統合する計算モデルを研究・実装する。言語フロントエンド、意味解析、コード生成、コンパイラバックエンド、成果物、キャッシュ、実行を、言語横断の計画とリソース認識スケジューリングに利用できる、単一の説明可能な Action Graph へ分解する。

## 1. 動機

Rust と Nim を組み合わせるプロジェクトでは、次の基盤が繰り返し個別実装されている。

- Cargo と Nimble の別々の依存解決
- Rust-to-Nim / Nim-to-Rust 統合用の独自 build script
- 生成された C / C++ ソースのコンパイル
- ヘッダーと binding の生成
- `cargo`、`rustc`、Nim、native compiler、linker の協調
- ローカルと CI で重複するキャッシュ設定
- 独立したツール間で発生する nested parallelism
- workspace 固有の toolchain 検出と診断
- target が再ビルドまたは失敗した理由を説明する個別ロジック

混成言語リポジトリが増えるたびに、同じ依存、FFI、schedule、cache の仕組みが再実装される。一方、それぞれの言語ツールは、他言語側の作業を見通せないまま局所的な判断を行う。LAMINARIA はこれをプロジェクト固有スクリプトの集合ではなく、共通基盤の問題として扱う。

## 2. 中心仮説

LAMINARIA は `cargo build` と `nimble build` を包むだけの薄い command wrapper を目指さない。内部に依存グラフ、コンパイラパイプライン、並列 scheduler を持つツール同士は、外側の task runner だけでは完全に協調できない。

有効な言語横断最適化には、言語単位の build command の内側に隠れた作業を段階的に公開する必要がある。

```text
Source Graph
    ↓
Compiler Pipeline
    ↓
Unified Program Graph
    ↓
Variant Graph
    ↓
Artifact Graph
    ↓
Action Graph
    ↓
Nim Planning Kernel
    ↓
Rust Runtime Scheduler
```

このモデルは、一つの build invocation にまとめられがちな次の関心事を分離する。

1. どの source / package entity が存在するか
2. どの semantic transformation と compiler transformation が必要か
3. どの variant と artifact が要求されるか
4. どの実行可能 action が artifact を生成できるか
5. 現在の machine 上で action をどう実行するか

## 3. 研究上の問い

LAMINARIA は次の問いを中心に構成する。

1. Rust と Nim の compiler pipeline を、言語固有の意味を失わず shared graph へ投影できるか
2. compiler analysis、artifact planning、execution 間の最小かつ安定した契約は何か
3. backend 選択を言語 toolchain の固定属性ではなく、制約付き graph variant として扱えるか
4. Rust codegen unit と Nim 生成 native compilation を一つの resource-aware schedule に載せられるか
5. FFI generation と ABI validation を、正確な invalidation を持つ通常の graph dependency にできるか
6. compiler stage と artifact 境界で content identity を定義し、worktree、CI checkout、machine 間で再利用できるか
7. target、profile、feature、backend、host/target role、artifact kind、FFI variant の組合せ爆発を demand-driven expansion で制御できるか
8. dependency 選択、rebuild、backend 選択、cache miss、critical path を、人間と software agent の双方へ説明できるか

## 4. グラフ階層

LAMINARIA は、すべてを一つの未分化なグラフへ詰め込まない。各層は異なる問いに答える。

### 4.1 Source Graph

Source Graph は package、crate、Nim module、local workspace、generated source、言語横断の source relationship を記録する。Rust と Nim を人工的な共通構文へ押し込まず、元の ecosystem における identity を保つ。

### 4.2 Compiler Pipeline Graph

Compiler Pipeline Graph は、言語ツールが実行する変換をモデル化する。bootstrap 段階では compiler invocation 全体を opaque action として扱ってもよいが、観測可能で有用な境界を段階的に細分化する。

Rust 側の概念的な経路は次のとおりである。

```text
source
  → parsing / expansion
  → HIR / type analysis
  → MIR construction / transformation
  → monomorphization collection
  → codegen-unit partitioning
  → backend lowering / optimization
  → object generation
  → archive / link
```

Nim 側の概念的な経路は次のとおりである。

```text
source
  → frontend / semantic processing
  → backend transformation
  → C / C++ / Objective-C / JavaScript generation
  → native compilation（必要な場合）
  → object generation
  → archive / link
```

これらは研究用の地図である。LAMINARIA は、安定した integration point と compiler 内部または実験的 interface を区別しなければならない。

### 4.3 Unified Program Graph

Unified Program Graph は共有の semantic planning layer である。MIR と Nim の内部表現を同一化するのではなく、各 compiler が計画に必要な entity と relationship を共通契約へ投影する。

- logical program unit
- dependency edge
- demanded capability
- specialization / variant dimension
- required / produced artifact
- source / diagnostic provenance
- invalidation relationship

言語固有の詳細は typed metadata として保持する。

### 4.4 Variant Graph

Variant Graph は次の選択軸を表す。

```text
package
× target
× profile
× feature set
× generic / specialized instance
× host / target role
× code-generation backend
× native compiler
× artifact kind
× FFI configuration
```

この直積を eager に実体化してはならない。demand-driven expansion、constraint propagation、canonicalization、memoization、equivalent-state merging、SCC condensation、pruning により、要求に関係する状態だけを構築する。

### 4.5 Artifact Graph

依存は producer-artifact-consumer の関係としてモデル化する。

```text
Producer Action → Artifact → Consumer Action
```

artifact には semantic metadata、Rust metadata、generated C / C++、header、binding、object file、backend IR、bitcode、static archive、dynamic library、executable、test result、diagnostic report などを含められる。`check`、`build`、`test` は別々の command 実装ではなく、一つの graph に対する異なる artifact demand となる。

### 4.6 Action Graph

Action Graph は実行可能な作業を含む。候補となる action kind は次のとおりである。

```text
RustFrontend
RustAnalysis
RustMIR
RustMonomorphization
RustCodegenUnit
NimFrontend
NimAnalysis
NimBackendGeneration
BackendLowering
BackendOptimization
CCompile
CppCompile
BindingGeneration
ObjectGeneration
Archive
Link
Test
CodeGeneration
Custom
```

言語は action の metadata であり、別々の scheduling universe に分離する理由ではない。

## 5. Backend Graph

LLVM を排除することも、LAMINARIA の固定基盤とすることもしない。source compiler が選択を許す場合、LLVM、Cranelift、GCC 系 code generation、将来の経路を選択可能な backend component として扱う。

```text
Language representation
    → Backend selection
    → Backend lowering
    → Backend optimization
    → Machine artifact
```

Nim の C、C++、Objective-C、JavaScript 生成も、それぞれ下流の artifact と execution requirement を持つ backend family の選択肢である。研究課題は最速 backend の選択だけではない。backend choice、toolchain compatibility、artifact type、diagnostic quality、cache identity、downstream linking requirement を同じ plan の制約として表現する。

## 6. Graph primitive としての FFI

FFI を custom build script の偶発的な side effect のままにしない。明示的な中間 artifact を持つ first-class relationship としてモデル化する。

Nim-to-Rust の例：

```text
Nim exported surface
    → C ABI description
    → header generation
    → native object / archive
    → Rust binding generation
    → Rust compilation / link
```

Rust-to-Nim の例：

```text
Rust exported surface
    → staticlib / cdylib
    → C header generation
    → Nim import representation
    → Nim compilation / link
```

これにより ABI change、binding regeneration、compatibility check、rebuild propagation、cache invalidation を通常の graph operation として扱える。

## 7. Planning と execution の境界

LAMINARIA 自体も Rust と Nim で実装する。責務分割は処理対象の言語ではなく、計算モデルに従う。

### Nim Planning Kernel

Nim component は、主に決定論的な計算を担う。

- graph normalization / traversal
- cycle detection / SCC decomposition
- constraint propagation
- variant expansion / pruning
- state canonicalization / merging
- artifact-demand propagation
- critical-path analysis
- candidate-plan generation
- combinatorial optimization

望ましい契約は粗粒度の関数に近い。

```text
plan(PlanningInput) → ExecutionPlan
```

planning kernel は filesystem、network、process、OS への直接的な side effect を避ける。

### Rust Runtime Scheduler

Rust component は execution と可変な machine state を担う。

- CLI / daemon service
- workspace / toolchain discovery
- filesystem access / watching
- process lifecycle / cancellation
- asynchronous execution
- CPU / memory / I/O accounting
- sandboxing
- cache / content-addressed storage
- local / remote executor
- diagnostics transport

planner は action 完了のたびに呼び直さない。replanning は failure、dynamic dependency discovery、resource budget change、executor availability change など、重要な変化がある場合に限定する。

## 8. Resource-aware scheduling

Cargo と Nim を独立実行すると nested parallelism が発生し得る。各ツールが machine 全体を所有すると仮定し、同じ CPU 数を基準に作業を生成するためである。LAMINARIA の scheduler は、代わりに言語横断の Action Graph を操作する。

各 action は次の resource profile を宣言または学習できる。

```text
cpu demand
memory demand
I/O characteristics
estimated duration
cache-hit probability
executor requirements
```

schedule は CPU utilization だけでなく、要求された artifact までの wall-clock time を最小化すべきである。priority は graph readiness と推定 remaining critical path に基づき、live resource budget の制約を受ける。historical telemetry は duration と memory の推定を改善できるが、学習値は説明可能で、planning の意味的結果を変更してはならない。

## 9. Cache と content identity

LAMINARIA は三つの cache layer を分離する。

```text
compiler invocation cache
        ↓
action cache
        ↓
content-addressed artifact storage
```

action または compiler stage の identity には、意味的に関係する入力だけを含める。

```text
operation kind
command / normalized arguments
relevant environment
input content digests
dependency artifact identities
toolchain identity
target / profile
backend selection
relevant configuration
```

物理 checkout path を identity に含めない。source と toolchain を論理 location へ正規化し、Git worktree、CI directory、同じ source state を持つ repository、互換 toolchain を持つ machine の間で同等の作業を再利用できるようにする。

## 10. Incremental Compiler Graph

package 単位の invalidation は長期目標には粗すぎる。次のような invalidation chain を研究する。

```text
changed source
  → affected semantic node
  → affected specialization
  → affected codegen unit
  → affected backend action
  → affected object
  → affected final artifact
```

incrementality は三つの関連層として扱う。

1. workspace-level affected analysis
2. compiler-semantic / codegen invalidation
3. artifact / action-cache reuse

compiler が粗い境界しか公開しない場合も correctness を保つ。fine-grained integration は最適化であり、正しい coarse-grained build の前提条件ではない。

## 11. Agent-oriented explainability

LAMINARIA は人間と software agent の双方を対象とする。graph と scheduler は unstructured build log から推測するのではなく、直接 query できるべきである。

```text
laminaria dependency-graph
laminaria program-graph
laminaria action-graph
laminaria compiler-pipeline
laminaria codegen-units
laminaria critical-path
laminaria explain-dependency
laminaria explain-rebuild
laminaria explain-codegen
laminaria explain-backend-selection
laminaria explain-cache-miss
```

重要な判断には structured explanation を持たせる。selected variant、dependency path、invalidation cause、cache-key difference、critical-path contribution、rejected backend constraint などである。

## 12. Delivery strategy

compiler 内部への統合は段階的に進める。

### Phase 1: Unified interface

build、run、test、check、format、lint、graph inspection、cache status、toolchain diagnostics を一つの CLI から提供する。既存 ecosystem tool は引き続き execution engine として利用する。

### Phase 2: Unified package and target graph

Cargo と Nimble の metadata を共通 workspace model へ正規化し、言語横断 dependency、affected analysis、dependency explanation を追加する。

### Phase 3: Planning IR and Nim kernel

`PlanningInput` と `ExecutionPlan` を安定化する。Nim で SCC analysis、constraint resolution、lazy variant expansion、pruning、critical-path computation を実装する。

### Phase 4: Unified action scheduler

generated native compilation、binding、archive、linking を action として表現し、Rust runtime が global CPU / memory budget を強制する。

### Phase 5: Fine-grained compiler integration

維持可能な範囲で compiler stage、codegen unit、semantic artifact、backend boundary を実験する。

### Phase 6: Persistent and distributed execution

新しい分散 protocol を早期に発明せず、daemon、durable graph state、remote cache、sandbox execution、abstract remote executor を追加する。

### Phase 7: Self-hosting

LAMINARIA 自身の Rust host と Nim planning kernel を LAMINARIA で build する。連続 stage の plan と artifact を比較し、mixed-language architecture を継続的に検証する。

## 13. 評価計画

異なる graph 特性を分離できる workload を用いる。

- 多数の crate、specialization、codegen unit を持つ Rust-heavy graph
- 大量の generated native source を持つ Nim-heavy graph
- Rust と Nim の native compilation が同時進行する mixed graph
- backend variant workload
- 複数 ABI boundary を持つ FFI-heavy project
- 深い critical path と広い independent action frontier
- variant-heavy workspace
- incremental semantic change
- Git worktree をまたぐ repeated build
- local / CI 間の cache reuse

主要な測定項目は次のとおりである。

- graph construction cost / graph growth
- explored / pruned variant states
- incremental invalidation precision
- semantic / codegen / object / final-artifact reuse
- requested-artifact critical-path duration
- CPU utilization / peak memory
- physical checkout 間の cache hit rate
- FFI invalidation precision
- explanation の completeness / stability

重要なのは full build が速くなることだけではない。どの計算を回避できたか、なぜ結果を再利用できたか、残った作業がなぜ必要だったかを特定できなければならない。

## 14. 研究仮説

- **H1:** package/task graph は言語横断最適化には粗すぎる。compiler-pipeline work の公開により、追加の並列性と再利用が可能になる。
- **H2:** LLVM など特定 backend を普遍的基盤にせず、backend selection を constrained graph variant として表現できる。
- **H3:** Rust codegen unit と Nim-generated native compilation は一つの scheduler を共有し、nested parallelism と global critical-path length を削減できる。
- **H4:** semantic artifact と machine artifact を分離し、`check`、`build`、`test` を一つの graph に対する異なる artifact demand として扱える。
- **H5:** FFI を graph primitive として扱うことで、external build script より精密な regeneration と invalidation が可能になる。
- **H6:** compiler-stage content identity により package-level cache より細粒度の再利用が可能になる。
- **H7:** demand-driven graph construction と state canonicalization により variant explosion を制御できる。
- **H8:** structured compiler/build graph により failure、rebuild、backend choice、cache miss を software agent へ直接説明できる。

## 15. Non-goals と制約

LAMINARIA は当初 Rust と Nim に特化する。Bazel、Buck2、Cargo、Nimble、`rustc`、Nim、LLVM、native compiler をすべて置き換える汎用システムを目指さない。

導入時には既存 manifest と lockfile を保持し、責務を置き換える前に成熟した resolver と compiler を再利用する。fine-grained compiler integration が、正しい coarse-grained build を不可能にしてはならない。hermeticity と remote execution は段階的機能であり、初期要件ではない。既存 host toolchain を用いた local development を有用なまま保つ。

## 16. ライセンス方針

Rust implementation と Nim Planning Kernel の双方に `MIT OR Apache-2.0` を適用する。混成言語 codebase 全体で contribution / reuse policy を統一しつつ、Apache-2.0 の明示的な patent grant を利用できる。

repository には次を置く。

```text
LICENSE-MIT
LICENSE-APACHE
THIRD_PARTY_LICENSES.md
```

third-party compiler、backend、library、generated support code、linked runtime component は、それぞれ固有の license obligation を維持する。将来 LAMINARIA が runtime または startup code を user artifact へ組み込む場合、その部分は top-level tool license が自動的に適切と仮定せず、個別にレビューする。

## 17. 未解決の設計課題

1. 最小で有用な `PlanningInput` / `ExecutionPlan` schema は何か
2. 初期実装で first-class identity が必要な artifact kind は何か
3. adapter に利用できる安定した Rust / Nim compiler boundary はどこか
4. constant replanning なしで dynamic dependency をどう取り込むか
5. Nim planner の constraint と runtime admission rule をどう分けるか
6. machine 間で toolchain identity をどう正規化するか
7. agent 向け structured explanation schema の最小形は何か
8. self-hosting で plan determinism と artifact reproducibility をどう検証するか

最優先の deliverable は Nim kernel と Rust runtime 間の planning contract である。この契約が安定すれば、graph resolution、scheduling、caching、diagnostics、将来の executor implementation を独立して発展させられる。

## 18. 先行する scheduling 実験と reference benchmark

### 18.1 Cargo scheduler の再構成

2026年8月31日に公開された [*Could Cargo's scheduler be better?*](https://spirali.github.io/blog/cargo-scheduler/) は、Cargo と child process の system call trace から実行可能な Rust build graph を再構成した研究である。17件の Rust project の debug build を対象に、観測した Cargo schedule の replay と代替 scheduler を parallelism 4 / 16 で比較している。著者は Cargo の内部 scheduling algorithm を説明したとは主張しておらず、外部から観測・再生した Cargo schedule を baseline としている。

LAMINARIA にとって特に重要なのは、一回の `rustc` invocation を不可分な一 node として扱わない点である。dependency の code generation と link が完了する前でも、Rust metadata（`.rmeta`）が利用可能になれば dependent crate を開始できる。再構成 graph は次のように表現される。

```text
crate frontend
    → .rmeta available
    ├────────────→ dependent crate frontend
    → remaining compilation / code generation / link
```

frontend と残りの compilation は同じ OS process の継続であるため、forced continuation となる。scheduler は同じ worker 上で両者の間に別 task を挿入できない。それでも `.rmeta` production boundary により、invocation 全体の完了より早い実在の dependency edge が公開される。

これは LAMINARIA が置く semantic artifact と machine artifact の分離に対する具体的な先行証拠である。metadata production、semantic analysis、specialization、codegen unit、backend execution、object generation、linking を producer-artifact-consumer relationship として研究する根拠になる。ただし、これらの境界が stable public compiler API であることまでは示さないため、adapter feasibility は別の研究課題として残る。

### 18.2 Critical-path-aware b-level scheduling

各 task `t` の bottom level を次のように計算する。

```text
b-level(t) = duration(t) + max(b-level(child))
```

worker が空くと、ready task のうち b-level が最大のものを選ぶ。この greedy rule は、直ちに runnable な task 数を最大化するのではなく、最長 remaining dependency path 上またはその近傍の作業を優先する。

17 project 中、4 CPU では15件で replayed Cargo baseline を改善し、wall-clock time の中央値を約8%、最大約16%短縮した。16 CPU でも14件で改善し、中央値約2%、最大約15%短縮した。全 heuristic と randomized search で見つかった最良 schedule を pseudo-optimum とすると、4 CPU で b-level は中央値1.3%差、Cargo は9.6%差だった。16 CPU ではそれぞれ0.4%差と2.3%差だった。

この結果は、制約された resource 下では local parallelism の最大化より global critical path の短縮が重要になり得る、という LAMINARIA の scheduler 仮説を支持する。

### 18.3 不完全な cost 情報による scheduling

同研究は、b-level に正確な execution time prediction が必要かも検証した。task duration 推定へ最大60%の Gaussian noise を加えても、中央値の schedule は exact duration 使用時に近かった。ただし一部の tail outcome は大きく悪化し得る。

task を short / long に分ける one-bit model でも効果の大半が残った。pseudo-optimum に対して4 CPU で中央値1.5%差、16 CPU で0.5%差となり、exact-duration b-level の1.3%差、0.4%差に近い。timing を使わない graph-depth variant も、4 CPU では17件中16件、16 CPU では17件中12件で Cargo baseline を上回ったが、tail case は明確に悪化した。one-bit signal は中央値を大きく改善するというより、病的な schedule を防ぐ保険として有効と考えられる。

LAMINARIA は scheduler の高度化を次の順に比較する。

1. uniform action cost を用いる graph-depth scheduling
2. binary-cost b-level scheduling
3. historical-cost b-level scheduling
4. resource-aware b-level scheduling
5. cross-language critical-path scheduling

これにより、精密な telemetry を初期 scheduler の前提条件にしない。historical duration、cache-hit probability、memory demand、I/O behavior、backend characteristic、executor constraint は、個別に測定できる refinement として追加する。

### 18.4 言語横断への拡張

公開研究は外部観測で再構成した Rust/Cargo task graph を対象とする。LAMINARIA は、次の作業を一つの Action Graph へ載せることで compiler / language boundary を越えて拡張する。

- Rust metadata production と dependent frontend
- Rust codegen unit
- LLVM、Cranelift、GCC-family backend action
- Nim semantic processing / backend generation
- Nim-generated C / C++ / Objective-C / JavaScript artifact
- native C / C++ compilation
- FFI header / binding generation
- archive creation / linking
- cache materialization / artifact availability

中心的な問いは、Rust build 内で観測された critical-path-aware scheduling の効果が、Rust と Nim の semantic stage、backend work、native compilation、FFI generation、linking が一つの resource budget を共有する場合にも維持または拡大するかである。forced continuation は freely preemptible action とせず、明示的な constraint として保持する。

### 18.5 Reference benchmark protocol

言語横断 scheduling の改善を主張する前に、公開研究と同じ17 project、または根拠を明記した代表 subset で実験を再現する。reconstructed graph と recorded task duration を固定し、scheduling policy だけを変更して比較する。

```text
replayed Cargo baseline
    → LAMINARIA graph-depth
    → LAMINARIA binary-cost b-level
    → LAMINARIA historical-cost b-level
    → LAMINARIA resource-aware b-level
```

初期再現では、4 CPU / 16 CPU、median / tail behavior、発見された最良 schedule との差、cost estimate の欠損や noise に対する感度を測る。第2段階では同じ protocol を Rust/Nim mixed workspace に適用し、peak memory、I/O pressure、cache availability、backend selection、forced-continuation constraint を追加する。

これにより、公開された Cargo scheduling baseline から LAMINARIA の cross-language scheduler へ進む falsifiable な研究経路ができる。単に速いかだけでなく、どの critical-path decision が変わったか、どの estimate を使ったか、改善が graph decomposition、priority policy、resource admission、cache availability のどれに由来するかを報告する。

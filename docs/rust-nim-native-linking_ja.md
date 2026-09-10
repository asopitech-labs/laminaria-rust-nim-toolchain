# Rust–Nim Native Linking 研究計画

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

本書のLLVM、Nim、rustc、native-link、Wasm経路は比較・観測実験である。図・adapter API・checkpoint条件をLAMINARIA独自コンパイラの規定にしない。#25/#3が独自実装するsource/IR経路を#5/#13で実証し、将来variantとして許容するだけにしない。外部backendによる成果は独自コンパイルの認定にならない。

## 目的

LAMINARIAでは、RustとNimの両方を同一native artifact/link planへ参加させる際に、cross-language boundaryを必ず一度「C向けexport + header + C ABI」へ縮退させる必要があるのかを検証する。

これは任意のRust/Nim言語機能を直接交換できるという主張ではない。native object/link compatibilityとlanguage-level semantic compatibilityを分離し、adapterが本当に必要な場所だけを明示する研究である。

## 従来経路との違い

一般的なmixed-language integrationは次の形を取る。

```text
Language A
  -> C-compatible exported surface
  -> header/binding description
  -> C ABI
  -> Language B declaration
  -> native linker
```

LAMINARIAが研究するのは次の形である。

```text
Rust semantic/codegen pipeline ----┐
                                  ├-> shared artifact/link model -> final native artifact
Nim semantic/codegen pipeline -----┘
```

adapterを全廃することが目的ではない。どの境界にadapterが必要で、どの境界はnative linkとして直接扱えるかを測定に基づいて判定する。

## 研究課題

1. RustとNimのどの生成物が、C header contractを介さず同一native linkへ参加できるか。
2. symbol naming、visibility、calling conventionのどこがdirect referenceを妨げるか。
3. LAMINARIAがobject generation前にcross-language symbol identityを制御できるか。
4. integer/float/pointer/record/string/seq/closure/exception/panic/ownership/destructor/runtimeのうち、どこまでcompatible contractを定義できるか。
5. 同一process image内でNim runtimeの初期化・終了処理をどう保証するか。
6. LTO、whole-program optimization、dead-code elimination、linker GCへcross-language call edgeを参加させられるか。
7. debug information、stack unwind、symbolizationはどうなるか。
8. static executable、shared library、WASMで条件はどう変わるか。
9. このboundary contractをcache identity/invalidationへ利用できるか。

## 実験レイヤ

### 1. Object / Link Compatibility

まずlanguage semanticsより下を調べる。

対象:

- object format
- architecture / target triple
- relocation
- symbol visibility
- name mangling
- calling convention metadata
- runtime initialization section
- linker/archive behavior

最初のproofは極小にする。Rust object 1個とNim object 1個を、generated C header contractを作らず同一linkへ入れ、明示したsymbol relationshipを成立させる。

### 2. Symbol Contract

LAMINARIAがuser-facing C exportとは独立したcross-language symbol identityを定義できるかを調べる。

`nm`、`objdump`等によるsymbol/relocation inspectionを必須証拠とする。

### 3. Type / Layout Contract

互換性を仮定せずmatrix化する。

候補:

- fixed-width integer
- float
- bool
- pointer
- fixed-layout record
- array
- slice/openArray
- string
- seq/vector
- enum/tagged union
- closure/function value
- opaque handle

採用する型classごとにsize、alignment、field offset、ownership、lifetime、mutationを記録する。

adapterが必要なら、そのadapter自体をAction Graph上の明示的node/artifactとして扱う。

**比較経路での観測**（`fixtures/direct-native-link/NOTES.md`）: テストしたfixed-layout recordはポインタ経由で`nim c`/`nlvm`の対象経路で一致し、値渡しaggregateには経路依存の失敗があった。これは限定した外部ツールのABI観測であり、Rust/Nim全般の互換保証ではない。「手書きFFIでは一般的でない」ことは、独自コンパイラでの優先度を決める根拠にならない。#25/#3の対応ソース意味から値・所有権・layout・adapterの責務を導出し、未解決を不要扱いしない。

### 4. Runtime / Failure Semantics

以下を検証する。

- Nim runtime initialization
- allocator ownership
- ARC/ORC等のmemory management
- Rust allocator interaction
- panic/unwind
- Nim exception
- thread/TLS
- destructor/finalizer
- process/library teardown

failure semanticsが互換でない境界は、fail closedまたは明示的translation adapterを使う。

### 5. Optimization

最低でも次を比較する。

1. conventional C ABI baseline
2. adapterを必要箇所のみに限定したdirect native-object boundary
3. backend/link modeが許す場合のcross-language optimization path

測定項目:

- call overhead
- code size
- inlining/LTO evidence
- dead-code elimination
- duplicate runtime/support code
- link time
- incremental rebuild scope

flagを付けただけで最適化されたと判断せず、最終artifactと実行経路を検査する。

### 6. WebAssembly

WASMでは以下を分けて評価する。

- 両言語pipelineから単一final moduleを生成できる経路
- single linkが不可能な場合のmodule/component boundary
- generated adaptation layer
- duplicate runtime state
- code size / boundary cost
- direct graph integrationによるinvalidaton/scheduling/cache reuse改善

## C ABIの位置づけ

C ABIを研究対象から排除しない。baselineおよび互換手段として残す。

比較対象は、

```text
C ABIを必須のarchitectural boundaryとする
```

対

```text
必要な境界だけでC ABI/adapter artifactを選択する
```

である。

一部の型やtargetがC-compatible adapterを必要とする結果になっても構わない。その理由とコストが明示され、それ以外のnative linkまで不必要にC ABIモデルへ制約されなければよい。

## 必須証拠

各実験では以下をcommitまたは再生成可能にする。

- Rust/Nim両方のsource
- compiler/linker version
- exact commandまたはLAMINARIA plan
- target/backend configuration
- object/archive/module inventory
- symbol/relocation inspection
- runtime output
- failure behavior
- size/resource measurements
- C ABI baseline comparison
- generated adapter/fallbackの説明

## 非目標

- 任意のRust/Nim型をlayout-compatibleと仮定すること
- 測定前に新しいuniversal ABIを発明すること
- runtime requirementを無視すること
- unsafe transmutationをinterop designとすること
- C shimを隠してABI-freeと表現すること
- このlinking Issue内でfrontend全体を実装すること（独自source/IR処理は#3/#25の責務であり、プロジェクトの非目標ではない）

## 成功条件

supported targetについて、LAMINARIAが再現可能な証拠とともに以下を説明できること。

1. どのRust/Nim unitが同一linkへ入るか
2. cross-language symbol/artifactは何か
3. C ABI adapter不要な部分はどこか
4. adapterが必要な部分とその理由
5. runtime obligation
6. artifact invalidation条件
7. conventional C ABI baselineと比較したcorrectness、code size、build cost、runtime cost、optimization opportunity

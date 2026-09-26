# Issue #86: A7 Rust worklist BFS 型設計

## Goal

A7 が `GenericDefinition` から依存クローズ全体で一意な
`InstantiationKey` を決定的に収集し、呼び出し元・コード生成単位
（CGU）に依存せず、後段の A8/A9 が並列に消費できる物理 work unit 集合を
返す。これは実装ではなく、実装を拘束する型と遷移の契約である。

## Checkpoint contract

- **Result**: roots から BFS で到達可能な単相化要求を走査し、各キーを一度だけ
  `InstantiationWorkItem` として確定する設計。
- **Consumes**: A6 の `SemanticFact`、`GenericDefinition`、型/const 引数、ABI・
  target・active feature、公開 export/FFI/reflection root。
- **Must preserve**: `InstantiationKey` は宣言識別子・型引数・const generic 値・
  ABI/target・active feature のみで決まり、caller identity・探索順・CGU を含めない。
  同じキーの同じ内容の commit は冪等で、依存クローズ内に重複を作らない。
- **Evidence**: 型定義、状態遷移、不変条件、決定性と cycle/衝突時の構造化された
  結果を本書で検査可能にする。
- **Enables**: A8 の最適化判断、A9 の `InstantiationKey → LoweredModule` 1:1
  適用、A10 の並列スケジューリング。CGU のような後付け収束点は導入しない。

## Concrete types (Rust-like pseudocode)

```rust
struct InstantiationKey {
    declaration: DeclarationId,
    type_arguments: SmallVec<[TypeId; 4]>,
    const_arguments: SmallVec<[ConstValueId; 2]>,
    abi_target: AbiTarget,
    active_features: FeatureSet,
}

struct InstantiationRequest {
    key: InstantiationKey,
    definition: GenericDefinitionId,
    source: RequestSource, // semantic edge, export, FFI, or reflection root
}

struct InstantiationWorkItem {
    key: InstantiationKey,
    definition: GenericDefinitionId,
    dependencies: BTreeSet<InstantiationKey>,
}

struct A7Worklist {
    queue: VecDeque<InstantiationRequest>,
    mentioned: BTreeSet<InstantiationKey>,
    visited: BTreeSet<InstantiationKey>,
    usage_map: BTreeMap<InstantiationKey, BTreeSet<UsageSite>>,
    committed: BTreeMap<InstantiationKey, InstantiationWorkItem>,
}
```

`mentioned` は要求をキューへ投入済みかを表す予約集合、`visited` は依存展開を
完了した集合、`committed` は A7 の keyed compare-and-commit 済み状態である。
したがって `mentioned` と `visited` を一つに潰してはならない。前者を分けることで
同一キーへの複数要求を一度だけキューに入れつつ、未展開の依存を失わない。

## Root collection and BFS transition

1. `RootSet` は entry/export、FFI、reflection/dynamic の保守的 root を収集する。
   root に caller module や CGU 名を保存しない。
2. 各 root を `InstantiationRequest` に正規化し、`mentioned.insert(key)` が新規の
   ときだけ `queue.push_back` する。挿入順は `InstantiationKey` の canonical byte
   order でソートし、実行時の map iteration に依存しない。
3. キュー先頭を取り出し、`visited` 済みなら usage のみ記録する。未訪問なら
   `GenericDefinition` の semantic body が参照する generic call、trait/vtable、
   export/FFI edge を同じ target context で解決する。
4. 解決した依存ごとに `usage_map[dependency].insert(site)` を行い、新しいキーだけ
   `mentioned` とキューへ追加する。依存を全て走査後に現在の item を
   `committed` へ compare-and-commit し、最後に `visited.insert(key)` する。
5. queue が空になった時点で `committed.values()` を canonical key order で返す。

未知の型引数、解決不能な trait/vtable、矛盾する target/feature は黙って捨てず、
`A7Outcome::Rejected { key, reason, source }` として返す。保守的 root の推論不能も
`RetainedConservatively` と区別する。

## Invariants and conflict policy

- **Uniqueness**: `committed.keys()` に同一 `InstantiationKey` は一つだけ。
- **Closure**: committed item の全 dependency は同じ結果集合に含まれるか、構造化
  rejection が存在する。
- **Caller independence**: caller の変更・import 順・並列投入順を変えても key と
  work item の内容は不変。
- **Determinism**: 同じ semantic inputs から canonical serialization と item 順が
  常に一致する。
- **No CGU**: 一つの key は一つの item。物理的な CPU/process partition は A10
  scheduler の責務であり、A7 の型や key に表現しない。
- **Conflict**: 同じ key に異なる definition/body fingerprint が到達した場合は
  `ConflictingDefinition` として全体を失敗させる。先着順で上書きしない。
- **Cycles**: generic recursion は `visited` により有限化する。自己/相互再帰は
  エラーではなく閉包内の一つの key として記録し、未確定な解決だけを rejection にする。

## Scope boundary

本書は A7 の型、root、BFS、dedupe、衝突回避だけを定義する。A6 が semantic edge
を作る方法、A8 の最適化、A9 の lowering、A10 の resource reservation、並列実行の
分割は別 checkpoint の契約である。実装着手時はこの文書の型を production code に
写し、最小の generic recursion・diamond dependency・caller-order permutation の
3ケースを直接実装へ投入して不変条件を確認する。

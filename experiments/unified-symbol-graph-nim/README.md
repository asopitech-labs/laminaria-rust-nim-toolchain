# unified-symbol-graph-nim

`experiments/unified-symbol-graph`(Rust実装)の`SharedSymbolGraph`
(スケジューラー/プランナー: `declare_analyzed_symbol`/`require_symbol`の
デマンド駆動`Analyzed`→`Committed`昇格、`assign_layout`、
`apply_elf_x86_64_relocations`)をNimへ移植した検証実装。

## スコープ

これは本番実装への置き換えではなく、以下を検証するための実験である:

- `SharedSymbolGraph`が持つスケジューラー/プランナーロジック(状態機械・
  デマンド駆動promotion・レイアウト割り当て・relocation解決)は、
  Nimで実装可能か。
- `docs/01-foundations/compiler-ownership-contract_ja.md`が定める
  「Nim独自アルゴリズムの記述は最小限、既存C/C++ライブラリ・データ構造を
  現代風の構文で利用する」という方針に沿った実装ができるか。

**含まないもの**: MIRテキストパーサー(`mir_text`)・`target_ir`の
コード生成部分は、本ポートの対象外。issue #68がスコープとした
「単一関数内でのコード生成・命令エンコーディング」の検証は、既にRust側で
完了している。本ポートは複数シンボル間の状態管理・スケジューリング層のみを
対象とする。

## 既存C/C++ライブラリの利用

- ハッシュテーブル: Nim標準ライブラリ`std/tables`(内部的にNimの
  ハッシュテーブル実装を使うが、これはNim言語自体のコア機能であり、
  本ポート独自の再実装ではない)。
- 排他制御: `std/locks`(`pthread_mutex_t`の薄いラッパー)。
- 代数的Option型: `std/options`(Nim標準ライブラリ)。
- ソート: `std/algorithm`の`sort`(比較関数のみ本ポートで指定)。

Rust版が使っていた`RwLock`(読み書き分離ロック)3つを、本ポートでは
単一の`Lock`(排他ロック)へ単純化している。これは意図的な簡略化であり、
並行性の粒度が粗くなっている点を正直に記録する(詳細は
`src/shared_symbol_graph.nim`自身のコメントを参照)。

## 実行方法(コンテナ内)

```bash
docker build -f ../unified-symbol-graph/nim-scheduler-check.Dockerfile \
  -t laminaria-nim-scheduler-check ../unified-symbol-graph
docker run --rm -v "$(pwd)":/work -w /work laminaria-nim-scheduler-check \
  nim c -r --path:src src/test_shared_symbol_graph.nim
```

## Rust版との関係

Rust版(`experiments/unified-symbol-graph/src/lib.rs`)は削除せず、
比較実験用のbaselineとして残す。

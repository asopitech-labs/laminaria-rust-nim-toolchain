# プロジェクトビルド: 単一言語および明示的最小宣言による混成対象 (issue #26)

本ドキュメントは `laminaria build`/`plan-build` を説明する。これは
**ユーザーの対象プロジェクト**を計画・ビルドするための入口であり、
**LAMINARIA自身**を計画・ビルドする `laminaria self-build`
(`docs/self-build.md`) とは別物である。issue #26 が明示的に求める区別は次の3つ:

1. **LAMINARIA自身の実装**はRust + Nimである。Rust単独の対象をビルドする
   場合でも、実際の本番Nim Planning Kernelで計画しなければならない —
   これはRust側で計算した計画で代替してよいという意味ではない。
2. **LAMINARIA自身のビルド** (`self-build`) は、LAMINARIA自身の両コンポー
   ネントを再ビルドするために、Rust・Nim両ツールチェーンを正当に必要とする。
3. **対象プロジェクトのビルド**は、そのプロジェクト自身が要求する成果物と
   依存関係グラフから決まるツールチェーンのみを必要とする —
   LAMINARIA自身の実装言語を無関係な対象に持ち込んではならない。

`build`/`plan-build` は `self-build` と全く同じ本番Nim planner
(`laminaria_plan::call_planner`) とRust実行系
(`laminaria_run::run_and_record_with_doctor`) を再利用する — 別系統の
重複実装ではない。実装前の調査で、`PlanningInput -> ExecutionPlan` 契約と
Nimカーネル自体 (`nim-planner/src/planning_kernel.nim`) は、計画に
`CargoBuild`/`NimBuild` アクションがいくつ含まれるかに一切分岐しないことを
確認済みであり、契約やNimカーネル側の変更は不要だった —
言語固有のロジックはすべて `self_build.rs` のハードコードされた3アクション
形状と無条件の両ツールチェーン解決に存在しており、`project_build.rs` は
これらを一切継承していない。

## 対象プロジェクトの必要要件の決定

ファイルの単なる同居から推測することはなく、同居を「非対応」として一律に
拒否することもない — `laminaria_run::project_build::determine_project_requirements`
を参照:

1. **`--requires rust|nim|rust,nim` が与えられた場合、常に優先される。**
   ファイルの存在はこれを上書きするために参照されることはない。これにより、
   純粋なファイルベースの推論では表現できない2つの実際のケースが表現可能に
   なる:
   - 同一ディレクトリにRustアプリケーションと無関係なNim製補助ツールが
     同居しており、Rust側だけをビルドしたい場合 (`--requires rust` —
     Nimファイルは一切検査されない)。
   - Cargoプロジェクトの `build.rs` が実際に `nim` を呼び出すが、
     コンパイルすべき独立したNimソースファイルは存在しない場合
     (`--requires rust,nim` かつ解決可能なNimエントリが存在しない — 詳細は
     後述)。
2. **それ以外は、曖昧でない場合のみ推論する**: `--project-root` に
   `Cargo.toml` か、解決可能なNimエントリ (標準的な `nimble init` の慣習 —
   単一の `<name>.nimble` と `src/<name>.nim` または `<name>.nim`、あるいは
   明示的な `--nim-entry`) のどちらか一方だけが存在する場合。このケースでは
   設定ファイルは不要。
3. **両方存在し、`--requires` が明示されていない場合**: これは両方の候補を
   名指しした *曖昧なプロジェクト* エラーとなり、明示的な指定を求める —
   「混成プロジェクトは非対応」という一律の判定ではなく、どちらかへの
   無言の誘導でもない。
4. **どちらも存在しない場合**: 「ビルド可能なソースがない」エラー。

## 2ツールチェーン形状を正直に扱う

RustとNimの両方が要求された場合 (`--requires rust,nim` — 上記ルール3の
とおり、これは常に明示指定を要する)、実際に計画される内容は、実在する
Nimプロデューサーのエントリポイントがあるかどうかで変わる:

- **実在するNimエントリポイントがある** (解決可能な `.nimble` +
  ソース、または明示的な `--nim-entry`): **独立した**2つのプロデューサー
  アクション — `CargoBuild` 1つと `NimBuild` 1つ — が計画され、両方が
  `demanded_artifacts` に含まれ、**両者の間に依存エッジは宣言されない**。
  「両方とも欲しいが、両者の間に既知の関係はない」ことを正直に表した計画
  であり、ここで `Integrate` ステップを捏造することは、需要・依存グラフが
  一度も要求していないアクションをでっち上げることになる。`self-build`
  自身の `Integrate` アクションは *LAMINARIA自身*
  の世代ルートレイアウト(4つの特定の名前付き兄弟バイナリ)を組み立てる
  ものであり、任意の対象プロジェクトに対する汎用的な等価物は存在せず、
  本コードはそう装わない。
- **Nimエントリポイントが存在しない**: 単一の `CargoBuild` アクションが
  計画される。Nimツールチェーンは(要求された以上)解決・検証されたままだが、
  解決済みの `bin` ディレクトリはそのアクション自身の `PATH`
  の先頭に追加される — `build.rs` が `nim` を呼び出せるようにするためで
  あり、存在しない第2のプロデューサーとして扱われることはない。

**本実装が試みないこと**: `build.rs` が `nim` を呼び出していることの検出や、
2つのプロデューサー間の実在するリンク成果物関係の解決など、ソース・
マニフェスト検査からクロス言語依存を自動的に *推論* することは、
variant/compatibilityモデル (issue #22) の責務であり、本コードの範囲外
である。`project_build.rs` は `--requires`/`--nim-entry`
を通じて呼び出し側が明示的に宣言した内容にのみ基づいて動作する。

## 実際に必要なツールチェーンのみを解決する

`laminaria_fingerprint::doctor::build_selective(lock_path, root, need_rust,
need_nim)` は、呼び出し側がそのツールチェーンファミリーを必要としない
場合、それを解決してから結果を捨てるのではなく、**一切解決しない** —
`rustup` の呼び出しも、`nim`/`nimble` のPATHや `bin_dir`
探索も行わない。これにより、単一言語のみを必要とする対象プロジェクトに
対して「不要なコンパイラの呼び出しを一切試みない」という性質が、
たまたま不要なツールが不在だったからではなく、構造的に保証される。
`doctor::build` (常に両方を必要とする `self-build` が使用) は今や
`build_selective(.., true, true)` の薄いラッパーに過ぎず、既存の呼び出し元
の挙動には一切変更がない純粋なリファクタリングである。

もう一つ見落としやすい漏れの経路: `laminaria-run` 自身の `run_and_record`
は、トレースされた `Run` を記録する際に内部で **もう一度** `doctor::build`
を呼び出しており、`project_build.rs` が事前に必要なツールチェーンのみを
選択的に解決していたとしても、記録の時点で両ファミリーを黙って再探索して
しまう可能性があった。`run_and_record` は今や、既に解決済みの `DoctorRun`
を受け取る `run_and_record_with_doctor` の薄いラッパーであり、
`project_build.rs` はツールチェーン解決時に構築した *同じ* `DoctorRun`
を渡すため、不要なファミリーが記録時に再度探索されることもない。

各 `RootCommand` の `program` には、解決・lock検証済みの正確な
`cargo`/`rustc`/`nim` 実行ファイルが使われ(起動時にPATHから暗黙に解決される
裸の `"cargo"`/`"nim"` ではない)、`RUSTC` はすべての `CargoBuild`
アクションに明示的な環境変数オーバーライドとして設定される — これは
`self-build` 自身の `cargo_build_root` に既に適用されているコンパイラ固定
の修正と同じものであり、新しいコードパスで黙って落とされることなく
引き継がれている。

`project_build.rs` 内のすべてのdoctor/環境呼び出しは `--project-root`
自体をフィンガープリントする対象とする — `self-build`
とは異なり、本モジュールには「LAMINARIA自身のリポジトリルート」という
概念は一切存在しない。`--lock` のパスは呼び出しプロセス自身のカレント
ディレクトリを基準に解決され、`--project-root` とは独立している —
対象プロジェクトが自前の `toolchains.lock.toml` を持つことは想定していない。

## 推測ではなく実際の成果物パスを報告する

`CargoBuild` アクションの `artifacts` は、実際に実行されたCargo自身の
`--message-format=json` テレメトリ (`filenames`/`executable`。すべての
Cargoルートコマンドに対して `run_and_record` が既に無条件でパース済み)
から取得される — `--target <triple>`、カスタムプロファイル、複数の
binターゲットのいずれの下でも正しく、`.build/cargo-target/release`
のような固定推測ディレクトリでは成立しないケースをすべてカバーする。
`NimBuild` アクションの `artifacts` は、本コード自身が渡した `-o:`
パスそのものである — Nimには可変の出力レイアウトという懸念が存在しない
ため、事前に完全に判明している。

## 使い方

```
# プロジェクトファイルから能力を推論する(設定不要):
laminaria plan-build --project-root path/to/rust-only-project --json
laminaria build --project-root path/to/rust-only-project \
  --generation-root /tmp/gen --json

laminaria plan-build --project-root path/to/nim-only-project --json

# プロジェクトファイルが曖昧な場合、またはファイルの存在だけでは表現できない
# 実在の依存関係がある場合の明示指定:
laminaria build --project-root path/to/mixed-project --requires rust,nim \
  --generation-root /tmp/gen --json

laminaria build --project-root path/to/rust-project-with-nim-build-rs \
  --requires rust,nim --generation-root /tmp/gen --json
```

`--json` 失敗時は常に **標準出力**に解析可能な `{"ok": false, "error_kind":
..., "detail": ..., "result": null}` エンベロープを出力する —
このモードで標準エラーへの平文が出ることはない。これにより、スクリプトで
呼び出す側はどちらの結果でも必ず何かを解析できる。

## 検証済みの否定的挙動

以下はすべて自動テストであり (`crates/laminaria-run/src/project_build.rs`、
`crates/laminaria-cli/tests/project_build_cli.rs`)、単なる説明ではない:

- 純粋なRustフィクスチャと純粋なNimフィクスチャは、それぞれ設定なしで
  正しい単一の能力を推論する。
- `--requires` なしで2種類のファイルが同居している場合、両方の候補を
  名指しした曖昧なプロジェクトとして拒否される。
- 解決可能なNimエントリも存在するディレクトリに対して明示的に
  `--requires rust` を指定するとRustのみをビルドし、Nimファミリーは
  一切解決されない(事前の意図だけでなく、`Run` 自身の
  `resolved_toolchain_fingerprint` で直接確認)。
- 同じディレクトリに対して明示的に `--requires rust,nim`
  を指定すると、`Integrate` ステップのない独立した2つのプロデューサー
  アクションが計画される。
- Nimファイルが一切存在しないCargo単体のディレクトリに対して明示的に
  `--requires rust,nim` を指定すると、単一の `CargoBuild`
  アクションが計画される。
- 解決可能なNimエントリが存在しないディレクトリに対して `--requires nim`
  を指定すると、アクション0個を黙って生成するのではなく拒否される。
- 実際のRust単独ビルドは、ビルド自身のPATHの先頭にセンチネルの
  `nim`/`nimble` を配置し、解決が試みられればPATH探索に落ちるよう仕向けた
  lockファイルを使っても、実際のplanner・実際の `cargo` によりエンドツー
  エンドで成功する — センチネルは一度も呼び出されない。
- 実際に壊れたソースは、偽の成功ではなく報告された失敗としてビルドを中断
  させる。
- 混成プロジェクトの `self-build` (stage0 → stage1) は上記のいずれの影響も
  受けない — そもそも `project_build.rs` を一切経由しない。

# Reproducible reference-project source setup

参照コードの取得手順とrevisionは、版管理された
[`reference-projects.lock.json`](../reference-projects.lock.json) と
[`scripts/reference_projects.py`](../scripts/reference_projects.py) に固定する。
既存の `.reference/` にあった12プロジェクトのoriginとHEAD、およびIssue #43の
Hike固定revisionを収録したもので、default branchの最新コードへ追従する仕組みではない。

## セットアップ

必要なのは **Python 3.11以上とGit**。Python外部パッケージ、Make、Nix、Rust/Nimの
コンパイラは不要。Linux/macOS/Windowsで同じスクリプトを使う。
Windowsでは以下の `python3` を `py -3`（または `python`）へ読み替える。

リポジトリルートから実行:

```sh
python3 scripts/reference_projects.py list
python3 scripts/reference_projects.py setup
python3 scripts/reference_projects.py status

# 必要なものだけ取得・検証する（名前は大文字小文字を区別）
python3 scripts/reference_projects.py setup buck2 bazel pants nx
python3 scripts/reference_projects.py status buck2 bazel pants nx

# 取得先を変更。スクリプトを別cwdから呼んでも既定値は本repoの.reference/
python3 scripts/reference_projects.py setup --root /path/to/reference-sources
```

`setup` は指定がなければ13件を順次処理する。1件失敗しても他の選択済みprojectは
処理し、1件でも失敗すると終了コード1を返す。`list` と `status` はclone/fetchせず、
保存先ディレクトリも作らない。認証プロンプトは無効で、HTTPS通信できる環境が必要。
初回は大きなソースツリー（特にLLVMとLinux）の取得容量・時間を見込むこと。

### Linuxソースと大文字小文字の区別

固定したLinux treeには、大文字小文字だけが違うパスが13組ある。
例えば `xt_CONNMARK.h` と `xt_connmark.h` は別ファイル。
大文字小文字を区別しないvolumeでは完全なcheckoutを再現できないため、
**新規取得前に実際の保存先filesystemを検査して拒否する**。
OS名だけで判定しない。小文字化やファイル除外によって成功扱いにもしない。

```sh
# 通常のcase-insensitive volumeではまず残り12件を取得
python3 scripts/reference_projects.py setup --exclude linux
python3 scripts/reference_projects.py status --exclude linux

# Linuxだけはcase-sensitive volumeへ（WindowsならWSLのLinux filesystem等）
python3 scripts/reference_projects.py setup linux --root /case-sensitive/reference-sources
python3 scripts/reference_projects.py status linux --root /case-sensitive/reference-sources
```

既存のローカルLinux cloneには上記パスの変更表示があったため、今回のセットアップ
実装では一切修復・変更していない。変更表示を自動で「無視してよい差分」と判断しない。

## 再現する範囲と保護方針

- URLと40桁commit SHAをlockで固定。`git init` → SHA指定の`fetch --depth=1`
  → 検証 → detached checkout。動くbranch名やtagからrevisionを決めない。
- Git履歴はshallow、上位repoのsource treeは全体を取得する。sparse cloneではない。
  新規checkoutでは改行の自動CRLF変換を無効化する。
- **submoduleは取得しない**（lockの`submodules: none`）。既存cloneと同じ範囲。
  Rust内のLLVM/Cargo、nlvm内のNim/LLVM、nimonyのmimalloc等はgitlinkのみ。
  独立cloneの `llvm-project` は、Rust/nlvmが指定するsubmoduleの代用品ではない。
- upstreamのbuild/installやGit hookは実行しない。これは**参照コードのセットアップ**であり、
  コンパイラのインストール、外部compilerへのtarget compilation委譲ではない。
  実験を再現する際は別途その実験のtoolchain・SDK・runtime契約を用いる。
- 既存ディレクトリはorigin・HEAD・clean状態を検証するだけ。`pull`、`reset`、
  `clean`、checkout切替、既存データの削除は行わない。dirty、違うSHA/URL、
  非Gitディレクトリ、symlinkは失敗として報告し、そのまま残す。
- cloneは同じ保存先内の一意な一時ディレクトリで完成・検証してから公開する。
  通常の失敗ではこの一時領域だけを削除し、再実行できる。
  プロジェクトごとの排他lockで同時setupを防ぐ。
  強制終了後に `.setup-NAME.lock` / `.NAME-setup-*` が残った場合、他のsetupが
  動いていないことと対象を確認して、その残骸だけを手動整理する。自動削除しない。
- 再実行時に同じSHAのclean cloneがあればネットワーク通信も変更も不要。
  lock更新でSHAが変わった場合も既存cloneを自動変更しない。変更を保管したうえで
  別の `--root` にセットアップするのが安全。

GitHubが固定commitを提供しなくなった場合は明示的に失敗する。別commitへfallbackしない。
長期のオフライン保存が必要なら固定commitを保持したGit mirrorを別途保管し、
URLだけをそのmirrorの絶対 `file://` URLにしたlockコピーを `--lock` で指定できる。
取得前にURL/commitの妥当性とproject名の重複・パストラバーサルを検査する。
clone自体やローカル認証情報は本repoへコミットしない。

## 参照先の役割

正確なURL/SHAの唯一の取得設定はlockファイル。以下は読む場所の案内であり、
clone済みであることは全ソースを調査・検証済みという意味ではない。

| 名前 | 参照する実装・研究対象 |
| --- | --- |
| `Nim` | `compiler/extccomp.nim`、`compiler/lineinfos.nim`等、Nim compilerの段階・外部C連携・telemetry |
| `cargo` | `src/util/machine_message.rs`等、依存・build graphとmachine-readable出力 |
| `nimony` | 次世代Nim frontend／compilerの表現・段階比較 |
| `nlvm` | `nlvm/llgen.nim`等、NimからLLVMへのloweringの参照実装 |
| `rust` | `compiler/rustc_codegen_llvm`、`compiler/rustc_target`等、rustc側の意味・target由来の差異 |
| `llvm-project` | LLVM/Clang/LLD、IR・analysis・pass・LTO/ThinLTO/DTLTO（Clangを別cloneしない） |
| `linux` | Kbuild、Makefile、`scripts/kconfig`、`scripts/mod`等、大規模buildの依存構成 |
| `rustc-perf` | `collector/src`、`collector/benchlib/src`等、compiler性能計測 |
| `buck2` | artifact-mediated依存、Dice、action計算、critical path、CAS |
| `bazel` | Skyframe、`SimpleCycleDetector`等、依存計算・cycle診断 |
| `pants` | `src/rust/rule_graph`等、依存推論・決定的graph構築 |
| `nx` | `packages/nx/src/tasks-runner/task-graph-utils.ts`等、task graph・topological order |

元のローカル `.reference/README.md` はignore対象なので他環境に配布されない。
今後の取得手順・参照先案内はこのtracked文書を正本とする。ローカルの旧メモは上書きしない。
Bun等、文書に登場しても現在cloneされていなかったプロジェクトは今回追加していない。
必要なprojectを追加する際は取得元・完全なSHA・参照目的・filesystem制約を確認して
lockと本表を同じ変更で更新する。

## テスト

```sh
python3 -m unittest discover -s scripts/tests -v
```

ローカルGit repoをテストごとに生成するため外部ネットワーク不要。
古い固定commitの取得、再実行、dirty/異なるorigin/異なるHEAD/既存ディレクトリの保護、
失敗後の再試行、選択・除外、filesystem制約、lock競合を確認する。
専用CIでLinux/macOS/Windowsの同じテストを実行し、大規模reference cloneはCIに持ち込まない。

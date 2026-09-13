# Reproducible reference-project source setup

参照コードの取得手順とrevisionは、版管理された
[`reference-projects.lock.json`](../../reference-projects.lock.json) と
[`scripts/reference_projects.py`](../../scripts/reference_projects.py) に固定する。
現在は既存の12プロジェクトに、Rust/C++ test研究で選定した7つのreference
implementationを加えた19プロジェクトを管理する。originと完全commit SHAを固定し、
default branchの最新コードへ追従する仕組みではない。

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

# Rust/C++ test研究のreference setだけを取得・検証
python3 scripts/reference_projects.py setup cargo-nextest googletest Catch2 CMake miri kani libabigail
python3 scripts/reference_projects.py status cargo-nextest googletest Catch2 CMake miri kani libabigail

# 取得先を変更。スクリプトを別cwdから呼んでも既定値は本repoの.reference/
python3 scripts/reference_projects.py setup --root /path/to/reference-sources
```

`setup` は指定がなければ19件を順次処理する。1件失敗しても他の選択済みprojectは
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
# 通常のcase-insensitive volumeではまず残り11件を取得
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
| `cargo-nextest` | per-test process、retry/timeout、reporting、archive、target runnerとtest execution model |
| `googletest` | C++ test registration、assertion、parameterized/death test、gMock、machine-readable report |
| `Catch2` | C++ section/generator/reportingと`catch_discover_tests`によるdynamic discovery |
| `CMake` | CTestのtest model、resource/fixture/repeat/timeout/JUnitとGoogleTest discovery integration |
| `miri` | MIR interpretation、UB検出、isolation、native executionとの差異 |
| `kani` | proof harness、contract、bounded model checking、unsupported/resource-exhaustedの扱い |
| `libabigail` | ELF/DWARF ABI corpus、`abidiff`、dependency-followingとsuppressionの意味 |
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

### Rust/C++ test研究のreference選定境界

7件はtoolの人気順ではなく、Lane Cで欠けていた実装境界ごとに選んだ。
既存の`rust`がlibtest、compiletest、Rust sanitizer integrationを、既存の
`llvm-project`がlit、FileCheck、sanitizer、libFuzzer、object inspection、
`llvm-reduce`を含むため、それらを重複cloneしない。`assert_cmd`、trybuild、
proptest、Loom、coverage、mutation、snapshot、benchmark等は調査対象には残すが、
M1 architectureのreference sourceを増やす目的だけでは個別cloneしない。新しい
counterexampleで固有実装の検証が必要になった時点で、同じlock更新手順を使う。

## Rust/C++ test研究の再現手順

この手順は調査時点のsourceを再現し、後から「別versionの実装を読んだ」ことを防ぐ。
upstreamをbuildしてLAMINARIAのproduction testを代替する手順ではない。

```sh
# 1. lock/schema/保護動作を、本repoの実行可能テストで確認
python3 -m unittest discover -s scripts/tests -p 'test_reference_projects.py' -v

# 2. 選定project、完全SHA、originを表示（network accessなし）
python3 scripts/reference_projects.py list cargo-nextest googletest Catch2 CMake miri kani libabigail

# 3. 完全SHAだけをshallow fetchし、detached checkoutとして取得
python3 scripts/reference_projects.py setup cargo-nextest googletest Catch2 CMake miri kani libabigail

# 4. 以降はnetworkへ出ず、origin/HEAD/clean treeを再検証
python3 scripts/reference_projects.py status cargo-nextest googletest Catch2 CMake miri kani libabigail
```

再現時に読む主な実装境界は次のとおり。

| checkout | 確認する境界 |
| --- | --- |
| `.reference/cargo-nextest/` | testごとのprocess lifecycle、retry/timeout、reportとarchive/runnerのidentity |
| `.reference/googletest/` | registration/listing、test main、death-test process、gMockの境界 |
| `.reference/Catch2/` | test registry、section/generator、reporter、`extras/Catch.cmake`のdiscovery |
| `.reference/CMake/` | CTest execution/resource modelと`Modules/GoogleTest.cmake` |
| `.reference/miri/` | interpreter machine、UB checks、test suiteとnative executionとの差 |
| `.reference/kani/` | proof harness/contractからcompiler/backendへ渡るpathとresult分類 |
| `.reference/libabigail/` | binary/debug infoからABI corpusを構成し比較するpath |

再現完了条件は、7件すべてを`status`が`pinned and clean`と報告し、観察結果が
project名だけでなくlock上のSHAとsource pathに紐付いていることである。取得に成功した
だけでは調査結果の再現にならず、upstream testのPASSだけでもLAMINARIA artifactの
qualificationにはならない。[Rust/C++ test tool調査](../02-research-areas/toolchains/rust-cpp-testing-tool-landscape_ja.md)
のsubject/fault-class比較と、[Lane C contract](../02-research-areas/toolchains/lane-c-executable-verification-foundations_ja.md)
のM0/M1反証実験へ観察を戻す。

初回登録時の2026-09-13には、空の一時rootに対して上記7件の`setup`を実行し、
続くnetwork非依存の`status`が全件`pinned and clean`になること、および表に示した
主要source directory/fileが各checkoutに存在することを確認した。これはその時点の
取得・layout検証であり、将来のremote可用性やupstream build成功を保証する記録ではない。

## テスト

```sh
python3 -m unittest discover -s scripts/tests -v
```

ローカルGit repoをテストごとに生成するため外部ネットワーク不要。
古い固定commitの取得、再実行、dirty/異なるorigin/異なるHEAD/既存ディレクトリの保護、
失敗後の再試行、選択・除外、filesystem制約、lock競合を確認する。
専用CIでLinux/macOS/Windowsの同じテストを実行し、大規模reference cloneはCIに持ち込まない。

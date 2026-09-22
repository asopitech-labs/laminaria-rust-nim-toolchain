# Windowsでのwslcコンテナ開発手順

## 必須規則

Windowsホストからこのプロジェクトを開発する場合、ビルド、テスト、lint、
format check、toolchain診断はすべて `wslc` コンテナ内で実行する。
Windowsホストにある `cargo`、`rustc`、`nim`、`nimble` を直接使用しない。
標準イメージtagは `laminaria-bootstrap:latest` とするが、実行identityには使用しない。
イメージのbuildおよび実行は既定で専用の非rootユーザー `laminaria`
（UID/GID 1000）として行い、root所有の成果物を作らない。

この規則は、開発者のWindows環境に依存しないRust/Nim/LLVM/Wasm toolchainを
使い、`toolchains.lock.toml` と `docker/bootstrap.Dockerfile` の契約を再現するための
ものである。

## 初回セットアップと更新

前提はWSLと `wslc` が利用可能であること。リポジトリルートから実行する。

```powershell
wslc version
scripts/windows-wslc-ci.ps1
```

Dockerfile、`Cargo.toml`、`Cargo.lock`、Nimソース、Rustソース、toolchain lock、
またはビルドに影響するfixtureを変更した後はowner harnessを再実行する。
Docker layer cacheは利用してよい。

`.dockerignore` はGit metadata、参照clone、build/cache/trace出力をbuild contextから除外する。
参照プロジェクトのcloneやホスト側の生成物をイメージへ混入させない。

## 標準コマンド

full modeはfmt、clippy、workspace test、Nim planning-kernel testを同一containerで
逐次実行する。fast/test-onlyは開発中の限定確認であり、full receiptの代替ではない。

```powershell
# Toolchain診断
scripts/windows-wslc-ci.ps1 -Mode doctor

# Repository verification
scripts/windows-wslc-ci.ps1
scripts/windows-wslc-ci.ps1 -Mode fast
scripts/windows-wslc-ci.ps1 -Mode test-only -TestFilter <cargo-test-filter>
```

host制御harness自身のテストだけはWindows上で直接実行する。このテストはfake `wslc`を
注入し、実sessionへ接続せずmutex、service preflight、lease、receiptを検証する。

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/test-windows-wslc-ci.ps1
```

WSLC sessionはVM/daemon/VHDを所有する。このrepositoryはCLIがon-demand作成する
既定のper-user sessionを、全terminal・全worktreeで一つだけ使用する。このsessionを
各process固有の資源として扱ってはならない。
repository automationは必ず `scripts/windows-wslc-ci.ps1` を使用し、
独立した `wslc build` / `wslc run` を並行起動してはならない。

managed agentのcommand sandbox内では、sandbox job境界によりWSLC VM生成が
`E_ACCESSDENIED`になる場合がある。この場合はowner harness全体をsandbox外で
実行する許可を得る。これはUAC昇格ではなく、callerはmedium integrityのままにする。
昇格すると別のdefault session identityになるため、同じsingletonの検証にならない。
userが設定した `session.storagePath` は保持し、sandbox内だけの拒否をVHD配置不良の
根拠にしてはならない。

owner harnessは、HCS serviceが遷移中または外部`wslc` clientが存在する場合は
WSLCを一度も呼ばずfail closedする。build・run・cleanup・receipt発行の全区間を
named per-user mutexで直列化し、すべての呼出しを同じ既定sessionへ送る。buildが出力した
immutable image IDをそのままrunへ渡し、呼出しごとに一意なcontainer名を割り当てる。
正常に戻った`run --rm`だけが自身のcontainerを削除し、二重cleanupは行わない。
owner processが強制終了した場合だけ、次のownerが共有leaseに記録された厳密な
container名を回収する。回収に失敗したleaseは保持し、新しいworkを開始しない。Windowsの
Git hookは同一source fingerprintのreceiptを検査し、新たなWSLC consumerを起動しない。
`--rm`はcontainer lifecycleでありsynchronizationではない。mutable tagは実行identity
ではない。`wsl --shutdown`、`wslcsession` kill、`WSLService` stopは通常cleanupに使わない。

恒久的に必要なツールはDockerfileまたはrepository-owned lockへ追加し、owner
harnessからイメージを再構築する。
`--user root` を指定して通常のbuild/testを実行してはならない。
`nimble test` はdependency解決によって別のNim compilerを取得する場合があるため、
標準のNimテスト経路では使用せず、`scripts/local-ci.sh` が固定済み2.2.10を
`nim c -r` で直接使う。fixture専用validatorではなく、これらのテストがproduction
planning実装を直接検証する。

## ソース変更を反映する方法

標準手順はowner harnessが現在のcheckoutを `COPY` してbuildし、`--iidfile` が
返したexact imageを同じlock ownership内で実行する方式である。ホストcheckoutの
bind mountを標準経路にはしない。harnessはlock取得後と実行後にもsource fingerprintを
検査し、途中で入力が変わったrunにはreceiptを発行しない。

## 性能計測の扱い

この手順はWindowsから行う通常開発とbootstrap/correctness再現の標準である。
`doctor` は `environment_class = "container"` を記録する。コンテナ結果を
native Windowsのcanonical performance baselineとして扱ってはならない。
性能計測を行う場合は `docs/02-research-areas/measurement/measurement-foundation.md` の環境分離・比較可能性規則に従う。

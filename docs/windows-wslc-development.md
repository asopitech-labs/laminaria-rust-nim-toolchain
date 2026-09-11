# Windowsでのwslcコンテナ開発手順

## 必須規則

Windowsホストからこのプロジェクトを開発する場合、ビルド、テスト、lint、
format check、toolchain診断はすべて `wslc` コンテナ内で実行する。
Windowsホストにある `cargo`、`rustc`、`nim`、`nimble` を直接使用しない。
標準イメージ名は `laminaria-bootstrap:latest` とする。
イメージのbuildおよび実行は既定で専用の非rootユーザー `laminaria`
（UID/GID 1000）として行い、root所有の成果物を作らない。

この規則は、開発者のWindows環境に依存しないRust/Nim/LLVM/Wasm toolchainを
使い、`toolchains.lock.toml` と `docker/bootstrap.Dockerfile` の契約を再現するための
ものである。

## 初回セットアップと更新

前提はWSLと `wslc` が利用可能であること。リポジトリルートから実行する。

```powershell
wslc version
wslc build --progress plain -f docker/bootstrap.Dockerfile -t laminaria-bootstrap .
wslc run --rm --pull never laminaria-bootstrap doctor
```

Dockerfile、`Cargo.toml`、`Cargo.lock`、Nimソース、Rustソース、toolchain lock、
またはビルドに影響するfixtureを変更した後は、コマンド実行前に同じ
`wslc build` を再実行する。Docker layer cacheは利用してよい。

`.dockerignore` は `.git/`、`.reference/`、`target/` をbuild contextから除外する。
参照プロジェクトのcloneやホスト側の生成物をイメージへ混入させない。

## 標準コマンド

イメージのentrypointはLAMINARIA CLIである。CLI以外のツールを実行するときは
`--entrypoint` を明示する。

```powershell
# Toolchain診断
wslc run --rm --pull never laminaria-bootstrap doctor

# Rust workspace
wslc run --rm --pull never --entrypoint cargo laminaria-bootstrap build --workspace
wslc run --rm --pull never --entrypoint cargo laminaria-bootstrap test --workspace
wslc run --rm --pull never --entrypoint cargo laminaria-bootstrap clippy --workspace --all-targets -- -D warnings
wslc run --rm --pull never --entrypoint cargo laminaria-bootstrap fmt --all -- --check

# Nim planning kernel（固定済みNimを直接使用）
wslc run --rm --pull never --entrypoint nim -w /workspace/nim-planner laminaria-bootstrap c -r --path:src --hints:off -o:bin/test_planning_kernel tests/test_planning_kernel.nim
```

コンテナは実行ごとに `--rm` で破棄し、ツールや依存を対話的に追加して
状態を残さない。恒久的に必要なツールはDockerfileまたはrepository-owned lockへ
追加し、イメージを再構築する。
`--user root` を指定して通常のbuild/testを実行してはならない。
`nimble test` はdependency解決によって別のNim compilerを取得する場合があるため、
標準のNimテスト経路では使用せず、上記の `nim c -r` で固定済み2.2.10を直接使う。

## ソース変更を反映する方法

標準手順は、現在のcheckoutを `COPY` する `wslc build` を再実行してから、
生成されたイメージに対して `wslc run` を実行する方式である。ホストcheckoutの
bind mountを標準経路にはしない。これにより、テスト対象のソースとイメージ内の
ビルド済み成果物が同じbuild contextに由来することを保つ。

## 性能計測の扱い

この手順はWindowsから行う通常開発とbootstrap/correctness再現の標準である。
`doctor` は `environment_class = "container"` を記録する。コンテナ結果を
native Windowsのcanonical performance baselineとして扱ってはならない。
性能計測を行う場合は `docs/measurement-foundation.md` の環境分離・比較可能性規則に従う。

# LAMINARIA

**Rust Nim Unified Toolchain**

LAMINARIA researches and develops **its own compiler, IRs, and scheduler for Rust and Nim**. It is not an orchestrator whose compilation engine is Cargo/rustc, Nim, or LLVM. Rust-only, Nim-only and mixed source use the same LAMINARIA-owned compilation substrate.

`Source + resolved dependencies → owned semantic analysis / IR → owned transformations and compiler-work planning → resource-aware execution → owned target generation`

Cargo/Nim ecosystem tools may supply package/dependency resolution. Existing compilers and backends are separately identified reference/observation and external-bootstrap tools, not target-build fallbacks. LLVM is prior art whose design pressures are independently re-derived, not a fixed foundation or an optional route that substitutes for compiler ownership.

LAMINARIA itself is implemented in Rust and Nim: the **Nim Planning Kernel** owns deterministic graph/constraint computation; the **Rust Runtime Scheduler** owns execution, OS effects and live resource accounting. The ultimate self-hosting goal is to compile this Rust + Nim implementation and its dependencies using LAMINARIA's own compiler.

Vertical integration and horizontal distribution are joint research subjects: compiler work, analysis lifetime, memory/cache/NUMA, storage/network, persistence and heterogeneous host/target placement must be planned together. Single-language speedups are an evaluation opportunity, not a prerequisite or a claim of current performance.

## Current implementation versus goal

The current `build`/`plan-build` and `self-build` commands still schedule coarse Cargo/Nim actions sequentially. They are **delegated-build/bootstrap baselines**, not the intended compiler or independent self-hosting. The semantic-substrate fixture contains hand-authored IR, a limited evaluator/transform and an LLVM comparison projection; it is not a Rust/Nim source compiler.

See the [compiler ownership contract](docs/compiler-ownership-contract.md), [日本語の責務契約](docs/compiler-ownership-contract_ja.md), and [audit and corrected issue order](docs/research-intent-audit-2026-09-10.md). The next core slice is **#25 + #3 + #6 + #8**, with evidence infrastructure built alongside it. [Project-build](docs/project-build.md) and [self-build](docs/self-build.md) retain accurate instructions for the current baseline commands; this documentation correction does not change their behavior.

## Core concepts

- Unified Program Graph
- Compiler Pipeline Decomposition
- Multi-version Rust / Nim Toolchain Variants
- Validated Toolchain Profiles / Progressive Configuration
- Agent-Oriented Toolchain UX / Bounded Explainable Planning
- Backend Route Selection
- Backend Pipeline White-boxing
- Artifact Graph
- Action Graph
- Measurement Spine / Environment Fingerprinting
- Combinatorial Graph Resolution
- Codegen Unit Scheduling
- Incremental Compiler Graph
- FFI as a Graph Primitive
- Rust–Nim Native Linking
- Unified Cache Identity
- Nim Planning Kernel
- Rust Runtime Scheduler
- Cross-Language Critical Path
- WebAssembly Target Pipeline
- Agent-Oriented / Explainable Toolchain

## Environment and toolchain setup

The first permanent code in this repository is the environment/toolchain identity layer for issue [#18](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/18): a repository-owned multi-toolchain lock and a `doctor` command that resolves it to exact `EnvironmentFingerprint`/`ToolchainFingerprint` records.

```bash
scripts/bootstrap.sh              # report missing tools against toolchains.lock.toml
scripts/bootstrap.sh --install    # also install them (macOS/Homebrew + rustup)

cargo run -p laminaria-cli -- doctor          # human-readable environment/toolchain report
cargo run -p laminaria-cli -- doctor --json   # machine-readable EnvironmentFingerprint/ToolchainFingerprint
```

Named toolchain selectors live in [`toolchains.lock.toml`](toolchains.lock.toml); `rust-toolchain.toml` only pins the toolchain used to build LAMINARIA's own Rust code, not the toolchains under measurement.

`scripts/bootstrap.sh` prefers exact, non-system-package-manager sources where one exists (`rustup` toolchains + its `llvm-tools` component, `choosenim` for exact Nim versions, `cargo install --version` for pure-Rust CLI tools); a system package manager is only used for the couple of tools with no such alternative (`clang`/`llvm-config`, Binaryen's `wasm-opt`). For a fully reproducible bootstrap independent of any one host's package manager state — e.g. to sanity-check the toolchain set on a clean machine — [`docker/bootstrap.Dockerfile`](docker/bootstrap.Dockerfile) builds and runs the same stack in a container:

### Required Windows development path

Development initiated from Windows must use the `wslc` container path for all
builds, tests, checks, linting, formatting checks, and toolchain diagnostics.
Do not run the host Windows Rust or Nim toolchain directly for this project.

```powershell
wslc build --progress plain -f docker/bootstrap.Dockerfile -t laminaria-bootstrap .
wslc run --rm --pull never laminaria-bootstrap doctor
```

The complete lifecycle and canonical commands are in the
[Windows `wslc` container development procedure](docs/windows-wslc-development.md).

Per `docs/measurement-foundation.md` §2, this is for bootstrap/correctness reproduction only — canonical performance measurements should run natively on the host being measured, and `doctor` records `environment_class = "container"` so such runs are never silently compared to a native baseline.

## Documentation

### Reference-project source setup

The 12 locally used prior-art repositories are pinned by full commit SHA in
[`reference-projects.lock.json`](reference-projects.lock.json). With Python 3.11+
and Git, reproduce their source checkouts without installing any compiler:

```sh
python3 scripts/reference_projects.py setup buck2 bazel pants nx  # selected projects
python3 scripts/reference_projects.py setup                     # all 12
python3 scripts/reference_projects.py status                    # offline verification
```

Existing clones are verified, never reset or overwritten. Linux's source tree
requires a case-sensitive volume; use `--exclude linux` on other volumes and
set up Linux separately with `--root`. Submodules are not fetched.
See [the setup procedure and reference map](docs/reference-projects.md) for
Windows commands, destination selection, recovery, and revision updates.

### Research and implementation documents

- [Hike極小Wasmコンパイラのエンジニアリング研究](docs/hike-micro-wasm-engineering-research_ja.md)
- [Windows `wslc` container development procedure](docs/windows-wslc-development.md)
- [Agent-oriented toolchain UX and bounded planning (English)](docs/agent-oriented-toolchain-ux.md)
- [エージェント指向ツールチェーンUX・探索抑制方針 (日本語)](docs/agent-oriented-toolchain-ux_ja.md)
- [Validated toolchain profiles and progressive configuration (English)](docs/validated-toolchain-profiles.md)
- [検証済みツールチェーンプロファイルと段階設定 (日本語)](docs/validated-toolchain-profiles_ja.md)
- [Multi-version Rust/Nim toolchain policy (English)](docs/multi-version-toolchains.md)
- [Rust/Nim複数コンパイラバージョン対応方針 (日本語)](docs/multi-version-toolchains_ja.md)
- [Measurement foundation and environment/trace strategy (English)](docs/measurement-foundation.md)
- [計測基盤・環境・処理フロー観測方針 (日本語)](docs/measurement-foundation_ja.md)
- [Backend pipeline white-boxing research direction (English)](docs/backend-pipeline-whiteboxing.md)
- [Backend Pipeline White-boxing 研究方針 (日本語)](docs/backend-pipeline-whiteboxing_ja.md)
- [Research foundations and architecture direction (English)](docs/research-foundations.md)
- [研究基盤とアーキテクチャ方針 (日本語)](docs/research-foundations_ja.md)
- [Research program and evidence policy (English)](docs/research-program.md)
- [研究プログラムと証拠ポリシー (日本語)](docs/research-program_ja.md)
- [Rust–Nim native linking research plan (English)](docs/rust-nim-native-linking.md)
- [Rust–Nim Native Linking 研究計画 (日本語)](docs/rust-nim-native-linking_ja.md)
- [Metrics-first research policy](docs/metrics-policy.md)
- [Research issue plan](docs/issue-plan.md)
- [Project proposal (English)](docs/project-proposal.md)
- [Project proposal (Japanese)](docs/project-proposal_ja.md)

## License

LAMINARIA, including its Rust implementation and Nim Planning Kernel, is licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.

Third-party components remain subject to their respective licenses. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

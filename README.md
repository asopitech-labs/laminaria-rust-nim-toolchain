# LAMINARIA

**Rust Nim Unified Toolchain**

LAMINARIA researches and develops a **cross-ecosystem dependency resolver, compiler, IRs, and scheduler for Rust and Nim**. Its current delivery goal is an ordinary runnable native binary for which Cargo/Nimble/C/C++ package choices, source semantics, language/intermediate IR, ABI, symbols, and link obligations are jointly resolved and discharged or explicitly externalized. It is not an orchestrator whose compilation engine is Cargo/rustc, Nim, or LLVM.

`Native executable demand → Cargo/Nimble/C/C++ package constraints ↔ owned source semantics / language and intermediate IR ↔ artifact/ABI/symbol/link constraints → obligation discharge / explicit externalization → runnable native artifact`

Cargo/Nimble/C/C++ ecosystem tools may supply metadata, lockfiles, sources, and system-library facts. LAMINARIA must normalize and solve the combined graph rather than treating a package manager's opaque build as resolution. Existing compilers and backends are separately identified reference/observation and external-bootstrap tools, not hidden Rust/Nim target-build fallbacks.

LAMINARIA itself is implemented in Rust and Nim: the **Nim Planning Kernel** owns deterministic graph/constraint computation; the **Rust Runtime Scheduler** owns execution, OS effects and live resource accounting. The ultimate self-hosting goal is to resolve and compile this Rust + Nim implementation and its transitive native dependencies through the same path.

WebAssembly is an optional future target and comparison surface. It is not the current goal or a prerequisite for the native executable milestone.

Vertical integration and horizontal distribution are joint research subjects: compiler work, analysis lifetime, memory/cache/NUMA, storage/network, persistence and heterogeneous host/target placement must be planned together. Single-language speedups are an evaluation opportunity, not a prerequisite or a claim of current performance.

## Current implementation versus goal

The current `build`/`plan-build` and `self-build` commands still schedule coarse Cargo/Nim actions sequentially. They are **delegated-build/bootstrap baselines**, not the intended compiler or independent self-hosting. The semantic-substrate fixture contains hand-authored IR, a limited evaluator/transform and an LLVM comparison projection; it is not a Rust/Nim source compiler.

Start with the [project progression and near-term research goal](docs/near-term-research-program.md) ([日本語](docs/near-term-research-program_ja.md)). It is the canonical source for the current milestone and task order. The [project work portfolio](docs/03-work-items/project-portfolio.md) tracks whole-project outcomes, capabilities, unissued gaps, implementation, verification, release, and later expansion. The [documentation map](docs/README.md) then leads to foundations, research areas, issue-specific work, operational guides, and history. GitHub issues are bounded tracking projections, not the project backlog itself.

## Core concepts

- Cross-Ecosystem Dependency Graph
- Cargo / Nimble / C / C++ Resolution
- Native Executable Production
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
- WebAssembly Target Pipeline (optional target research)
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

`scripts/bootstrap.sh` prefers exact, non-system-package-manager sources where one exists (`rustup` toolchains + its `llvm-tools` component, `choosenim` for exact Nim versions, `cargo install --version` for pure-Rust CLI tools); a system package manager is only used for the couple of tools with no such alternative (`clang`/`llvm-config`, Binaryen's `wasm-opt`). The research lock also inventories tools for optional tracks such as WASM; their presence does not make those tracks current milestones. For a fully reproducible bootstrap independent of any one host's package manager state — e.g. to sanity-check the toolchain set on a clean machine — [`docker/bootstrap.Dockerfile`](docker/bootstrap.Dockerfile) builds and runs the same stack in a container:

### Required Windows development path

Development initiated from Windows must use the `wslc` container path for all
builds, tests, checks, linting, formatting checks, and toolchain diagnostics.
Do not run the host Windows Rust or Nim toolchain directly for this project.

```powershell
scripts/windows-wslc-ci.ps1
```

This owner harness serializes every repository build/test use of the shared
per-user WSLC session, runs the exact image ID produced by its build, cleans up
only its uniquely named container, and publishes a source-bound verification
receipt for the Windows Git hooks. Do not run independent `wslc build`/`run`
clients in parallel from other terminals or worktrees.

The complete lifecycle and canonical commands are in the
[Windows `wslc` container development procedure](docs/04-guides/windows-wslc-development.md).

Per `docs/02-research-areas/measurement/measurement-foundation.md` §2, this is for bootstrap/correctness reproduction only — canonical performance measurements should run natively on the host being measured, and `doctor` records `environment_class = "container"` so such runs are never silently compared to a native baseline.

## Documentation

The documentation is intentionally rooted at the [project progression and near-term research goal](docs/near-term-research-program.md). See the [documentation map](docs/README.md) for the reading order and complete directory structure. For implemented behavior, direct executable tests against the production Rust/Nim code are authoritative; the [fixture policy](docs/01-foundations/fixture-policy.md) defines when a fixture is a legitimate input, state, counterexample, workload, or artifact subject rather than a duplicate specification.

### Reference-project source setup

The 19 locally used prior-art repositories are pinned by full commit SHA in
[`reference-projects.lock.json`](reference-projects.lock.json). With Python 3.11+
and Git, reproduce their source checkouts without installing any compiler:

```sh
python3 scripts/reference_projects.py setup buck2 bazel pants nx  # selected projects
python3 scripts/reference_projects.py setup cargo-nextest googletest Catch2 CMake miri kani libabigail  # test research set
python3 scripts/reference_projects.py setup                     # all 19
python3 scripts/reference_projects.py status                    # offline verification
```

Existing clones are verified, never reset or overwritten. Linux's source tree
requires a case-sensitive volume; use `--exclude linux` on other volumes and
set up Linux separately with `--root`. Submodules are not fetched.
See [the setup procedure and reference map](docs/04-guides/reference-projects.md) for
Windows commands, destination selection, recovery, and revision updates.

## License

LAMINARIA, including its Rust implementation and Nim Planning Kernel, is licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.

Third-party components remain subject to their respective licenses. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

# LAMINARIA

**Rust Nim Unified Toolchain**

LAMINARIA is a research and development project for a unified computational model and toolchain for Rust and Nim. It decomposes dependency resolution, compiler pipelines, backend selection and backend-internal pipelines, artifact production, linking, caching, and execution into a single explainable action graph.

It does not treat Cargo, `rustc`, Nimble, Nim, LLVM, LTO, linkers, or WebAssembly tooling as opaque commands joined by an outer build script when finer boundaries are observable and useful. LAMINARIA models the path from source programs to executable actions:

`Source Graph → Compiler Pipeline → Unified Program Graph → Variant / Artifact Graph → Backend Pipeline Graph → Action Graph → Nim Planning Kernel → Rust Runtime Scheduler`

Rust compiler stages—including HIR, MIR, monomorphization, codegen units, `rustc_codegen_ssa`, LLVM, Cranelift, GCC backends, LTO, object generation, and linking—are studied alongside Nim frontend and semantic processing, backend generation, generated native source, native compilation, object generation, and linking.

Compiler version is a first-class graph dimension rather than an ambient machine setting. LAMINARIA is designed to support multiple exact Rust toolchains as well as Nim 2 and Nim 3/Nimony, resolving package/workspace constraints into a selected toolchain while preserving the producing compiler/toolchain identity in Run, Action, Artifact, cache, and compatibility decisions. Cross-version compiler-semantic artifacts are not assumed compatible without explicit evidence.

The internal variant space may be broad, but ordinary users should not need to solve exact Rust/Nim/backend/linker combinations manually. LAMINARIA exposes evidence-backed **Validated Toolchain Profiles** such as `recommended`, `latest-validated`, `long-term`, and `preview`, then progressively exposes intent presets, advanced overrides, and expert graph constraints. Profile aliases always resolve to immutable exact bundle revisions before execution; overrides re-evaluate qualification rather than inheriting a validation badge blindly.

**Tool UX is itself a project objective.** LAMINARIA must not turn its combinatorial flexibility into a requirement for humans or coding agents to repeatedly edit configuration, run a build, parse a failure, and try another compiler/backend combination. The normal path is constraint resolution and pruning first, then a small ranked set of evidence-backed viable plans, then execution. Known incompatibilities and qualification failures are preserved as structured negative knowledge so equivalent failures do not need to be rediscovered across runs or agent sessions. Broad brute-force exploration remains available explicitly for research, not as the default UX.

LLVM is neither excluded nor treated as the fixed foundation. LLVM, Cranelift, GCC, and other code-generation routes are selectable backend engines, and selected routes may expand into observable nested backend pipelines. WebAssembly is modeled as a target pipeline that can include code generation, `wasm-ld`, post-link optimization, WIT/adapters, and componentization rather than as a peer backend value to LLVM.

Before LAMINARIA optimizes these paths, it establishes a permanent measurement spine that fingerprints the real environment/toolchains and records end-to-end process/resource traces, compiler-native telemetry, artifact deltas, scenario/cache state, and measurement overhead. The same evidence model is then reused by compiler, backend, scheduler, cache, profile qualification, agent-oriented UX, and WASM research.

LAMINARIA is itself implemented in Rust and Nim:

- **Nim Planning Kernel:** graph normalization, constraint solving, combinatorial resolution, artifact-demand propagation, pruning, critical-path analysis, and planning optimization.
- **Rust Runtime Scheduler:** CLI, toolchain discovery, operating-system interaction, process execution, resource accounting, cache/CAS, daemon services, measurement/tracing infrastructure, and scheduling.

The boundary is **computation and planning vs. execution and side effects**, not “Rust processing vs. Nim processing.”

**Rust and Nim, as one computational graph.**

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

## Documentation

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

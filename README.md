# LAMINARIA

**Rust Nim Unified Toolchain**

LAMINARIA is a research and development project for a unified computational model and toolchain for Rust and Nim. It decomposes dependency resolution, compiler pipelines, backend route selection and backend-internal pipelines, artifact production, linking, caching, and execution into a single explainable action graph.

It does not treat Cargo, `rustc`, Nimble, Nim, LLVM, linkers, or WebAssembly post-link tooling as inherently opaque commands joined by an outer build script. LAMINARIA models the path from source programs to executable actions:

`Source Graph → Compiler Pipeline → Unified Program Graph → Variant / Artifact Graph → Backend Pipeline Graph → Action Graph → Nim Planning Kernel → Rust Runtime Scheduler`

Rust compiler stages—including HIR, MIR, monomorphization, codegen units, `rustc_codegen_ssa`, LLVM, Cranelift, GCC backends, object generation, and linking—are studied alongside Nim frontend and semantic processing, backend generation, generated native source, native compilation, object generation, and linking.

LLVM is neither excluded nor treated as the fixed foundation. LLVM, Cranelift, GCC, and other code-generation routes are selectable components of the **Backend Graph**, and selected routes may expand into observable nested backend pipelines. LAMINARIA does not equate white-boxing with one process per compiler pass: logical and observation boundaries are separated from materialized checkpoint and execution boundaries.

WebAssembly is modeled as a **Target Pipeline**, not as a peer backend value to LLVM or Cranelift. Code generation, relocatable Wasm objects, `wasm-ld`, post-link optimization such as Binaryen, WIT/adaptation, and componentization are separate candidate stages and artifacts.

LAMINARIA is itself implemented in Rust and Nim:

- **Nim Planning Kernel:** graph normalization, constraint solving, combinatorial resolution, artifact-demand propagation, pruning, critical-path analysis, and planning optimization.
- **Rust Runtime Scheduler:** CLI, toolchain discovery, operating-system interaction, process execution, resource accounting, cache/CAS, daemon services, and scheduling.

The boundary is **computation and planning vs. execution and side effects**, not “Rust processing vs. Nim processing.”

**Rust and Nim, as one computational graph.**

## Core concepts

- Unified Program Graph
- Compiler Pipeline Decomposition
- Backend Route Selection
- Backend Pipeline White-boxing
- Backend Pipeline Graph
- Dynamic Graph Expansion
- ThinLTO / DTLTO Graph Integration
- WebAssembly Target Pipeline
- Artifact Graph
- Action Graph
- Combinatorial Graph Resolution
- Codegen Unit Scheduling
- Incremental Compiler Graph
- FFI as a Graph Primitive
- Rust–Nim Native Linking
- Unified Cache Identity
- Nim Planning Kernel
- Rust Runtime Scheduler
- Cross-Language Critical Path
- Agent-Oriented / Explainable Toolchain

## Documentation

- [Research foundations and architecture direction (English)](docs/research-foundations.md)
- [Research program and evidence policy (English)](docs/research-program.md)
- [研究プログラムと証拠ポリシー (日本語)](docs/research-program_ja.md)
- [Backend pipeline white-boxing research direction (English)](docs/backend-pipeline-whiteboxing.md)
- [Backend Pipeline White-boxing 研究方針 (日本語)](docs/backend-pipeline-whiteboxing_ja.md)
- [Rust–Nim native linking research plan (English)](docs/rust-nim-native-linking.md)
- [Rust–Nim Native Linking 研究計画 (日本語)](docs/rust-nim-native-linking_ja.md)
- [Project proposal (English)](docs/project-proposal.md)
- [Project proposal (Japanese)](docs/project-proposal_ja.md)

## License

LAMINARIA, including its Rust implementation and Nim Planning Kernel, is licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.

Third-party components remain subject to their respective licenses. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

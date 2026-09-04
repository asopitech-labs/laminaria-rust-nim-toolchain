# LAMINARIA

**Rust Nim Unified Toolchain**

LAMINARIA is a research and development project for a unified computational model and toolchain for Rust and Nim. It decomposes dependency resolution, compiler pipelines, backend selection, artifact production, linking, caching, and execution into a single explainable action graph.

It does not treat Cargo, `rustc`, Nimble, and Nim as opaque commands joined by an outer build script. LAMINARIA models the path from source programs to executable actions:

`Source Graph → Compiler Pipeline → Unified Program Graph → Variant / Artifact Graph → Action Graph → Nim Planning Kernel → Rust Runtime Scheduler`

Rust compiler stages—including HIR, MIR, monomorphization, codegen units, `rustc_codegen_ssa`, LLVM, Cranelift, GCC backends, object generation, and linking—are studied alongside Nim frontend and semantic processing, backend generation, generated native source, native compilation, object generation, and linking.

LLVM is neither excluded nor treated as the fixed foundation: LLVM, Cranelift, GCC, and other code-generation routes are selectable components of the **Backend Graph**.

LAMINARIA is itself implemented in Rust and Nim:

- **Nim Planning Kernel:** graph normalization, constraint solving, combinatorial resolution, artifact-demand propagation, pruning, critical-path analysis, and planning optimization.
- **Rust Runtime Scheduler:** CLI, toolchain discovery, operating-system interaction, process execution, resource accounting, cache/CAS, daemon services, and scheduling.

The boundary is **computation and planning vs. execution and side effects**, not “Rust processing vs. Nim processing.”

**Rust and Nim, as one computational graph.**

## Core concepts

- Unified Program Graph
- Compiler Pipeline Decomposition
- Backend Graph
- Artifact Graph
- Action Graph
- Combinatorial Graph Resolution
- Codegen Unit Scheduling
- Incremental Compiler Graph
- FFI as a Graph Primitive
- Unified Cache Identity
- Nim Planning Kernel
- Rust Runtime Scheduler
- Cross-Language Critical Path
- Agent-Oriented / Explainable Toolchain

## Documentation

- [Project proposal (Japanese)](docs/project-proposal.md)

## License

LAMINARIA, including its Rust implementation and Nim Planning Kernel, is licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.

Third-party components remain subject to their respective licenses. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

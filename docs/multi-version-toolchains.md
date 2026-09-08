# Multi-version Toolchains Research Direction

## Purpose

LAMINARIA treats multiple compiler/toolchain versions as first-class graph variants for both Rust and Nim. Nim 2/Nim 3 support is not a special case: Rust must also support multiple exact compiler/toolchain versions.

Multiple-version support is not merely an installer feature. Compiler version affects frontend semantics, internal IRs, backend routes, bundled LLVM, metadata formats, telemetry capabilities, cache identity, and artifact compatibility. It therefore belongs in the Variant Graph and Artifact Graph.

Central rule:

> Compiler version is a graph dimension, not an ambient machine setting.

## 1. Version is an entry point, but its identity must survive normalization

```text
Rust source -> Rust Toolchain Adapter(version/build) --+
                                                      +-> Unified Program / Artifact / Action Graph
Nim source  -> Nim Toolchain Adapter(version/build) --+
```

LAMINARIA may normalize useful planning relationships across compiler versions, but the producing toolchain identity remains typed metadata. Different versions are not assumed to have identical semantics or artifact compatibility.

## 2. Add compiler toolchain to the Variant Graph

```text
package
x language
x compiler toolchain
x target
x profile
x feature set
x edition/language mode
x backend route
x native compiler/linker
x artifact kind
x cross-language boundary
```

A Rust toolchain identity may distinguish exact stable releases, exact beta/nightly dates or revisions, source-built rustc revisions, Cargo build identity, components, sysroot/standard-library identity, bundled/selected LLVM or codegen backend identity, and host/target components.

A Nim toolchain identity may distinguish exact Nim 2 versions/revisions, Nimble, Nimony/Nim 3 revisions/builds, nlvm revisions, and downstream backend/compiler identities.

Moving labels such as `stable`, `nightly`, or `latest` are selectors, not sufficient artifact/cache identities. Every Run resolves them to an exact ToolchainFingerprint.

## 3. Separate Rust `rust-version`, edition, and selected compiler

Cargo's `rust-version` field expresses a package's minimum supported Rust version and can participate in toolchain selection constraints.

Keep these dimensions separate:

- `rust-version`: package MSRV declaration;
- Rust edition: source/language mode;
- selected rustc toolchain: compiler that actually runs;
- Cargo resolver behavior: workspace dependency-resolution semantics;
- nightly-feature requirements: capability constraints that may reject stable toolchains.

Reference: https://doc.rust-lang.org/cargo/reference/rust-version.html

## 4. Default Rust multi-version execution model

LAMINARIA may install, discover, and measure many Rust toolchains concurrently, but one connected Cargo/Rust crate compilation graph normally selects one Rust toolchain.

```text
Rust workspace / connected crate graph
  -> Toolchain Constraint Resolution
  -> selected rustc/cargo toolchain
  -> compiler pipeline
```

Do not connect crate metadata, `rlib`, or `rmeta` produced by different rustc versions through ordinary Rust dependency edges without explicit compatibility evidence. rustc metadata carries version/format information and is not treated as a cross-version stable interchange format by LAMINARIA.

Reference: https://doc.rust-lang.org/stable/nightly-rustc/rustc_metadata/rmeta/index.html

## 5. Cross-version composition is an explicit artifact-boundary experiment

LAMINARIA does not forbid multiple rustc versions from contributing to one final native artifact, but this is modeled through explicit native artifacts/ABI contracts rather than implicit Rust metadata compatibility.

```text
Rust(toolchain A) -> native object/staticlib --+
                                                +-> Link -> Final Artifact
Rust(toolchain B) -> native object/staticlib --+
```

Such experiments must inspect object format, target triple, symbol/calling convention, allocator/runtime ownership, panic/unwind behavior, standard-library/runtime duplication, LTO compatibility, debug/unwind metadata, and linker behavior.

Successful linking is not evidence of a stable Rust language ABI.

## 6. Toolchain Adapter Capability Model

Compiler versions expose different integration boundaries. LAMINARIA records capabilities per resolved toolchain, for example:

```text
ToolchainCapability
  frontend_observation
  semantic_boundary
  mir_or_equivalent_access
  codegen_unit_visibility
  llvm_ir_or_bitcode_emission
  self_profile_support
  optimization_remark_support
  lto_modes
  backend_routes
  wasm_targets
  artifact_formats
```

Unavailable boundaries remain explicit opaque/coarse-grained regions.

## 7. Cache and artifact identity

Compiler-semantic artifact identity includes the producing toolchain and relevant backend/runtime identity. At minimum consider:

```text
language
compiler family
exact compiler version/revision/build
Cargo/Nimble identity where semantically relevant
frontend/adapter version
standard library/sysroot identity
backend engine/version
bundled LLVM/backend revision where relevant
target/profile/features
pass/LTO configuration
input/dependency artifact identities
```

Coincidentally equal content digests across compiler versions do not by themselves prove semantic compatibility. Reuse policy is artifact-kind-specific and must fail closed when compatibility is unknown.

## 8. Measurement Foundation integration

`toolchains.lock.toml` must describe a named set of toolchains rather than one global Rust stable/nightly pair.

Conceptual schema:

```toml
[rust.toolchains.release_a]
selector = "<exact release>"
components = ["rustc", "cargo", "rust-std"]

[rust.toolchains.nightly_a]
selector = "<exact nightly date or revision>"
components = ["rustc", "cargo", "rust-std", "rust-src"]

[nim.toolchains.nim2_a]
selector = "<exact version or revision>"

[nim.toolchains.nimony_a]
revision = "<exact source revision>"
```

Names and versions are schema examples, not a fixed support policy.

Each Run stores both the requested selector and the resolved ToolchainFingerprint.

## 9. Version matrices as research workloads

The same workload should be executable across multiple compiler versions to compare:

- total build latency;
- frontend/semantic/codegen/backend time;
- codegen-unit structure;
- LLVM/pass-pipeline changes;
- incremental invalidation;
- CPU/memory/I/O;
- artifact size;
- generated-code runtime/size;
- cache identity/reuse opportunities;
- telemetry availability.

Compiler-version comparisons should normally hold EnvironmentFingerprint, scenario, and cache state constant.

## 10. Success criteria

Multi-version support is not complete merely because several compilers can be installed. LAMINARIA should be able to:

1. represent Rust and Nim compiler versions in the Variant Graph;
2. evaluate package/workspace requirements against candidate toolchains;
3. propagate the resolved toolchain into Run/Action/Artifact identity;
4. explain per-version pipeline capabilities and opaque boundaries;
5. reject cross-version artifact reuse when compatibility is unproven;
6. reproducibly measure one workload across multiple Rust versions;
7. model Nim 2/Nim 3 and multiple Rust versions through the same toolchain-version abstraction.

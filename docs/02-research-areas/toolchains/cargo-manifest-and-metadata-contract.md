# Cargo manifest and metadata contract

## Names and authority

Cargo's package/workspace manifest is `Cargo.toml`; `cargo.yaml` is not a
Cargo-defined manifest format. Cargo's separate configuration convention is
`.cargo/config.toml` (or `.cargo/config`). These files have different roles:
the manifest describes package, target, dependency, feature, profile, and
workspace inputs, while configuration changes Cargo behavior for a particular
invocation/environment. A Cargo-compatible consumer must not treat one as a
substitute for the other.

The Cargo Book is the contract authority for the manifest, workspace, target,
and metadata command surfaces:

- [Manifest format](https://doc.rust-lang.org/cargo/reference/manifest.html)
- [Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)
- [`cargo metadata`](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html)

For behavior not established by the reference, inspect Cargo's implementation.
The metadata command and workspace/package projection are implemented in
Cargo's [`metadata` command](https://github.com/rust-lang/cargo/blob/98a09e7e7d62850f14e5b6132101fc1edd19a16f/src/bin/cargo/commands/metadata.rs),
[`cargo_metadata` operation](https://github.com/rust-lang/cargo/blob/98a09e7e7d62850f14e5b6132101fc1edd19a16f/src/ops/cargo_metadata.rs),
and [`Package`](https://github.com/rust-lang/cargo/blob/98a09e7e7d62850f14e5b6132101fc1edd19a16f/src/workspace/package.rs).
When relying on implementation details, pin the Cargo source revision in the
claim, as above, rather than implying that an observation is a permanent
manifest guarantee.

## Consumer boundary in LAMINARIA

The subprocess remains owned by `CommandRunner`, which enforces the
repository's restricted command boundary; do not let a convenience API launch
an untracked Cargo process. Parse the captured JSON as `serde_json::Value` first
and preserve it as the loss-minimizing source record. Use `cargo_metadata`'s
typed projection as the structured view. The production test verifies that
the selected MSRV-compatible `cargo_metadata` 0.18.1 API preserves edition
2024 as emitted by current Cargo. `--no-deps` provides workspace package facts
without fetching or projecting dependency packages. It does not provide
Cargo's resolved dependency graph (`resolve` is absent), nor does it reproduce
every input file or option that may influence a particular Cargo invocation.

The crate version must obey the repository MSRV in the root `Cargo.toml`
(`rust-version = 1.74`). `cargo_metadata` 0.23.1 declares Rust 1.86 and is
therefore not usable here; 0.18.1 declares Rust 1.56 and is the selected
typed API baseline. The test establishes that this version's typed API accepts
edition 2024 metadata despite its older MSRV. Revisit that finding whenever
the crate or Cargo output schema changes. Neither representation is a lossless
round-trip of `Cargo.toml`, `.cargo/config.toml`, `Cargo.lock`, or the exact
command-line/environment used by Cargo.

`cargo_toml` can parse manifest syntax, but a standalone manifest parse is not
Cargo's effective workspace/target view: workspace inheritance, automatic
target discovery, path resolution, and configuration are contextual. Prefer
Cargo's own normalized metadata for package/workspace/target identity; consult
the original manifest/config/lock when a contract requires source-level
details that metadata omits.

## Current implementation status and compatibility gap

`crates/laminaria-run/src/cross_ecosystem_ingest.rs` preserves raw Cargo
metadata JSON, parses `cargo_metadata::Metadata`, and selects the
requested package by its canonical manifest path, rather than assuming the
first `packages` entry is the requested package. `CargoManifestFacts` retains
the complete raw metadata and typed projection. A focused
production-consumer test exercises a virtual workspace with multiple members,
workspace-inherited package fields, a path dependency, declared features,
Cargo-discovered library/binary/example targets, and an explicitly declared
binary target with `required-features`. It verifies selection independent of
package-array order, Cargo's canonicalized path-dependency location, target
source paths, and that edition 2024 survives the typed API. These are coverage
points for Cargo's normalized metadata consumer, not a declaration that every
Cargo feature or invocation mode is already supported by LAMINARIA.

This is not yet a general Cargo project model. The current downstream
`DependencyResolutionInput` is still a fixture-oriented projection; it does
not preserve every member/target/dependency edge or all effective build
settings as first-class planning inputs. `--no-deps` metadata is not evidence
of complete dependency resolution. Before claiming arbitrary existing Rust
project compatibility, subsequent work must define which Cargo invocation
semantics LAMINARIA promises to preserve, carry the needed normalized facts to
planning/execution, and verify representative real manifests against Cargo's
own behavior. Any unsupported semantic must fail explicitly rather than be
silently flattened or ignored.

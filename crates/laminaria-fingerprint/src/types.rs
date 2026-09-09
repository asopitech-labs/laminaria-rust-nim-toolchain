//! Schema types for `EnvironmentFingerprint` / `ToolchainFingerprint`, as
//! specified in `docs/measurement-foundation.md` (#11/#18) and
//! `docs/multi-version-toolchains.md` (#18/#22).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Schema version of the fingerprint records emitted by this crate.
/// Bump whenever a field is added, renamed, or removed so stored Runs remain
/// interpretable (`docs/measurement-foundation.md` section 5).
pub const SCHEMA_VERSION: &str = "0.1.0";

/// Identity of an on-disk executable: where it was resolved from and a
/// content digest, so "which exact binary ran" survives a PATH change.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutableIdentity {
    pub path: Option<PathBuf>,
    pub digest_sha256: Option<String>,
}

/// A single named Rust toolchain resolved against `toolchains.lock.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustToolchainFingerprint {
    pub logical_name: String,
    pub requested_selector: Option<String>,
    pub compiler_family: String,
    pub resolved_version: Option<String>,
    pub resolved_commit_hash: Option<String>,
    pub resolved_commit_date: Option<String>,
    pub host_triple: Option<String>,
    /// "stable" / "beta" / "nightly", derived from the resolved version
    /// string rather than the requested selector — kept separate from
    /// `resolved_version` so nightly-only capability requirements can be
    /// checked without parsing a version string ad hoc every time
    /// (`docs/multi-version-toolchains.md` section 3).
    pub channel: Option<String>,
    /// Bundled/selected LLVM identity, where the resolved rustc build
    /// exposes it. Left `None` rather than guessed when unavailable.
    pub llvm_version: Option<String>,
    pub rustc: ExecutableIdentity,
    pub cargo_version: Option<String>,
    pub cargo: ExecutableIdentity,
    pub sysroot: Option<PathBuf>,
    pub components: Vec<String>,
    pub adapter_version: String,
    pub resolution_notes: Vec<String>,
}

/// A single named Nim toolchain resolved against `toolchains.lock.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NimToolchainFingerprint {
    pub logical_name: String,
    pub requested_selector: Option<String>,
    pub compiler_family: String,
    pub resolved_version: Option<String>,
    /// Requested exact source revision (Nimony/Nim 3, nlvm, ...), from the
    /// lock entry's `revision` field. Recorded explicitly rather than
    /// folded into `requested_selector` — a moving `selector` and an exact
    /// `revision` answer different questions
    /// (`docs/multi-version-toolchains.md` sections 2 and 8).
    pub requested_source_revision: Option<String>,
    pub target_os: Option<String>,
    pub target_cpu: Option<String>,
    pub compiled_at: Option<String>,
    pub nim: ExecutableIdentity,
    pub nimble_version: Option<String>,
    pub nimble: ExecutableIdentity,
    pub adapter_version: String,
    pub resolution_notes: Vec<String>,
}

/// Any other backend/tool referenced by `toolchains.lock.toml`
/// (LLVM/Clang/LLD, `wasm-ld`, Binaryen, `wasm-tools`, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalToolFingerprint {
    pub logical_name: String,
    pub requested_selector: Option<String>,
    pub present: bool,
    pub resolved_version: Option<String>,
    pub executable: ExecutableIdentity,
    pub notes: Vec<String>,
}

/// Records that the resolved rustc host triple architecture differs from
/// the physical host CPU architecture (e.g. an x86_64 Homebrew rustc running
/// under Rosetta 2 on an Apple Silicon Mac). Left `None` when they match, so
/// this never has to be inferred by comparing two other fields by hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureNotice {
    pub host_architecture: String,
    pub toolchain_host_triple: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryState {
    pub commit: Option<String>,
    pub dirty: Option<bool>,
}

/// Environment-level fingerprint, independent of any single toolchain.
/// Field set follows `docs/measurement-foundation.md` section 4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentFingerprint {
    pub schema_version: String,
    pub captured_at_unix: u64,
    pub os: String,
    pub os_version: Option<String>,
    pub kernel: Option<String>,
    pub architecture: String,
    pub cpu_model: Option<String>,
    pub cpu_physical_cores: Option<u32>,
    pub cpu_logical_cores: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub filesystem_type: Option<String>,
    /// Best-effort environment class: "native", "container", "wsl", "ci",
    /// or "unknown" when no signal was observable. Never merged silently
    /// with another class in comparisons (measurement-foundation.md §2).
    pub environment_class: String,
    pub repository: RepositoryState,
    pub sdk_path: Option<PathBuf>,
    pub measurement_harness: String,
    pub architecture_notice: Option<ArchitectureNotice>,
    /// Set when the doctor process itself is running under binary
    /// translation (e.g. Rosetta 2). Every subprocess this tool spawns to
    /// detect other facts inherits the same translation, so this is
    /// surfaced rather than left implicit.
    pub self_process_translation_notice: Option<String>,
    /// Set when a non-`rustup` `cargo`/`rustc` earlier on `PATH` shadows the
    /// toolchain `rustup`/`rust-toolchain.toml` would otherwise select —
    /// e.g. a Homebrew Rust install ahead of `~/.cargo/bin`. This silently
    /// changes which compiler builds LAMINARIA's own code.
    pub path_toolchain_shadow: Option<String>,
    /// Allow-listed environment variables only — see `env_allowlist`. Never
    /// a raw dump of the process environment, so secrets cannot leak in.
    pub allowed_environment_variables: BTreeMap<String, String>,
    pub unobserved_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolchainReport {
    pub environment: EnvironmentFingerprint,
    pub rust_toolchains: Vec<RustToolchainFingerprint>,
    pub nim_toolchains: Vec<NimToolchainFingerprint>,
    pub external_tools: Vec<ExternalToolFingerprint>,
    pub rust_requirement_evaluations: Vec<crate::rust_requirements::RustRequirementEvaluation>,
}

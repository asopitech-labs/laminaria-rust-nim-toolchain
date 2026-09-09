//! Parses the repository-owned `toolchains.lock.toml` manifest described in
//! `docs/multi-version-toolchains.md` section 8. The lock file is the set of
//! *requested selectors*; doctor resolves each one to an exact fingerprint.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct LockFile {
    pub schema_version: String,
    #[serde(default)]
    pub rust: RustLockSection,
    #[serde(default)]
    pub nim: NimLockSection,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolSelector>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RustLockSection {
    #[serde(default)]
    pub toolchains: BTreeMap<String, RustToolchainSelector>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RustToolchainSelector {
    pub selector: String,
    #[serde(default)]
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NimLockSection {
    #[serde(default)]
    pub toolchains: BTreeMap<String, NimToolchainSelector>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NimToolchainSelector {
    pub selector: String,
    /// Optional explicit directory holding this toolchain's `nim`/`nimble`
    /// executables (e.g. a `choosenim` toolchain dir, or a manually
    /// extracted Nim/Nimony build). When set, doctor resolves this named
    /// toolchain from that directory instead of whatever is active on
    /// `PATH`, which is what lets multiple exact Nim toolchains coexist
    /// and be independently selected rather than only ever reporting the
    /// one Nim install that happens to be on `PATH`
    /// (`docs/multi-version-toolchains.md` section 2).
    #[serde(default)]
    pub bin_dir: Option<PathBuf>,
    /// Exact source revision for Nimony/Nim 3 (or nlvm) builds — see
    /// `docs/multi-version-toolchains.md` section 8's conceptual schema.
    /// Recorded on the fingerprint as a requested fact; this crate does not
    /// attempt to verify it against the resolved binary.
    #[serde(default)]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolSelector {
    pub selector: String,
    /// Where this tool is expected to come from, so doctor/bootstrap don't
    /// default to a system package manager when a more exact, already-
    /// managed source exists:
    ///
    /// - `"rustup-llvm-tools"`: resolved from a Rust toolchain's own sysroot
    ///   (`rustup component add llvm-tools`) instead of `PATH` — this LLVM
    ///   identity is exactly the one bundled with the resolved rustc, per
    ///   `docs/multi-version-toolchains.md` section 8's "bundled or
    ///   selected LLVM/backend identity" field, rather than an independently
    ///   versioned system install;
    /// - `"cargo"`: installed with `cargo install <tool> --version
    ///   <selector>`, an exact pin independent of any system package
    ///   manager;
    /// - omitted: whatever resolves on `PATH` (system package manager or
    ///   manual install).
    #[serde(default)]
    pub via: Option<String>,
}

#[derive(Debug)]
pub enum LockLoadError {
    Io(std::io::Error),
    Parse(toml::de::Error),
}

impl std::fmt::Display for LockLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockLoadError::Io(e) => write!(f, "could not read lock file: {e}"),
            LockLoadError::Parse(e) => write!(f, "could not parse lock file: {e}"),
        }
    }
}

pub fn load(path: &Path) -> Result<LockFile, LockLoadError> {
    let text = std::fs::read_to_string(path).map_err(LockLoadError::Io)?;
    toml::from_str(&text).map_err(LockLoadError::Parse)
}

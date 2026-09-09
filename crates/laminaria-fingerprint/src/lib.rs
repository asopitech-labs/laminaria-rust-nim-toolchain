//! Environment and toolchain fingerprinting primitives for LAMINARIA.
//!
//! Implements the `EnvironmentFingerprint` / `ToolchainFingerprint` schema
//! and the `doctor` responsibility from issue #18, following the canonical
//! design in `docs/measurement-foundation.md` and
//! `docs/multi-version-toolchains.md`.

pub mod comparability;
pub mod doctor;
pub mod env;
pub mod exec;
pub mod external;
pub mod lock;
pub mod nim_toolchain;
pub mod rust_requirements;
pub mod rust_toolchain;
pub mod types;

pub use types::{
    ArchitectureNotice, EnvironmentFingerprint, ExecutableIdentity, ExternalToolFingerprint,
    NimToolchainFingerprint, RepositoryState, RustToolchainFingerprint, ToolchainReport,
    SCHEMA_VERSION,
};

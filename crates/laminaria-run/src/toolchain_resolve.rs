//! Shared toolchain verification, used by both `self_build.rs` (always
//! resolves and verifies both a Rust and a Nim toolchain) and
//! `project_build.rs` (resolves and verifies only the toolchain family a
//! target project actually needs). Factored out so there is exactly one
//! implementation of "what does a verified toolchain mean" rather than two
//! copies that could drift -- `self_build.rs`'s own `resolve_verified_toolchain`
//! is a thin wrapper over the functions here.

use std::path::{Path, PathBuf};

use laminaria_fingerprint::doctor::{self, DoctorRun};

#[derive(Debug)]
pub enum ToolchainResolutionError {
    LockUnreadable(String),
    RustToolchainMissing,
    RustUnverifiedSelector {
        selector: String,
        resolved_version: Option<String>,
        resolved_channel: Option<String>,
    },
    RustExecutableUnresolved(String),
    NimToolchainMissing,
    NimUnverifiedSelector {
        selector: String,
        resolved_version: Option<String>,
    },
    NimExecutableUnresolved(String),
}

impl std::fmt::Display for ToolchainResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolchainResolutionError::LockUnreadable(detail) => {
                write!(f, "failed to load toolchain lock file: {detail}")
            }
            ToolchainResolutionError::RustToolchainMissing => {
                write!(f, "toolchain lock file declares no Rust toolchain")
            }
            ToolchainResolutionError::RustUnverifiedSelector {
                selector,
                resolved_version,
                resolved_channel,
            } => write!(
                f,
                "Rust toolchain requested selector '{selector}' but resolved version is \
                 {resolved_version:?} (channel {resolved_channel:?}) -- refusing to build with \
                 a toolchain that does not match, or cannot be verified against, what the lock \
                 file actually pinned"
            ),
            ToolchainResolutionError::RustExecutableUnresolved(detail) => {
                write!(f, "{detail}")
            }
            ToolchainResolutionError::NimToolchainMissing => {
                write!(f, "toolchain lock file declares no Nim toolchain")
            }
            ToolchainResolutionError::NimUnverifiedSelector {
                selector,
                resolved_version,
            } => write!(
                f,
                "Nim toolchain requested selector '{selector}' but resolved version is \
                 {resolved_version:?} -- refusing to build with a toolchain that does not \
                 match, or cannot be verified against, what the lock file actually pinned"
            ),
            ToolchainResolutionError::NimExecutableUnresolved(detail) => {
                write!(f, "{detail}")
            }
        }
    }
}

impl std::error::Error for ToolchainResolutionError {}

/// Whether `resolved_version`/`resolved_channel` is consistent with a
/// requested `selector`. Four cases, in order:
/// - An empty selector: no constraint was requested, always matches.
/// - A numeric-looking selector (`"2.2.10"`, `"2.2"`): must match the
///   resolved version component-by-component (split on `.`), for as
///   many components as the selector itself specifies -- *not* a raw
///   string-prefix check (`"2.2.10".starts_with("2.2.1")` is `true` as
///   plain strings, but `"2.2.1"` and `"2.2.10"` are different patch
///   versions).
/// - A recognized Rust channel name (exactly `"stable"`/`"beta"`/
///   `"nightly"`): verified against `resolved_channel`, not accepted
///   unconditionally. `resolved_channel` is `None` for Nim (which has no
///   channel concept), so a Nim selector can never match this branch.
/// - Anything else non-numeric (a dated nightly like
///   `"nightly-2024-01-15"`, a custom toolchain name, ...): this
///   function has no way to actually verify it against what was
///   resolved, so it is rejected outright rather than silently trusted --
///   "cannot verify" fails closed.
pub fn selector_matches_resolved(
    selector: &str,
    resolved_version: Option<&str>,
    resolved_channel: Option<&str>,
) -> bool {
    if selector.is_empty() {
        return true;
    }
    if selector.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        let Some(resolved) = resolved_version else {
            return false;
        };
        let selector_parts: Vec<&str> = selector.split('.').collect();
        let resolved_parts: Vec<&str> = resolved.split('.').collect();
        return resolved_parts.len() >= selector_parts.len()
            && selector_parts
                .iter()
                .zip(resolved_parts.iter())
                .all(|(s, r)| s == r);
    }
    if matches!(selector, "stable" | "beta" | "nightly") {
        return resolved_channel == Some(selector);
    }
    false
}

/// Loads `lock_path` and resolves only the toolchain families `need_rust`/
/// `need_nim` ask for (`doctor::build_selective`) against `fingerprint_root`
/// (the directory whose environment/dirty-state/rust-version-requirements
/// get fingerprinted -- for `self_build.rs` this is LAMINARIA's own
/// `repo_root`; for `project_build.rs` this is the target project's own
/// root, never LAMINARIA's).
pub fn run_doctor(
    lock_path: &Path,
    fingerprint_root: &Path,
    need_rust: bool,
    need_nim: bool,
) -> Result<DoctorRun, ToolchainResolutionError> {
    let doctor_run = doctor::build_selective(lock_path, fingerprint_root, need_rust, need_nim);
    if let Some(load_error) = &doctor_run.lock_load_error {
        return Err(ToolchainResolutionError::LockUnreadable(format!(
            "{}: {load_error:?}",
            lock_path.display()
        )));
    }
    Ok(doctor_run)
}

/// Requires `doctor_run` to have resolved at least one Rust toolchain with
/// a real `cargo`/`rustc` executable path *and* a resolved version
/// consistent with what the lock actually requested. Takes the first
/// resolved entry -- this project's `toolchains.lock.toml` declares
/// exactly one Rust toolchain today, so this is unambiguous; a lock
/// declaring more than one would need an explicit toolchain selector this
/// first slice does not yet have (an open gap, not silently guessed at).
pub fn resolve_verified_rust(
    doctor_run: &DoctorRun,
) -> Result<(PathBuf, PathBuf), ToolchainResolutionError> {
    let rust_toolchain = doctor_run
        .report
        .rust_toolchains
        .first()
        .ok_or(ToolchainResolutionError::RustToolchainMissing)?;
    let cargo = rust_toolchain.cargo.path.clone().ok_or_else(|| {
        ToolchainResolutionError::RustExecutableUnresolved(format!(
            "Rust toolchain '{}' has no resolved cargo executable (see its own resolution \
             notes from `laminaria doctor`)",
            rust_toolchain.logical_name
        ))
    })?;
    let rustc = rust_toolchain.rustc.path.clone().ok_or_else(|| {
        ToolchainResolutionError::RustExecutableUnresolved(format!(
            "Rust toolchain '{}' has no resolved rustc executable (see its own resolution \
             notes from `laminaria doctor`)",
            rust_toolchain.logical_name
        ))
    })?;

    let selector = rust_toolchain.requested_selector.as_deref().unwrap_or("");
    if !selector_matches_resolved(
        selector,
        rust_toolchain.resolved_version.as_deref(),
        rust_toolchain.channel.as_deref(),
    ) {
        return Err(ToolchainResolutionError::RustUnverifiedSelector {
            selector: selector.to_string(),
            resolved_version: rust_toolchain.resolved_version.clone(),
            resolved_channel: rust_toolchain.channel.clone(),
        });
    }

    Ok((cargo, rustc))
}

/// Same reasoning as `resolve_verified_rust`, for the Nim toolchain family.
pub fn resolve_verified_nim(doctor_run: &DoctorRun) -> Result<PathBuf, ToolchainResolutionError> {
    let nim_toolchain = doctor_run
        .report
        .nim_toolchains
        .first()
        .ok_or(ToolchainResolutionError::NimToolchainMissing)?;
    let nim = nim_toolchain.nim.path.clone().ok_or_else(|| {
        ToolchainResolutionError::NimExecutableUnresolved(format!(
            "Nim toolchain '{}' has no resolved nim executable (see its own resolution notes \
             from `laminaria doctor`)",
            nim_toolchain.logical_name
        ))
    })?;

    let selector = nim_toolchain.requested_selector.as_deref().unwrap_or("");
    if !selector_matches_resolved(selector, nim_toolchain.resolved_version.as_deref(), None) {
        return Err(ToolchainResolutionError::NimUnverifiedSelector {
            selector: selector.to_string(),
            resolved_version: nim_toolchain.resolved_version.clone(),
        });
    }

    Ok(nim)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bug a fourth external review caught: naive
    /// `str::starts_with` treats `"2.2.1"` as a match for `"2.2.10"`
    /// because it really is a character-for-character string prefix of
    /// it, even though they are different patch versions.
    #[test]
    fn selector_matches_resolved_rejects_a_string_prefix_that_is_not_a_real_version_match() {
        assert!(!selector_matches_resolved("2.2.1", Some("2.2.10"), None));
    }

    #[test]
    fn selector_matches_resolved_accepts_an_exact_full_version_match() {
        assert!(selector_matches_resolved("2.2.10", Some("2.2.10"), None));
    }

    #[test]
    fn selector_matches_resolved_accepts_a_partial_major_minor_selector() {
        assert!(selector_matches_resolved("2.2", Some("2.2.10"), None));
    }

    #[test]
    fn selector_matches_resolved_rejects_a_completely_different_version() {
        assert!(!selector_matches_resolved("999.0.0", Some("2.2.10"), None));
    }

    #[test]
    fn selector_matches_resolved_accepts_a_channel_name_that_matches_the_resolved_channel() {
        assert!(selector_matches_resolved(
            "stable",
            Some("1.97.1"),
            Some("stable")
        ));
    }

    #[test]
    fn selector_matches_resolved_rejects_a_channel_name_that_does_not_match_the_resolved_channel() {
        assert!(!selector_matches_resolved(
            "nightly",
            Some("1.97.1"),
            Some("stable")
        ));
    }

    #[test]
    fn selector_matches_resolved_rejects_an_unverifiable_dated_selector() {
        assert!(!selector_matches_resolved(
            "nightly-2024-01-15",
            Some("1.97.1"),
            Some("nightly")
        ));
    }

    #[test]
    fn selector_matches_resolved_rejects_any_non_numeric_selector_when_there_is_no_channel_to_check_against(
    ) {
        assert!(!selector_matches_resolved("devel", Some("2.2.10"), None));
    }

    #[test]
    fn selector_matches_resolved_always_matches_an_empty_selector() {
        assert!(selector_matches_resolved("", Some("anything"), None));
    }

    #[test]
    fn selector_matches_resolved_rejects_a_numeric_selector_with_no_resolved_version() {
        assert!(!selector_matches_resolved("2.2.10", None, None));
    }
}

//! Issue #28 D1-a: identifies the *owned* executor/planner a measurement
//! actually exercised -- deliberately not
//! `laminaria_run::scenario::compare_reports`'s `toolchain_digest_sha256`
//! concept, which identifies an *external* rustc/Nim toolchain
//! installation (a real review round's own warning: "外部rustc/Nim
//! compilerのidentityを代用品にしてはいけません" -- substituting that for
//! LAMINARIA's own owned-code identity would misrepresent what's being
//! compared). What actually needs to match, for a budget-1-vs-budget-2
//! (M3) or scale-vs-scale (M8) comparison to mean anything, is: the same
//! revision of LAMINARIA's own owned source (`repository.commit`, from
//! a clean, non-dirty tree), and -- when the measurement exercises a
//! separately-built artifact file rather than in-process code (M8's
//! `laminaria-planner` binary) -- the exact same content of that file.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnedIdentity {
    /// The git commit LAMINARIA's own owned code (the currently-running
    /// `laminaria-experiment`/`laminaria-run`/`nim-planner` sources) was
    /// built from -- from `EnvironmentFingerprint::repository::commit`,
    /// already captured per-Run, not re-derived here.
    pub repo_commit: Option<String>,
    /// Whether the working tree had uncommitted changes at measurement
    /// time -- `Some(true)` means this measurement cannot be trusted as
    /// identifying a specific, reproducible revision.
    pub repo_dirty: Option<bool>,
    /// SHA-256 of `std::env::current_exe()` -- the actual compiled binary
    /// that is executing this measurement (the `laminaria-m3-owned-baseline`/
    /// `laminaria-m8-owned-baseline` bin, or the test binary under `cargo
    /// test`). Binds identity to the exact artifact that ran, not merely
    /// its source commit: the same commit rebuilt with different flags,
    /// a stale `target/` directory, or a different toolchain would
    /// silently name the same `repo_commit` while running different
    /// code -- this field catches that a review round named directly as
    /// a gap ("M3の実行体identityが実物を識別していない").
    pub measurement_executable_sha256: Option<String>,
    /// SHA-256 of the `laminaria-planner` binary this measurement's Nim
    /// planner call actually spawned (M3 and M8 both invoke it). Distinct
    /// from `measurement_executable_sha256`: the planner is a separate
    /// build artifact from the Rust measurement binary.
    pub planner_binary_sha256: Option<String>,
}

impl OwnedIdentity {
    pub fn from_environment_fingerprint(
        fingerprint: &laminaria_fingerprint::EnvironmentFingerprint,
        measurement_executable_sha256: Option<String>,
        planner_binary_sha256: Option<String>,
    ) -> Self {
        OwnedIdentity {
            repo_commit: fingerprint.repository.commit.clone(),
            repo_dirty: fingerprint.repository.dirty,
            measurement_executable_sha256,
            planner_binary_sha256,
        }
    }
}

/// SHA-256 of a file's full contents -- used to identify the exact
/// `laminaria-planner` binary a measurement actually spawned, distinct
/// from (and a strictly stronger claim than) "the same repo commit"
/// (the same commit could in principle be rebuilt into a different
/// binary; this catches that).
pub fn content_sha256(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// SHA-256 of the currently-running executable itself -- see
/// `OwnedIdentity::measurement_executable_sha256`.
pub fn current_exe_sha256() -> Result<String, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("failed to resolve the current executable's path: {e}"))?;
    content_sha256(&exe)
}

/// `Ok(())` only if both identities name the same, non-dirty repo commit,
/// and (when either side names an artifact digest) the same artifact
/// content. Fails closed: an unresolved (`None`) commit on either side,
/// or a dirty tree on either side, is never treated as comparable --
/// mirroring `laminaria_run::scenario::compare_reports`'s own
/// "an unresolved digest on either side is never treated as a match"
/// discipline, applied to LAMINARIA's own owned-code identity instead
/// of an external toolchain's.
pub fn identities_comparable(a: &OwnedIdentity, b: &OwnedIdentity) -> Result<(), String> {
    let (Some(commit_a), Some(commit_b)) = (&a.repo_commit, &b.repo_commit) else {
        return Err(format!(
            "owned identity unresolved: repo_commit (a={:?}, b={:?}) -- an unresolved commit on \
             either side is never treated as comparable",
            a.repo_commit, b.repo_commit
        ));
    };
    if commit_a != commit_b {
        return Err(format!(
            "owned identity differs: repo_commit (a={commit_a:?}, b={commit_b:?})"
        ));
    }
    if a.repo_dirty != Some(false) || b.repo_dirty != Some(false) {
        return Err(format!(
            "owned identity uses a dirty working tree (a.repo_dirty={:?}, b.repo_dirty={:?}) -- \
             not a comparable baseline",
            a.repo_dirty, b.repo_dirty
        ));
    }
    match (
        &a.measurement_executable_sha256,
        &b.measurement_executable_sha256,
    ) {
        (Some(x), Some(y)) if x == y => {}
        (x, y) => {
            return Err(format!(
                "owned identity unresolved or differs: measurement_executable_sha256 (a={x:?}, \
                 b={y:?}) -- an unresolved or mismatched measurement executable is never treated \
                 as comparable"
            ))
        }
    }
    match (&a.planner_binary_sha256, &b.planner_binary_sha256) {
        (Some(x), Some(y)) if x == y => Ok(()),
        (x, y) => Err(format!(
            "owned identity unresolved or differs: planner_binary_sha256 (a={x:?}, b={y:?}) -- an \
             unresolved or mismatched planner binary is never treated as comparable"
        )),
    }
}

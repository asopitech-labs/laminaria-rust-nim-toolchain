//! Minimal reuse-decision core -- the first slice of issues #7 ("Define
//! artifact identity, incremental invalidation, and CAS reuse") and #12
//! ("Eliminate unnecessary compiler work and define no-op build
//! invariants"), scoped down from those two [Research] issues' combined
//! 23 acceptance criteria to exactly one question: **given two Runs of
//! the same logical scenario, can LAMINARIA decide "this artifact is
//! reusable" vs. "this must rebuild," with the decision explained from
//! identity differences** -- not asserted by fiat.
//!
//! **Deliberately not attempted in this slice** (real, large, separate
//! work -- named here so it isn't silently assumed done):
//! - No CAS storage or actual artifact retrieval -- this only computes an
//!   identity key and a yes/no decision, never stores or fetches bytes.
//! - No cross-machine, cross-worktree-path, or cross-toolchain-version
//!   reuse -- single machine, single already-resolved toolchain per call.
//! - No ThinLTO/WASM-specific identity, no host/target distinction beyond
//!   the one triple/OS-CPU pair `laminaria-fingerprint` already resolves,
//!   no persistence-tier/replica modeling.
//! - No actual "skip the compiler" behavior -- this module only decides
//!   and explains; it does not yet change what `run_and_record` executes.
//!
//! The identity key deliberately reuses issue #18/#19's own already-
//! resolved evidence rather than re-deriving toolchain identity from
//! scratch: `ExecutableIdentity::digest_sha256` (a real SHA-256 of the
//! resolved compiler binary, not a version string) is exactly the "exact
//! resolved compiler/toolchain identity" issue #7's acceptance criteria
//! ask a cache identity to carry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::types::Run;

pub const REUSE_SCHEMA_VERSION: &str = "0.1.0";

/// The four identity components this first slice tracks -- issue #7's own
/// list, narrowed to what a single-node, single-toolchain scenario can
/// actually populate:
/// - `source_digest_sha256`: content (not mtime) of every file under the
///   given source roots, combined into one digest -- so a `touch` with no
///   real content change is correctly seen as "source unchanged," a real,
///   deliberate difference from Cargo's own mtime-based invalidation (see
///   NOTES.md for what this fixture found when comparing the two).
/// - `toolchain_digest_sha256`: the resolved compiler binary's own SHA-256
///   (`ExecutableIdentity::digest_sha256`), reused directly from issue
///   #18/#19's already-captured `ToolchainReport` -- not re-derived.
/// - `command_identity`: the logical command being run (program + args),
///   *not* the wrapper-substituted `Run::root_command` -- see this
///   module's own doc comment on why those two must not be conflated.
/// - `target_identity`: host triple (Rust) or target OS/CPU (Nim), from
///   the same already-resolved `ToolchainReport`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactIdentityKey {
    pub schema_version: String,
    pub source_digest_sha256: String,
    pub toolchain_digest_sha256: Option<String>,
    pub command_identity: String,
    pub target_identity: Option<String>,
}

/// Content-hashes every regular file under `source_roots` (sorted by
/// path, so the combined digest is deterministic regardless of directory
/// walk order), and combines them into one digest. Deliberately content-
/// based, not mtime-based -- the whole point of tracking this separately
/// from Cargo's/Nim's own invalidation, which is mtime-based and will
/// disagree with this key on a `touch`-only edit.
pub fn hash_source_roots(source_roots: &[PathBuf]) -> std::io::Result<String> {
    let mut files: BTreeMap<PathBuf, ()> = BTreeMap::new();
    for root in source_roots {
        walk_files(root, &mut files);
    }

    let mut combined = Sha256::new();
    for path in files.keys() {
        let contents = std::fs::read(path)?;
        combined.update(path.to_string_lossy().as_bytes());
        combined.update(b"\0");
        let mut file_hash = Sha256::new();
        file_hash.update(&contents);
        combined.update(format!("{:x}", file_hash.finalize()).as_bytes());
        combined.update(b"\n");
    }
    Ok(format!("{:x}", combined.finalize()))
}

fn walk_files(dir: &Path, out: &mut BTreeMap<PathBuf, ()>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            walk_files(&path, out);
        } else if file_type.is_file() {
            out.insert(path, ());
        }
    }
}

/// Extracts the resolved toolchain's own executable digest and
/// target/host identity from an already-captured `ToolchainReport`
/// (`Run::resolved_toolchain_fingerprint`) -- reused directly, never
/// re-derived. Returns `(None, None)` when the report has no toolchains
/// of the requested kind resolved (a real "unknown, not assumed
/// compatible" outcome, not silently defaulted to `Some("")`).
pub fn toolchain_identity_from_run(run: &Run, is_nim: bool) -> (Option<String>, Option<String>) {
    let Some(report) = &run.resolved_toolchain_fingerprint else {
        return (None, None);
    };
    if is_nim {
        let toolchain = report.nim_toolchains.first();
        let digest = toolchain.and_then(|t| t.nim.digest_sha256.clone());
        let target = toolchain.map(|t| {
            format!(
                "{}/{}",
                t.target_os.as_deref().unwrap_or("?"),
                t.target_cpu.as_deref().unwrap_or("?")
            )
        });
        (digest, target)
    } else {
        let toolchain = report.rust_toolchains.first();
        let digest = toolchain.and_then(|t| t.rustc.digest_sha256.clone());
        let target = toolchain.and_then(|t| t.host_triple.clone());
        (digest, target)
    }
}

/// Builds the full `ArtifactIdentityKey` for one Run: `command_identity`
/// is supplied by the caller (the *logical* command -- see this module's
/// doc comment), not read from `run.root_command` (which may be
/// wrapper-substituted).
pub fn compute_identity_key(
    command_identity: &str,
    source_roots: &[PathBuf],
    run: &Run,
    is_nim: bool,
) -> std::io::Result<ArtifactIdentityKey> {
    let source_digest_sha256 = hash_source_roots(source_roots)?;
    let (toolchain_digest_sha256, target_identity) = toolchain_identity_from_run(run, is_nim);
    Ok(ArtifactIdentityKey {
        schema_version: REUSE_SCHEMA_VERSION.to_string(),
        source_digest_sha256,
        toolchain_digest_sha256,
        command_identity: command_identity.to_string(),
        target_identity,
    })
}

/// A safe, panic-free truncation for human-readable diagnostics -- real
/// SHA-256 hex digests are always 64 characters, but this must not panic
/// on a shorter string either (byte-index slicing on a non-ASCII
/// boundary, or simply a shorter string, both a real risk this module's
/// own early tests caught before it ever ran on real digests).
fn short_digest(digest: &str) -> &str {
    let end = digest
        .char_indices()
        .nth(12)
        .map(|(i, _)| i)
        .unwrap_or(digest.len());
    &digest[..end]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReuseDecision {
    Reusable,
    MustRebuild { reasons: Vec<String> },
}

/// Compares `previous` (an already-produced artifact's identity) against
/// `candidate` (what the next Run would need), and decides reuse --
/// issue #7's "every hit/miss can be explained from identity
/// differences" acceptance criterion: `MustRebuild`'s `reasons` names
/// *which* component(s) actually differ, never a bare "no."
/// `toolchain_digest_sha256: None` on either side (an unresolved
/// toolchain) is treated as a difference, not silently ignored --
/// "unknown" must fail closed, matching issue #7's multi-version safety
/// rule ("do not assume cross-version compatibility... fail closed when
/// unknown").
pub fn decide_reuse(
    previous: &ArtifactIdentityKey,
    candidate: &ArtifactIdentityKey,
) -> ReuseDecision {
    let mut reasons = Vec::new();

    if previous.source_digest_sha256 != candidate.source_digest_sha256 {
        reasons.push(format!(
            "source content digest differs ({} -> {})",
            short_digest(&previous.source_digest_sha256),
            short_digest(&candidate.source_digest_sha256)
        ));
    }
    if previous.toolchain_digest_sha256 != candidate.toolchain_digest_sha256
        || previous.toolchain_digest_sha256.is_none()
    {
        reasons.push(format!(
            "toolchain digest differs or unresolved ({:?} -> {:?})",
            previous.toolchain_digest_sha256, candidate.toolchain_digest_sha256
        ));
    }
    if previous.command_identity != candidate.command_identity {
        reasons.push(format!(
            "command identity differs ({:?} -> {:?})",
            previous.command_identity, candidate.command_identity
        ));
    }
    if previous.target_identity != candidate.target_identity {
        reasons.push(format!(
            "target identity differs ({:?} -> {:?})",
            previous.target_identity, candidate.target_identity
        ));
    }

    if reasons.is_empty() {
        ReuseDecision::Reusable
    } else {
        ReuseDecision::MustRebuild { reasons }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "laminaria-run-reuse-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn identical_content_hashes_the_same_even_after_mtime_only_touch() {
        let dir = tmp_dir("touch");
        let file = dir.join("lib.rs");
        std::fs::write(&file, b"fn main() {}").unwrap();
        let before = hash_source_roots(std::slice::from_ref(&dir)).unwrap();

        // A real `touch`: content byte-identical, mtime bumped.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let now = std::time::SystemTime::now();
        filetime_touch(&file, now);

        let after = hash_source_roots(&[dir]).unwrap();
        assert_eq!(
            before, after,
            "a content-identical touch must not change the source digest -- this is the whole \
             point of using content hashing instead of mtime"
        );
    }

    fn filetime_touch(path: &Path, _time: std::time::SystemTime) {
        // No filetime crate dependency for this first slice -- re-writing
        // the same content is a portable enough stand-in for "touch" to
        // prove the point (a real mtime bump without a content change),
        // verified by std::fs::write itself always updating mtime.
        let contents = std::fs::read(path).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn a_real_content_change_produces_a_different_digest() {
        let dir = tmp_dir("edit");
        let file = dir.join("lib.rs");
        std::fs::write(&file, b"fn main() {}").unwrap();
        let before = hash_source_roots(std::slice::from_ref(&dir)).unwrap();

        std::fs::write(&file, b"fn main() { println!(\"hi\"); }").unwrap();
        let after = hash_source_roots(&[dir]).unwrap();

        assert_ne!(before, after);
    }

    fn key(
        source_digest: &str,
        toolchain: Option<&str>,
        command: &str,
        target: Option<&str>,
    ) -> ArtifactIdentityKey {
        ArtifactIdentityKey {
            schema_version: REUSE_SCHEMA_VERSION.to_string(),
            source_digest_sha256: source_digest.to_string(),
            toolchain_digest_sha256: toolchain.map(str::to_string),
            command_identity: command.to_string(),
            target_identity: target.map(str::to_string),
        }
    }

    #[test]
    fn identical_keys_are_reusable() {
        let a = key("abc", Some("tc1"), "cargo build", Some("x86_64"));
        let b = key("abc", Some("tc1"), "cargo build", Some("x86_64"));
        assert!(matches!(decide_reuse(&a, &b), ReuseDecision::Reusable));
    }

    #[test]
    fn a_source_change_is_explained_as_such() {
        let a = key("abc", Some("tc1"), "cargo build", Some("x86_64"));
        let b = key("xyz", Some("tc1"), "cargo build", Some("x86_64"));
        match decide_reuse(&a, &b) {
            ReuseDecision::MustRebuild { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("source content digest differs"));
            }
            ReuseDecision::Reusable => panic!("expected MustRebuild"),
        }
    }

    #[test]
    fn an_unresolved_toolchain_fails_closed_even_if_everything_else_matches() {
        let a = key("abc", None, "cargo build", Some("x86_64"));
        let b = key("abc", None, "cargo build", Some("x86_64"));
        match decide_reuse(&a, &b) {
            ReuseDecision::MustRebuild { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("toolchain digest")));
            }
            ReuseDecision::Reusable => panic!(
                "an unresolved toolchain must never be treated as a compatibility match, per \
                 issue #7's fail-closed rule"
            ),
        }
    }

    #[test]
    fn multiple_differences_are_all_named_not_just_the_first() {
        let a = key("abc", Some("tc1"), "cargo build", Some("x86_64"));
        let b = key("xyz", Some("tc2"), "cargo check", Some("aarch64"));
        match decide_reuse(&a, &b) {
            ReuseDecision::MustRebuild { reasons } => assert_eq!(reasons.len(), 4),
            ReuseDecision::Reusable => panic!("expected MustRebuild"),
        }
    }

    /// Real-fixture verification (issue #7/#12's own ask, not just unit
    /// tests against synthetic keys): runs cold/true-noop/touch-edit/
    /// real-content-edit scenarios against a throwaway copy of
    /// `fixtures/rust-heavy-workspace` (never the tracked fixture itself
    /// -- copied first so this test can never leave the real repo dirty),
    /// and confirms `decide_reuse` matches what should actually happen:
    /// cold and true-noop are reusable; a touch-only edit is *also*
    /// reusable by LAMINARIA's content-based decision even though it
    /// triggered a real Cargo rebuild (the real, honest divergence this
    /// module's own doc comment claims); a genuine content edit is
    /// correctly detected and explained.
    #[test]
    fn reuse_decision_matches_real_fixture_behavior_across_cold_noop_and_edits() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let real_fixture = repo_root.join("fixtures/rust-heavy-workspace");

        let workdir = std::env::temp_dir().join(format!(
            "laminaria-run-reuse-fixture-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&workdir);
        std::fs::create_dir_all(&workdir).unwrap();
        let status = std::process::Command::new("cp")
            .args([
                "-r",
                real_fixture.to_str().unwrap(),
                workdir.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(
            status.success(),
            "failed to copy the fixture to a throwaway location"
        );
        let fixture = workdir.join("rust-heavy-workspace");

        let manifest_path = fixture.join("Cargo.toml");
        let target_dir = fixture.join("target");
        let source_dir = fixture.join("crates");
        let edited_file = source_dir.join("fixture-core/src/lib.rs");
        let runs_root = workdir.join("runs");
        // The real repo-root lock file, not an empty stub -- an empty
        // lock file resolves zero toolchains, which correctly makes
        // toolchain_digest_sha256 None on every side and therefore fails
        // closed on *every* comparison (per this module's own "unknown is
        // never treated as a match" rule) -- exercising that rule needs a
        // toolchain that actually resolves.
        let lock_path = repo_root.join("toolchains.lock.toml");

        let command_identity = "cargo build (rust-heavy-workspace)";

        let cold = crate::scenario::rust_heavy_workspace_scenario(
            crate::scenario::CacheStateLabel::Cold,
            &manifest_path,
            &target_dir,
            None,
        );
        let cold_run =
            crate::scenario::run_scenario_once(&cold, &runs_root, &lock_path, &repo_root).unwrap();
        let cold_key = compute_identity_key(
            command_identity,
            std::slice::from_ref(&source_dir),
            &cold_run,
            false,
        )
        .unwrap();

        let noop = crate::scenario::rust_heavy_workspace_scenario(
            crate::scenario::CacheStateLabel::TrueNoop,
            &manifest_path,
            &target_dir,
            None,
        );
        let noop_run =
            crate::scenario::run_scenario_once(&noop, &runs_root, &lock_path, &repo_root).unwrap();
        let noop_key = compute_identity_key(
            command_identity,
            std::slice::from_ref(&source_dir),
            &noop_run,
            false,
        )
        .unwrap();
        assert!(
            matches!(decide_reuse(&cold_key, &noop_key), ReuseDecision::Reusable),
            "cold and true-noop must have identical source/toolchain/command/target identity"
        );

        let touch_edit = crate::scenario::rust_heavy_workspace_scenario(
            crate::scenario::CacheStateLabel::Warm,
            &manifest_path,
            &target_dir,
            Some(&edited_file),
        );
        let touch_run =
            crate::scenario::run_scenario_once(&touch_edit, &runs_root, &lock_path, &repo_root)
                .unwrap();
        let touch_key = compute_identity_key(
            command_identity,
            std::slice::from_ref(&source_dir),
            &touch_run,
            false,
        )
        .unwrap();
        assert!(
            matches!(decide_reuse(&cold_key, &touch_key), ReuseDecision::Reusable),
            "a touch-only edit must not change the content-based source digest -- LAMINARIA's \
             own decision correctly differs from Cargo's own mtime-based rebuild here"
        );
        // Confirm the touch actually triggered real Cargo work, via the
        // artifact inventory -- not process_trace's own process count,
        // which relies on RUSTC-wrapper substitution finding the sibling
        // laminaria-rustc-wrapper binary next to the *running executable*
        // (see cargo_wrapper.rs's own doc comment): inside `cargo test`,
        // that's this test binary itself, not laminaria-cli, so wrapper
        // substitution silently doesn't engage here and process count
        // alone would misleadingly always read 1. The artifact delta
        // (observation_roots = [target_dir], populated by run_and_record
        // regardless of wrapper substitution) is the reliable signal.
        let touch_artifact_touched = touch_run
            .artifact_delta
            .as_ref()
            .and_then(|d| d.get("records"))
            .and_then(|r| r.as_array())
            .map(|records| {
                records.iter().any(|r| {
                    matches!(
                        r.get("state").and_then(|s| s.as_str()),
                        Some("Created") | Some("Modified")
                    )
                })
            })
            .unwrap_or(false);
        assert!(
            touch_artifact_touched,
            "expected the touch to actually trigger real Cargo rebuild output (a Created/\
             Modified artifact under target/), or this divergence from LAMINARIA's content-based \
             decision wouldn't be meaningful"
        );

        let original_content = std::fs::read(&edited_file).unwrap();
        let mut new_content = original_content.clone();
        new_content.extend_from_slice(b"\n// reuse.rs integration test: real content edit\n");
        std::fs::write(&edited_file, &new_content).unwrap();
        let content_edit = crate::scenario::rust_heavy_workspace_scenario(
            crate::scenario::CacheStateLabel::TrueNoop,
            &manifest_path,
            &target_dir,
            None,
        );
        let content_run =
            crate::scenario::run_scenario_once(&content_edit, &runs_root, &lock_path, &repo_root)
                .unwrap();
        let content_key = compute_identity_key(
            command_identity,
            std::slice::from_ref(&source_dir),
            &content_run,
            false,
        )
        .unwrap();

        match decide_reuse(&cold_key, &content_key) {
            ReuseDecision::MustRebuild { reasons } => {
                assert!(reasons
                    .iter()
                    .any(|r| r.contains("source content digest differs")));
            }
            ReuseDecision::Reusable => panic!("a real content edit must be detected as such"),
        }

        let _ = std::fs::remove_dir_all(&workdir);
    }
}

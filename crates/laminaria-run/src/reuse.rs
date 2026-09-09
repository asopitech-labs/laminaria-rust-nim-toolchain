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
///
/// **Fails closed on anything it can't actually walk** -- a real bug an
/// external review caught: a missing root, an unreadable directory, or a
/// single *file* passed as a "root" (not a directory at all -- `read_dir`
/// on a file simply errors) were all silently swallowed and treated as
/// contributing zero entries, so e.g. a single-file source root always
/// hashed to the same digest regardless of that file's actual content --
/// reproduced directly: editing the file's content did not change the
/// digest at all. An incomplete source enumeration must never be reported
/// as "nothing here, therefore reusable"; each of those cases now returns
/// a real `Err` instead.
pub fn hash_source_roots(source_roots: &[PathBuf]) -> std::io::Result<String> {
    let mut files: BTreeMap<PathBuf, ()> = BTreeMap::new();
    for root in source_roots {
        let metadata = std::fs::symlink_metadata(root).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!(
                    "hash_source_roots: cannot stat source root {}: {e}",
                    root.display()
                ),
            )
        })?;
        if !metadata.is_dir() {
            // Explicitly unsupported (not silently treated as an empty
            // directory): a single-file source root, a symlink, or any
            // other non-directory entry. Supporting a single file
            // directly is reasonable future work, not attempted here.
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "hash_source_roots: {} is not a directory -- single-file (or other \
                     non-directory) source roots are not supported; pass its parent directory \
                     instead, or extend this function deliberately rather than silently treating \
                     it as empty",
                    root.display()
                ),
            ));
        }
        walk_files(root, &mut files)?;
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

fn walk_files(dir: &Path, out: &mut BTreeMap<PathBuf, ()>) -> std::io::Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "hash_source_roots: cannot read directory {}: {e}",
                dir.display()
            ),
        )
    })?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            walk_files(&path, out)?;
        } else if file_type.is_file() {
            out.insert(path, ());
        }
    }
    Ok(())
}

/// Extracts the *actually-invoked* toolchain's own executable digest and
/// target/host identity from an already-captured `ToolchainReport`
/// (`Run::resolved_toolchain_fingerprint`) -- reused directly, never
/// re-derived. Returns `(None, None)` whenever the specific toolchain this
/// Run actually used can't be identified, including when the report has
/// no toolchains of the requested kind at all.
///
/// **Deliberately does not fall back to "the first declared toolchain"**
/// -- a real bug an external review caught: `toolchains.lock.toml` can
/// declare more than one Rust/Nim toolchain (e.g. `system_stable` and a
/// pinned nightly), and `ToolchainReport` resolves *all* of them
/// regardless of which one a given Run actually invoked. Picking
/// `.first()` meant two Runs that used genuinely different toolchains
/// (the caller changed a requested selector from A to B) still compared
/// as `Reusable`, because both keys carried the same first-declared
/// digest. Instead this identifies the real compiler binary the Run
/// invoked and matches it against the report by path:
/// - Rust: the wrapper's own recorded real-`rustc` path
///   (`root_command.env_overrides["LAMINARIA_WRAPPED_RUSTC"]`, set by
///   `resolve_real_rustc`/`prepare_cargo_wrapping` -- present whenever
///   RUSTC-wrapper substitution actually engaged for this Run).
/// - Nim: the root process record's own resolved executable path
///   (`process_trace.processes[0].executable`, from `tracer::
///   resolve_executable` -- Nim itself isn't wrapper-substituted, so this
///   is the real ground truth of which `nim` binary ran).
///
/// When that actual path can't be determined (wrapper substitution didn't
/// engage -- e.g. Level 0 probing, a non-Unix target, or a non-Cargo/Nim
/// root command) or doesn't match any declared toolchain's own recorded
/// path, this returns `None` rather than guessing -- the same fail-closed
/// rule `decide_reuse` already applies to an unresolved toolchain digest.
pub fn toolchain_identity_from_run(run: &Run, is_nim: bool) -> (Option<String>, Option<String>) {
    let Some(report) = &run.resolved_toolchain_fingerprint else {
        return (None, None);
    };
    if is_nim {
        let actual_nim_path = run
            .process_trace
            .processes
            .first()
            .and_then(|p| p.executable.as_ref());
        let Some(actual_nim_path) = actual_nim_path else {
            return (None, None);
        };
        let toolchain = report
            .nim_toolchains
            .iter()
            .find(|t| t.nim.path.as_ref() == Some(actual_nim_path));
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
        let actual_rustc_path = run
            .root_command
            .env_overrides
            .get(crate::cargo_wrapper::ENV_WRAPPED_RUSTC)
            .map(PathBuf::from);
        let Some(actual_rustc_path) = actual_rustc_path else {
            return (None, None);
        };
        let toolchain = report
            .rust_toolchains
            .iter()
            .find(|t| t.rustc.path.as_ref() == Some(&actual_rustc_path));
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

    /// Portable recursive directory copy for
    /// `reuse_decision_matches_real_fixture_behavior_across_cold_noop_and_edits`
    /// -- deliberately not `std::process::Command::new("cp")`; see that
    /// test's own comment for why.
    fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let dst_path = dst.join(entry.file_name());
            if file_type.is_dir() {
                copy_dir_recursive(&entry.path(), &dst_path)?;
            } else if file_type.is_file() {
                std::fs::copy(entry.path(), dst_path)?;
            }
        }
        Ok(())
    }

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

    /// The exact bug an external review caught: a nonexistent root was
    /// silently treated as an empty (and therefore always-identical)
    /// source set instead of a real error.
    #[test]
    fn hash_source_roots_errors_on_a_nonexistent_root_instead_of_hashing_an_empty_set() {
        let missing = std::env::temp_dir().join(format!(
            "laminaria-run-reuse-test-does-not-exist-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&missing);

        let result = hash_source_roots(std::slice::from_ref(&missing));

        assert!(
            result.is_err(),
            "a nonexistent source root must be an error, not an empty-set digest"
        );
    }

    /// The exact bug an external review caught, reproduced literally:
    /// passing a single file as a "root" silently hashed to an empty set,
    /// so editing that file's content never changed the digest at all.
    #[test]
    fn hash_source_roots_errors_on_a_single_file_root_instead_of_hashing_an_empty_set() {
        let dir = tmp_dir("single-file-root");
        let file = dir.join("lib.rs");
        std::fs::write(&file, b"fn main() {}").unwrap();

        let before = hash_source_roots(std::slice::from_ref(&file));
        assert!(
            before.is_err(),
            "a single file passed as a source root must be an error, not an empty-set digest"
        );

        // The literal reproduction: content changes must not silently
        // keep "matching" via an unreachable, always-empty digest.
        std::fs::write(&file, b"fn main() { println!(\"changed\"); }").unwrap();
        let after = hash_source_roots(std::slice::from_ref(&file));
        assert!(after.is_err());
    }

    /// An unreadable directory (permission denied) must also fail
    /// closed, not silently contribute zero entries. Unix-only: there is
    /// no portable way to construct an unreadable directory on Windows
    /// via `std::fs` permissions alone.
    #[test]
    #[cfg(unix)]
    fn hash_source_roots_errors_on_an_unreadable_directory() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tmp_dir("unreadable");
        std::fs::write(dir.join("lib.rs"), b"fn main() {}").unwrap();
        let mut perms = std::fs::metadata(&dir).unwrap().permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&dir, perms.clone()).unwrap();

        let result = hash_source_roots(std::slice::from_ref(&dir));

        // Restore permissions before asserting, so a failed assertion
        // still leaves this directory cleanable by the test's own tmp
        // dir naming convention (no explicit cleanup elsewhere in this
        // module relies on it, but leaving an unreadable directory behind
        // is bad hygiene regardless).
        perms.set_mode(0o755);
        std::fs::set_permissions(&dir, perms).unwrap();

        assert!(
            result.is_err(),
            "an unreadable directory must be an error, not an empty-set digest"
        );
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

    fn sample_rust_toolchain(
        logical_name: &str,
        rustc_path: &str,
        digest: &str,
    ) -> laminaria_fingerprint::RustToolchainFingerprint {
        laminaria_fingerprint::RustToolchainFingerprint {
            logical_name: logical_name.to_string(),
            requested_selector: None,
            compiler_family: "rust".to_string(),
            resolved_version: None,
            resolved_commit_hash: None,
            resolved_commit_date: None,
            host_triple: Some("x86_64-unknown-linux-gnu".to_string()),
            channel: None,
            llvm_version: None,
            rustc: laminaria_fingerprint::ExecutableIdentity {
                path: Some(PathBuf::from(rustc_path)),
                digest_sha256: Some(digest.to_string()),
            },
            cargo_version: None,
            cargo: laminaria_fingerprint::ExecutableIdentity {
                path: None,
                digest_sha256: None,
            },
            sysroot: None,
            components: Vec::new(),
            adapter_version: "0.1.0".to_string(),
            resolution_notes: Vec::new(),
        }
    }

    fn sample_run_with_toolchains(
        rust_toolchains: Vec<laminaria_fingerprint::RustToolchainFingerprint>,
        wrapped_rustc: Option<&str>,
    ) -> Run {
        use crate::types::{
            CacheState, ExitStatusRecord, PreparationRecord, ProbeLevel, ProcessRecord,
            ProcessTrace, ResourceUsage, RootCommand, RunResult, SCHEMA_VERSION,
        };
        let mut env_overrides = BTreeMap::new();
        if let Some(rustc) = wrapped_rustc {
            env_overrides.insert(
                crate::cargo_wrapper::ENV_WRAPPED_RUSTC.to_string(),
                rustc.to_string(),
            );
        }
        Run {
            run_id: "test-run".to_string(),
            schema_version: SCHEMA_VERSION.to_string(),
            workload_id: "test-workload".to_string(),
            scenario_id: "test-scenario".to_string(),
            requested_artifact: None,
            environment_fingerprint: laminaria_fingerprint::EnvironmentFingerprint {
                schema_version: "0.1.0".to_string(),
                captured_at_unix: 0,
                os: "test-os".to_string(),
                os_version: None,
                kernel: None,
                architecture: "test-arch".to_string(),
                cpu_model: None,
                cpu_physical_cores: None,
                cpu_logical_cores: None,
                memory_bytes: None,
                filesystem_type: None,
                environment_class: "unknown".to_string(),
                repository: laminaria_fingerprint::RepositoryState {
                    commit: None,
                    dirty: None,
                },
                sdk_path: None,
                measurement_harness: "test".to_string(),
                architecture_notice: None,
                self_process_translation_notice: None,
                path_toolchain_shadow: None,
                allowed_environment_variables: BTreeMap::new(),
                unobserved_fields: Vec::new(),
            },
            requested_toolchain_selector: None,
            resolved_toolchain_fingerprint: Some(laminaria_fingerprint::ToolchainReport {
                environment: laminaria_fingerprint::EnvironmentFingerprint {
                    schema_version: "0.1.0".to_string(),
                    captured_at_unix: 0,
                    os: "test-os".to_string(),
                    os_version: None,
                    kernel: None,
                    architecture: "test-arch".to_string(),
                    cpu_model: None,
                    cpu_physical_cores: None,
                    cpu_logical_cores: None,
                    memory_bytes: None,
                    filesystem_type: None,
                    environment_class: "unknown".to_string(),
                    repository: laminaria_fingerprint::RepositoryState {
                        commit: None,
                        dirty: None,
                    },
                    sdk_path: None,
                    measurement_harness: "test".to_string(),
                    architecture_notice: None,
                    self_process_translation_notice: None,
                    path_toolchain_shadow: None,
                    allowed_environment_variables: BTreeMap::new(),
                    unobserved_fields: Vec::new(),
                },
                rust_toolchains,
                nim_toolchains: Vec::new(),
                external_tools: Vec::new(),
                rust_requirement_evaluations: Vec::new(),
            }),
            preparation_record: PreparationRecord::default(),
            cache_state: CacheState::default(),
            root_command: RootCommand {
                program: "cargo".to_string(),
                args: Vec::new(),
                cwd: None,
                env_overrides,
            },
            run_started_at_unix_ns: 0,
            run_ended_at_unix_ns: Some(0),
            result: Some(RunResult {
                success: true,
                root_exit_status: ExitStatusRecord {
                    success: true,
                    code: Some(0),
                    signal: None,
                },
            }),
            process_trace: ProcessTrace {
                processes: vec![ProcessRecord {
                    pid: Some(1),
                    parent_pid: Some(1),
                    executable: None,
                    argv: vec!["cargo".to_string()],
                    cwd: None,
                    start_elapsed_ns: 0,
                    end_elapsed_ns: Some(0),
                    exit_status: Some(ExitStatusRecord {
                        success: true,
                        code: Some(0),
                        signal: None,
                    }),
                    resource_usage: ResourceUsage::default(),
                    probe_level: ProbeLevel::Level1ProcessResource,
                    coverage_note: "test".to_string(),
                }],
                known_gaps: Vec::new(),
            },
            compiler_telemetry: None,
            artifact_delta: None,
            measurement_overhead: None,
        }
    }

    /// The exact bug an external review caught: `toolchains.lock.toml`
    /// declares two Rust toolchains (A, B); a Run that actually used B
    /// must not have its identity computed from A's digest just because
    /// A happens to be declared first.
    #[test]
    fn toolchain_identity_uses_the_actually_invoked_toolchain_not_the_first_declared() {
        let toolchain_a = sample_rust_toolchain("toolchain_a", "/opt/a/bin/rustc", "digest-a");
        let toolchain_b = sample_rust_toolchain("toolchain_b", "/opt/b/bin/rustc", "digest-b");

        let run_using_a = sample_run_with_toolchains(
            vec![toolchain_a.clone(), toolchain_b.clone()],
            Some("/opt/a/bin/rustc"),
        );
        let (digest_a, _) = toolchain_identity_from_run(&run_using_a, false);
        assert_eq!(digest_a, Some("digest-a".to_string()));

        let run_using_b =
            sample_run_with_toolchains(vec![toolchain_a, toolchain_b], Some("/opt/b/bin/rustc"));
        let (digest_b, _) = toolchain_identity_from_run(&run_using_b, false);
        assert_eq!(
            digest_b,
            Some("digest-b".to_string()),
            "the Run that actually invoked toolchain B must resolve to B's own digest, not A's \
             (A is declared first in the lock file, which is the exact bug: .first() would \
             return A's digest here regardless of which toolchain actually ran)"
        );
        assert_ne!(
            digest_a, digest_b,
            "two Runs that used genuinely different toolchains must never compare as identical"
        );
    }

    #[test]
    fn toolchain_identity_fails_closed_when_the_actually_used_rustc_matches_no_declared_toolchain()
    {
        let toolchain_a = sample_rust_toolchain("toolchain_a", "/opt/a/bin/rustc", "digest-a");
        // The wrapper recorded a real rustc path that isn't any of the
        // lock file's own declared toolchains (e.g. a PATH-shadowed rustc
        // -- a real, reproduced case on this session's own dev machine,
        // see reuse_decision_matches_real_fixture_behavior_across_cold_noop_and_edits's
        // own pinned_rustc workaround).
        let run = sample_run_with_toolchains(vec![toolchain_a], Some("/usr/local/bin/rustc"));
        let (digest, _) = toolchain_identity_from_run(&run, false);
        assert_eq!(
            digest, None,
            "an actually-used rustc that matches none of the lock file's declared toolchains \
             must resolve to None (unknown), never silently fall back to a declared toolchain \
             that wasn't actually used"
        );
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

    /// Locates (building first if necessary) the real `laminaria` CLI
    /// binary. Running scenarios *through the CLI subprocess*, not via
    /// `crate::scenario::run_scenario_once` called directly from this test
    /// binary, is required for this test to mean anything post-fix:
    /// RUSTC-wrapper substitution (which `toolchain_identity_from_run` now
    /// depends on to identify the actually-used rustc, see that function's
    /// own doc comment) only finds `laminaria-rustc-wrapper` next to the
    /// *running executable* -- inside `cargo test`, that's this test
    /// binary itself, not `laminaria-cli`, so wrapper substitution would
    /// silently never engage and every toolchain digest would read `None`
    /// regardless of what this test is trying to check.
    fn laminaria_cli_binary(repo_root: &Path) -> PathBuf {
        // `--workspace`, not `-p laminaria-cli`: `laminaria-rustc-wrapper`/
        // `laminaria-cc-wrapper` are laminaria-run's own [[bin]] targets,
        // not a dependency of the CLI binary itself, so building only the
        // CLI package would leave them missing from target/debug/ and
        // silently disable wrapper substitution -- the exact "cargo build
        // --workspace is required first" gotcha this crate's own NOTES.md
        // already documents, now reproduced by this test's own first draft.
        let status = std::process::Command::new("cargo")
            .args(["build", "--workspace"])
            .current_dir(repo_root)
            .status()
            .unwrap();
        assert!(status.success(), "failed to build the workspace");
        let name = if cfg!(windows) {
            "laminaria.exe"
        } else {
            "laminaria"
        };
        let bin = repo_root.join("target/debug").join(name);
        assert!(
            bin.is_file(),
            "expected {} to exist after cargo build",
            bin.display()
        );
        bin
    }

    /// The exact `rustc` path `toolchains.lock.toml` itself resolves to
    /// (via `laminaria doctor --json`), *not* whatever `which rustc` finds
    /// on `PATH`. On a machine where a non-rustup `rustc` shadows `PATH`
    /// ahead of the rustup-managed one -- confirmed to be genuinely true
    /// in this session's own dev environment, surfaced by
    /// `EnvironmentFingerprint::path_toolchain_shadow` -- those two can be
    /// different real binaries. Pinning `RUSTC` to this exact path before
    /// running scenarios through the CLI keeps this test deterministic
    /// and focused on what it's actually meant to check (whether
    /// `toolchain_identity_from_run` correctly matches the *used*
    /// toolchain against the lock file), rather than depending on this
    /// machine's own PATH configuration to happen to agree with it.
    fn lock_resolved_rustc_path(cli_bin: &Path, repo_root: &Path, lock_path: &Path) -> PathBuf {
        let output = std::process::Command::new(cli_bin)
            .args(["doctor", "--json"])
            .arg("--lock")
            .arg(lock_path)
            .arg("--repo-root")
            .arg(repo_root)
            .output()
            .unwrap();
        assert!(output.status.success(), "laminaria doctor --json failed");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let path = report["rust_toolchains"][0]["rustc"]["path"]
            .as_str()
            .expect("expected toolchains.lock.toml to resolve at least one Rust toolchain");
        PathBuf::from(path)
    }

    /// Runs one scenario through the real CLI (`laminaria scenario-run`,
    /// `--repeat 1`), returning the single resulting `Run` read back from
    /// disk -- not the `ScenarioReport` this subcommand prints, which
    /// doesn't carry the full `Run` (toolchain fingerprint, root_command)
    /// this test needs.
    #[allow(clippy::too_many_arguments)]
    fn run_scenario_via_cli(
        cli_bin: &Path,
        repo_root: &Path,
        lock_path: &Path,
        runs_root: &Path,
        workload: &str,
        kind: &str,
        manifest_path: &Path,
        target_dir: &Path,
        edited_source_path: Option<&Path>,
        pinned_rustc: &Path,
    ) -> Run {
        let mut cmd = std::process::Command::new(cli_bin);
        cmd.env("RUSTC", pinned_rustc);
        cmd.args([
            "scenario-run",
            "--workload",
            workload,
            "--kind",
            kind,
            "--manifest-path",
        ])
        .arg(manifest_path)
        .arg("--target-dir")
        .arg(target_dir)
        .arg("--runs-root")
        .arg(runs_root)
        .arg("--lock")
        .arg(lock_path)
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--json");
        if let Some(edited) = edited_source_path {
            cmd.arg("--edited-source-path").arg(edited);
        }
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "scenario-run --kind {kind} failed: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let run_id = report["run_ids"][0].as_str().unwrap();
        crate::store::read_run(runs_root, run_id).unwrap()
    }

    /// Real-fixture verification (issue #7/#12's own ask, not just unit
    /// tests against synthetic keys): runs cold/true-noop/touch-edit/
    /// real-content-edit scenarios (through the real CLI, see
    /// `laminaria_cli_binary`'s doc comment) against a throwaway copy of
    /// `fixtures/rust-heavy-workspace` (never the tracked fixture itself
    /// -- copied first so this test can never leave the real repo dirty),
    /// and confirms `decide_reuse` matches what should actually happen:
    /// cold and true-noop are reusable; a touch-only edit is *also*
    /// reusable by LAMINARIA's content-based decision even though it
    /// triggered a real Cargo rebuild (the real, honest divergence this
    /// module's own doc comment claims); a genuine content edit is
    /// correctly detected and explained.
    ///
    /// Unix-only: this test's own premise (`toolchain_digest_sha256` gets
    /// resolved through the real CLI) depends on RUSTC-wrapper
    /// substitution actually engaging, which `lib.rs::prepare_cargo_wrapping`
    /// deliberately never attempts on non-Unix targets (see that
    /// function's own doc comment -- the wrapper binary's own measurement
    /// is `wait4`-based). Confirmed directly: this test passes on both
    /// Linux and macOS CI, and fails only on the `windows` job, exactly
    /// where wrapper substitution is expected to never engage -- a
    /// platform limitation of the wrapper mechanism itself, not a bug in
    /// this fix.
    #[test]
    #[cfg(unix)]
    fn reuse_decision_matches_real_fixture_behavior_across_cold_noop_and_edits() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let real_fixture = repo_root.join("fixtures/rust-heavy-workspace");
        let cli_bin = laminaria_cli_binary(&repo_root);
        // The real repo-root lock file, not an empty stub -- an empty
        // lock file resolves zero toolchains, which correctly makes
        // toolchain_digest_sha256 None on every side and therefore fails
        // closed on *every* comparison (per this module's own "unknown is
        // never treated as a match" rule) -- exercising that rule needs a
        // toolchain that actually resolves.
        let lock_path = repo_root.join("toolchains.lock.toml");
        let pinned_rustc = lock_resolved_rustc_path(&cli_bin, &repo_root, &lock_path);

        let workdir = std::env::temp_dir().join(format!(
            "laminaria-run-reuse-fixture-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&workdir);
        let fixture = workdir.join("rust-heavy-workspace");
        // A portable Rust copy, not `cp -r`: shelling out hit exactly the
        // same class of Windows-path-quoting bug this session already
        // fixed once (a Windows PathBuf's backslashes, or here also
        // `canonicalize()`'s `\\?\` extended-path prefix, mis-handled by
        // an MSYS-built `cp` reading its own argv) -- verified failing in
        // CI, not reproduced locally on macOS. A few lines of `std::fs`
        // sidesteps the whole class of issue instead of chasing another
        // one-off escaping fix.
        copy_dir_recursive(&real_fixture, &fixture).unwrap();

        let manifest_path = fixture.join("Cargo.toml");
        let target_dir = fixture.join("target");
        let source_dir = fixture.join("crates");
        let edited_file = source_dir.join("fixture-core/src/lib.rs");
        let runs_root = workdir.join("runs");

        let command_identity = "cargo build (rust-heavy-workspace)";

        let cold_run = run_scenario_via_cli(
            &cli_bin,
            &repo_root,
            &lock_path,
            &runs_root,
            "rust-heavy-workspace",
            "cold",
            &manifest_path,
            &target_dir,
            None,
            &pinned_rustc,
        );
        let cold_key = compute_identity_key(
            command_identity,
            std::slice::from_ref(&source_dir),
            &cold_run,
            false,
        )
        .unwrap();
        assert!(
            cold_key.toolchain_digest_sha256.is_some(),
            "expected RUSTC-wrapper substitution to actually engage through the real CLI, so \
             the toolchain digest is resolved -- otherwise this test can't exercise the \
             actually-used-toolchain fix at all"
        );

        let noop_run = run_scenario_via_cli(
            &cli_bin,
            &repo_root,
            &lock_path,
            &runs_root,
            "rust-heavy-workspace",
            "noop",
            &manifest_path,
            &target_dir,
            None,
            &pinned_rustc,
        );
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

        let touch_run = run_scenario_via_cli(
            &cli_bin,
            &repo_root,
            &lock_path,
            &runs_root,
            "rust-heavy-workspace",
            "edit",
            &manifest_path,
            &target_dir,
            Some(&edited_file),
            &pinned_rustc,
        );
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
        // artifact inventory.
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
        let content_run = run_scenario_via_cli(
            &cli_bin,
            &repo_root,
            &lock_path,
            &runs_root,
            "rust-heavy-workspace",
            "noop",
            &manifest_path,
            &target_dir,
            None,
            &pinned_rustc,
        );
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

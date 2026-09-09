//! No-op-safe artifact inventory and diff (issue #20's "Artifact
//! inventory" / "No-op-safe inventory" requirements): given one or more
//! observation roots, records what create/modify/delete/unchanged
//! evidence a Run's traced command actually produced, with metadata
//! enumeration, changed-candidate detection, and content hashing measured
//! as three *separate* costs -- so "nothing changed" never hides a
//! full-tree hash behind a cache-hit statistic
//! (`docs/measurement-foundation.md` section 8; issue #20's "No-op-safe
//! inventory" and "Artifact detection costs are separately measurable"
//! acceptance criteria).
//!
//! **Deliberately scoped for this first slice** (issue #20 is large; this
//! is one independently-reviewable unit of it, not the whole issue):
//!
//! - The caller supplies explicit observation roots (`laminaria run
//!   --observe <path>`, repeatable) rather than this crate attempting to
//!   infer Cargo's target dir / Nim's output dir on its own -- that
//!   inference (`CARGO_TARGET_DIR`, workspace resolution, Nim's `-o`/
//!   `--nimcache` flags) is real, separate work, not attempted here. No
//!   `--observe` roots means an empty inventory, explicitly noted in
//!   `Run::process_trace::known_gaps`, not silently absent.
//! - The diff is *within one Run*: a snapshot taken immediately before
//!   the traced command spawns, and another immediately after it exits,
//!   diffed against each other. This deliberately avoids needing a
//!   separate cross-Run snapshot store for this first slice -- a cold
//!   build's pre-snapshot is empty (nothing exists yet), a true no-op
//!   rebuild's pre/post snapshots are identical (nothing this Run's own
//!   command touched), and a single-file edit's pre/post differ only for
//!   the artifacts actually rebuilt -- exactly the three scenarios issue
//!   #20's completion condition names.
//! - Producer identity (which compiler invocation produced a given
//!   artifact) is always `Unknown` here -- correlating that would need
//!   connecting this module to the wrapper-invocation events
//!   (`cargo_wrapper`/`nim_wrapper`) or Cargo's own
//!   `--message-format=json` artifact records (`cargo_telemetry`), not
//!   done in this slice. `Unknown` is the honest default per issue #20's
//!   "producer identity is represented as proven/unknown rather than
//!   inferred from filenames alone" acceptance criterion -- never a
//!   filename-based guess.
//! - Change detection uses size + modification time, not a full content
//!   hash, to decide *candidacy* -- matching the design doc's explicit
//!   phase separation (metadata enumeration -> changed-candidate
//!   detection -> content hashing -> producer correlation). A content
//!   hash is only computed for files that changed-candidate detection
//!   flagged (created, or size/mtime differs) or that no longer exist
//!   (deleted files get no post-hash, for the obvious reason).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use laminaria_fingerprint::exec::sha256_file;

pub const ARTIFACT_SCHEMA_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactState {
    Created,
    /// `(size, modification time)` differs from the pre-command snapshot --
    /// **not** a claim that content actually changed. Confirmed against a
    /// real Cargo no-op rebuild: Cargo rewrites its own `.d` dep-info files
    /// on every invocation, even a true no-op, with byte-identical content
    /// but a fresh mtime -- those records land here with identical
    /// `digest_sha256` to what a prior Run recorded for the same path,
    /// which a caller comparing successive Runs' digests can detect. This
    /// module only hashes the post-state (never the pre-state, per the
    /// design doc's own phase separation), so it cannot itself tell
    /// "content changed" from "metadata changed but content didn't" within
    /// one Run -- a real, named limitation, not silently glossed over.
    Modified,
    Deleted,
    Unchanged,
}

/// Which process produced a given artifact. Always `Unknown` in this first
/// slice -- see this module's doc comment. `Proven` exists in the schema
/// now so a later change that wires up wrapper-invocation/Cargo-message
/// correlation doesn't need a schema migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProducerIdentity {
    Proven(String),
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    /// Path relative to the observation root that contains it -- "logical"
    /// in the sense of issue #20's "separate logical artifact from
    /// physical storage location": this is not yet a stable cross-machine
    /// identity (e.g. it still embeds a target triple or profile
    /// directory), just decoupled from the absolute filesystem path of one
    /// particular run so two Runs' inventories are comparable at all.
    pub logical_path: PathBuf,
    pub size_bytes: Option<u64>,
    /// SHA-256, lowercase hex. `None` for `Deleted` records (nothing left
    /// to hash) and for `Unchanged` records (deliberately not
    /// recomputed -- that's the whole point of the no-op-safe design).
    pub digest_sha256: Option<String>,
    pub state: ArtifactState,
    pub producer: ProducerIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactInventory {
    pub schema_version: String,
    pub observation_roots: Vec<PathBuf>,
    pub records: Vec<ArtifactRecord>,
    /// Wall time spent walking `observation_roots` and `stat()`-ing every
    /// entry (both the pre- and post-command snapshots combined) --
    /// separate from `hash_seconds` so "how much did just finding out
    /// what's there cost" is never conflated with "how much did
    /// confirming content identity cost."
    pub enumeration_seconds: f64,
    /// Wall time spent computing SHA-256 digests -- only for changed
    /// candidates (created, or size/mtime differs from the pre-snapshot).
    pub hash_seconds: f64,
    pub hashed_bytes: u64,
    /// How many entries changed-candidate detection flagged for hashing --
    /// distinct from `records.len()`, which also counts `Unchanged`/
    /// `Deleted` entries that were never hashed at all. On a true no-op
    /// rebuild this is 0 regardless of how large the observed tree is.
    pub changed_candidate_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FileStat {
    size: u64,
    modified_unix_ns: u128,
}

/// Walks every observation root and records each regular file's size and
/// modification time (no hashing here -- that's a separate, later phase).
/// A root that doesn't exist yet (e.g. a cold build's target directory,
/// before the traced command has run) is treated as contributing zero
/// entries, not an error. Symlinks are skipped, not followed -- avoids any
/// risk of a symlink cycle inside an observed build-output tree, at the
/// cost of not observing symlinked artifacts (a named, small limitation).
///
/// Returns the `(root, relative_path) -> FileStat` map and the wall time
/// this walk itself took.
fn snapshot(roots: &[PathBuf]) -> (BTreeMap<PathBuf, FileStat>, f64) {
    let start = Instant::now();
    let mut out = BTreeMap::new();
    for root in roots {
        walk_into(root, &mut out);
    }
    (out, start.elapsed().as_secs_f64())
}

/// Recurses into `dir`, inserting an absolute-path key -> `FileStat` for
/// every regular file found. Keys are absolute paths at this stage --
/// `diff`/`to_logical_path` are responsible for turning them into the
/// root-relative `logical_path` an `ArtifactRecord` actually stores.
fn walk_into(dir: &Path, out: &mut BTreeMap<PathBuf, FileStat>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return, // missing/unreadable root or subdir: zero entries, not an error
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
            walk_into(&path, out);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified_unix_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        out.insert(
            path,
            FileStat {
                size: metadata.len(),
                modified_unix_ns,
            },
        );
    }
}

/// Diffs a pre- and post-command snapshot of `roots` into an
/// `ArtifactInventory`: every path present in either snapshot becomes one
/// `ArtifactRecord`, classified `Created`/`Modified`/`Deleted`/`Unchanged`
/// by comparing `(size, modified_unix_ns)` -- a changed candidate (created,
/// or size/mtime differs) gets its post-state content hashed; an unchanged
/// or deleted entry does not.
fn diff(
    roots: &[PathBuf],
    pre: &BTreeMap<PathBuf, FileStat>,
    post: &BTreeMap<PathBuf, FileStat>,
    enumeration_seconds: f64,
) -> ArtifactInventory {
    let mut records = Vec::new();
    let mut changed_candidate_count = 0usize;
    let mut hash_seconds = 0.0f64;
    let mut hashed_bytes = 0u64;

    let mut all_paths: Vec<&PathBuf> = pre.keys().chain(post.keys()).collect();
    all_paths.sort();
    all_paths.dedup();

    for path in all_paths {
        let pre_stat = pre.get(path);
        let post_stat = post.get(path);
        let logical_path = to_logical_path(roots, path);

        let (state, hash_target) = match (pre_stat, post_stat) {
            (None, Some(_)) => (ArtifactState::Created, post_stat),
            (Some(_), None) => (ArtifactState::Deleted, None),
            (Some(pre), Some(post)) if pre == post => (ArtifactState::Unchanged, None),
            (Some(_), Some(_)) => (ArtifactState::Modified, post_stat),
            (None, None) => unreachable!("path came from pre or post's own keys"),
        };

        let size_bytes = post_stat.or(pre_stat).map(|s| s.size);
        let mut digest_sha256 = None;
        if hash_target.is_some() {
            changed_candidate_count += 1;
            let hash_start = Instant::now();
            if let Some(digest) = sha256_file(path) {
                hashed_bytes += size_bytes.unwrap_or(0);
                digest_sha256 = Some(digest);
            }
            hash_seconds += hash_start.elapsed().as_secs_f64();
        }

        records.push(ArtifactRecord {
            logical_path,
            size_bytes,
            digest_sha256,
            state,
            producer: ProducerIdentity::Unknown,
        });
    }

    ArtifactInventory {
        schema_version: ARTIFACT_SCHEMA_VERSION.to_string(),
        observation_roots: roots.to_vec(),
        records,
        enumeration_seconds,
        hash_seconds,
        hashed_bytes,
        changed_candidate_count,
    }
}

/// Turns an absolute walked path into a `root`-relative logical path
/// (issue #20's "separate logical artifact from physical storage
/// location"). Falls back to the absolute path if it somehow isn't under
/// any observation root -- should not happen given how `snapshot` builds
/// its keys, but never panics on it.
fn to_logical_path(roots: &[PathBuf], path: &Path) -> PathBuf {
    for root in roots {
        if let Ok(rel) = path.strip_prefix(root) {
            return root
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.clone())
                .join(rel);
        }
    }
    path.to_path_buf()
}

/// Runs the full pre/post diff for one Run: takes a snapshot of `roots`,
/// calls `run_command` (which is expected to spawn/wait the traced
/// command), takes a second snapshot, and returns the resulting
/// `ArtifactInventory` alongside whatever `run_command` returned.
pub fn observe_around<T, E>(
    roots: &[PathBuf],
    run_command: impl FnOnce() -> Result<T, E>,
) -> Result<(T, ArtifactInventory), E> {
    let (pre, pre_seconds) = snapshot(roots);
    let result = run_command()?;
    let (post, post_seconds) = snapshot(roots);
    let inventory = diff(roots, &pre, &post, pre_seconds + post_seconds);
    Ok((result, inventory))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "laminaria-run-artifact-inventory-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cold_build_reports_every_file_as_created_and_hashes_all_of_them() {
        let root = tmp_dir("cold");
        let roots = vec![root.clone()];

        let inventory = observe_around(&roots, || -> Result<(), ()> {
            std::fs::write(root.join("a.rlib"), b"aaa").unwrap();
            std::fs::write(root.join("b.rlib"), b"bbbb").unwrap();
            Ok(())
        })
        .unwrap()
        .1;

        assert_eq!(inventory.records.len(), 2);
        assert!(inventory
            .records
            .iter()
            .all(|r| r.state == ArtifactState::Created));
        assert!(inventory.records.iter().all(|r| r.digest_sha256.is_some()));
        assert_eq!(inventory.changed_candidate_count, 2);
        assert_eq!(inventory.hashed_bytes, 3 + 4);
    }

    #[test]
    fn true_noop_rebuild_reports_everything_unchanged_and_hashes_nothing() {
        let root = tmp_dir("noop");
        std::fs::write(root.join("a.rlib"), b"aaa").unwrap();
        let roots = vec![root.clone()];

        // Nothing touches the tree between snapshots -- the literal
        // "true no-op" case.
        let inventory = observe_around(&roots, || -> Result<(), ()> { Ok(()) })
            .unwrap()
            .1;

        assert_eq!(inventory.records.len(), 1);
        assert_eq!(inventory.records[0].state, ArtifactState::Unchanged);
        assert_eq!(inventory.records[0].digest_sha256, None);
        assert_eq!(inventory.changed_candidate_count, 0);
        assert_eq!(inventory.hashed_bytes, 0);
    }

    #[test]
    fn single_file_edit_reports_only_the_touched_file_as_modified() {
        let root = tmp_dir("edit");
        std::fs::write(root.join("a.rlib"), b"aaa").unwrap();
        std::fs::write(root.join("b.rlib"), b"bbbb").unwrap();
        let roots = vec![root.clone()];

        let inventory = observe_around(&roots, || -> Result<(), ()> {
            // Sleep past filesystem mtime granularity so a real edit is
            // guaranteed to bump modified_unix_ns even on coarse-grained
            // filesystems (e.g. some CI images round to 1s).
            std::thread::sleep(std::time::Duration::from_millis(1100));
            std::fs::write(root.join("a.rlib"), b"aaa-edited").unwrap();
            Ok(())
        })
        .unwrap()
        .1;

        assert_eq!(inventory.records.len(), 2);
        let a = inventory
            .records
            .iter()
            .find(|r| r.logical_path.ends_with("a.rlib"))
            .unwrap();
        let b = inventory
            .records
            .iter()
            .find(|r| r.logical_path.ends_with("b.rlib"))
            .unwrap();
        assert_eq!(a.state, ArtifactState::Modified);
        assert!(a.digest_sha256.is_some());
        assert_eq!(b.state, ArtifactState::Unchanged);
        assert_eq!(b.digest_sha256, None);
        assert_eq!(inventory.changed_candidate_count, 1);
    }

    #[test]
    fn deleted_files_are_reported_without_a_post_state_hash() {
        let root = tmp_dir("delete");
        std::fs::write(root.join("a.rlib"), b"aaa").unwrap();
        let roots = vec![root.clone()];

        let inventory = observe_around(&roots, || -> Result<(), ()> {
            std::fs::remove_file(root.join("a.rlib")).unwrap();
            Ok(())
        })
        .unwrap()
        .1;

        assert_eq!(inventory.records.len(), 1);
        assert_eq!(inventory.records[0].state, ArtifactState::Deleted);
        assert_eq!(inventory.records[0].digest_sha256, None);
        assert_eq!(inventory.records[0].size_bytes, Some(3));
        assert_eq!(inventory.changed_candidate_count, 0);
    }

    #[test]
    fn cold_build_from_a_not_yet_existing_root_is_not_an_error() {
        let root = std::env::temp_dir().join(format!(
            "laminaria-run-artifact-inventory-test-missing-root-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let roots = vec![root.clone()];

        let inventory = observe_around(&roots, || -> Result<(), ()> {
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("a.rlib"), b"aaa").unwrap();
            Ok(())
        })
        .unwrap()
        .1;

        assert_eq!(inventory.records.len(), 1);
        assert_eq!(inventory.records[0].state, ArtifactState::Created);
    }

    #[test]
    fn producer_identity_is_always_unknown_in_this_first_slice() {
        let root = tmp_dir("producer");
        let roots = vec![root.clone()];

        let inventory = observe_around(&roots, || -> Result<(), ()> {
            std::fs::write(root.join("a.rlib"), b"aaa").unwrap();
            Ok(())
        })
        .unwrap()
        .1;

        assert!(inventory
            .records
            .iter()
            .all(|r| r.producer == ProducerIdentity::Unknown));
    }

    #[test]
    fn propagates_the_traced_command_error_without_losing_the_ability_to_diff() {
        let root = tmp_dir("error");
        let roots = vec![root.clone()];

        let result: Result<((), ArtifactInventory), &str> =
            observe_around(&roots, || -> Result<(), &str> { Err("boom") });

        assert_eq!(result.err(), Some("boom"));
    }
}

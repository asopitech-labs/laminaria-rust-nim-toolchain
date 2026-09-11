//! Resolves the real `laminaria-planner` Nim binary for this crate's
//! measurement runners (issue #28 D1-a), building it from source via
//! `nim c` if it isn't already sitting next to the current executable --
//! the same two-step resolution `crates/laminaria-plan/src/
//! nim_planner_client.rs`'s own `real_planner_binary()` test helper
//! uses, promoted here to production code so `laminaria-m3-owned-
//! baseline`/`laminaria-m8-owned-baseline` work from a fresh checkout
//! without requiring the caller to build `nim-planner` by hand first.
//! Building LAMINARIA's own already-existing Nim planner source via
//! `nim c` is the project's own established build convention (see
//! `docs/self-build.md`), not a compiler-ownership-contract violation --
//! this never invokes `nim c`/`nimble` on any *target* being measured,
//! only on `nim-planner/` itself.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Repository root, found relative to this crate's own manifest
/// directory (`crates/laminaria-experiment/../..`) -- mirrors
/// `nim_planner_client.rs`'s own `real_planner_binary()`.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("crates/laminaria-experiment/../.. must resolve to the repo root")
}

/// Resolves the real `laminaria-planner` binary: first via
/// `laminaria_plan::find_planner_binary()` (next to the current
/// executable, e.g. after `cargo build --workspace`), then by building
/// `nim-planner/src/laminaria_planner.nim` directly with `nim c` (not
/// `nimble build` -- `laminaria-run`'s own `self_build.rs` found
/// `nimble` 0.22.2 exits 0 even after a build-failure message, so `nim
/// c`'s reliable exit code is used here too).
///
/// Cached in a process-local `OnceLock`, not a bare `if !bin.is_file()`
/// check -- this crate's own tests (`m3_baseline_test.rs`/
/// `m8_baseline_test.rs`) call this concurrently from multiple threads
/// of the same test binary by default, the same real race
/// `laminaria-run`'s own `compiler_work_executor.rs` tests hit and fixed
/// (see that module's `test_support::real_planner_binary`): without a
/// lock, two threads seeing the binary missing would both invoke `nim c`
/// against the same output path at once. A residual, smaller risk
/// remains across *separate* processes (this crate's test binaries vs.
/// `laminaria-run`'s own, if `cargo test --workspace` schedules both
/// concurrently) -- the same class of risk already accepted elsewhere in
/// this repo's test suite, not newly introduced here.
pub fn resolve_or_build_planner_binary() -> Result<PathBuf, String> {
    static RESOLVED: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    RESOLVED
        .get_or_init(resolve_or_build_planner_binary_uncached)
        .clone()
}

fn resolve_or_build_planner_binary_uncached() -> Result<PathBuf, String> {
    if let Some(found) = laminaria_plan::find_planner_binary() {
        return Ok(found);
    }

    let nim_planner_dir = repo_root().join("nim-planner");
    let bin = nim_planner_dir.join(if cfg!(windows) {
        "bin/laminaria-planner.exe"
    } else {
        "bin/laminaria-planner"
    });
    if bin.is_file() {
        return Ok(bin);
    }

    let status = Command::new("nim")
        .args([
            "c",
            "--path:src",
            // Workspace-local nimcache, not Nim's OS-default
            // (`~/.cache/nim/...` on Unix) -- matches
            // `nim_planner_client.rs`'s own `real_planner_binary()` fix
            // for the identical issue: an environment where that default
            // isn't writable would otherwise fail here with an error
            // that looks like a genuine planner-binary defect rather
            // than the actually-unrelated cache-path permission problem
            // it is.
            "--nimcache:nimcache",
            "-o:bin/laminaria-planner",
            "src/laminaria_planner.nim",
        ])
        .current_dir(&nim_planner_dir)
        .status()
        .map_err(|e| format!("failed to invoke nim -- is Nim installed? ({e})"))?;
    if !status.success() {
        return Err("nim c failed to build laminaria-planner".to_string());
    }
    if !bin.is_file() {
        return Err(format!(
            "expected {} to exist after building it",
            bin.display()
        ));
    }
    Ok(bin)
}

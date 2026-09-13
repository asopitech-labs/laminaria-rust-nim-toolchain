//! `#[cfg(test)]`-only support shared across this crate's own test
//! modules that need a real, compiled Nim planner binary
//! (`self_build.rs`, `project_build.rs`, `compiler_work_executor.rs` for
//! the one-shot `laminaria-planner`; `incremental_executor.rs`,
//! `incremental_session_client.rs` for the session-scoped
//! `laminaria-incremental-planner`).
//!
//! Each of those modules used to guard its own `nim c -o:bin/...` build
//! with a *separate* per-module `OnceLock` -- each one correctly
//! prevented a race *within* its own module, but not against the
//! others, since every module building the same binary targets the
//! exact same output path and all of them compile into this crate's one
//! shared test binary. A fresh checkout's CI run caught this directly,
//! twice, for both binaries: two different modules' `nim c` invocations
//! raced each other, one truncating the binary file while another
//! test's already-spawned process still had it open for execution
//! (`ExecutableFileBusy`/"Text file busy"). `laminaria-plan`'s own
//! `nim_planner_client.rs` test module had already hit and fixed the
//! *within-one-module* version of this exact bug class; this is the
//! *across-modules* version of it, inside one crate's own single test
//! binary. Fixed by sharing one `OnceLock` per binary across every
//! module in this crate that needs it, rather than one per module --
//! `real_planner_binary` (issue #27/#8, fixed first) and
//! `real_incremental_planner_binary` (issue #36/#48, this exact same
//! bug class recurring for the sibling binary once enough concurrent
//! test load made the race actually manifest on CI) share this one
//! file for exactly that reason: a second, still-uncentralized copy is
//! how this bug reappeared the first time.
//!
//! `#![cfg(all(test, unix))]` on the whole file, not just the two
//! functions inside it: this crate's Windows CI job never provisions
//! Nim, so gating only the functions (leaving their `use` items
//! ungated) would strip their only callers there while leaving the
//! imports themselves compiled -- unused-import errors under
//! `-D warnings`, the exact mistake class this session already hit
//! twice elsewhere in this crate (see `compiler_work_executor.rs`'s own
//! test module doc comment).
#![cfg(all(test, unix))]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Builds `nim-planner/bin/laminaria-planner` via `nim c` directly --
/// not `nimble build`, which this crate's own tests found exits `0` on a
/// genuine compile failure, and which `apt`'s packaged `nim`/`nimble` on
/// Ubuntu CI fails outright with a false `Unsatisfied dependency` even
/// when the installed `nim --version` genuinely satisfies it -- exactly
/// once per test binary process, no matter which of this crate's test
/// modules asks first or how many ask concurrently.
#[cfg(unix)]
pub(crate) fn real_planner_binary(repo_root: &Path) -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let nim_planner_dir = repo_root.join("nim-planner");
            let status = std::process::Command::new("nim")
                .args([
                    "c",
                    "--path:src",
                    "-o:bin/laminaria-planner",
                    "src/laminaria_planner.nim",
                ])
                .current_dir(&nim_planner_dir)
                .status()
                .expect("failed to invoke nim -- is Nim installed?");
            assert!(status.success(), "nim c failed to build laminaria-planner");
            let bin = nim_planner_dir.join("bin/laminaria-planner");
            assert!(
                bin.is_file(),
                "expected {} to exist after building it",
                bin.display()
            );
            bin
        })
        .clone()
}

/// The session-scoped counterpart of [`real_planner_binary`] --
/// `nim-planner/bin/laminaria-incremental-planner`, built exactly once
/// per test binary process no matter which of `incremental_executor.rs`/
/// `incremental_session_client.rs` asks first or how many ask
/// concurrently (see this module's own doc comment for the exact CI
/// failure this fixes: two separate per-module `OnceLock`s racing to
/// write the same output path).
#[cfg(unix)]
pub(crate) fn real_incremental_planner_binary(repo_root: &Path) -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let nim_planner_dir = repo_root.join("nim-planner");
            let status = std::process::Command::new("nim")
                .args([
                    "c",
                    "--path:src",
                    "--nimcache:nimcache",
                    "-o:bin/laminaria-incremental-planner",
                    "src/laminaria_incremental_planner.nim",
                ])
                .current_dir(&nim_planner_dir)
                .status()
                .expect("failed to invoke nim -- is Nim installed?");
            assert!(
                status.success(),
                "nim c failed to build laminaria-incremental-planner"
            );
            let bin = nim_planner_dir.join("bin/laminaria-incremental-planner");
            assert!(
                bin.is_file(),
                "expected {} to exist after building it",
                bin.display()
            );
            bin
        })
        .clone()
}

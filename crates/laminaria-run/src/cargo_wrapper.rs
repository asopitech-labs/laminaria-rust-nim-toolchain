//! Per-`rustc`-invocation measurement via wrapper substitution -- **not**
//! OS-level process-tree walking (ptrace/`/proc`). This is the technique
//! `rust-lang/rustc-perf` (the reference project
//! `docs/measurement-foundation.md` section 16 names) actually uses in its
//! production collector (`collector/src/bin/rustc-fake.rs`,
//! `collector/src/compile/execute/mod.rs`'s `.env("RUSTC", &*FAKE_RUSTC)`):
//! override Cargo's `RUSTC` environment variable to point at a thin wrapper
//! binary. Cargo then invokes the wrapper once per compilation unit instead
//! of the real compiler, so `Run::process_trace` can get one `ProcessRecord`
//! per actual `rustc` invocation -- the "parent/child process relationships
//! are preserved" acceptance criterion, for the Rust side of a Cargo build
//! specifically -- without needing platform-specific tree-walking code at
//! all. Studied from the real, cloned `rustc-perf` source before writing
//! this, not re-derived from first principles -- see `NOTES.md`.
//!
//! Linker cost is not separately recorded, matching `rustc-fake`'s own
//! accepted scope: `reap`'s cumulative subtree accounting (see
//! `tracer.rs`'s doc comment) means the linker's usage rolls up into
//! whichever `rustc` invocation spawned it.
//!
//! `src/bin/rustc_wrapper.rs` is the actual substituted binary; this module
//! holds the env var contract and the JSONL event-file protocol both that
//! binary and `crate::lib`'s `run_and_record` depend on, so the two stay in
//! sync by construction rather than by convention.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::types::ProcessRecord;

/// Absolute path to the real `rustc` the wrapper should ultimately invoke.
pub const ENV_WRAPPED_RUSTC: &str = "LAMINARIA_WRAPPED_RUSTC";
/// JSONL file the wrapper appends one `ProcessRecord` to per invocation.
pub const ENV_EVENTS_PATH: &str = "LAMINARIA_RUN_EVENTS_PATH";
/// The outer Run's `RunClock` anchor (`RunClock::anchor_unix_ns`, decimal
/// nanoseconds since the Unix epoch), so a wrapper invocation -- necessarily
/// a separate process, unable to share the outer `Instant`-based clock
/// directly -- can still compute a `start_elapsed_ns`/`end_elapsed_ns`
/// comparable to the outer command's own clock. This is a real, accepted
/// trade-off: cross-process correlation falls back to wall-clock deltas
/// against a shared anchor (`SystemTime`), which cannot offer the same
/// monotonicity guarantee `Instant`-based deltas have within one process --
/// called out here rather than silently assumed equivalent.
pub const ENV_CLOCK_ANCHOR_UNIX_NS: &str = "LAMINARIA_RUN_CLOCK_ANCHOR_UNIX_NS";

/// Appends one `ProcessRecord` to the JSONL events file at `path`, as a
/// single `write_all` call. Relied on to be safe under concurrent writers
/// (Cargo parallelizes independent `rustc` invocations across build jobs):
/// POSIX guarantees a `write(2)` to a file opened `O_APPEND` is atomic with
/// respect to other writers on the same local file, *provided the whole
/// line is written in one syscall* -- true here, since the line is fully
/// formatted in memory before the single `write_all`.
pub fn append_event(path: &Path, record: &ProcessRecord) -> std::io::Result<()> {
    let mut line = serde_json::to_string(record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push('\n');

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}

/// Reads back every `ProcessRecord` appended to `path` by wrapper
/// invocations. Missing file (no wrapper ever ran -- e.g. the traced
/// command wasn't Cargo, or ran no compilation units) is treated as "zero
/// events", not an error.
pub fn read_events(path: &Path) -> std::io::Result<Vec<ProcessRecord>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })
        .collect()
}

/// Resolves the path to the `laminaria-rustc-wrapper` binary this crate
/// builds, relative to the currently-running executable's own directory --
/// the standard "auxiliary binary ships next to the main one" layout
/// (`cargo build` places every `[[bin]]` target in the same output
/// directory). Returns `None` when it can't be found there, so a caller can
/// fall back to not wrapping rather than fail the whole traced command.
pub fn find_rustc_wrapper_binary() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let dir = current_exe.parent()?;
    let candidate = dir.join(if cfg!(windows) {
        "laminaria-rustc-wrapper.exe"
    } else {
        "laminaria-rustc-wrapper"
    });
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExitStatusRecord, ProbeLevel, ResourceUsage};

    fn sample_record(pid: u32) -> ProcessRecord {
        ProcessRecord {
            pid: Some(pid),
            parent_pid: Some(1),
            executable: None,
            argv: vec!["rustc".to_string()],
            cwd: None,
            start_elapsed_ns: 0,
            end_elapsed_ns: Some(1000),
            exit_status: Some(ExitStatusRecord {
                success: true,
                code: Some(0),
                signal: None,
            }),
            resource_usage: ResourceUsage::default(),
            probe_level: ProbeLevel::Level1ProcessResource,
            coverage_note: "test".to_string(),
        }
    }

    #[test]
    fn append_then_read_round_trips_multiple_events_in_order() {
        let path = std::env::temp_dir().join(format!(
            "laminaria-run-cargo-wrapper-test-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        append_event(&path, &sample_record(100)).unwrap();
        append_event(&path, &sample_record(200)).unwrap();
        append_event(&path, &sample_record(300)).unwrap();

        let events = read_events(&path).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].pid, Some(100));
        assert_eq!(events[1].pid, Some(200));
        assert_eq!(events[2].pid, Some(300));
    }

    #[test]
    fn read_events_on_a_missing_file_returns_empty_not_an_error() {
        let path = std::env::temp_dir().join(format!(
            "laminaria-run-cargo-wrapper-test-missing-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let events = read_events(&path).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn concurrent_appends_never_interleave_a_line() {
        // Cargo runs independent rustc invocations in parallel (separate
        // OS processes), so this module's concurrency-safety claim relies
        // on O_APPEND's atomic-write guarantee -- a property of the
        // underlying file descriptor/kernel write path, not of whether the
        // caller is a thread or a process. This test exercises it with
        // concurrent threads each independently opening and appending to
        // the same path (not a full separate-process harness, which would
        // need its own throwaway binary), which is a faithful enough stand-in
        // for the real multi-process case: if any write interleaved with
        // another, at least one line would fail to parse as valid JSON.
        let path = std::env::temp_dir().join(format!(
            "laminaria-run-cargo-wrapper-test-concurrent-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let n = 20;
        let children: Vec<_> = (0..n)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    append_event(&path, &sample_record(1000 + i)).unwrap();
                })
            })
            .collect();
        for child in children {
            child.join().unwrap();
        }

        let events = read_events(&path).unwrap();
        assert_eq!(
            events.len(),
            n as usize,
            "expected exactly {n} well-formed lines with no interleaving corruption"
        );
    }
}

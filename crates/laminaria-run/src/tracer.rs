//! Level 0/1 process tracer (`docs/measurement-foundation.md` section 6):
//! spawns the Run's root command, redirects its stdout/stderr to files, and
//! reaps it with `wait4` (Unix) so the returned `ProcessRecord` carries not
//! just lifecycle/exit-status (Level 0) but CPU/RSS/I/O/fault/
//! context-switch counters (Level 1).
//!
//! **What this does and does not capture, verified rather than assumed**:
//! calling `wait4` on the direct child returns resource usage that
//! *aggregates that child's own already-reaped descendants* -- confirmed
//! empirically (a child that forks a grandchild, waits on it, then exits;
//! the parent's `wait4` on that child reports the grandchild's CPU time).
//! So as long as every intermediate process in the tree (Cargo reaping
//! rustc, rustc reaping the linker, ...) properly waits on its own
//! children -- true of every tool this crate targets -- one `wait4` call
//! on the root command yields cumulative subtree CPU time and the single
//! largest peak-RSS-per-call observed anywhere in the tree. It does
//! **not** give a per-node breakdown: individual descendant pid/parent/
//! argv/timing is not recorded by this module. See `ProcessTrace::known_gaps`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::clock::RunClock;
use crate::types::{ExitStatusRecord, ProbeLevel, ProcessRecord, ResourceUsage, RootCommand};

pub const COVERAGE_NOTE: &str = "resource_usage is cumulative over the root process's entire \
    reaped descendant subtree, obtained via a single wait4() call on the root pid; individual \
    descendant processes (their own pid/parent/argv/timing) are not separately recorded by this \
    tracer -- see ProcessTrace::known_gaps";

/// Spawns `root`, redirecting stdout/stderr to `stdout_path`/`stderr_path`,
/// waits for it to exit, and returns its `ProcessRecord`. All timestamps
/// are relative to `clock`.
pub fn trace_root_command(
    clock: &RunClock,
    root: &RootCommand,
    stdout_path: &Path,
    stderr_path: &Path,
) -> io::Result<ProcessRecord> {
    let stdout_file = File::create(stdout_path)?;
    let stderr_file = File::create(stderr_path)?;

    let mut command = Command::new(&root.program);
    command.args(&root.args);
    if let Some(cwd) = &root.cwd {
        command.current_dir(cwd);
    }
    for (key, value) in &root.env_overrides {
        command.env(key, value);
    }
    command.stdout(stdout_file);
    command.stderr(stderr_file);

    let start_elapsed_ns = clock.elapsed_ns();
    let child = command.spawn()?;
    let pid = child.id();

    let (exit_status, resource_usage) = reap(pid)?;
    let end_elapsed_ns = clock.elapsed_ns();

    // `child` is intentionally dropped here without calling `child.wait()`
    // -- `reap` already consumed the process via wait4 (verified separately
    // not to double-reap or panic; see crates/laminaria-run/NOTES.md).
    // Rust's `Child::drop` does not itself call `waitpid`, so this is safe.
    drop(child);

    Ok(ProcessRecord {
        pid: Some(pid),
        parent_pid: Some(std::process::id()),
        executable: resolve_executable(&root.program),
        argv: std::iter::once(root.program.clone())
            .chain(root.args.iter().cloned())
            .collect(),
        cwd: root.cwd.clone(),
        start_elapsed_ns,
        end_elapsed_ns: Some(end_elapsed_ns),
        exit_status: Some(exit_status),
        resource_usage,
        probe_level: ProbeLevel::Level1ProcessResource,
        coverage_note: COVERAGE_NOTE.to_string(),
    })
}

/// Resolves `program` to an absolute path when it's found on `PATH`,
/// falling back to treating it as already a path. Best-effort only -- this
/// is recorded evidence, not something later code should rely on for
/// correctness.
fn resolve_executable(program: &str) -> Option<PathBuf> {
    laminaria_fingerprint::exec::which(program).or_else(|| {
        let candidate = PathBuf::from(program);
        candidate.is_file().then_some(candidate)
    })
}

#[cfg(unix)]
fn reap(pid: u32) -> io::Result<(ExitStatusRecord, ResourceUsage)> {
    let mut status: libc::c_int = 0;
    let mut rusage: libc::rusage = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::wait4(pid as libc::pid_t, &mut status, 0, &mut rusage) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    let exit_status = if libc::WIFEXITED(status) {
        let code = libc::WEXITSTATUS(status);
        ExitStatusRecord {
            success: code == 0,
            code: Some(code),
            signal: None,
        }
    } else if libc::WIFSIGNALED(status) {
        let signal = libc::WTERMSIG(status);
        ExitStatusRecord {
            success: false,
            code: None,
            signal: Some(signal),
        }
    } else {
        ExitStatusRecord {
            success: false,
            code: None,
            signal: None,
        }
    };

    Ok((exit_status, resource_usage_from_rusage(&rusage)))
}

#[cfg(unix)]
fn resource_usage_from_rusage(rusage: &libc::rusage) -> ResourceUsage {
    let user_cpu_seconds =
        rusage.ru_utime.tv_sec as f64 + (rusage.ru_utime.tv_usec as f64 / 1_000_000.0);
    let system_cpu_seconds =
        rusage.ru_stime.tv_sec as f64 + (rusage.ru_stime.tv_usec as f64 / 1_000_000.0);

    // ru_maxrss's unit is platform-specific -- verified, not assumed:
    // kilobytes on Linux, bytes on Darwin/macOS (confirmed against this
    // repo's own CI: a trivial test binary's ru_maxrss reads as plausible
    // bytes (~976KB) on macOS, which would be an implausible ~976MB if
    // treated as kilobytes there).
    #[cfg(target_os = "linux")]
    let peak_rss_bytes = Some((rusage.ru_maxrss as u64).saturating_mul(1024));
    #[cfg(target_os = "macos")]
    let peak_rss_bytes = Some(rusage.ru_maxrss as u64);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let peak_rss_bytes: Option<u64> = None;

    #[allow(unused_mut)]
    let mut unsupported_fields = Vec::new();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    unsupported_fields.push(
        "peak_rss_bytes (ru_maxrss unit not verified on this target; see resource_usage_from_rusage)"
            .to_string(),
    );

    ResourceUsage {
        user_cpu_seconds: Some(user_cpu_seconds),
        system_cpu_seconds: Some(system_cpu_seconds),
        peak_rss_bytes,
        block_input_ops: Some(rusage.ru_inblock as u64),
        block_output_ops: Some(rusage.ru_oublock as u64),
        minor_faults: Some(rusage.ru_minflt as u64),
        major_faults: Some(rusage.ru_majflt as u64),
        voluntary_context_switches: Some(rusage.ru_nvcsw as u64),
        involuntary_context_switches: Some(rusage.ru_nivcsw as u64),
        unsupported_fields,
    }
}

#[cfg(not(unix))]
fn reap(_pid: u32) -> io::Result<(ExitStatusRecord, ResourceUsage)> {
    // Level 1 (resource accounting) is Unix-only in this first pass --
    // explicitly unsupported here rather than fabricating zeros, per
    // docs/measurement-foundation.md section 6's acceptance criterion.
    // A caller on a non-Unix target should fall back to Level 0 lifecycle
    // tracing only (not implemented as a separate path in this crate yet).
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Level 1 resource tracing (wait4-based) is only implemented for Unix targets",
    ))
}

pub fn env_overrides_from<I, K, V>(pairs: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    pairs
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RootCommand;
    use std::collections::BTreeMap;

    fn tmp_paths(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "laminaria-run-tracer-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (dir.join("stdout.log"), dir.join("stderr.log"))
    }

    #[test]
    fn traces_a_successful_command_with_resource_usage() {
        let clock = RunClock::start();
        let (stdout_path, stderr_path) = tmp_paths("success");
        let root = RootCommand {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "echo hi; exit 0".to_string()],
            cwd: None,
            env_overrides: BTreeMap::new(),
        };
        let record = trace_root_command(&clock, &root, &stdout_path, &stderr_path).unwrap();

        assert_eq!(record.probe_level, ProbeLevel::Level1ProcessResource);
        let status = record.exit_status.as_ref().unwrap();
        assert!(status.success);
        assert_eq!(status.code, Some(0));
        assert!(record.end_elapsed_ns.unwrap() >= record.start_elapsed_ns);
        assert!(record.resource_usage.user_cpu_seconds.is_some());
        assert!(record.resource_usage.unsupported_fields.is_empty());
        assert_eq!(std::fs::read_to_string(&stdout_path).unwrap().trim(), "hi");
    }

    #[test]
    fn traces_a_failing_command_without_losing_evidence() {
        let clock = RunClock::start();
        let (stdout_path, stderr_path) = tmp_paths("failure");
        let root = RootCommand {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "echo oops 1>&2; exit 3".to_string()],
            cwd: None,
            env_overrides: BTreeMap::new(),
        };
        let record = trace_root_command(&clock, &root, &stdout_path, &stderr_path).unwrap();

        let status = record.exit_status.as_ref().unwrap();
        assert!(!status.success);
        assert_eq!(status.code, Some(3));
        assert_eq!(status.signal, None);
        // Evidence collected up to the failure must still be present, not
        // discarded (docs/measurement-foundation.md's "failure/cancellation
        // does not discard partial evidence" acceptance criterion).
        assert!(record.resource_usage.user_cpu_seconds.is_some());
        assert_eq!(
            std::fs::read_to_string(&stderr_path).unwrap().trim(),
            "oops"
        );
    }

    #[test]
    fn traces_a_signal_killed_command() {
        let clock = RunClock::start();
        let (stdout_path, stderr_path) = tmp_paths("signal");
        let root = RootCommand {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "kill -TERM $$".to_string()],
            cwd: None,
            env_overrides: BTreeMap::new(),
        };
        let record = trace_root_command(&clock, &root, &stdout_path, &stderr_path).unwrap();

        let status = record.exit_status.as_ref().unwrap();
        assert!(!status.success);
        assert_eq!(status.code, None);
        assert_eq!(status.signal, Some(libc::SIGTERM));
    }

    #[test]
    fn resource_usage_aggregates_a_grandchild_process_cumulatively() {
        // Same claim this module's doc comment makes, exercised through
        // the real trace_root_command path rather than the standalone C
        // probe used to first establish it: spawn a shell that forks a
        // CPU-burning subshell and waits on it, then confirm the returned
        // user_cpu_seconds reflects that grandchild's work.
        let clock = RunClock::start();
        let (stdout_path, stderr_path) = tmp_paths("grandchild");
        let root = RootCommand {
            program: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                // ~200k pure-shell arithmetic iterations: slow enough to
                // reliably clear the 0.01s assertion threshold below, fast
                // enough not to make this test suite noticeably slower.
                "sh -c 'i=0; while [ $i -lt 200000 ]; do i=$((i+1)); done'".to_string(),
            ],
            cwd: None,
            env_overrides: BTreeMap::new(),
        };
        let record = trace_root_command(&clock, &root, &stdout_path, &stderr_path).unwrap();
        let cpu = record.resource_usage.user_cpu_seconds.unwrap()
            + record.resource_usage.system_cpu_seconds.unwrap();
        assert!(
            cpu > 0.01,
            "expected the grandchild's busy-loop CPU time to be attributed to the root record, got {cpu}s"
        );
    }
}

//! The binary substituted for `rustc` via Cargo's `RUSTC` environment
//! variable (see `laminaria_run::cargo_wrapper`'s module doc for the
//! design this is modeled on -- `rust-lang/rustc-perf`'s `rustc-fake`,
//! studied from its real source before writing this, not re-derived).
//!
//! Cargo invokes this binary once per compilation unit, believing it *is*
//! rustc. It runs the real compiler (path from `LAMINARIA_WRAPPED_RUSTC`)
//! as a genuine child so it can measure it (reusing
//! `laminaria_run::tracer::reap`, the same, already-verified `wait4` logic
//! the outer `laminaria run` command itself uses), appends a `ProcessRecord`
//! to the JSONL file named by `LAMINARIA_RUN_EVENTS_PATH`, then exits with
//! the real compiler's own exit code.
//!
//! stdout/stderr are **not** captured or redirected -- Cargo depends on
//! rustc's real-time JSON diagnostic stream on stdout/stderr to render
//! progress and errors, so they're left inherited (`Command`'s default),
//! passing straight through exactly as if this wrapper weren't here at all.

use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use laminaria_run::cargo_wrapper::{
    append_event, ENV_CLOCK_ANCHOR_UNIX_NS, ENV_EVENTS_PATH, ENV_WRAPPED_RUSTC,
};
use laminaria_run::tracer::reap;
use laminaria_run::types::{ProbeLevel, ProcessRecord};

fn main() -> ExitCode {
    let real_rustc = match env::var(ENV_WRAPPED_RUSTC) {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            eprintln!(
                "laminaria-rustc-wrapper: {ENV_WRAPPED_RUSTC} is not set -- this binary is only \
                 meant to be invoked by `laminaria run` via Cargo's RUSTC override, not directly"
            );
            return ExitCode::from(2);
        }
    };

    let args: Vec<String> = env::args().skip(1).collect();
    let cwd = env::current_dir().ok();

    let anchor_unix_ns: u128 = env::var(ENV_CLOCK_ANCHOR_UNIX_NS)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let start_wall_ns = now_unix_ns();
    let start_elapsed_ns = start_wall_ns.saturating_sub(anchor_unix_ns) as u64;

    let mut command = Command::new(&real_rustc);
    command.args(&args);
    // Deliberately no .stdout()/.stderr() overrides -- inherited, so Cargo
    // sees the real compiler's output exactly as if talking to it directly.

    let spawn_result = command.spawn();
    let child = match spawn_result {
        Ok(child) => child,
        Err(err) => {
            eprintln!(
                "laminaria-rustc-wrapper: failed to spawn real rustc at {}: {err}",
                real_rustc.display()
            );
            return ExitCode::from(2);
        }
    };
    let pid = child.id();

    let (exit_status, resource_usage) = match reap(pid) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("laminaria-rustc-wrapper: failed to reap real rustc (pid {pid}): {err}");
            return ExitCode::from(2);
        }
    };
    drop(child);

    let end_elapsed_ns = now_unix_ns().saturating_sub(anchor_unix_ns) as u64;

    if let Ok(events_path) = env::var(ENV_EVENTS_PATH) {
        let record = ProcessRecord {
            pid: Some(pid),
            // This wrapper's own pid -- the true, directly-observed parent
            // of the measured rustc invocation. One level short of Cargo's
            // own pid (Cargo spawned this wrapper, not the measured rustc
            // directly), since Cargo does not expose its own pid to a
            // RUSTC-substituted child through any documented channel; noted
            // in coverage_note rather than silently treated as "Cargo's pid".
            parent_pid: Some(std::process::id()),
            executable: Some(real_rustc.clone()),
            argv: std::iter::once(real_rustc.display().to_string())
                .chain(args.iter().cloned())
                .collect(),
            cwd,
            start_elapsed_ns,
            end_elapsed_ns: Some(end_elapsed_ns),
            exit_status: Some(exit_status.clone()),
            resource_usage,
            probe_level: ProbeLevel::Level1ProcessResource,
            coverage_note: "one real rustc invocation, substituted via Cargo's RUSTC env var \
                (see laminaria_run::cargo_wrapper); parent_pid is this wrapper's own transient \
                pid, one level short of Cargo's own pid, which Cargo does not expose to a \
                RUSTC-substituted child; start/end_elapsed_ns are wall-clock deltas against the \
                outer Run's clock anchor, not Instant-based, since Instant cannot cross a \
                process boundary"
                .to_string(),
        };
        // Best-effort: a failure to record this one invocation's evidence
        // must not fail the actual compilation Cargo is waiting on.
        if let Err(err) = append_event(&PathBuf::from(events_path), &record) {
            eprintln!("laminaria-rustc-wrapper: failed to append Run event: {err}");
        }
    }

    match (exit_status.code, exit_status.signal) {
        // std::process::exit, not ExitCode::from(code as u8): the real
        // compiler's exit code is an i32 (verified: ExitStatusRecord::code
        // is Option<i32>), and Cargo may care about the exact value beyond
        // 0/non-zero. ExitCode::from only accepts a u8 on every platform
        // (not just Windows), so it silently truncates any code above 255 --
        // std::process::exit passes the full i32 through to the OS instead.
        (Some(code), _) => std::process::exit(code),
        (None, Some(_signal)) => ExitCode::from(1),
        (None, None) => ExitCode::from(1),
    }
}

fn now_unix_ns() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

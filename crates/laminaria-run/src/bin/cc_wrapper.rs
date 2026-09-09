//! The binary substituted for Nim's C/C++ compiler via `--<ccname>.exe:`/
//! `--<ccname>.linkerexe:` overrides (see `laminaria_run::nim_wrapper`'s
//! module doc for the design -- studied from Nim's own real
//! `compiler/extccomp.nim` source before writing this).
//!
//! Structurally identical to `rustc_wrapper.rs`: run the real compiler
//! (path from `LAMINARIA_WRAPPED_CC`) as a genuine child, measure it via
//! `laminaria_run::tracer::reap` (the same, already-verified `wait4`
//! logic), append a `ProcessRecord` to the JSONL events file, forward the
//! real exit code. stdout/stderr stay inherited -- Nim inspects the C
//! compiler's own output for diagnostics.
//!
//! Invoked identically whether Nim is compiling one `.c` file or linking
//! the final binary; this wrapper does not need to distinguish the two,
//! since it only ever forwards whatever argv it was given.

use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use laminaria_run::cargo_wrapper::append_event;
use laminaria_run::nim_wrapper::{ENV_CLOCK_ANCHOR_UNIX_NS, ENV_EVENTS_PATH, ENV_WRAPPED_CC};
use laminaria_run::tracer::reap;
use laminaria_run::types::{ProbeLevel, ProcessRecord};

fn main() -> ExitCode {
    let real_cc = match env::var(ENV_WRAPPED_CC) {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            eprintln!(
                "laminaria-cc-wrapper: {ENV_WRAPPED_CC} is not set -- this binary is only meant \
                 to be invoked by `laminaria run` via Nim's --<ccname>.exe override, not directly"
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

    let mut command = Command::new(&real_cc);
    command.args(&args);

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!(
                "laminaria-cc-wrapper: failed to spawn real compiler at {}: {err}",
                real_cc.display()
            );
            return ExitCode::from(2);
        }
    };
    let pid = child.id();

    let (exit_status, resource_usage) = match reap(pid) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("laminaria-cc-wrapper: failed to reap real compiler (pid {pid}): {err}");
            return ExitCode::from(2);
        }
    };
    drop(child);

    let end_elapsed_ns = now_unix_ns().saturating_sub(anchor_unix_ns) as u64;

    if let Ok(events_path) = env::var(ENV_EVENTS_PATH) {
        let record = ProcessRecord {
            pid: Some(pid),
            parent_pid: Some(std::process::id()),
            executable: Some(real_cc.clone()),
            argv: std::iter::once(real_cc.display().to_string())
                .chain(args.iter().cloned())
                .collect(),
            cwd,
            start_elapsed_ns,
            end_elapsed_ns: Some(end_elapsed_ns),
            exit_status: Some(exit_status.clone()),
            resource_usage,
            probe_level: ProbeLevel::Level1ProcessResource,
            coverage_note: "one real C/C++ compiler invocation (compile or link -- this wrapper \
                does not distinguish the two, it only forwards argv), substituted via Nim's \
                --<ccname>.exe/--<ccname>.linkerexe overrides (see laminaria_run::nim_wrapper); \
                parent_pid is this wrapper's own transient pid, one level short of Nim's own pid, \
                which Nim does not expose to a substituted child; start/end_elapsed_ns are \
                wall-clock deltas against the outer Run's clock anchor, not Instant-based, since \
                Instant cannot cross a process boundary"
                .to_string(),
        };
        if let Err(err) = append_event(&PathBuf::from(events_path), &record) {
            eprintln!("laminaria-cc-wrapper: failed to append Run event: {err}");
        }
    }

    match (exit_status.code, exit_status.signal) {
        // std::process::exit, not ExitCode::from(code as u8) -- see
        // rustc_wrapper.rs's identical fix for why: ExitCode::from only
        // accepts a u8 on every platform, silently truncating any exit code
        // above 255, whereas std::process::exit passes the real compiler's
        // full i32 exit code through to the OS.
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

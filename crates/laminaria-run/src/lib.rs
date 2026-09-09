//! Versioned Run envelope and Level 0/1 process/resource tracer for
//! LAMINARIA (issue #19), following `docs/measurement-foundation.md`.
//!
//! This crate implements a first, honest slice of that design, not the
//! whole thing. See each module's doc comment for exactly what it covers;
//! the summary:
//!
//! - `types`: the versioned `Run`/`ProcessRecord`/`Summary` schema
//!   (section 5).
//! - `clock`: one monotonic clock per Run (section 6).
//! - `tracer`: spawns and reaps the root command, capturing Level 0
//!   lifecycle data and Level 1 `wait4`-derived resource usage for that
//!   one process (cumulative over its reaped subtree -- see `tracer`'s
//!   doc comment for what that does and does not mean).
//! - `store`: the `runs/<run-id>/` on-disk layout (section 5) and
//!   `summary.json` regeneration from raw evidence.
//!
//! **Not yet implemented, deliberately left as explicit gaps rather than
//! silently assumed**: individual descendant-process enumeration (only
//! the root command's own record exists so far), Level 2 compiler-native
//! telemetry adapters, Level 3 platform profiler integration, artifact
//! inventory (section 8), cache-state/preparation-record population
//! (sections 9-10), and measurement-overhead comparison across probe
//! levels (section 12).

pub mod cargo_wrapper;
pub mod clock;
pub mod store;
pub mod tracer;
pub mod types;

use std::path::{Path, PathBuf};

use laminaria_fingerprint::doctor;

use crate::cargo_wrapper::{
    find_rustc_wrapper_binary, read_events, ENV_CLOCK_ANCHOR_UNIX_NS, ENV_EVENTS_PATH,
    ENV_WRAPPED_RUSTC,
};
use crate::clock::RunClock;
use crate::types::{
    CacheState, MeasurementOverhead, PreparationRecord, ProbeLevel, ProcessTrace, RootCommand, Run,
    RunResult, SCHEMA_VERSION,
};

/// Whether `root` invokes Cargo (by program name, ignoring any leading
/// directory) -- the trigger for RUSTC-wrapper substitution below. A bare
/// name check, not a resolved-path check: `cargo build`, `/usr/bin/cargo
/// test`, and `~/.cargo/bin/cargo check` should all qualify.
fn is_cargo_command(root: &RootCommand) -> bool {
    Path::new(&root.program)
        .file_stem()
        .and_then(|s| s.to_str())
        == Some("cargo")
}

/// Attempts to set up RUSTC-wrapper substitution (`cargo_wrapper` module
/// doc) for a Cargo root command: resolves a real `rustc` on `PATH` and
/// this crate's own `laminaria-rustc-wrapper` binary, then returns the env
/// vars to inject plus the events file they'll write to. Returns `None`
/// (with an explanatory note) when either can't be resolved -- wrapping is
/// an enhancement over the existing root-command-only tracing, not a
/// requirement, so its absence must never fail the traced command.
fn prepare_cargo_wrapping(
    events_path: &Path,
    anchor_unix_ns: u128,
) -> Result<Vec<(String, String)>, String> {
    let real_rustc = laminaria_fingerprint::exec::which("rustc")
        .ok_or_else(|| "could not resolve a `rustc` on PATH".to_string())?;
    let wrapper_bin = find_rustc_wrapper_binary().ok_or_else(|| {
        "could not find the laminaria-rustc-wrapper binary next to the running executable \
         (expected it built alongside laminaria-cli)"
            .to_string()
    })?;

    Ok(vec![
        ("RUSTC".to_string(), wrapper_bin.display().to_string()),
        (
            ENV_WRAPPED_RUSTC.to_string(),
            real_rustc.display().to_string(),
        ),
        (
            ENV_EVENTS_PATH.to_string(),
            events_path.display().to_string(),
        ),
        (
            ENV_CLOCK_ANCHOR_UNIX_NS.to_string(),
            anchor_unix_ns.to_string(),
        ),
    ])
}

/// Generates a Run id from the current wall-clock time and process id.
/// Sufficiently unique for this crate's purpose (distinct `runs/<id>/`
/// directories within one repository checkout); not a claim of global
/// uniqueness across machines.
pub fn generate_run_id() -> String {
    let unix_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{unix_ns}-{}", std::process::id())
}

/// Runs `root` end-to-end: builds the `EnvironmentFingerprint`/
/// `ToolchainReport` (reusing issue #18's `laminaria-fingerprint::doctor`),
/// traces the command via `tracer::trace_root_command`, assembles a `Run`,
/// and writes it to `runs_root/<run-id>/` via `store::write_run`.
///
/// Returns the written `Run` and the directory it was written to.
#[allow(clippy::too_many_arguments)]
pub fn run_and_record(
    runs_root: &Path,
    workload_id: &str,
    scenario_id: &str,
    requested_artifact: Option<String>,
    lock_path: &Path,
    repo_root: &Path,
    root: RootCommand,
) -> std::io::Result<(Run, PathBuf)> {
    let clock = RunClock::start();
    let run_id = generate_run_id();

    let doctor_run = doctor::build(lock_path, repo_root);

    let run_dir = runs_root.join(&run_id);
    std::fs::create_dir_all(&run_dir)?;
    let stdout_path = run_dir.join("stdout.log");
    let stderr_path = run_dir.join("stderr.log");
    let rustc_events_path = run_dir.join("rustc-invocations.jsonl");

    // Per-rustc-invocation granularity via Cargo's RUSTC env var, modeled
    // on rustc-perf's real `rustc-fake` (studied from its source, not
    // re-derived -- see cargo_wrapper's module doc and NOTES.md) --
    // *not* OS-level process-tree walking. Only attempted for a Cargo root
    // command; a resolution failure only adds a known_gaps note; it never
    // fails the traced command itself.
    let mut effective_root = root.clone();
    let mut cargo_wrapping_note: Option<String> = None;
    if is_cargo_command(&root) {
        match prepare_cargo_wrapping(&rustc_events_path, clock.anchor_unix_ns()) {
            Ok(env_vars) => {
                for (key, value) in env_vars {
                    effective_root.env_overrides.insert(key, value);
                }
            }
            Err(reason) => {
                cargo_wrapping_note = Some(format!(
                    "per-rustc-invocation tracing via RUSTC-wrapper substitution was not applied: {reason}"
                ));
            }
        }
    }

    let tracer_wall_start = std::time::Instant::now();
    let process_record =
        tracer::trace_root_command(&clock, &effective_root, &stdout_path, &stderr_path)?;
    let tracer_overhead_seconds = tracer_wall_start.elapsed().as_secs_f64();

    let rustc_invocation_records = read_events(&rustc_events_path).unwrap_or_default();
    let _ = std::fs::remove_file(&rustc_events_path);

    let run_ended_at_unix_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let exit_status = process_record
        .exit_status
        .clone()
        .expect("trace_root_command always populates exit_status on success");

    let run = Run {
        run_id: run_id.clone(),
        schema_version: SCHEMA_VERSION.to_string(),
        workload_id: workload_id.to_string(),
        scenario_id: scenario_id.to_string(),
        requested_artifact,
        environment_fingerprint: doctor_run.report.environment.clone(),
        requested_toolchain_selector: None,
        resolved_toolchain_fingerprint: Some(doctor_run.report),
        preparation_record: PreparationRecord::default(),
        cache_state: CacheState::default(),
        root_command: root,
        run_started_at_unix_ns: clock.anchor_unix_ns(),
        run_ended_at_unix_ns: Some(run_ended_at_unix_ns),
        result: Some(RunResult {
            success: exit_status.success,
            root_exit_status: exit_status,
        }),
        process_trace: {
            let rustc_invocation_count = rustc_invocation_records.len();
            let mut processes = vec![process_record];
            processes.extend(rustc_invocation_records);

            let mut known_gaps = Vec::new();
            if rustc_invocation_count > 0 {
                known_gaps.push(format!(
                    "{rustc_invocation_count} individual rustc invocation(s) were recorded via \
                     RUSTC-wrapper substitution (see laminaria_run::cargo_wrapper), but this is \
                     Cargo/rustc-specific -- no equivalent per-invocation wrapping exists yet for \
                     the linker, Nim's compiler, or any non-Cargo root command; those still only \
                     have the root record's cumulative wait4-based resource_usage"
                ));
            } else {
                known_gaps.push(
                    "individual descendant process enumeration (per-node pid/parent/argv/timing) \
                     is not implemented for this Run; resource_usage on the root record is \
                     cumulative over its entire reaped subtree via wait4"
                        .to_string(),
                );
            }
            if let Some(note) = cargo_wrapping_note {
                known_gaps.push(note);
            }
            known_gaps.push(
                "Level 2 compiler-native telemetry and Level 3 platform profiler data are not \
                 captured"
                    .to_string(),
            );
            known_gaps.push(
                "artifact inventory (docs/measurement-foundation.md section 8) is not captured"
                    .to_string(),
            );

            ProcessTrace {
                processes,
                known_gaps,
            }
        },
        compiler_telemetry: None,
        artifact_delta: None,
        measurement_overhead: Some(MeasurementOverhead {
            probe_level: ProbeLevel::Level1ProcessResource,
            tracer_overhead_seconds: Some(tracer_overhead_seconds),
            notes: vec![
                "tracer_overhead_seconds is this crate's own wall time around spawn/reap/\
                 record-building, not a delta against a Level 0-only baseline -- section 12's \
                 cross-probe-level overhead comparison is not yet implemented"
                    .to_string(),
            ],
        }),
    };

    let dir = store::write_run(runs_root, &run)?;
    Ok((run, dir))
}

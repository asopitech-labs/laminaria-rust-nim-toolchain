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

pub mod clock;
pub mod store;
pub mod tracer;
pub mod types;

use std::path::{Path, PathBuf};

use laminaria_fingerprint::doctor;

use crate::clock::RunClock;
use crate::types::{
    CacheState, MeasurementOverhead, PreparationRecord, ProbeLevel, ProcessTrace, RootCommand, Run,
    RunResult, SCHEMA_VERSION,
};

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

    let tracer_wall_start = std::time::Instant::now();
    let stdout_path = runs_root.join(&run_id).join("stdout.log");
    let stderr_path = runs_root.join(&run_id).join("stderr.log");
    std::fs::create_dir_all(runs_root.join(&run_id))?;
    let process_record = tracer::trace_root_command(&clock, &root, &stdout_path, &stderr_path)?;
    let tracer_overhead_seconds = tracer_wall_start.elapsed().as_secs_f64();

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
        process_trace: ProcessTrace {
            processes: vec![process_record],
            known_gaps: vec![
                "individual descendant process enumeration (per-node pid/parent/argv/timing) \
                 is not implemented; resource_usage on the root record is cumulative over its \
                 entire reaped subtree via wait4"
                    .to_string(),
                "Level 2 compiler-native telemetry and Level 3 platform profiler data are not \
                 captured"
                    .to_string(),
                "artifact inventory (docs/measurement-foundation.md section 8) is not captured"
                    .to_string(),
            ],
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

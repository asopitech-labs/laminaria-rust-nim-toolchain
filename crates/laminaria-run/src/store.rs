//! Writes and reads the `runs/<run-id>/` on-disk layout
//! (`docs/measurement-foundation.md` section 5's candidate layout) and
//! regenerates `summary.json` from already-written raw evidence.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::types::{ProcessRecord, Run, Summary};

/// Directory for one Run: `<runs_root>/<run_id>/`.
pub fn run_dir(runs_root: &Path, run_id: &str) -> PathBuf {
    runs_root.join(run_id)
}

/// Writes the full `runs/<run-id>/` layout for `run`. `stdout`/`stderr`
/// have already been written by the tracer directly to their target paths
/// (see `crate::tracer::trace_root_command`), so this only writes the
/// JSON/JSONL evidence files plus a freshly regenerated `summary.json`.
pub fn write_run(runs_root: &Path, run: &Run) -> io::Result<PathBuf> {
    let dir = run_dir(runs_root, &run.run_id);
    fs::create_dir_all(&dir)?;

    write_json(&dir.join("run.json"), run)?;
    write_json(&dir.join("environment.json"), &run.environment_fingerprint)?;
    write_jsonl(&dir.join("processes.jsonl"), &run.process_trace.processes)?;
    // Level 2 (compiler telemetry) and artifact-inventory evidence are not
    // yet produced by this crate -- the files still exist (empty), matching
    // docs/measurement-foundation.md section 5's candidate layout, so a
    // later writer can append to them without a layout migration.
    touch(&dir.join("compiler-events.jsonl"))?;
    touch(&dir.join("artifacts.jsonl"))?;

    let summary = regenerate_summary(run);
    write_json(&dir.join("summary.json"), &summary)?;

    Ok(dir)
}

/// Reads back a previously written `run.json`.
pub fn read_run(runs_root: &Path, run_id: &str) -> io::Result<Run> {
    let path = run_dir(runs_root, run_id).join("run.json");
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Rebuilds `summary.json` from an in-memory `Run` -- the pure function
/// `regenerate_summary_from_disk` below builds on. Kept separate so a
/// caller that already has a `Run` in hand (e.g. right after tracing) does
/// not need to round-trip it through disk first.
pub fn regenerate_summary(run: &Run) -> Summary {
    let root: Option<&ProcessRecord> = run.process_trace.processes.first();
    let wall_seconds = match (root, run.run_ended_at_unix_ns) {
        (Some(record), Some(_)) => record
            .end_elapsed_ns
            .map(|end| (end - record.start_elapsed_ns) as f64 / 1_000_000_000.0),
        _ => None,
    };

    Summary {
        run_id: run.run_id.clone(),
        schema_version: run.schema_version.clone(),
        workload_id: run.workload_id.clone(),
        scenario_id: run.scenario_id.clone(),
        success: run.result.as_ref().map(|r| r.success),
        wall_seconds,
        root_user_cpu_seconds: root.and_then(|r| r.resource_usage.user_cpu_seconds),
        root_system_cpu_seconds: root.and_then(|r| r.resource_usage.system_cpu_seconds),
        root_peak_rss_bytes: root.and_then(|r| r.resource_usage.peak_rss_bytes),
        process_record_count: run.process_trace.processes.len(),
        known_gaps: run.process_trace.known_gaps.clone(),
    }
}

/// Reads `run.json` back from disk and rewrites `summary.json` from it --
/// the literal "regenerate `summary.json` without rerunning the workload"
/// acceptance criterion, exercised end-to-end (disk round trip included),
/// not just as an in-memory pure function.
pub fn regenerate_summary_from_disk(runs_root: &Path, run_id: &str) -> io::Result<Summary> {
    let run = read_run(runs_root, run_id)?;
    let summary = regenerate_summary(&run);
    write_json(&run_dir(runs_root, run_id).join("summary.json"), &summary)?;
    Ok(summary)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, text)
}

fn write_jsonl<T: serde::Serialize>(path: &Path, items: &[T]) -> io::Result<()> {
    let mut text = String::new();
    for item in items {
        let line = serde_json::to_string(item)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        text.push_str(&line);
        text.push('\n');
    }
    fs::write(path, text)
}

fn touch(path: &Path) -> io::Result<()> {
    if !path.exists() {
        fs::write(path, "")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CacheState, ExitStatusRecord, PreparationRecord, ProbeLevel, ProcessTrace, ResourceUsage,
        RootCommand, RunResult, SCHEMA_VERSION,
    };
    use laminaria_fingerprint::{EnvironmentFingerprint, RepositoryState};
    use std::collections::BTreeMap;

    fn sample_environment() -> EnvironmentFingerprint {
        EnvironmentFingerprint {
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
            repository: RepositoryState {
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
        }
    }

    fn sample_run(run_id: &str) -> Run {
        Run {
            run_id: run_id.to_string(),
            schema_version: SCHEMA_VERSION.to_string(),
            workload_id: "test-workload".to_string(),
            scenario_id: "test-scenario".to_string(),
            requested_artifact: None,
            environment_fingerprint: sample_environment(),
            requested_toolchain_selector: None,
            resolved_toolchain_fingerprint: None,
            preparation_record: PreparationRecord::default(),
            cache_state: CacheState::default(),
            root_command: RootCommand {
                program: "true".to_string(),
                args: Vec::new(),
                cwd: None,
                env_overrides: BTreeMap::new(),
            },
            run_started_at_unix_ns: 1_000_000_000,
            run_ended_at_unix_ns: Some(2_000_000_000),
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
                    pid: Some(1234),
                    parent_pid: Some(1),
                    executable: None,
                    argv: vec!["true".to_string()],
                    cwd: None,
                    start_elapsed_ns: 0,
                    end_elapsed_ns: Some(500_000_000),
                    exit_status: Some(ExitStatusRecord {
                        success: true,
                        code: Some(0),
                        signal: None,
                    }),
                    resource_usage: ResourceUsage {
                        user_cpu_seconds: Some(0.01),
                        system_cpu_seconds: Some(0.005),
                        peak_rss_bytes: Some(4096),
                        ..ResourceUsage::default()
                    },
                    probe_level: ProbeLevel::Level1ProcessResource,
                    coverage_note: "test".to_string(),
                }],
                known_gaps: vec!["no per-descendant enumeration".to_string()],
            },
            compiler_telemetry: None,
            artifact_delta: None,
            measurement_overhead: None,
        }
    }

    #[test]
    fn write_run_produces_the_documented_layout() {
        let tmp = std::env::temp_dir().join(format!(
            "laminaria-run-store-test-layout-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let run = sample_run("run-layout-test");

        let dir = write_run(&tmp, &run).unwrap();

        for name in [
            "run.json",
            "environment.json",
            "processes.jsonl",
            "compiler-events.jsonl",
            "artifacts.jsonl",
            "summary.json",
        ] {
            assert!(dir.join(name).exists(), "missing {name}");
        }
    }

    #[test]
    fn regenerate_summary_from_disk_matches_the_in_memory_summary() {
        let tmp = std::env::temp_dir().join(format!(
            "laminaria-run-store-test-regen-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let run = sample_run("run-regen-test");
        write_run(&tmp, &run).unwrap();

        let in_memory = regenerate_summary(&run);

        // Delete summary.json entirely, then rebuild it purely from
        // run.json -- the literal acceptance criterion, not just the pure
        // function in isolation.
        fs::remove_file(run_dir(&tmp, &run.run_id).join("summary.json")).unwrap();
        let from_disk = regenerate_summary_from_disk(&tmp, &run.run_id).unwrap();

        assert_eq!(in_memory, from_disk);
        assert_eq!(from_disk.process_record_count, 1);
        assert_eq!(from_disk.root_user_cpu_seconds, Some(0.01));
        assert_eq!(from_disk.wall_seconds, Some(0.5));
        assert_eq!(
            from_disk.known_gaps,
            vec!["no per-descendant enumeration".to_string()]
        );

        // And the regenerated file is actually back on disk, not just
        // returned in memory.
        let reloaded: Summary = serde_json::from_str(
            &fs::read_to_string(run_dir(&tmp, &run.run_id).join("summary.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(reloaded, from_disk);
    }

    #[test]
    fn read_run_round_trips_through_json() {
        let tmp = std::env::temp_dir().join(format!(
            "laminaria-run-store-test-roundtrip-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let run = sample_run("run-roundtrip-test");
        write_run(&tmp, &run).unwrap();

        let reloaded = read_run(&tmp, &run.run_id).unwrap();
        assert_eq!(reloaded.run_id, run.run_id);
        assert_eq!(
            reloaded.process_trace.processes.len(),
            run.process_trace.processes.len()
        );
        assert_eq!(
            reloaded.environment_fingerprint.os,
            run.environment_fingerprint.os
        );
    }
}

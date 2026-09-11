//! Issue #28 D1-a: `M8-many-unrequested-nim-planner` owned baseline.
//!
//! Measures the real Nim planner's own demand-closure pruning
//! (`nim-planner/src/planning_kernel.nim`, issue #27) directly over a
//! `PlanningInput` -- no Cargo workspace, no fixture files on disk,
//! per `docs/design/issue-35-d0-cases.yaml`'s `M8-many-unrequested-nim-planner`
//! case. `used-core`/`used-util` are siblings (`fixture-bin` depends on
//! both, they don't depend on each other); `unused-pkg-*` never connects
//! to `fixture-bin-out`'s demand at all.
//!
//! **Revised per a code-review round** (issue #28 D1-a, findings R2-R4):
//! a failed repetition is recorded (never dropped by an early `?`, and
//! `needed_set_matches_in_every_successful_rep` is explicitly `false`
//! when there are zero successful repetitions, never vacuously `true`);
//! every repetition is persisted as a genuine
//! `laminaria_run::types::Run` (`store::write_run`), so the round-trip
//! wall time is a real `ScenarioReport` (`regenerate_report_from_disk`),
//! identified by the actual `laminaria-planner` binary path used and a
//! real environment fingerprint; and the case's own confirmed
//! `measurement_boundary` ("計測開始はplanning_kernel.plan呼び出し直前、
//! 終了はExecutionPlan受領直後", `docs/design/issue-35-d0-cases.yaml`)
//! is now actually measured -- `nim-planner/src/laminaria_planner.nim`
//! reports its own internal `planFromJson`-only wall time on stderr as
//! `kernel_nanos`, captured here as a value distinct from the Run's own
//! round-trip time (which still includes process spawn/IPC/JSON, kept
//! as its own separate measurement, not replaced).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use laminaria_plan::{Action, ActionKind, ArtifactRef, PlanOutcome, PlanningInput};
use laminaria_run::clock::RunClock;
use laminaria_run::scenario::{regenerate_report_from_disk, ScenarioReport, Stats};
use laminaria_run::store::write_run;
use laminaria_run::types::{
    CacheState, ExitStatusRecord, PreparationRecord, ProbeLevel, ProcessRecord, ProcessTrace,
    ResourceUsage, RootCommand, Run, RunResult,
};
use serde::{Deserialize, Serialize};

use crate::planner_binary::resolve_or_build_planner_binary;

pub const WORKLOAD_ID: &str = "M8-many-unrequested-nim-planner@issue35-d0-accepted-c812d70-v1";

/// `ActionKind::Integrate` with no `compiler_work` descriptor: a
/// structural placeholder action (this case tests demand-closure
/// pruning itself, not any particular build/compiler-work kind) --
/// never dispatched by any executor in this measurement, only planned.
fn structural_action(id: &str, inputs: Vec<ArtifactRef>, output_artifact: &str) -> Action {
    Action {
        id: id.to_string(),
        kind: ActionKind::Integrate,
        command_identity: format!("d1a-m8-structural-placeholder:{id}"),
        inputs,
        outputs: vec![ArtifactRef::declared(output_artifact)],
        compiler_work: None,
    }
}

/// `used-core`/`used-util` (siblings, no edge between them) <-
/// `fixture-bin` (depends on both) <- demand, plus `unused_actions`
/// independent `unused-pkg-NN` actions connected to nothing demanded.
pub fn build_planning_input(unused_actions: usize) -> PlanningInput {
    let used_core = structural_action("used-core", vec![], "used-core-out");
    let used_util = structural_action("used-util", vec![], "used-util-out");
    let fixture_bin = structural_action(
        "fixture-bin",
        vec![
            ArtifactRef::declared("used-core-out"),
            ArtifactRef::declared("used-util-out"),
        ],
        "fixture-bin-out",
    );
    let mut actions = vec![used_core, used_util, fixture_bin];
    for i in 0..unused_actions {
        let id = format!("unused-pkg-{:02}", i + 1);
        let out = format!("{id}-out");
        actions.push(structural_action(&id, vec![], &out));
    }
    PlanningInput::new(vec!["fixture-bin-out".to_string()], actions)
}

const EXPECTED_NEEDED_SET: [&str; 3] = ["fixture-bin", "used-core", "used-util"];

/// Spawns `planner_bin` directly (rather than
/// `laminaria_plan::call_planner`, which doesn't expose stderr on a
/// successful call) so this measurement can read both the `PlanOutcome`
/// from stdout *and* the Nim kernel's own self-reported
/// `kernel_nanos=<u64>` line from stderr in one subprocess invocation --
/// the round trip is measured once, not twice, avoiding two separate
/// (and potentially inconsistent) subprocess calls per repetition.
fn call_planner_with_kernel_timing(
    planner_bin: &Path,
    input: &PlanningInput,
) -> Result<(PlanOutcome, Option<u64>), String> {
    let input_json =
        serde_json::to_vec(input).map_err(|e| format!("failed to serialize PlanningInput: {e}"))?;

    let mut child = Command::new(planner_bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", planner_bin.display()))?;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(&input_json)
        .map_err(|e| format!("failed to write PlanningInput to stdin: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to read planner output: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "{} exited {:?}: {}",
            planner_bin.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let outcome: PlanOutcome = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("malformed PlanOutcome on stdout: {e}"))?;

    let kernel_nanos = String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("laminaria-planner: kernel_nanos="))
        .and_then(|s| s.trim().parse::<u64>().ok());

    Ok((outcome, kernel_nanos))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepetitionRecord {
    pub run_id: String,
    pub success: bool,
    pub failure_reason: Option<String>,
    /// `None` on failure. `Some(false)` is a genuine (successful-call)
    /// defect: the planner returned a plan, but it wasn't exactly
    /// `{used-core, used-util, fixture-bin}`.
    pub needed_set_matches: Option<bool>,
    pub ordered_action_count: Option<usize>,
    /// The Nim kernel's own self-reported `planFromJson`-only wall time
    /// -- `None` if the planner failed before reporting it, or (on an
    /// older planner binary predating this instrumentation) never
    /// emitted the line at all.
    pub kernel_nanos: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScaleOutcome {
    pub unused_actions: usize,
    pub warmup_runs: usize,
    pub repetitions: Vec<RepetitionRecord>,
    /// Round-trip wall time (process spawn + stdin write + IPC + stdout
    /// parse), as a real `ScenarioReport` built from persisted `Run`s.
    pub scenario_report: Option<ScenarioReport>,
    pub scenario_report_error: Option<String>,
    /// The Nim-kernel-only wall time (`docs/design/issue-35-d0-cases.yaml`'s
    /// own confirmed `measurement_boundary`), a *separate* item from
    /// `scenario_report.wall_seconds` -- never conflated with the
    /// round-trip time above.
    pub kernel_nanos_stats: Option<Stats>,
}

impl ScaleOutcome {
    /// `false` (not vacuously `true`) when there are zero successful
    /// repetitions -- a scale where every repetition failed must never
    /// read as "the needed set matched."
    pub fn needed_set_matches_in_every_successful_rep(&self) -> bool {
        let mut successful = self.repetitions.iter().filter(|r| r.success);
        let any = successful.next().is_some();
        any && self
            .repetitions
            .iter()
            .filter(|r| r.success)
            .all(|r| r.needed_set_matches == Some(true))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct M8Report {
    pub schema_version: String,
    pub case_id: String,
    pub runs_root: std::path::PathBuf,
    pub scales: Vec<ScaleOutcome>,
}

/// Rebuilds every scale's `ScenarioReport` (round-trip time) purely from
/// the `Run` files already written under `report.runs_root` -- no
/// re-planning. `kernel_nanos_stats` is likewise re-derived from each
/// repetition's already-recorded `kernel_nanos`, not from a fresh
/// subprocess call.
pub fn regenerate(mut report: M8Report) -> Result<M8Report, String> {
    for scale in &mut report.scales {
        let scenario_id = format!("unused-actions-{}", scale.unused_actions);
        let run_ids: Vec<String> = scale.repetitions.iter().map(|r| r.run_id.clone()).collect();
        match regenerate_report_from_disk(&report.runs_root, &scenario_id, &run_ids) {
            Ok(scenario_report) => {
                scale.scenario_report = Some(scenario_report);
                scale.scenario_report_error = None;
            }
            Err(e) => {
                scale.scenario_report = None;
                scale.scenario_report_error = Some(e.to_string());
            }
        }
        let kernel_samples: Vec<f64> = scale
            .repetitions
            .iter()
            .filter(|r| r.success)
            .filter_map(|r| r.kernel_nanos)
            .map(|n| n as f64)
            .collect();
        scale.kernel_nanos_stats = if kernel_samples.is_empty() {
            None
        } else {
            Some(Stats::from_samples(kernel_samples))
        };
    }
    Ok(report)
}

pub fn run(
    unused_action_scales: &[usize],
    warmup: usize,
    repetitions: usize,
) -> Result<M8Report, String> {
    let repo_root = crate::planner_binary::repo_root();
    let runs_root = repo_root.join("runs");
    let environment_fingerprint =
        laminaria_fingerprint::env::detect(&repo_root, "laminaria-m8-owned-baseline", "0.1.0");
    let planner_bin = resolve_or_build_planner_binary()?;

    let mut scales = Vec::new();

    for &unused_actions in unused_action_scales {
        let input = build_planning_input(unused_actions);

        // Warmup: a failure here aborts the whole run (a setup/
        // environment problem), unlike a measured repetition's failure
        // below, which is recorded, not fatal.
        for _ in 0..warmup {
            let (outcome, _kernel_nanos) = call_planner_with_kernel_timing(&planner_bin, &input)?;
            if !matches!(outcome, PlanOutcome::Planned(_)) {
                return Err(format!(
                    "warmup planner call for unused_actions={unused_actions} was rejected: \
                     {outcome:?}"
                ));
            }
        }

        let mut repetition_records = Vec::new();
        for _ in 0..repetitions {
            let clock = RunClock::start();
            let start_elapsed_ns = clock.elapsed_ns();
            let call_result = call_planner_with_kernel_timing(&planner_bin, &input);
            let end_elapsed_ns = clock.elapsed_ns();
            let run_ended_at_unix_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();

            let (success, failure_reason, needed_set_matches, ordered_action_count, kernel_nanos) =
                match call_result {
                    Ok((PlanOutcome::Planned(plan), kernel_nanos)) => {
                        let mut got: Vec<&str> =
                            plan.ordered_actions.iter().map(|s| s.as_str()).collect();
                        got.sort_unstable();
                        let mut expected = EXPECTED_NEEDED_SET.to_vec();
                        expected.sort_unstable();
                        let matches = got == expected;
                        (
                            true,
                            None,
                            Some(matches),
                            Some(plan.ordered_actions.len()),
                            kernel_nanos,
                        )
                    }
                    Ok((PlanOutcome::Rejected(r), _kernel_nanos)) => (
                        false,
                        Some(format!("expected a plan, got a rejection: {r:?}")),
                        None,
                        None,
                        None,
                    ),
                    Err(e) => (false, Some(e), None, None, None),
                };

            let run_id = laminaria_run::generate_run_id();
            let run = Run {
                run_id: run_id.clone(),
                schema_version: laminaria_run::types::SCHEMA_VERSION.to_string(),
                workload_id: WORKLOAD_ID.to_string(),
                scenario_id: format!("unused-actions-{unused_actions}"),
                requested_artifact: Some("fixture-bin-out".to_string()),
                environment_fingerprint: environment_fingerprint.clone(),
                requested_toolchain_selector: None,
                resolved_toolchain_fingerprint: None,
                preparation_record: PreparationRecord::default(),
                cache_state: CacheState::default(),
                root_command: RootCommand {
                    program: planner_bin.display().to_string(),
                    args: vec![],
                    cwd: None,
                    env_overrides: Default::default(),
                },
                run_started_at_unix_ns: clock.anchor_unix_ns(),
                run_ended_at_unix_ns: Some(run_ended_at_unix_ns),
                result: Some(RunResult {
                    success,
                    root_exit_status: ExitStatusRecord {
                        success,
                        code: None,
                        signal: None,
                    },
                }),
                process_trace: ProcessTrace {
                    processes: vec![ProcessRecord {
                        pid: None,
                        parent_pid: None,
                        executable: Some(planner_bin.clone()),
                        argv: vec![],
                        cwd: None,
                        start_elapsed_ns,
                        end_elapsed_ns: Some(end_elapsed_ns),
                        exit_status: Some(ExitStatusRecord {
                            success,
                            code: None,
                            signal: None,
                        }),
                        resource_usage: ResourceUsage {
                            unsupported_fields: [
                                "user_cpu_seconds",
                                "system_cpu_seconds",
                                "peak_rss_bytes",
                                "block_input_ops",
                                "block_output_ops",
                                "minor_faults",
                                "major_faults",
                                "voluntary_context_switches",
                                "involuntary_context_switches",
                            ]
                            .into_iter()
                            .map(String::from)
                            .collect(),
                            ..Default::default()
                        },
                        probe_level: ProbeLevel::Level0Lifecycle,
                        coverage_note: "real laminaria-planner subprocess, spawned directly (not \
                                        via laminaria_plan::call_planner, to also capture its \
                                        stderr kernel_nanos line); pid/resource_usage not \
                                        captured, only wall-clock timing"
                            .to_string(),
                    }],
                    known_gaps: vec![
                        "no wait4-derived resource_usage for the planner subprocess, only \
                         wall-clock timing"
                            .to_string(),
                    ],
                },
                compiler_telemetry: None,
                artifact_delta: None,
                measurement_overhead: None,
            };
            write_run(&runs_root, &run)
                .map_err(|e| format!("failed to write run {run_id}: {e}"))?;

            repetition_records.push(RepetitionRecord {
                run_id,
                success,
                failure_reason,
                needed_set_matches,
                ordered_action_count,
                kernel_nanos,
            });
        }

        let scenario_id = format!("unused-actions-{unused_actions}");
        let run_ids: Vec<String> = repetition_records
            .iter()
            .map(|r| r.run_id.clone())
            .collect();
        let (scenario_report, scenario_report_error) =
            match regenerate_report_from_disk(&runs_root, &scenario_id, &run_ids) {
                Ok(report) => (Some(report), None),
                Err(e) => (None, Some(e.to_string())),
            };
        let kernel_samples: Vec<f64> = repetition_records
            .iter()
            .filter(|r| r.success)
            .filter_map(|r| r.kernel_nanos)
            .map(|n| n as f64)
            .collect();
        let kernel_nanos_stats = if kernel_samples.is_empty() {
            None
        } else {
            Some(Stats::from_samples(kernel_samples))
        };

        scales.push(ScaleOutcome {
            unused_actions,
            warmup_runs: warmup,
            repetitions: repetition_records,
            scenario_report,
            scenario_report_error,
            kernel_nanos_stats,
        });
    }

    Ok(M8Report {
        schema_version: "0.2.0".to_string(),
        case_id: "M8-many-unrequested-nim-planner".to_string(),
        runs_root,
        scales,
    })
}

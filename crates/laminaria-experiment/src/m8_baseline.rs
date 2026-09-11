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
//! **Revised across two review rounds** (issue #28 D1-a):
//! - Round 1 (R2-R4): a failed repetition is recorded (never dropped by
//!   an early `?`); every repetition is persisted as a genuine
//!   `laminaria_run::types::Run` (`store::write_run`), so the round-trip
//!   wall time is a real `ScenarioReport`; and
//!   `nim-planner/src/laminaria_planner.nim` reports its own internal
//!   `planFromJson`-only wall time on stderr as `kernel_nanos`.
//! - Round 2: (a) the judgment data (`kernel_nanos`, `needed_set_matches`)
//!   lived only in the in-memory/serialized report, not in the
//!   persisted `Run`s, so `regenerate` couldn't rebuild the pass/fail
//!   verdict from disk alone -- fixed by serializing it into each
//!   `Run.compiler_telemetry` and having `regenerate` read it back,
//!   discarding the input report's own claims; (b) `kernel_nanos`'s
//!   *interval* was still `planFromJson` as a whole (schema gate +
//!   decode + `plan`), wider than the case's own confirmed
//!   `measurement_boundary` -- `laminaria_planner.nim` now times only
//!   `plan` itself, via `planning_kernel.decodePlanningInputOrReject`
//!   run un-timed beforehand; (c) an owned-code identity (git commit +
//!   dirty flag, plus the actual planner binary's own content SHA-256 --
//!   never an external Nim *compiler*'s identity) is now recorded per
//!   repetition and required before a cross-scale comparison is trusted.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use laminaria_plan::{Action, ActionKind, ArtifactRef, PlanOutcome, PlanningInput};
use laminaria_run::clock::RunClock;
use laminaria_run::scenario::{regenerate_report_from_disk, ScenarioReport, Stats};
use laminaria_run::store::{read_run, write_run};
use laminaria_run::types::{
    CacheState, ExitStatusRecord, PreparationRecord, ProbeLevel, ProcessRecord, ProcessTrace,
    ResourceUsage, RootCommand, Run, RunResult,
};
use serde::{Deserialize, Serialize};

use crate::owned_identity::{
    content_sha256, current_exe_sha256, identities_comparable, OwnedIdentity,
};
use crate::planner_binary::resolve_or_build_planner_binary;

pub const WORKLOAD_ID: &str = "M8-many-unrequested-nim-planner@issue35-d0-accepted-c812d70-v1";

const TELEMETRY_KIND: &str = "m8-owned-baseline-v1";
const EXPECTED_CASE_ID: &str = "M8-many-unrequested-nim-planner";
const EXPECTED_SCHEMA_VERSION: &str = "0.3.0";
/// The exact `unused_actions` scale set this case's `pass_criteria.d1`
/// requires -- no missing, duplicate, or extra scale group is ever a
/// valid baseline (a review round's own reproduction: renaming one
/// scale's `unused_actions` from `4` to `30` must be rejected, not
/// silently treated as a second `30` group).
const REQUIRED_SCALES: [usize; 3] = [4, 10, 30];

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

/// What's actually persisted into each repetition's `Run.compiler_telemetry`
/// -- the single source of truth `regenerate` reads back, never trusted
/// from an in-memory/serialized `M8Report` alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct M8Telemetry {
    kind: String,
    unused_actions: usize,
    success: bool,
    failure_reason: Option<String>,
    needed_set_matches: Option<bool>,
    ordered_action_count: Option<usize>,
    kernel_nanos: Option<u64>,
    owned_identity: OwnedIdentity,
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
    /// The Nim kernel's own self-reported `plan`-only wall time (issue
    /// #28 D1-a review round 2: previously `planFromJson` as a whole,
    /// which also covers the schema gate and JSON decode -- narrowed to
    /// match the case's own confirmed `measurement_boundary`). `None` if
    /// the planner failed before reporting it, or rejected the input at
    /// the schema gate (never reaching `plan` at all).
    pub kernel_nanos: Option<u64>,
    pub owned_identity: OwnedIdentity,
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

    /// `false` unless every successful repetition actually recorded a
    /// `kernel_nanos` value -- a review round reproduced this exact gap
    /// directly: dropping `kernel_nanos` from every repetition still
    /// left the overall pass criteria reading `true`.
    pub fn kernel_nanos_present_in_every_successful_rep(&self) -> bool {
        let mut successful = self.repetitions.iter().filter(|r| r.success);
        let any = successful.next().is_some();
        any && self
            .repetitions
            .iter()
            .filter(|r| r.success)
            .all(|r| r.kernel_nanos.is_some())
    }

    pub fn owned_identity_if_consistent(&self) -> Option<&OwnedIdentity> {
        let mut identities = self
            .repetitions
            .iter()
            .filter(|r| r.success)
            .map(|r| &r.owned_identity);
        let first = identities.next()?;
        if identities.all(|i| i == first) {
            Some(first)
        } else {
            None
        }
    }

    /// A complete, ready-for-comparison scale: it produced a
    /// `ScenarioReport`, every successful repetition's needed-set and
    /// kernel timing were present, and its owned identity was resolved
    /// and internally consistent.
    pub fn is_a_valid_baseline(&self) -> bool {
        !self.repetitions.is_empty()
            && self.scenario_report.is_some()
            && self.needed_set_matches_in_every_successful_rep()
            && self.kernel_nanos_present_in_every_successful_rep()
            && self.owned_identity_if_consistent().is_some_and(|id| {
                id.repo_commit.is_some()
                    && id.repo_dirty == Some(false)
                    && id.measurement_executable_sha256.is_some()
                    && id.planner_binary_sha256.is_some()
            })
    }
}

/// `Ok(())` only if `scales` is exactly the required `{4, 10, 30}` set --
/// no scale missing, none duplicated, none extra. Checked structurally,
/// before any per-scale validity or identity comparison, so a tampered
/// group label (e.g. `unused_actions` changed from `4` to `30`) is
/// rejected for exactly that reason.
fn scale_set_matches_required(scales: &[ScaleOutcome]) -> Result<(), String> {
    let mut found: Vec<usize> = scales.iter().map(|s| s.unused_actions).collect();
    found.sort_unstable();
    let mut required = REQUIRED_SCALES.to_vec();
    required.sort_unstable();
    if found != required {
        return Err(format!(
            "scale set {found:?} does not exactly match the required {{4, 10, 30}} set -- \
             missing, duplicate, or extra unused_actions groups are never a valid baseline"
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct M8Report {
    pub schema_version: String,
    pub case_id: String,
    pub runs_root: PathBuf,
    pub scales: Vec<ScaleOutcome>,
}

impl M8Report {
    /// `Ok(())` only if every scale is `is_a_valid_baseline()` and every
    /// pair of scales' owned identities are `identities_comparable`
    /// (same clean repo commit *and* the identical planner binary
    /// content) -- the gate a genuine cross-scale comparison must pass.
    pub fn owned_baseline_comparable_across_scales(&self) -> Result<(), String> {
        scale_set_matches_required(&self.scales)?;
        for scale in &self.scales {
            if !scale.is_a_valid_baseline() {
                return Err(format!(
                    "unused_actions={}: not a valid baseline (scenario_report_error={:?}, \
                     needed_set_ok={}, kernel_nanos_present={}, identity_consistent={})",
                    scale.unused_actions,
                    scale.scenario_report_error,
                    scale.needed_set_matches_in_every_successful_rep(),
                    scale.kernel_nanos_present_in_every_successful_rep(),
                    scale.owned_identity_if_consistent().is_some()
                ));
            }
        }
        for pair in self.scales.windows(2) {
            let a = pair[0]
                .owned_identity_if_consistent()
                .expect("already checked is_a_valid_baseline above");
            let b = pair[1]
                .owned_identity_if_consistent()
                .expect("already checked is_a_valid_baseline above");
            identities_comparable(a, b).map_err(|e| {
                format!(
                    "unused_actions={} and unused_actions={} not comparable: {e}",
                    pair[0].unused_actions, pair[1].unused_actions
                )
            })?;
        }
        Ok(())
    }
}

/// Rebuilds an `M8Report` purely from the `Run` files already written
/// under `report.runs_root` (using only `report`'s own `run_id`s as
/// pointers) -- every `RepetitionRecord` field is re-read from each
/// `Run`'s own `compiler_telemetry`, not trusted from the input report's
/// own claims, and `scenario_report`/`kernel_nanos_stats` are rebuilt
/// from that freshly-read data.
pub fn regenerate(report: M8Report) -> Result<M8Report, String> {
    if report.case_id != EXPECTED_CASE_ID {
        return Err(format!(
            "case_id {:?} is not {EXPECTED_CASE_ID:?} -- refusing to regenerate a report for a \
             different case",
            report.case_id
        ));
    }
    if report.schema_version != EXPECTED_SCHEMA_VERSION {
        return Err(format!(
            "schema_version {:?} is not {EXPECTED_SCHEMA_VERSION:?} -- refusing to regenerate an \
             unsupported schema",
            report.schema_version
        ));
    }

    let mut seen_run_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut scales = Vec::new();
    for scale in &report.scales {
        let expected_scenario_id = format!("unused-actions-{}", scale.unused_actions);
        let mut repetitions = Vec::new();
        for old in &scale.repetitions {
            if !seen_run_ids.insert(old.run_id.clone()) {
                return Err(format!(
                    "run_id {} is referenced more than once across scales/repetitions -- a \
                     genuine measurement never reuses a run_id",
                    old.run_id
                ));
            }
            let run = read_run(&report.runs_root, &old.run_id)
                .map_err(|e| format!("failed to read run {}: {e}", old.run_id))?;
            if run.workload_id != WORKLOAD_ID {
                return Err(format!(
                    "run {}: workload_id {:?} is not {WORKLOAD_ID:?}",
                    old.run_id, run.workload_id
                ));
            }
            if run.scenario_id != expected_scenario_id {
                return Err(format!(
                    "run {}: scenario_id {:?} does not match its outer group unused_actions={} \
                     (expected {expected_scenario_id:?}) -- a run belonging to one group must \
                     never be attributed to another",
                    old.run_id, run.scenario_id, scale.unused_actions
                ));
            }
            let telemetry_value = run.compiler_telemetry.clone().ok_or_else(|| {
                format!(
                    "run {} has no compiler_telemetry -- cannot regenerate its judgment from disk",
                    old.run_id
                )
            })?;
            let telemetry: M8Telemetry = serde_json::from_value(telemetry_value)
                .map_err(|e| format!("run {}: malformed compiler_telemetry: {e}", old.run_id))?;
            if telemetry.kind != TELEMETRY_KIND {
                return Err(format!(
                    "run {}: compiler_telemetry.kind {:?} is not {TELEMETRY_KIND:?}",
                    old.run_id, telemetry.kind
                ));
            }
            if telemetry.unused_actions != scale.unused_actions {
                return Err(format!(
                    "run {}: telemetry.unused_actions={} does not match its outer group \
                     unused_actions={} -- a repetition must never be attributed to a different \
                     group than the one it actually measured",
                    old.run_id, telemetry.unused_actions, scale.unused_actions
                ));
            }
            let run_result_success = run.result.as_ref().map(|r| r.success);
            if run_result_success != Some(telemetry.success) {
                return Err(format!(
                    "run {}: telemetry.success={} does not match Run.result.success={:?}",
                    old.run_id, telemetry.success, run_result_success
                ));
            }
            repetitions.push(RepetitionRecord {
                run_id: old.run_id.clone(),
                success: telemetry.success,
                failure_reason: telemetry.failure_reason,
                needed_set_matches: telemetry.needed_set_matches,
                ordered_action_count: telemetry.ordered_action_count,
                kernel_nanos: telemetry.kernel_nanos,
                owned_identity: telemetry.owned_identity,
            });
        }

        let scenario_id = format!("unused-actions-{}", scale.unused_actions);
        let run_ids: Vec<String> = repetitions.iter().map(|r| r.run_id.clone()).collect();
        let (scenario_report, scenario_report_error) =
            match regenerate_report_from_disk(&report.runs_root, &scenario_id, &run_ids) {
                Ok(sr) => (Some(sr), None),
                Err(e) => (None, Some(e.to_string())),
            };
        let kernel_samples: Vec<f64> = repetitions
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
            unused_actions: scale.unused_actions,
            warmup_runs: scale.warmup_runs,
            repetitions,
            scenario_report,
            scenario_report_error,
            kernel_nanos_stats,
        });
    }

    Ok(M8Report {
        schema_version: report.schema_version,
        case_id: report.case_id,
        runs_root: report.runs_root,
        scales,
    })
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
    let planner_binary_sha256 = content_sha256(&planner_bin)?;
    let measurement_executable_sha256 = current_exe_sha256()?;
    let owned_identity = OwnedIdentity::from_environment_fingerprint(
        &environment_fingerprint,
        Some(measurement_executable_sha256),
        Some(planner_binary_sha256),
    );

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

            let telemetry = M8Telemetry {
                kind: TELEMETRY_KIND.to_string(),
                unused_actions,
                success,
                failure_reason: failure_reason.clone(),
                needed_set_matches,
                ordered_action_count,
                kernel_nanos,
                owned_identity: owned_identity.clone(),
            };

            let run_id = crate::unique_run_id();
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
                compiler_telemetry: Some(
                    serde_json::to_value(&telemetry).expect("telemetry always serializes"),
                ),
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
                owned_identity: owned_identity.clone(),
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
        schema_version: EXPECTED_SCHEMA_VERSION.to_string(),
        case_id: EXPECTED_CASE_ID.to_string(),
        runs_root,
        scales,
    })
}

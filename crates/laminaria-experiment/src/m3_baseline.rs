//! Issue #28 D1-a: `M3-owned-independent-chains` owned baseline.
//!
//! Reuses the exact chain construction
//! `crates/laminaria-run/src/compiler_work_executor.rs`'s own
//! `independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two`
//! test exercises (`independent_rust_and_nim_chains_planning_input`,
//! shared between that test and this module -- not a parallel
//! reimplementation) and its opt-in-traced production entry point
//! (`run_compiler_work_plan_with_concurrency_trace`), per
//! `docs/design/issue-35-d0-cases.yaml`'s `M3-owned-independent-chains`
//! case: real Nim planner -> Rust plan validation -> the same owned
//! executor, at CPU budget 1 and 2, a fresh `ArtifactStore` and
//! concurrency counter per repetition, warmup discarded before the
//! measured repetitions.
//!
//! **Revised across two review rounds** (issue #28 D1-a):
//! - Round 1 (R1-R3): a failed repetition is recorded (never dropped by
//!   an early `?`), evidence is compared *across* CPU budgets (not only
//!   within one budget's own repetitions), and every repetition is
//!   persisted as a genuine `laminaria_run::types::Run` (`store::write_run`),
//!   so the per-budget aggregate is a real `ScenarioReport` built by
//!   `regenerate_report_from_disk`.
//! - Round 2: the judgment data itself (`peak_concurrency`,
//!   `evidence_digest`, success/failure) lived only in the in-memory/
//!   serialized `M3Report`, not in the persisted `Run`s -- so
//!   `regenerate` could rebuild `wall_seconds` from disk but not the
//!   pass/fail verdict, and a stale or hand-edited report JSON (with the
//!   `Run` files deleted) could still "pass." Fixed: each repetition's
//!   judgment is now serialized into its own `Run.compiler_telemetry`
//!   (the schema's own designated adapter-specific extension point), and
//!   `regenerate` rebuilds every `RepetitionRecord` field by reading
//!   that back from disk, discarding whatever the input report claimed.
//!   Also added: an owned-code identity (`crate::owned_identity`, git
//!   commit + dirty flag, never an external rustc/Nim toolchain digest)
//!   recorded per repetition and required to match, cleanly, across both
//!   budgets before a comparison is treated as valid.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use laminaria_plan::PlanOutcome;
use laminaria_run::clock::RunClock;
use laminaria_run::compiler_work_executor::{
    independent_rust_and_nim_chains_planning_input, run_compiler_work_plan_with_concurrency_trace,
    ArtifactStore,
};
use laminaria_run::scenario::{regenerate_report_from_disk, ScenarioReport};
use laminaria_run::store::{read_run, write_run};
use laminaria_run::types::{
    CacheState, ExitStatusRecord, PreparationRecord, ProbeLevel, ProcessRecord, ProcessTrace,
    ResourceUsage, RootCommand, Run, RunResult,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::owned_identity::{identities_comparable, OwnedIdentity};
use crate::planner_binary::resolve_or_build_planner_binary;

/// Identical to the existing concurrency test's own fixture source --
/// this measurement is the *same* six-action chain shape
/// (`LowerSource -> ValidateIr -> EvaluateEvidence` per language,
/// no `TransformFunction`), not a new workload.
pub const RUST_SOURCE_TEXT: &str = "fn f(x: i32) -> i32 { x }";
pub const NIM_SOURCE_TEXT: &str = "proc g(x: int32): int32 =\n  x\n";

/// Names the case + accepted spec revision this measurement is evidence
/// for (`docs/design/issue-35-d0-spec.md` section 10).
pub const WORKLOAD_ID: &str = "M3-owned-independent-chains@issue35-d0-accepted-c812d70-v1";

const TELEMETRY_KIND: &str = "m3-owned-baseline-v1";

fn unsupported_resource_fields() -> Vec<String> {
    [
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
    .collect()
}

/// A stable digest over one repetition's evidence -- `EvalOutcome`/
/// `CallEvent` (`laminaria-ir`) don't implement `Serialize` (that crate
/// stays free of a serde dependency on purpose), so the report stores a
/// SHA-256 of their `Debug` rendering rather than the structured values
/// themselves. Two digests differing means the underlying evidence
/// (`Vec<EvalOutcome>`) differed; two digests matching is exactly as
/// strong a claim as the direct `==` comparison the source test itself
/// uses, since `Debug`'s output for these types is a lossless rendering
/// of every field.
fn digest_evidence<T: std::fmt::Debug>(rust_evidence: &[T], nim_evidence: &[T]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{rust_evidence:?}").as_bytes());
    hasher.update(b"|");
    hasher.update(format!("{nim_evidence:?}").as_bytes());
    format!("{:x}", hasher.finalize())
}

/// What's actually persisted into each repetition's `Run.compiler_telemetry`
/// -- the single source of truth `regenerate` reads back, never trusted
/// from an in-memory/serialized `M3Report` alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct M3Telemetry {
    kind: String,
    cpu_budget: usize,
    success: bool,
    failure_reason: Option<String>,
    peak_concurrency: Option<usize>,
    evidence_digest: Option<String>,
    owned_identity: OwnedIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepetitionRecord {
    pub run_id: String,
    pub success: bool,
    /// `None` on success. Set (and `peak_concurrency`/`evidence_digest`
    /// left `None`) on a genuine dispatch failure -- the repetition is
    /// still recorded, never silently dropped.
    pub failure_reason: Option<String>,
    pub peak_concurrency: Option<usize>,
    pub evidence_digest: Option<String>,
    pub owned_identity: OwnedIdentity,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BudgetOutcome {
    pub cpu_budget: usize,
    pub warmup_runs: usize,
    /// One entry per measured repetition, success or failure -- never
    /// filtered before being recorded.
    pub repetitions: Vec<RepetitionRecord>,
    /// `None` only if every repetition in this budget failed (`build_report`'s
    /// own "no valid wall-time sample" rejection) -- see
    /// `scenario_report_error` for why.
    pub scenario_report: Option<ScenarioReport>,
    pub scenario_report_error: Option<String>,
}

impl BudgetOutcome {
    pub fn successful_peak_concurrency_values(&self) -> Vec<usize> {
        self.repetitions
            .iter()
            .filter(|r| r.success)
            .filter_map(|r| r.peak_concurrency)
            .collect()
    }

    /// `Some(digest)` only if every successful repetition in this budget
    /// produced the identical evidence digest; `None` if there were zero
    /// successful repetitions or they disagreed with each other.
    pub fn evidence_digest_if_consistent(&self) -> Option<&str> {
        let mut digests = self
            .repetitions
            .iter()
            .filter(|r| r.success)
            .filter_map(|r| r.evidence_digest.as_deref());
        let first = digests.next()?;
        if digests.all(|d| d == first) {
            Some(first)
        } else {
            None
        }
    }

    /// `Some(identity)` only if every successful repetition recorded the
    /// identical owned identity; `None` if there were zero successful
    /// repetitions or they disagreed (e.g. the tree was edited mid-run).
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

    /// A complete, ready-for-comparison budget: it produced a
    /// `ScenarioReport` (no repetition failed, environment/toolchain
    /// consistent per that function's own checks), its evidence was
    /// internally consistent, and its owned identity was resolved and
    /// internally consistent. All three must hold for this budget's
    /// numbers to mean anything.
    pub fn is_a_valid_baseline(&self) -> bool {
        !self.repetitions.is_empty()
            && self.scenario_report.is_some()
            && self.evidence_digest_if_consistent().is_some()
            && self
                .owned_identity_if_consistent()
                .is_some_and(|id| id.repo_commit.is_some() && id.repo_dirty == Some(false))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct M3Report {
    pub schema_version: String,
    pub case_id: String,
    pub plan_action_count: usize,
    /// Where each repetition's `Run` was written -- `regenerate` reads
    /// back from here.
    pub runs_root: PathBuf,
    pub budgets: Vec<BudgetOutcome>,
}

impl M3Report {
    /// The actual cross-budget comparison R1's review demanded: every
    /// budget must have a *consistent* evidence digest, and every
    /// budget's digest must equal every other budget's -- not merely
    /// "each budget agrees with itself."
    pub fn evidence_matches_across_budgets(&self) -> bool {
        let digests: Vec<&str> = self
            .budgets
            .iter()
            .filter_map(|b| b.evidence_digest_if_consistent())
            .collect();
        !digests.is_empty()
            && digests.len() == self.budgets.len()
            && digests.windows(2).all(|w| w[0] == w[1])
    }

    /// `Ok(())` only if every budget is `is_a_valid_baseline()` and every
    /// pair of budgets' owned identities are `identities_comparable`
    /// (same, clean repo commit) -- the gate a genuine budget-1-vs-
    /// budget-2 comparison must pass before its numbers are trusted.
    pub fn owned_baseline_comparable_across_budgets(&self) -> Result<(), String> {
        for budget in &self.budgets {
            if !budget.is_a_valid_baseline() {
                return Err(format!(
                    "budget {}: not a valid baseline (scenario_report={:?}, evidence_consistent={}, \
                     identity_consistent={})",
                    budget.cpu_budget,
                    budget.scenario_report_error,
                    budget.evidence_digest_if_consistent().is_some(),
                    budget.owned_identity_if_consistent().is_some()
                ));
            }
        }
        for pair in self.budgets.windows(2) {
            let a = pair[0]
                .owned_identity_if_consistent()
                .expect("already checked is_a_valid_baseline above");
            let b = pair[1]
                .owned_identity_if_consistent()
                .expect("already checked is_a_valid_baseline above");
            identities_comparable(a, b).map_err(|e| {
                format!(
                    "budgets {} and {} not comparable: {e}",
                    pair[0].cpu_budget, pair[1].cpu_budget
                )
            })?;
        }
        Ok(())
    }
}

/// Rebuilds an `M3Report` purely from the `Run` files already written
/// under `report.runs_root` (using only `report`'s own `run_id`s as
/// pointers) -- every `RepetitionRecord` field (success, peak_concurrency,
/// evidence_digest, owned_identity) is re-read from each `Run`'s own
/// `compiler_telemetry`, not trusted from the input report's own claims,
/// and `scenario_report` is rebuilt by `regenerate_report_from_disk`.
/// Returns `Err` if a referenced `run_id` (or its `compiler_telemetry`)
/// no longer exists (e.g. the caller cleaned up `runs/` in between) --
/// this must never silently fall back to the input report's own stale
/// data.
pub fn regenerate(report: M3Report) -> Result<M3Report, String> {
    let mut budgets = Vec::new();
    for budget in &report.budgets {
        let mut repetitions = Vec::new();
        for old in &budget.repetitions {
            let run = read_run(&report.runs_root, &old.run_id)
                .map_err(|e| format!("failed to read run {}: {e}", old.run_id))?;
            let telemetry_value = run.compiler_telemetry.ok_or_else(|| {
                format!(
                    "run {} has no compiler_telemetry -- cannot regenerate its judgment from disk",
                    old.run_id
                )
            })?;
            let telemetry: M3Telemetry = serde_json::from_value(telemetry_value)
                .map_err(|e| format!("run {}: malformed compiler_telemetry: {e}", old.run_id))?;
            if telemetry.kind != TELEMETRY_KIND {
                return Err(format!(
                    "run {}: compiler_telemetry.kind {:?} is not {TELEMETRY_KIND:?}",
                    old.run_id, telemetry.kind
                ));
            }
            repetitions.push(RepetitionRecord {
                run_id: old.run_id.clone(),
                success: telemetry.success,
                failure_reason: telemetry.failure_reason,
                peak_concurrency: telemetry.peak_concurrency,
                evidence_digest: telemetry.evidence_digest,
                owned_identity: telemetry.owned_identity,
            });
        }

        let scenario_id = format!("budget-{}", budget.cpu_budget);
        let run_ids: Vec<String> = repetitions.iter().map(|r| r.run_id.clone()).collect();
        let (scenario_report, scenario_report_error) =
            match regenerate_report_from_disk(&report.runs_root, &scenario_id, &run_ids) {
                Ok(sr) => (Some(sr), None),
                Err(e) => (None, Some(e.to_string())),
            };

        budgets.push(BudgetOutcome {
            cpu_budget: budget.cpu_budget,
            warmup_runs: budget.warmup_runs,
            repetitions,
            scenario_report,
            scenario_report_error,
        });
    }

    Ok(M3Report {
        schema_version: report.schema_version,
        case_id: report.case_id,
        plan_action_count: report.plan_action_count,
        runs_root: report.runs_root,
        budgets,
    })
}

pub fn run(warmup: usize, repetitions: usize) -> Result<M3Report, String> {
    let repo_root = crate::planner_binary::repo_root();
    let runs_root = repo_root.join("runs");
    let environment_fingerprint =
        laminaria_fingerprint::env::detect(&repo_root, "laminaria-m3-owned-baseline", "0.1.0");
    // M3 dispatches already-compiled owned Rust code in-process -- there
    // is no single separately-built artifact file to hash (unlike M8's
    // `laminaria-planner` binary), so `artifact_content_sha256` is
    // honestly `None`; the repo commit is the identity that matters here.
    let owned_identity =
        OwnedIdentity::from_environment_fingerprint(&environment_fingerprint, None);

    // `process::id()` alone collides when this crate's own tests call
    // `run()` concurrently from multiple threads of the same test
    // binary (a real race this session's own test run caught: one
    // call's cleanup `remove_dir_all` raced another's still-in-progress
    // dispatch reading the same `g.nim`) -- an added monotonic counter
    // makes every call's directory unique regardless of caller
    // concurrency, not just a test-only workaround.
    use std::sync::atomic::{AtomicU64, Ordering};
    static CALL_COUNTER: AtomicU64 = AtomicU64::new(0);
    let call_id = CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "laminaria-experiment-m3-owned-baseline-{}-{}",
        std::process::id(),
        call_id
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let rust_path = dir.join("f.rs");
    std::fs::write(&rust_path, RUST_SOURCE_TEXT).map_err(|e| e.to_string())?;
    let nim_path = dir.join("g.nim");
    std::fs::write(&nim_path, NIM_SOURCE_TEXT).map_err(|e| e.to_string())?;

    // Many test inputs per chain -- enough real interpreter work that
    // budget 2's concurrency is genuinely observable, same rationale as
    // the source test this reuses.
    let test_inputs: Vec<Vec<i64>> = (0..60_000i64).map(|n| vec![n]).collect();

    let (input, rust_evidence_id, nim_evidence_id) = independent_rust_and_nim_chains_planning_input(
        rust_path.to_str().expect("temp path is valid UTF-8"),
        RUST_SOURCE_TEXT,
        nim_path.to_str().expect("temp path is valid UTF-8"),
        NIM_SOURCE_TEXT,
        &test_inputs,
    );

    let planner_bin = resolve_or_build_planner_binary()?;
    let outcome = laminaria_plan::call_planner(&planner_bin, &input)
        .map_err(|e| format!("planner call failed: {e}"))?;
    let plan = match outcome {
        PlanOutcome::Planned(plan) => plan,
        PlanOutcome::Rejected(r) => return Err(format!("expected a plan, got a rejection: {r:?}")),
    };
    laminaria_plan::validate(&plan, &input)
        .map_err(|e| format!("plan failed validation: {e:?}"))?;
    if plan.actions.len() != 6 {
        std::fs::remove_dir_all(&dir).ok();
        return Err(format!(
            "expected 6 actions (LowerSource->ValidateIr->EvaluateEvidence x2 chains, no \
             TransformFunction), got {}",
            plan.actions.len()
        ));
    }

    let mut budgets = Vec::new();
    for &budget in &[1usize, 2usize] {
        let nz_budget = NonZeroUsize::new(budget).expect("budget is always >= 1");

        // Warmup: discarded, but still a real, fresh-store dispatch --
        // never a sleep-only stand-in. A warmup failure aborts the
        // whole run (a setup/environment problem), unlike a measured
        // repetition's failure below, which is recorded, not fatal.
        for _ in 0..warmup {
            let mut store = ArtifactStore::new();
            let (result, _peak) =
                run_compiler_work_plan_with_concurrency_trace(&plan, &mut store, nz_budget);
            result.map_err(|e| format!("warmup dispatch failed at budget {budget}: {e:?}"))?;
        }

        let mut repetition_records = Vec::new();
        for _ in 0..repetitions {
            let clock = RunClock::start();
            let mut store = ArtifactStore::new();
            let start_elapsed_ns = clock.elapsed_ns();
            let (result, peak) =
                run_compiler_work_plan_with_concurrency_trace(&plan, &mut store, nz_budget);
            let end_elapsed_ns = clock.elapsed_ns();
            let run_ended_at_unix_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();

            let (success, failure_reason, peak_concurrency, evidence_digest) = match result {
                Ok(()) => {
                    let rust_evidence = store
                        .evidence(&rust_evidence_id)
                        .expect("rust evidence must be present after a successful dispatch")
                        .to_vec();
                    let nim_evidence = store
                        .evidence(&nim_evidence_id)
                        .expect("nim evidence must be present after a successful dispatch")
                        .to_vec();
                    let digest = digest_evidence(&rust_evidence, &nim_evidence);
                    (true, None, Some(peak), Some(digest))
                }
                Err(e) => (false, Some(format!("{e:?}")), None, None),
            };

            let telemetry = M3Telemetry {
                kind: TELEMETRY_KIND.to_string(),
                cpu_budget: budget,
                success,
                failure_reason: failure_reason.clone(),
                peak_concurrency,
                evidence_digest: evidence_digest.clone(),
                owned_identity: owned_identity.clone(),
            };

            let run_id = crate::unique_run_id();
            let run = Run {
                run_id: run_id.clone(),
                schema_version: laminaria_run::types::SCHEMA_VERSION.to_string(),
                workload_id: WORKLOAD_ID.to_string(),
                scenario_id: format!("budget-{budget}"),
                requested_artifact: Some(format!(
                    "rust:{rust_evidence_id},nim:{nim_evidence_id}"
                )),
                environment_fingerprint: environment_fingerprint.clone(),
                requested_toolchain_selector: None,
                resolved_toolchain_fingerprint: None,
                preparation_record: PreparationRecord::default(),
                cache_state: CacheState::default(),
                root_command: RootCommand {
                    program:
                        "laminaria_run::compiler_work_executor::run_compiler_work_plan_with_concurrency_trace"
                            .to_string(),
                    args: vec![format!("cpu_budget={budget}")],
                    cwd: None,
                    env_overrides: BTreeMap::new(),
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
                        executable: None,
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
                            unsupported_fields: unsupported_resource_fields(),
                            ..Default::default()
                        },
                        probe_level: ProbeLevel::Level0Lifecycle,
                        coverage_note: "in-process compiler_work_executor dispatch -- no OS \
                                        subprocess was spawned for this measurement, only \
                                        wall-clock timing (via RunClock) is captured; pid and \
                                        resource_usage are honestly absent, not fabricated"
                            .to_string(),
                    }],
                    known_gaps: vec![
                        "no OS-level pid/resource_usage: this Run measures an in-process \
                         dispatch, not a spawned subprocess"
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
                peak_concurrency,
                evidence_digest,
                owned_identity: owned_identity.clone(),
            });
        }

        let scenario_id = format!("budget-{budget}");
        let run_ids: Vec<String> = repetition_records
            .iter()
            .map(|r| r.run_id.clone())
            .collect();
        let (scenario_report, scenario_report_error) =
            match regenerate_report_from_disk(&runs_root, &scenario_id, &run_ids) {
                Ok(report) => (Some(report), None),
                Err(e) => (None, Some(e.to_string())),
            };

        budgets.push(BudgetOutcome {
            cpu_budget: budget,
            warmup_runs: warmup,
            repetitions: repetition_records,
            scenario_report,
            scenario_report_error,
        });
    }

    std::fs::remove_dir_all(&dir).ok();

    Ok(M3Report {
        schema_version: "0.3.0".to_string(),
        case_id: "M3-owned-independent-chains".to_string(),
        plan_action_count: plan.actions.len(),
        runs_root,
        budgets,
    })
}

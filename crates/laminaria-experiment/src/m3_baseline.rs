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

use std::num::NonZeroUsize;
use std::time::Instant;

use laminaria_plan::PlanOutcome;
use laminaria_run::compiler_work_executor::{
    independent_rust_and_nim_chains_planning_input, run_compiler_work_plan_with_concurrency_trace,
    ArtifactStore,
};
use laminaria_run::scenario::Stats;
use serde::{Deserialize, Serialize};

use crate::planner_binary::resolve_or_build_planner_binary;

/// Identical to the existing concurrency test's own fixture source --
/// this measurement is the *same* six-action chain shape
/// (`LowerSource -> ValidateIr -> EvaluateEvidence` per language,
/// no `TransformFunction`), not a new workload.
pub const RUST_SOURCE_TEXT: &str = "fn f(x: i32) -> i32 { x }";
pub const NIM_SOURCE_TEXT: &str = "proc g(x: int32): int32 =\n  x\n";

#[derive(Debug, Serialize, Deserialize)]
pub struct BudgetSample {
    pub success: bool,
    pub wall_seconds: f64,
    pub peak_concurrency: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BudgetReport {
    pub cpu_budget: usize,
    pub warmup_runs: usize,
    /// Raw per-repetition samples -- kept alongside the `Stats` summary
    /// so the report can be regenerated (re-run `Stats::from_samples`
    /// over `samples`) without re-executing anything.
    pub samples: Vec<BudgetSample>,
    pub wall_seconds: Stats,
    pub peak_concurrency_values: Vec<usize>,
    /// True only if every repetition's Rust-chain evidence was
    /// bit-for-bit identical to every other -- scheduling must never
    /// change what a pure computation produces.
    pub rust_evidence_matches_across_all_reps: bool,
    pub nim_evidence_matches_across_all_reps: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct M3Report {
    pub schema_version: String,
    pub case_id: String,
    pub plan_action_count: usize,
    pub budgets: Vec<BudgetReport>,
}

/// Re-derives every `Stats` summary from a report's own already-recorded
/// raw `samples` -- the "reproducible without re-executing anything"
/// requirement, mirroring `laminaria_run::scenario::regenerate_report_from_disk`'s
/// own raw-data-in, summary-out shape.
pub fn regenerate(mut report: M3Report) -> M3Report {
    for budget in &mut report.budgets {
        budget.wall_seconds =
            Stats::from_samples(budget.samples.iter().map(|s| s.wall_seconds).collect());
        budget.peak_concurrency_values =
            budget.samples.iter().map(|s| s.peak_concurrency).collect();
    }
    report
}

pub fn run(warmup: usize, repetitions: usize) -> Result<M3Report, String> {
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
        // never a sleep-only stand-in.
        for _ in 0..warmup {
            let mut store = ArtifactStore::new();
            let (result, _peak) =
                run_compiler_work_plan_with_concurrency_trace(&plan, &mut store, nz_budget);
            result.map_err(|e| format!("warmup dispatch failed at budget {budget}: {e:?}"))?;
        }

        let mut samples = Vec::new();
        let mut rust_evidences = Vec::new();
        let mut nim_evidences = Vec::new();
        for _ in 0..repetitions {
            let mut store = ArtifactStore::new();
            let start = Instant::now();
            let (result, peak) =
                run_compiler_work_plan_with_concurrency_trace(&plan, &mut store, nz_budget);
            let wall_seconds = start.elapsed().as_secs_f64();
            let success = result.is_ok();
            result.map_err(|e| format!("dispatch failed at budget {budget}: {e:?}"))?;
            rust_evidences.push(
                store
                    .evidence(&rust_evidence_id)
                    .expect("rust evidence must be present after a successful dispatch")
                    .to_vec(),
            );
            nim_evidences.push(
                store
                    .evidence(&nim_evidence_id)
                    .expect("nim evidence must be present after a successful dispatch")
                    .to_vec(),
            );
            samples.push(BudgetSample {
                success,
                wall_seconds,
                peak_concurrency: peak,
            });
        }

        let rust_evidence_matches_across_all_reps = rust_evidences.windows(2).all(|w| w[0] == w[1]);
        let nim_evidence_matches_across_all_reps = nim_evidences.windows(2).all(|w| w[0] == w[1]);
        let wall_seconds = Stats::from_samples(samples.iter().map(|s| s.wall_seconds).collect());
        let peak_concurrency_values = samples.iter().map(|s| s.peak_concurrency).collect();

        budgets.push(BudgetReport {
            cpu_budget: budget,
            warmup_runs: warmup,
            samples,
            wall_seconds,
            peak_concurrency_values,
            rust_evidence_matches_across_all_reps,
            nim_evidence_matches_across_all_reps,
        });
    }

    std::fs::remove_dir_all(&dir).ok();

    Ok(M3Report {
        schema_version: "0.1.0".to_string(),
        case_id: "M3-owned-independent-chains".to_string(),
        plan_action_count: plan.actions.len(),
        budgets,
    })
}

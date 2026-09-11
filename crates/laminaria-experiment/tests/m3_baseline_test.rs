//! Issue #28 D1-a regression coverage for `M3-owned-independent-chains`:
//! runs the real owned baseline (real Nim planner, real
//! `compiler_work_executor` dispatch) and checks the exact
//! `docs/design/issue-35-d0-cases.yaml` `pass_criteria.d1` properties --
//! not merely that it runs without panicking. Requires `nim` on `PATH`
//! (builds `nim-planner/bin/laminaria-planner` if missing), same as
//! `laminaria-run`'s own real-binary tests.
#![cfg(unix)]

use laminaria_experiment::m3_baseline;

#[test]
fn budget_one_never_shows_more_than_one_concurrent_computation() {
    let report = m3_baseline::run(1, 5).expect("M3 owned baseline must succeed");
    let budget1 = report
        .budgets
        .iter()
        .find(|b| b.cpu_budget == 1)
        .expect("budget 1 report must be present");
    assert!(
        budget1.samples.iter().all(|s| s.success),
        "a failed repetition must never be silently treated as a success"
    );
    assert!(
        budget1.peak_concurrency_values.iter().all(|&p| p == 1),
        "budget 1 must never observe more than one real computation at a time, got: {:?}",
        budget1.peak_concurrency_values
    );
}

#[test]
fn budget_two_genuinely_overlaps_both_chains_every_repetition() {
    let report = m3_baseline::run(1, 5).expect("M3 owned baseline must succeed");
    let budget2 = report
        .budgets
        .iter()
        .find(|b| b.cpu_budget == 2)
        .expect("budget 2 report must be present");
    assert!(budget2.samples.iter().all(|s| s.success));
    assert!(
        budget2.peak_concurrency_values.iter().all(|&p| p == 2),
        "budget 2 must observe genuine two-way concurrency in every repetition, got: {:?}",
        budget2.peak_concurrency_values
    );
}

#[test]
fn results_are_identical_across_cpu_budgets_scheduling_never_changes_pure_computation() {
    let report = m3_baseline::run(1, 5).expect("M3 owned baseline must succeed");
    for budget in &report.budgets {
        assert!(
            budget.rust_evidence_matches_across_all_reps,
            "budget {}: rust chain evidence must be identical across repetitions",
            budget.cpu_budget
        );
        assert!(
            budget.nim_evidence_matches_across_all_reps,
            "budget {}: nim chain evidence must be identical across repetitions",
            budget.cpu_budget
        );
    }
}

#[test]
fn the_plan_is_exactly_six_actions_no_transform_function() {
    let report = m3_baseline::run(1, 1).expect("M3 owned baseline must succeed");
    assert_eq!(
        report.plan_action_count, 6,
        "LowerSource->ValidateIr->EvaluateEvidence per language x 2 chains, no TransformFunction"
    );
}

#[test]
fn a_report_is_reproducible_from_its_own_raw_samples_without_re_executing_anything() {
    let report = m3_baseline::run(1, 3).expect("M3 owned baseline must succeed");
    let json = serde_json::to_string(&report).expect("report always serializes");
    let round_tripped: m3_baseline::M3Report =
        serde_json::from_str(&json).expect("report always deserializes");
    let regenerated = m3_baseline::regenerate(round_tripped);

    for (original, again) in report.budgets.iter().zip(regenerated.budgets.iter()) {
        assert_eq!(original.wall_seconds.mean, again.wall_seconds.mean);
        assert_eq!(original.wall_seconds.stddev, again.wall_seconds.stddev);
        assert_eq!(
            original.peak_concurrency_values,
            again.peak_concurrency_values
        );
    }
}

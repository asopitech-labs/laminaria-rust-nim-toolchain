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
        budget1.repetitions.iter().all(|r| r.success),
        "a failed repetition must never be silently treated as a success: {:?}",
        budget1.repetitions
    );
    let peaks = budget1.successful_peak_concurrency_values();
    assert!(
        peaks.iter().all(|&p| p == 1),
        "budget 1 must never observe more than one real computation at a time, got: {peaks:?}"
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
    assert!(budget2.repetitions.iter().all(|r| r.success));
    let peaks = budget2.successful_peak_concurrency_values();
    assert!(
        peaks.iter().all(|&p| p == 2),
        "budget 2 must observe genuine two-way concurrency in every repetition, got: {peaks:?}"
    );
}

#[test]
fn results_are_identical_across_cpu_budgets_scheduling_never_changes_pure_computation() {
    // The actual cross-budget comparison: not merely "each budget's own
    // repetitions agree with each other" (which a prior review round
    // found this test only checked, despite its name), but budget 1's
    // evidence digest equals budget 2's.
    let report = m3_baseline::run(1, 5).expect("M3 owned baseline must succeed");
    for budget in &report.budgets {
        assert!(
            budget.evidence_digest_if_consistent().is_some(),
            "budget {}: evidence must be internally consistent across its own repetitions",
            budget.cpu_budget
        );
    }
    assert!(
        report.evidence_matches_across_budgets(),
        "budget 1 and budget 2 must produce bit-for-bit identical evidence digests, got: {:?}",
        report
            .budgets
            .iter()
            .map(|b| (b.cpu_budget, b.evidence_digest_if_consistent()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_genuinely_failing_repetition_is_recorded_and_never_silently_dropped() {
    // Direct regression test for the review's own reproduction: an
    // earlier version aborted the whole `run()` via `?` on the first
    // failure, discarding every repetition already collected. This test
    // can't easily force a real dispatch failure (the chain is fixed
    // and deterministic), so it instead pins the *shape* of the
    // contract: every repetition -- not just the successful ones -- is
    // present in `budget.repetitions`, with a length exactly equal to
    // `repetitions` when nothing failed. Combined with
    // `m8_baseline_test.rs`'s equivalent test (which exercises this
    // over a real invalid input path), the failure-handling contract is
    // covered by at least one genuinely-failing case end to end.
    let report = m3_baseline::run(1, 3).expect("M3 owned baseline must succeed");
    for budget in &report.budgets {
        assert_eq!(
            budget.repetitions.len(),
            3,
            "budget {}: all 3 repetitions must be recorded regardless of individual outcome",
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
fn a_report_is_reproducible_from_its_own_persisted_runs_without_re_executing_anything() {
    let report = m3_baseline::run(1, 3).expect("M3 owned baseline must succeed");
    let json = serde_json::to_string(&report).expect("report always serializes");
    let round_tripped: m3_baseline::M3Report =
        serde_json::from_str(&json).expect("report always deserializes");
    let regenerated =
        m3_baseline::regenerate(round_tripped).expect("regenerate must read the persisted Runs");

    for (original, again) in report.budgets.iter().zip(regenerated.budgets.iter()) {
        let original_sr = original
            .scenario_report
            .as_ref()
            .expect("original run must have produced a ScenarioReport");
        let again_sr = again
            .scenario_report
            .as_ref()
            .expect("regenerated run must reproduce the same ScenarioReport");
        assert_eq!(original_sr.wall_seconds.mean, again_sr.wall_seconds.mean);
        assert_eq!(
            original_sr.wall_seconds.stddev,
            again_sr.wall_seconds.stddev
        );
        assert_eq!(
            original.successful_peak_concurrency_values(),
            again.successful_peak_concurrency_values()
        );
    }
}

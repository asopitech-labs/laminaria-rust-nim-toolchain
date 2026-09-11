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

#[test]
fn regenerate_rebuilds_judgment_from_disk_and_ignores_a_tampered_input_report() {
    // Direct regression test for the review's own reproduction: a
    // report whose in-memory/serialized RepetitionRecord fields have
    // been hand-edited (or are simply stale) must not fool regenerate()
    // -- it must re-derive everything from each Run's own persisted
    // compiler_telemetry, not trust what's passed in.
    let mut report = m3_baseline::run(1, 2).expect("M3 owned baseline must succeed");
    let real_peak_concurrency = report.budgets[0].successful_peak_concurrency_values();
    let real_evidence_digest = report.budgets[0]
        .evidence_digest_if_consistent()
        .map(|s| s.to_string());

    // Tamper with the in-memory report: flip success, blank the digest,
    // zero the concurrency -- exactly the kind of stale/edited aggregate
    // the review reproduced.
    for repetition in &mut report.budgets[0].repetitions {
        repetition.success = false;
        repetition.evidence_digest = None;
        repetition.peak_concurrency = Some(0);
    }

    let regenerated =
        m3_baseline::regenerate(report).expect("regenerate must read the persisted Runs");
    assert_eq!(
        regenerated.budgets[0].successful_peak_concurrency_values(),
        real_peak_concurrency,
        "regenerate() must recover the real peak_concurrency from disk, ignoring the tampered \
         input"
    );
    assert_eq!(
        regenerated.budgets[0]
            .evidence_digest_if_consistent()
            .map(|s| s.to_string()),
        real_evidence_digest,
        "regenerate() must recover the real evidence_digest from disk, ignoring the tampered \
         input"
    );
    assert!(
        regenerated.budgets[0].repetitions.iter().all(|r| r.success),
        "regenerate() must recover the real success flag from disk, ignoring the tampered input"
    );
}

#[test]
fn regenerate_fails_closed_when_a_referenced_run_no_longer_exists() {
    let report = m3_baseline::run(1, 1).expect("M3 owned baseline must succeed");
    let run_id = report.budgets[0].repetitions[0].run_id.clone();
    let run_dir = report.runs_root.join(&run_id);
    std::fs::remove_dir_all(&run_dir).expect("must be able to delete the run dir for this test");

    let result = m3_baseline::regenerate(report);
    assert!(
        result.is_err(),
        "regenerate() must fail closed (never silently fall back to the input report's own \
         stale data) when a referenced run_id no longer exists on disk"
    );
}

#[test]
fn owned_baseline_comparable_across_budgets_rejects_a_genuine_identity_mismatch() {
    // Forces both budgets onto the same fixed, clean commit first -- the
    // real ambient working tree is dirty during this session's own
    // development, which `is_a_valid_baseline` correctly refuses on its
    // own (covered by the dedicated dirty-tree test below), so this test
    // isolates just the "same commit" comparison from that separate
    // check rather than depending on the repo actually being clean when
    // the test suite happens to run.
    let mut report = m3_baseline::run(1, 2).expect("M3 owned baseline must succeed");
    let fixed_commit = "1111111111111111111111111111111111111111".to_string();
    for budget in &mut report.budgets {
        for repetition in &mut budget.repetitions {
            repetition.owned_identity.repo_commit = Some(fixed_commit.clone());
            repetition.owned_identity.repo_dirty = Some(false);
        }
    }
    assert!(
        report.owned_baseline_comparable_across_budgets().is_ok(),
        "two budgets recording the identical, clean repo commit must be comparable"
    );

    for repetition in &mut report.budgets[1].repetitions {
        repetition.owned_identity.repo_commit =
            Some("0000000000000000000000000000000000000000".to_string());
    }
    assert!(
        report.owned_baseline_comparable_across_budgets().is_err(),
        "a budget whose owned identity names a different repo commit must never be treated as \
         comparable to another budget's"
    );
}

#[test]
fn owned_baseline_comparable_across_budgets_rejects_a_dirty_working_tree() {
    let mut report = m3_baseline::run(1, 1).expect("M3 owned baseline must succeed");
    for budget in &mut report.budgets {
        for repetition in &mut budget.repetitions {
            repetition.owned_identity.repo_commit = Some("clean-fixture-commit".to_string());
            repetition.owned_identity.repo_dirty = Some(true);
        }
    }
    assert!(
        report.owned_baseline_comparable_across_budgets().is_err(),
        "a dirty working tree must never be accepted as a comparable, reproducible baseline, \
         even when both sides agree on the commit"
    );
}

#[test]
fn owned_baseline_comparable_across_budgets_rejects_a_stale_measurement_or_planner_digest() {
    // Direct regression test for the correction instruction's own gap
    // ("M3の実行体identityが実物を識別していない"): M3's owned identity
    // previously never bound to any hash of the code actually running
    // the measurement, so two budgets could "compare" even if one ran a
    // stale/rebuilt binary. Now `measurement_executable_sha256` and
    // `planner_binary_sha256` must both resolve and agree.
    let mut report = m3_baseline::run(1, 1).expect("M3 owned baseline must succeed");
    let fixed_commit = "1111111111111111111111111111111111111111".to_string();
    for budget in &mut report.budgets {
        for repetition in &mut budget.repetitions {
            repetition.owned_identity.repo_commit = Some(fixed_commit.clone());
            repetition.owned_identity.repo_dirty = Some(false);
        }
    }
    assert!(report.owned_baseline_comparable_across_budgets().is_ok());

    for repetition in &mut report.budgets[1].repetitions {
        repetition.owned_identity.measurement_executable_sha256 = Some("stale-digest".to_string());
    }
    assert!(
        report.owned_baseline_comparable_across_budgets().is_err(),
        "a budget whose owned identity names a stale/different measurement executable digest \
         must never be treated as comparable to another budget's"
    );

    let real_measurement_executable_sha256 = report.budgets[0].repetitions[0]
        .owned_identity
        .measurement_executable_sha256
        .clone();
    for repetition in &mut report.budgets[1].repetitions {
        repetition.owned_identity.measurement_executable_sha256 =
            real_measurement_executable_sha256.clone();
        repetition.owned_identity.planner_binary_sha256 = Some("stale-planner-digest".to_string());
    }
    assert!(
        report.owned_baseline_comparable_across_budgets().is_err(),
        "a budget whose owned identity names a stale/different planner binary digest must never \
         be treated as comparable to another budget's"
    );
}

#[test]
fn owned_baseline_comparable_across_budgets_rejects_a_missing_or_duplicated_budget_group() {
    // Direct regression test for the correction instruction's own
    // reproduction case ("M3のbudgetを99へ変更"): renaming budget 2's
    // outer `cpu_budget` label to 99 (missing the required 2, adding an
    // unrequired 99) must be rejected, not silently compared.
    let mut report = m3_baseline::run(1, 1).expect("M3 owned baseline must succeed");
    report.budgets[1].cpu_budget = 99;
    assert!(
        report.owned_baseline_comparable_across_budgets().is_err(),
        "a budget set missing a required group (here: 2) and adding an unrequired one (here: 99) \
         must never be treated as comparable"
    );
}

#[test]
fn regenerate_rejects_a_run_id_reused_across_different_budget_groups() {
    // Direct regression test for the correction instruction's own
    // reproduction case ("異なるgroupで同じrun IDを再利用"): a report
    // whose two budget groups both point at the same run_id must be
    // rejected by regenerate(), never silently treated as two distinct
    // repetitions.
    let mut report = m3_baseline::run(1, 1).expect("M3 owned baseline must succeed");
    let reused_run_id = report.budgets[0].repetitions[0].run_id.clone();
    report.budgets[1].repetitions[0].run_id = reused_run_id;
    let result = m3_baseline::regenerate(report);
    assert!(
        result.is_err(),
        "regenerate() must reject a report where the same run_id is referenced from two \
         different budget groups"
    );
}

#[test]
fn regenerate_rejects_a_budget_whose_outer_group_label_does_not_match_its_own_scenario_id() {
    // Direct regression test for the correction instruction's own
    // reproduction case: mutating a budget's outer `cpu_budget` label
    // (without touching the underlying Run files) must be rejected by
    // regenerate(), since the Run's own `scenario_id` (and the
    // telemetry's own `cpu_budget`) no longer matches the label it's
    // filed under.
    let mut report = m3_baseline::run(1, 1).expect("M3 owned baseline must succeed");
    report.budgets[0].cpu_budget = 99;
    let result = m3_baseline::regenerate(report);
    assert!(
        result.is_err(),
        "regenerate() must reject a budget whose outer cpu_budget label doesn't match the \
         scenario_id/telemetry recorded in its own referenced Runs"
    );
}

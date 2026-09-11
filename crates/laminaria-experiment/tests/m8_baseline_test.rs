//! Issue #28 D1-a regression coverage for `M8-many-unrequested-nim-planner`:
//! runs the real owned baseline (real Nim planner) at all three scales
//! and checks the exact `docs/design/issue-35-d0-cases.yaml`
//! `pass_criteria.d1` property (`unused-pkg-*` never appears in
//! `ordered_actions`) -- not merely that it runs without panicking.
#![cfg(unix)]

use laminaria_experiment::m8_baseline;

#[test]
fn the_needed_set_never_includes_an_unused_package_at_any_scale() {
    let report = m8_baseline::run(&[4, 10, 30], 1, 3).expect("M8 owned baseline must succeed");
    assert_eq!(report.scales.len(), 3);
    for scale in &report.scales {
        assert!(
            scale.repetitions.iter().all(|r| r.success),
            "unused_actions={}: every repetition must succeed, got: {:?}",
            scale.unused_actions,
            scale.repetitions
        );
        assert!(
            scale.needed_set_matches_in_every_successful_rep(),
            "unused_actions={}: expected {{used-core, used-util, fixture-bin}} only in every \
             repetition, samples: {:?}",
            scale.unused_actions,
            scale.repetitions
        );
        for repetition in &scale.repetitions {
            assert_eq!(
                repetition.ordered_action_count,
                Some(3),
                "unused_actions={}: a failed (wrong-size) repetition must never be silently \
                 treated as a success",
                scale.unused_actions
            );
        }
    }
}

#[test]
fn the_nim_kernel_reports_its_own_planfromjson_only_timing_separately_from_the_round_trip() {
    let report = m8_baseline::run(&[4], 1, 3).expect("M8 owned baseline must succeed");
    let scale = &report.scales[0];
    for repetition in &scale.repetitions {
        assert!(
            repetition.kernel_nanos.is_some(),
            "expected the planner's stderr kernel_nanos= line to be captured for a successful \
             repetition, got: {repetition:?}"
        );
    }
    let kernel_stats = scale
        .kernel_nanos_stats
        .as_ref()
        .expect("kernel_nanos_stats must be present when repetitions succeeded");
    let round_trip = scale
        .scenario_report
        .as_ref()
        .expect("scenario_report must be present when repetitions succeeded");
    // The kernel-only interval must never be reported as *larger* than
    // the round trip that contains it (process spawn + IPC + JSON serialize
    // on top of the same planFromJson call) -- if it were, the two
    // measurements would have been mixed up.
    assert!(
        kernel_stats.mean <= round_trip.wall_seconds.mean * 1e9,
        "kernel-only nanos (mean={}) must not exceed the round-trip seconds (mean={}) it's \
         contained within",
        kernel_stats.mean,
        round_trip.wall_seconds.mean
    );
}

#[test]
fn needed_set_matches_in_every_successful_rep_is_never_vacuously_true_and_reacts_to_its_own_raw_data(
) {
    // Direct regression test for the review's own reproduction: mutating
    // one repetition's raw `needed_set_matches` must change this
    // derived method's answer -- there is no separately-cached boolean
    // left stale, because there no longer is one; this is computed
    // fresh from `repetitions` every call.
    let mut report = m8_baseline::run(&[4], 1, 3).expect("M8 owned baseline must succeed");
    let scale = &mut report.scales[0];
    assert!(scale.needed_set_matches_in_every_successful_rep());

    scale.repetitions[0].needed_set_matches = Some(false);
    assert!(
        !scale.needed_set_matches_in_every_successful_rep(),
        "flipping one successful repetition's needed_set_matches to false must be reflected \
         immediately, not masked by a stale cached aggregate"
    );

    // Restore, then instead mark every repetition as failed -- the
    // "zero successful repetitions" case must read as false, never
    // vacuously true.
    for repetition in &mut scale.repetitions {
        repetition.success = false;
        repetition.needed_set_matches = None;
    }
    assert!(
        !scale.needed_set_matches_in_every_successful_rep(),
        "a scale where every repetition failed must never report needed_set_matches_in_every_successful_rep() == true"
    );
}

#[test]
fn a_report_is_reproducible_from_its_own_persisted_runs_without_re_planning_anything() {
    let report = m8_baseline::run(&[4], 1, 3).expect("M8 owned baseline must succeed");
    let json = serde_json::to_string(&report).expect("report always serializes");
    let round_tripped: m8_baseline::M8Report =
        serde_json::from_str(&json).expect("report always deserializes");
    let regenerated =
        m8_baseline::regenerate(round_tripped).expect("regenerate must read the persisted Runs");

    for (original, again) in report.scales.iter().zip(regenerated.scales.iter()) {
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
            original.kernel_nanos_stats.as_ref().map(|s| s.mean),
            again.kernel_nanos_stats.as_ref().map(|s| s.mean)
        );
    }
}

#[test]
fn regenerate_rebuilds_judgment_from_disk_and_ignores_a_tampered_input_report() {
    let mut report = m8_baseline::run(&[4], 1, 2).expect("M8 owned baseline must succeed");
    let real_kernel_nanos: Vec<Option<u64>> = report.scales[0]
        .repetitions
        .iter()
        .map(|r| r.kernel_nanos)
        .collect();
    let real_needed_set_matches: Vec<Option<bool>> = report.scales[0]
        .repetitions
        .iter()
        .map(|r| r.needed_set_matches)
        .collect();

    for repetition in &mut report.scales[0].repetitions {
        repetition.success = false;
        repetition.kernel_nanos = None;
        repetition.needed_set_matches = Some(false);
    }

    let regenerated =
        m8_baseline::regenerate(report).expect("regenerate must read the persisted Runs");
    let regenerated_kernel_nanos: Vec<Option<u64>> = regenerated.scales[0]
        .repetitions
        .iter()
        .map(|r| r.kernel_nanos)
        .collect();
    let regenerated_needed_set_matches: Vec<Option<bool>> = regenerated.scales[0]
        .repetitions
        .iter()
        .map(|r| r.needed_set_matches)
        .collect();
    assert_eq!(
        regenerated_kernel_nanos, real_kernel_nanos,
        "regenerate() must recover the real kernel_nanos from disk, ignoring the tampered input"
    );
    assert_eq!(
        regenerated_needed_set_matches, real_needed_set_matches,
        "regenerate() must recover the real needed_set_matches from disk, ignoring the tampered \
         input"
    );
    assert!(regenerated.scales[0].repetitions.iter().all(|r| r.success));
}

#[test]
fn regenerate_fails_closed_when_a_referenced_run_no_longer_exists() {
    let report = m8_baseline::run(&[4], 1, 1).expect("M8 owned baseline must succeed");
    let run_id = report.scales[0].repetitions[0].run_id.clone();
    let run_dir = report.runs_root.join(&run_id);
    std::fs::remove_dir_all(&run_dir).expect("must be able to delete the run dir for this test");

    let result = m8_baseline::regenerate(report);
    assert!(
        result.is_err(),
        "regenerate() must fail closed when a referenced run_id no longer exists on disk"
    );
}

#[test]
fn a_scale_missing_kernel_nanos_on_any_successful_rep_is_not_a_valid_baseline() {
    // Direct regression test for the review's own reproduction:
    // dropping kernel_nanos from every repetition must not leave the
    // pass criteria reading true.
    let mut report = m8_baseline::run(&[4], 1, 2).expect("M8 owned baseline must succeed");
    // Force a clean identity first -- the real ambient working tree is
    // dirty during this session's own development, which
    // `is_a_valid_baseline` correctly refuses on its own (a separate,
    // dedicated concern from the kernel_nanos check this test isolates).
    for repetition in &mut report.scales[0].repetitions {
        repetition.owned_identity.repo_commit =
            Some("1111111111111111111111111111111111111111".to_string());
        repetition.owned_identity.repo_dirty = Some(false);
    }
    assert!(report.scales[0].is_a_valid_baseline());

    for repetition in &mut report.scales[0].repetitions {
        repetition.kernel_nanos = None;
    }
    assert!(
        !report.scales[0].kernel_nanos_present_in_every_successful_rep(),
        "dropping kernel_nanos from every repetition must be reflected immediately"
    );
    assert!(
        !report.scales[0].is_a_valid_baseline(),
        "a scale missing kernel_nanos on its successful repetitions must never be a valid \
         baseline"
    );
}

#[test]
fn owned_baseline_comparable_across_scales_rejects_a_genuine_identity_mismatch() {
    // Forces every scale onto the same fixed, clean commit first -- the
    // real ambient working tree is dirty during this session's own
    // development (covered by the dedicated dirty-tree test below), so
    // this isolates just the "same artifact content" comparison. Uses
    // the full required {4, 10, 30} scale set -- comparability now also
    // requires exactly that set (see the dedicated scale-set tests
    // below), so a partial set would never reach `.is_ok()`.
    let mut report = m8_baseline::run(&[4, 10, 30], 1, 2).expect("M8 owned baseline must succeed");
    for scale in &mut report.scales {
        for repetition in &mut scale.repetitions {
            repetition.owned_identity.repo_commit =
                Some("1111111111111111111111111111111111111111".to_string());
            repetition.owned_identity.repo_dirty = Some(false);
        }
    }
    assert!(
        report.owned_baseline_comparable_across_scales().is_ok(),
        "three scales recording the identical, clean repo commit and identical executable/planner \
         digests must be comparable: {:?}",
        report.owned_baseline_comparable_across_scales()
    );

    for repetition in &mut report.scales[1].repetitions {
        repetition.owned_identity.planner_binary_sha256 = Some("0".repeat(64));
    }
    assert!(
        report.owned_baseline_comparable_across_scales().is_err(),
        "a scale whose owned identity names a different planner binary content digest must \
         never be treated as comparable to another scale's"
    );
}

#[test]
fn owned_baseline_comparable_across_scales_rejects_a_stale_measurement_executable_digest() {
    // Direct regression test for the correction instruction's own
    // reproduction case ("古い／異なる実行体digest"): even with matching
    // commit, dirty flag, and planner binary digest, a scale whose
    // owned identity names a different *measurement executable* digest
    // (e.g. produced by a stale/rebuilt binary) must never be treated as
    // comparable.
    let mut report = m8_baseline::run(&[4, 10, 30], 1, 2).expect("M8 owned baseline must succeed");
    for scale in &mut report.scales {
        for repetition in &mut scale.repetitions {
            repetition.owned_identity.repo_commit =
                Some("1111111111111111111111111111111111111111".to_string());
            repetition.owned_identity.repo_dirty = Some(false);
        }
    }
    assert!(report.owned_baseline_comparable_across_scales().is_ok());

    for repetition in &mut report.scales[2].repetitions {
        repetition.owned_identity.measurement_executable_sha256 = Some("stale-digest".to_string());
    }
    assert!(
        report.owned_baseline_comparable_across_scales().is_err(),
        "a scale whose owned identity names a stale/different measurement executable digest must \
         never be treated as comparable to another scale's"
    );
}

#[test]
fn owned_baseline_comparable_across_scales_rejects_a_dirty_working_tree() {
    let mut report = m8_baseline::run(&[4], 1, 1).expect("M8 owned baseline must succeed");
    for scale in &mut report.scales {
        for repetition in &mut scale.repetitions {
            repetition.owned_identity.repo_commit = Some("clean-fixture-commit".to_string());
            repetition.owned_identity.repo_dirty = Some(true);
        }
    }
    assert!(
        report.owned_baseline_comparable_across_scales().is_err(),
        "a dirty working tree must never be accepted as a comparable, reproducible baseline"
    );
}

#[test]
fn owned_baseline_comparable_across_scales_rejects_a_missing_or_duplicated_scale_group() {
    // Direct regression test for the correction instruction's own
    // reproduction case ("M8の4を30へ変更"): renaming one scale's
    // outer `unused_actions` label so the required {4, 10, 30} set is no
    // longer present (here: {10, 30, 30}, missing 4 and duplicating 30)
    // must be rejected, not silently compared.
    let mut report = m8_baseline::run(&[4, 10, 30], 1, 1).expect("M8 owned baseline must succeed");
    report.scales[0].unused_actions = 30;
    assert!(
        report.owned_baseline_comparable_across_scales().is_err(),
        "a scale set missing a required group (here: 4) and duplicating another (here: 30) must \
         never be treated as comparable"
    );
}

#[test]
fn regenerate_rejects_a_run_id_reused_across_different_scale_groups() {
    // Direct regression test for the correction instruction's own
    // reproduction case ("異なるgroupで同じrun IDを再利用"): a report
    // whose two scale groups both point at the same run_id must be
    // rejected by regenerate(), never silently treated as two distinct
    // repetitions.
    let mut report = m8_baseline::run(&[4, 10], 1, 1).expect("M8 owned baseline must succeed");
    let reused_run_id = report.scales[0].repetitions[0].run_id.clone();
    report.scales[1].repetitions[0].run_id = reused_run_id;
    let result = m8_baseline::regenerate(report);
    assert!(
        result.is_err(),
        "regenerate() must reject a report where the same run_id is referenced from two \
         different scale groups"
    );
}

#[test]
fn regenerate_rejects_a_run_whose_outer_group_label_does_not_match_its_own_scenario_id() {
    // Direct regression test for the correction instruction's own
    // reproduction case: mutating a scale's outer `unused_actions` label
    // (without touching the underlying Run files) must be rejected by
    // regenerate(), since the Run's own `scenario_id` (and the
    // telemetry's own `unused_actions`) no longer matches the label it's
    // filed under.
    let mut report = m8_baseline::run(&[4], 1, 1).expect("M8 owned baseline must succeed");
    report.scales[0].unused_actions = 30;
    let result = m8_baseline::regenerate(report);
    assert!(
        result.is_err(),
        "regenerate() must reject a scale whose outer unused_actions label doesn't match the \
         scenario_id/telemetry recorded in its own referenced Runs"
    );
}

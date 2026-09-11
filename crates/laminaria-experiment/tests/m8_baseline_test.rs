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

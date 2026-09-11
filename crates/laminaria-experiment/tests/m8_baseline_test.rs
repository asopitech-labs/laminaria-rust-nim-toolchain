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
            scale.needed_set_matches_in_every_rep,
            "unused_actions={}: expected {{used-core, used-util, fixture-bin}} only in every \
             repetition, samples: {:?}",
            scale.unused_actions, scale.samples
        );
        for sample in &scale.samples {
            assert_eq!(
                sample.ordered_action_count, 3,
                "unused_actions={}: a failed (wrong-size) repetition must never be silently \
                 treated as a success",
                scale.unused_actions
            );
        }
    }
}

#[test]
fn a_report_is_reproducible_from_its_own_raw_samples_without_re_planning_anything() {
    let report = m8_baseline::run(&[4], 1, 3).expect("M8 owned baseline must succeed");
    let json = serde_json::to_string(&report).expect("report always serializes");
    let round_tripped: m8_baseline::M8Report =
        serde_json::from_str(&json).expect("report always deserializes");
    let regenerated = m8_baseline::regenerate(round_tripped);

    for (original, again) in report.scales.iter().zip(regenerated.scales.iter()) {
        assert_eq!(original.wall_seconds.mean, again.wall_seconds.mean);
        assert_eq!(original.wall_seconds.stddev, again.wall_seconds.stddev);
    }
}

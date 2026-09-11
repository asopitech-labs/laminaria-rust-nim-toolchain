//! Issue #28 D1-a: `M8-many-unrequested-nim-planner` owned baseline.
//!
//! Measures the real Nim planner's own demand-closure pruning
//! (`nim-planner/src/planning_kernel.nim`, issue #27) directly over a
//! `PlanningInput` -- no Cargo workspace, no fixture files on disk,
//! per `docs/design/issue-35-d0-cases.yaml`'s `M8-many-unrequested-nim-planner`
//! case. `used-core`/`used-util` are siblings (`fixture-bin` depends on
//! both, they don't depend on each other); `unused-pkg-*` never connects
//! to `fixture-bin-out`'s demand at all. This measures the *planner's*
//! own time (one `call_planner` round trip per repetition), never
//! `compiler_work_executor`'s dispatch time -- a different measurement
//! from `m3_baseline`, not the same processing compared twice.

use std::time::Instant;

use laminaria_plan::{Action, ActionKind, ArtifactRef, PlanOutcome, PlanningInput};
use laminaria_run::scenario::Stats;
use serde::{Deserialize, Serialize};

use crate::planner_binary::resolve_or_build_planner_binary;

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

#[derive(Debug, Serialize, Deserialize)]
pub struct ScaleSample {
    /// True iff this repetition's `ordered_actions` was exactly
    /// `{used-core, used-util, fixture-bin}` -- no `unused-pkg-*`.
    pub needed_set_matches: bool,
    pub wall_seconds: f64,
    pub ordered_action_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScaleReport {
    pub unused_actions: usize,
    pub warmup_runs: usize,
    pub samples: Vec<ScaleSample>,
    pub wall_seconds: Stats,
    pub needed_set_matches_in_every_rep: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct M8Report {
    pub schema_version: String,
    pub case_id: String,
    pub scales: Vec<ScaleReport>,
}

/// Re-derives every `Stats` summary from a report's own already-recorded
/// raw `samples`, without re-planning anything.
pub fn regenerate(mut report: M8Report) -> M8Report {
    for scale in &mut report.scales {
        scale.wall_seconds =
            Stats::from_samples(scale.samples.iter().map(|s| s.wall_seconds).collect());
    }
    report
}

pub fn run(
    unused_action_scales: &[usize],
    warmup: usize,
    repetitions: usize,
) -> Result<M8Report, String> {
    let planner_bin = resolve_or_build_planner_binary()?;
    let mut scales = Vec::new();

    for &unused_actions in unused_action_scales {
        let input = build_planning_input(unused_actions);

        for _ in 0..warmup {
            let outcome = laminaria_plan::call_planner(&planner_bin, &input)
                .map_err(|e| format!("warmup planner call failed: {e}"))?;
            if !matches!(outcome, PlanOutcome::Planned(_)) {
                return Err(format!(
                    "warmup planner call for unused_actions={unused_actions} was rejected: {outcome:?}"
                ));
            }
        }

        let mut samples = Vec::new();
        let mut needed_set_matches_in_every_rep = true;
        for _ in 0..repetitions {
            let start = Instant::now();
            let outcome = laminaria_plan::call_planner(&planner_bin, &input)
                .map_err(|e| format!("planner call failed: {e}"))?;
            let wall_seconds = start.elapsed().as_secs_f64();
            let plan = match outcome {
                PlanOutcome::Planned(plan) => plan,
                PlanOutcome::Rejected(r) => {
                    return Err(format!("expected a plan, got a rejection: {r:?}"))
                }
            };

            let mut got: Vec<&str> = plan.ordered_actions.iter().map(|s| s.as_str()).collect();
            got.sort_unstable();
            let mut expected = EXPECTED_NEEDED_SET.to_vec();
            expected.sort_unstable();
            let needed_set_matches = got == expected;
            needed_set_matches_in_every_rep &= needed_set_matches;

            samples.push(ScaleSample {
                needed_set_matches,
                wall_seconds,
                ordered_action_count: plan.ordered_actions.len(),
            });
        }

        let wall_seconds = Stats::from_samples(samples.iter().map(|s| s.wall_seconds).collect());
        scales.push(ScaleReport {
            unused_actions,
            warmup_runs: warmup,
            samples,
            wall_seconds,
            needed_set_matches_in_every_rep,
        });
    }

    Ok(M8Report {
        schema_version: "0.1.0".to_string(),
        case_id: "M8-many-unrequested-nim-planner".to_string(),
        scales,
    })
}

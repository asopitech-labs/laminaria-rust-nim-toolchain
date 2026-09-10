//! Rust-side structural validation of an `ExecutionPlan` returned by the
//! Nim planner, run *before* the plan is ever trusted for execution
//! (issue #8: "Rust must reject malformed plans before execution").
//!
//! This checks two distinct things, both required -- an internally
//! self-consistent plan is not automatically a plan that actually
//! answers the request: an empty `ExecutionPlan` (`actions: {}`,
//! `ordered_actions: []`) is internally consistent (the empty set
//! trivially equals the empty set) but produces nothing the caller
//! asked for, a real gap an external review caught by fault-injecting
//! exactly that plan and observing a successful, artifact-less
//! self-build.
//!
//! 1. **Correspondence with the original `PlanningInput`**: the plan
//!    must declare exactly the same action ids the input asked for (not
//!    fewer, not more), each with the same `kind`/`inputs`/`outputs` the
//!    input actually declared for it, and every artifact in
//!    `input.demanded_artifacts` must be produced by some action in the
//!    plan.
//! 2. **Internal ordering consistency**: `ordered_actions` is a
//!    permutation of `actions`, and every dependency (derived from
//!    matching `Declared` inputs against declared outputs) is satisfied
//!    before its dependent runs -- a lightweight re-check of the
//!    ordering property the Nim kernel already established, not a
//!    reimplementation of its cycle-detection/topological-sort solver
//!    (issue #8: "shared contract fixtures and integration tests prove
//!    the same planning behavior," not two independent solvers that
//!    must agree by coincidence).

use std::collections::{BTreeMap, BTreeSet};

use crate::types::{
    Action, ArtifactRef, ExecutionPlan, PlanningInput, PLAN_SCHEMA_VERSION, PRODUCED_BY,
};

#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    UnexpectedSchemaVersion {
        got: String,
    },
    UnexpectedProducer {
        got: String,
    },
    /// The plan's declared actions are not exactly the set the
    /// `PlanningInput` asked for -- catches a planner silently dropping
    /// (or fabricating) an action, including the degenerate empty-plan
    /// case.
    ActionSetMismatch {
        detail: String,
    },
    /// An action the plan kept has a different `kind`/`inputs`/
    /// `outputs` than what the `PlanningInput` actually declared for
    /// that id.
    ActionShapeMismatch {
        action_id: String,
        detail: String,
    },
    /// An artifact `PlanningInput.demanded_artifacts` asked for is not
    /// produced by any action's declared outputs in the plan.
    UnmetDemand {
        artifact_id: String,
    },
    /// Two actions both declare the same artifact id as an output --
    /// checked independently here rather than trusted from the Nim
    /// kernel's own (already-enforced) rejection of this case, per issue
    /// #8's "Rust must reject malformed plans before execution."
    DuplicateProducer {
        artifact_id: String,
        first_producer: String,
        second_producer: String,
    },
    OrderedActionsMismatch {
        detail: String,
    },
    /// `dependent` declares (via a `Declared` input) that `producer`
    /// must run first, but `producer` appears at or after `dependent`
    /// in `ordered_actions` -- the exact ordering violation that would
    /// let execution consume an artifact before it exists.
    DependencyOrderViolation {
        dependent: String,
        producer: String,
    },
    UnknownProducer {
        action: String,
        artifact_id: String,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::UnexpectedSchemaVersion { got } => write!(
                f,
                "ExecutionPlan.schema_version = {got:?}, expected {PLAN_SCHEMA_VERSION:?}"
            ),
            ValidationError::UnexpectedProducer { got } => write!(
                f,
                "ExecutionPlan.produced_by = {got:?}, expected {PRODUCED_BY:?} -- refusing to \
                 execute a plan that did not come from the real Nim planning kernel"
            ),
            ValidationError::ActionSetMismatch { detail } => write!(
                f,
                "ExecutionPlan does not declare exactly the actions PlanningInput asked for: {detail}"
            ),
            ValidationError::ActionShapeMismatch { action_id, detail } => write!(
                f,
                "action {action_id:?} in the returned plan does not match what PlanningInput \
                 declared for it: {detail}"
            ),
            ValidationError::UnmetDemand { artifact_id } => write!(
                f,
                "PlanningInput demanded artifact {artifact_id:?}, but no action in the returned \
                 plan produces it"
            ),
            ValidationError::DuplicateProducer {
                artifact_id,
                first_producer,
                second_producer,
            } => write!(
                f,
                "artifact {artifact_id:?} is declared as an output of both {first_producer:?} and \
                 {second_producer:?} -- the Nim kernel should never emit this; refusing to \
                 execute an internally inconsistent plan"
            ),
            ValidationError::OrderedActionsMismatch { detail } => {
                write!(
                    f,
                    "ExecutionPlan.ordered_actions is not a permutation of actions: {detail}"
                )
            }
            ValidationError::DependencyOrderViolation {
                dependent,
                producer,
            } => write!(
                f,
                "action {dependent:?} consumes an artifact produced by {producer:?}, but \
                 ordered_actions does not place {producer:?} before {dependent:?}"
            ),
            ValidationError::UnknownProducer {
                action,
                artifact_id,
            } => write!(
                f,
                "action {action:?} declares a Declared input {artifact_id:?} that no action in \
                 this plan declares as an output -- the Nim kernel should never emit this; \
                 refusing to execute an internally inconsistent plan"
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validates `plan` structurally. Returns `Ok(())` only when the plan is
/// safe to hand to an executor.
pub fn validate(plan: &ExecutionPlan, input: &PlanningInput) -> Result<(), ValidationError> {
    if plan.schema_version != PLAN_SCHEMA_VERSION {
        return Err(ValidationError::UnexpectedSchemaVersion {
            got: plan.schema_version.clone(),
        });
    }
    if plan.produced_by != PRODUCED_BY {
        return Err(ValidationError::UnexpectedProducer {
            got: plan.produced_by.clone(),
        });
    }

    // Correspondence with the original PlanningInput, checked before any
    // internal-consistency check below: a plan that dropped every
    // action (or fabricated one the input never asked for) must be
    // rejected here, not accepted because the empty set trivially
    // equals itself.
    let input_actions_by_id: BTreeMap<&str, &Action> =
        input.actions.iter().map(|a| (a.id.as_str(), a)).collect();
    let input_action_ids: BTreeSet<&str> = input_actions_by_id.keys().copied().collect();
    let plan_action_ids: BTreeSet<&str> = plan.actions.keys().map(String::as_str).collect();
    if input_action_ids != plan_action_ids {
        return Err(ValidationError::ActionSetMismatch {
            detail: format!(
                "PlanningInput declared actions {input_action_ids:?} but ExecutionPlan.actions \
                 declares {plan_action_ids:?}"
            ),
        });
    }
    for (action_id, plan_action) in &plan.actions {
        let input_action = input_actions_by_id[action_id.as_str()];
        if plan_action.kind != input_action.kind
            || plan_action.inputs != input_action.inputs
            || plan_action.outputs != input_action.outputs
        {
            return Err(ValidationError::ActionShapeMismatch {
                action_id: action_id.clone(),
                detail: format!(
                    "PlanningInput declared kind={:?} inputs={:?} outputs={:?}, but the plan's \
                     action has kind={:?} inputs={:?} outputs={:?}",
                    input_action.kind,
                    input_action.inputs,
                    input_action.outputs,
                    plan_action.kind,
                    plan_action.inputs,
                    plan_action.outputs
                ),
            });
        }
    }

    let ordered_ids: BTreeSet<&str> = plan.ordered_actions.iter().map(String::as_str).collect();
    if plan_action_ids != ordered_ids {
        return Err(ValidationError::OrderedActionsMismatch {
            detail: format!(
                "declared actions {plan_action_ids:?} vs. ordered_actions {ordered_ids:?}"
            ),
        });
    }
    if plan.ordered_actions.len() != plan.actions.len() {
        return Err(ValidationError::OrderedActionsMismatch {
            detail: format!(
                "ordered_actions has {} entries but {} of them are duplicates (actions map has {} \
                 unique ids)",
                plan.ordered_actions.len(),
                plan.ordered_actions.len() - ordered_ids.len(),
                plan.actions.len()
            ),
        });
    }

    // The same producer-index derivation `nim-planner/src/planning_kernel.nim`
    // does, recomputed independently here rather than trusted from the
    // wire -- an `ExecutionPlan.actions` map could in principle have been
    // corrupted/hand-edited in transit. Unlike a plain `BTreeMap::insert`,
    // a second action claiming an already-claimed output artifact is
    // rejected outright rather than silently overwriting the first
    // producer -- the Nim kernel already rejects this case itself, but
    // issue #8 asks Rust not to simply trust that it did.
    let mut producer_of: BTreeMap<&str, &str> = BTreeMap::new();
    for (action_id, action) in &plan.actions {
        for output in &action.outputs {
            if let ArtifactRef::Declared { artifact_id } = output {
                if let Some(&existing) = producer_of.get(artifact_id.as_str()) {
                    if existing != action_id.as_str() {
                        return Err(ValidationError::DuplicateProducer {
                            artifact_id: artifact_id.clone(),
                            first_producer: existing.to_string(),
                            second_producer: action_id.clone(),
                        });
                    }
                }
                producer_of.insert(artifact_id.as_str(), action_id.as_str());
            }
        }
    }

    // Every artifact the original PlanningInput actually demanded must
    // be produced by some action in the plan -- the direct fix for "an
    // internally consistent but unrelated plan (including an empty one)
    // still validates."
    for artifact_id in &input.demanded_artifacts {
        if !producer_of.contains_key(artifact_id.as_str()) {
            return Err(ValidationError::UnmetDemand {
                artifact_id: artifact_id.clone(),
            });
        }
    }

    let position: BTreeMap<&str, usize> = plan
        .ordered_actions
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    for (action_id, action) in &plan.actions {
        for action_input in &action.inputs {
            let ArtifactRef::Declared { artifact_id } = action_input else {
                continue;
            };
            let Some(producer_id) = producer_of.get(artifact_id.as_str()) else {
                return Err(ValidationError::UnknownProducer {
                    action: action_id.clone(),
                    artifact_id: artifact_id.clone(),
                });
            };
            if producer_id == action_id {
                return Err(ValidationError::DependencyOrderViolation {
                    dependent: action_id.clone(),
                    producer: (*producer_id).to_string(),
                });
            }
            let producer_pos = position[producer_id];
            let dependent_pos = position[action_id.as_str()];
            if producer_pos >= dependent_pos {
                return Err(ValidationError::DependencyOrderViolation {
                    dependent: action_id.clone(),
                    producer: (*producer_id).to_string(),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, ActionKind, PlanningInput};
    use std::collections::BTreeMap;

    fn action(id: &str, inputs: Vec<ArtifactRef>, outputs: Vec<ArtifactRef>) -> Action {
        Action {
            id: id.to_string(),
            kind: ActionKind::NimBuild,
            command_identity: id.to_string(),
            inputs,
            outputs,
            compiler_work: None,
        }
    }

    fn valid_input() -> PlanningInput {
        PlanningInput::new(
            vec!["out-a".to_string()],
            vec![
                action("a", vec![], vec![ArtifactRef::declared("out-a")]),
                action("b", vec![ArtifactRef::declared("out-a")], vec![]),
            ],
        )
    }

    fn valid_plan() -> ExecutionPlan {
        let mut actions = BTreeMap::new();
        actions.insert(
            "a".to_string(),
            action("a", vec![], vec![ArtifactRef::declared("out-a")]),
        );
        actions.insert(
            "b".to_string(),
            action("b", vec![ArtifactRef::declared("out-a")], vec![]),
        );
        ExecutionPlan {
            schema_version: PLAN_SCHEMA_VERSION.to_string(),
            produced_by: PRODUCED_BY.to_string(),
            producer_version: PLAN_SCHEMA_VERSION.to_string(),
            plan_id: "test".to_string(),
            ordered_actions: vec!["a".to_string(), "b".to_string()],
            actions,
        }
    }

    #[test]
    fn a_well_formed_plan_validates() {
        assert!(validate(&valid_plan(), &valid_input()).is_ok());
    }

    #[test]
    fn a_plan_not_produced_by_the_real_kernel_is_rejected() {
        let mut plan = valid_plan();
        plan.produced_by = "some-other-planner".to_string();
        assert_eq!(
            validate(&plan, &valid_input()),
            Err(ValidationError::UnexpectedProducer {
                got: "some-other-planner".to_string()
            })
        );
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let mut plan = valid_plan();
        plan.schema_version = "9.9.9".to_string();
        assert_eq!(
            validate(&plan, &valid_input()),
            Err(ValidationError::UnexpectedSchemaVersion {
                got: "9.9.9".to_string()
            })
        );
    }

    #[test]
    fn ordered_actions_out_of_dependency_order_is_rejected() {
        let mut plan = valid_plan();
        plan.ordered_actions = vec!["b".to_string(), "a".to_string()];
        match validate(&plan, &valid_input()) {
            Err(ValidationError::DependencyOrderViolation {
                dependent,
                producer,
            }) => {
                assert_eq!(dependent, "b");
                assert_eq!(producer, "a");
            }
            other => panic!("expected DependencyOrderViolation, got {other:?}"),
        }
    }

    #[test]
    fn ordered_actions_missing_a_declared_action_is_rejected() {
        let mut plan = valid_plan();
        plan.ordered_actions = vec!["a".to_string()];
        assert!(matches!(
            validate(&plan, &valid_input()),
            Err(ValidationError::OrderedActionsMismatch { .. })
        ));
    }

    #[test]
    fn a_duplicate_entry_in_ordered_actions_is_rejected_even_though_the_set_matches() {
        let mut plan = valid_plan();
        // {"a", "a", "b"} as a *set* equals {"a", "b"} exactly -- a naive
        // set-equality check alone would accept this, so the separate
        // length check is what actually catches "a" listed twice.
        plan.ordered_actions = vec!["a".to_string(), "a".to_string(), "b".to_string()];
        assert!(matches!(
            validate(&plan, &valid_input()),
            Err(ValidationError::OrderedActionsMismatch { .. })
        ));
    }

    #[test]
    fn an_input_naming_no_known_producer_is_rejected() {
        // "c" is declared identically on both sides (PlanningInput and
        // the plan agree on its shape) so this exercises UnknownProducer
        // specifically, not ActionSetMismatch/ActionShapeMismatch.
        let c = action("c", vec![ArtifactRef::declared("does-not-exist")], vec![]);
        let mut input = valid_input();
        input.actions.push(c.clone());
        let mut plan = valid_plan();
        plan.actions.insert("c".to_string(), c);
        plan.ordered_actions.push("c".to_string());
        assert!(matches!(
            validate(&plan, &input),
            Err(ValidationError::UnknownProducer { .. })
        ));
    }

    /// The exact bug an external review caught by fault injection: an
    /// empty `ExecutionPlan` (no actions at all) is internally
    /// consistent -- the empty set trivially equals the empty set -- but
    /// must still be rejected because it corresponds to nothing
    /// `PlanningInput` actually asked for.
    #[test]
    fn an_empty_plan_that_drops_every_requested_action_is_rejected() {
        let empty_plan = ExecutionPlan {
            schema_version: PLAN_SCHEMA_VERSION.to_string(),
            produced_by: PRODUCED_BY.to_string(),
            producer_version: PLAN_SCHEMA_VERSION.to_string(),
            plan_id: "test".to_string(),
            ordered_actions: vec![],
            actions: BTreeMap::new(),
        };
        assert!(matches!(
            validate(&empty_plan, &valid_input()),
            Err(ValidationError::ActionSetMismatch { .. })
        ));
    }

    #[test]
    fn a_plan_that_alters_a_requested_actions_declared_shape_is_rejected() {
        let mut plan = valid_plan();
        // "a" now claims a different output than PlanningInput actually
        // declared for it.
        plan.actions.insert(
            "a".to_string(),
            action("a", vec![], vec![ArtifactRef::declared("out-a-renamed")]),
        );
        assert!(matches!(
            validate(&plan, &valid_input()),
            Err(ValidationError::ActionShapeMismatch { .. })
        ));
    }

    #[test]
    fn a_plan_that_does_not_produce_a_demanded_artifact_is_rejected() {
        let mut input = valid_input();
        input.demanded_artifacts = vec!["never-produced".to_string()];
        assert!(matches!(
            validate(&valid_plan(), &input),
            Err(ValidationError::UnmetDemand { .. })
        ));
    }

    #[test]
    fn two_actions_claiming_the_same_output_artifact_is_rejected() {
        let c = action("c", vec![], vec![ArtifactRef::declared("out-a")]);
        let mut input = valid_input();
        input.actions.push(c.clone());
        let mut plan = valid_plan();
        plan.actions.insert("c".to_string(), c);
        plan.ordered_actions.push("c".to_string());
        assert!(matches!(
            validate(&plan, &input),
            Err(ValidationError::DuplicateProducer { .. })
        ));
    }
}

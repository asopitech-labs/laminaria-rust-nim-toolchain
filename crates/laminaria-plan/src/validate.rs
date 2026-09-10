//! Rust-side structural validation of an `ExecutionPlan` returned by the
//! Nim planner, run *before* the plan is ever trusted for execution
//! (issue #8: "Rust must reject malformed plans before execution").
//!
//! This is deliberately a lightweight re-check of the ordering property
//! the Nim kernel already established, not a reimplementation of its
//! cycle-detection/topological-sort solver -- the same layering issue
//! #8 itself asks for ("shared contract fixtures and integration tests
//! prove the same planning behavior," not two independent solvers that
//! must agree by coincidence).

use std::collections::{BTreeMap, BTreeSet};

use crate::types::{ArtifactRef, ExecutionPlan, PLAN_SCHEMA_VERSION, PRODUCED_BY};

#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    UnexpectedSchemaVersion {
        got: String,
    },
    UnexpectedProducer {
        got: String,
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
pub fn validate(plan: &ExecutionPlan) -> Result<(), ValidationError> {
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

    let declared_ids: BTreeSet<&str> = plan.actions.keys().map(String::as_str).collect();
    let ordered_ids: BTreeSet<&str> = plan.ordered_actions.iter().map(String::as_str).collect();
    if declared_ids != ordered_ids {
        return Err(ValidationError::OrderedActionsMismatch {
            detail: format!(
                "declared actions {declared_ids:?} vs. ordered_actions {ordered_ids:?}"
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
    // corrupted/hand-edited in transit.
    let mut producer_of: BTreeMap<&str, &str> = BTreeMap::new();
    for (action_id, action) in &plan.actions {
        for output in &action.outputs {
            if let ArtifactRef::Declared { artifact_id } = output {
                producer_of.insert(artifact_id.as_str(), action_id.as_str());
            }
        }
    }

    let position: BTreeMap<&str, usize> = plan
        .ordered_actions
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    for (action_id, action) in &plan.actions {
        for input in &action.inputs {
            let ArtifactRef::Declared { artifact_id } = input else {
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
    use crate::types::{Action, ActionKind};
    use std::collections::BTreeMap;

    fn action(id: &str, inputs: Vec<ArtifactRef>, outputs: Vec<ArtifactRef>) -> Action {
        Action {
            id: id.to_string(),
            kind: ActionKind::NimBuild,
            command_identity: id.to_string(),
            inputs,
            outputs,
        }
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
        assert!(validate(&valid_plan()).is_ok());
    }

    #[test]
    fn a_plan_not_produced_by_the_real_kernel_is_rejected() {
        let mut plan = valid_plan();
        plan.produced_by = "some-other-planner".to_string();
        assert_eq!(
            validate(&plan),
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
            validate(&plan),
            Err(ValidationError::UnexpectedSchemaVersion {
                got: "9.9.9".to_string()
            })
        );
    }

    #[test]
    fn ordered_actions_out_of_dependency_order_is_rejected() {
        let mut plan = valid_plan();
        plan.ordered_actions = vec!["b".to_string(), "a".to_string()];
        match validate(&plan) {
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
            validate(&plan),
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
            validate(&plan),
            Err(ValidationError::OrderedActionsMismatch { .. })
        ));
    }

    #[test]
    fn an_input_naming_no_known_producer_is_rejected() {
        let mut plan = valid_plan();
        plan.actions.insert(
            "c".to_string(),
            action("c", vec![ArtifactRef::declared("does-not-exist")], vec![]),
        );
        plan.ordered_actions.push("c".to_string());
        assert!(matches!(
            validate(&plan),
            Err(ValidationError::UnknownProducer { .. })
        ));
    }
}

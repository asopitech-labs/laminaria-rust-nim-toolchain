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
//!    must declare exactly the action ids needed to satisfy
//!    `input.demanded_artifacts` -- its dependency closure (see
//!    [`demand_closure`]), computed independently from `input` alone,
//!    never fewer and never an action `input.actions` never declared --
//!    each with the same `kind`/`inputs`/`outputs` the input actually
//!    declared for it. A review caught that this used to require an
//!    exact match against *every* action `input.actions` listed,
//!    rejecting a legitimately demand-pruned plan outright; issue #27's
//!    own "需要選択" fix is what [`demand_closure`] closes.
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

use crate::compiler_work::{validate_compiler_work_action, CompilerWorkContractError};
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
    /// One action's `compiler_work` descriptor fails its own contract
    /// (issue #27 B) -- presence-per-kind, schema version, recomputed
    /// identity, or semantic-dependency correspondence. See
    /// [`CompilerWorkContractError`]'s own doc comment for what each
    /// variant closes.
    CompilerWork(CompilerWorkContractError),
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
            ValidationError::CompilerWork(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Builds an artifact-id -> producer-action-id index, rejecting outright
/// the moment two different action ids claim the same declared output.
/// Shared between [`demand_closure`]'s own walk (over
/// `PlanningInput.actions`) and `validate`'s internal-consistency check
/// (over `ExecutionPlan.actions`) so both independently reject a
/// duplicate producer via the exact same rule, rather than two rules
/// that merely happen to agree.
fn producer_index<'a>(
    actions: impl IntoIterator<Item = (&'a str, &'a [ArtifactRef])>,
) -> Result<BTreeMap<&'a str, &'a str>, ValidationError> {
    let mut producer_of: BTreeMap<&str, &str> = BTreeMap::new();
    for (action_id, outputs) in actions {
        for output in outputs {
            if let ArtifactRef::Declared { artifact_id } = output {
                if let Some(&existing) = producer_of.get(artifact_id.as_str()) {
                    if existing != action_id {
                        return Err(ValidationError::DuplicateProducer {
                            artifact_id: artifact_id.clone(),
                            first_producer: existing.to_string(),
                            second_producer: action_id.to_string(),
                        });
                    }
                }
                producer_of.insert(artifact_id.as_str(), action_id);
            }
        }
    }
    Ok(producer_of)
}

/// The action ids actually needed to satisfy `input.demanded_artifacts`
/// -- the backward dependency closure over `Declared` inputs, starting
/// at each demanded artifact's own producer and following every
/// producer's own `Declared` inputs transitively (Buck2's own
/// demand-driven build: only an artifact's producing action, and
/// everything *it* needs, ever runs -- `action.inputs()` is exactly what
/// is waited on, `build_action_no_redirect`). Computed entirely from
/// `input` -- never from the returned plan -- so this is what the plan
/// is *expected* to contain, independent of what it actually does.
fn demand_closure(input: &PlanningInput) -> Result<BTreeSet<&str>, ValidationError> {
    let actions_by_id: BTreeMap<&str, &Action> =
        input.actions.iter().map(|a| (a.id.as_str(), a)).collect();
    let producer_of = producer_index(
        input
            .actions
            .iter()
            .map(|a| (a.id.as_str(), a.outputs.as_slice())),
    )?;

    let mut needed: BTreeSet<&str> = BTreeSet::new();
    let mut queue: Vec<&str> = Vec::new();
    for artifact_id in &input.demanded_artifacts {
        let Some(&producer) = producer_of.get(artifact_id.as_str()) else {
            return Err(ValidationError::UnmetDemand {
                artifact_id: artifact_id.clone(),
            });
        };
        if needed.insert(producer) {
            queue.push(producer);
        }
    }
    while let Some(action_id) = queue.pop() {
        let action = actions_by_id[action_id];
        for action_input in &action.inputs {
            if let ArtifactRef::Declared { artifact_id } = action_input {
                let Some(&producer) = producer_of.get(artifact_id.as_str()) else {
                    return Err(ValidationError::UnknownProducer {
                        action: action_id.to_string(),
                        artifact_id: artifact_id.clone(),
                    });
                };
                if needed.insert(producer) {
                    queue.push(producer);
                }
            }
        }
    }
    Ok(needed)
}

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
    // internal-consistency check below: a plan that dropped an action
    // its own demand closure actually needs (or fabricated one nothing
    // needs) must be rejected here, not accepted because the empty set
    // trivially equals itself. Issue #27's own demand-selection fix:
    // this used to require the plan to declare *every* action
    // `input.actions` listed, which rejected a legitimately pruned plan
    // outright -- now it's the closure `demand_closure` computes from
    // `input.demanded_artifacts` alone.
    let input_actions_by_id: BTreeMap<&str, &Action> =
        input.actions.iter().map(|a| (a.id.as_str(), a)).collect();
    let needed_action_ids = demand_closure(input)?;
    let plan_action_ids: BTreeSet<&str> = plan.actions.keys().map(String::as_str).collect();
    if needed_action_ids != plan_action_ids {
        return Err(ValidationError::ActionSetMismatch {
            detail: format!(
                "PlanningInput's demand closure requires actions {needed_action_ids:?} but \
                 ExecutionPlan.actions declares {plan_action_ids:?}"
            ),
        });
    }
    for (action_id, plan_action) in &plan.actions {
        let input_action = input_actions_by_id[action_id.as_str()];
        // `compiler_work` is compared too -- a review caught that it
        // previously was not, so a plan echoing back a *mutated*
        // descriptor (a different `caller`/`callee`, a stripped
        // `resource_request`, ...) for an otherwise-unchanged action id
        // would silently pass this check.
        if plan_action.kind != input_action.kind
            || plan_action.inputs != input_action.inputs
            || plan_action.outputs != input_action.outputs
            || plan_action.compiler_work != input_action.compiler_work
        {
            return Err(ValidationError::ActionShapeMismatch {
                action_id: action_id.clone(),
                detail: format!(
                    "PlanningInput declared kind={:?} inputs={:?} outputs={:?} \
                     compiler_work={:?}, but the plan's action has kind={:?} inputs={:?} \
                     outputs={:?} compiler_work={:?}",
                    input_action.kind,
                    input_action.inputs,
                    input_action.outputs,
                    input_action.compiler_work,
                    plan_action.kind,
                    plan_action.inputs,
                    plan_action.outputs,
                    plan_action.compiler_work,
                ),
            });
        }
        validate_compiler_work_action(plan_action).map_err(ValidationError::CompilerWork)?;
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
    // does, recomputed independently here (via the same `producer_index`
    // helper `demand_closure` used above, but over the wire `plan.actions`
    // this time) rather than trusted from the wire -- an
    // `ExecutionPlan.actions` map could in principle have been
    // corrupted/hand-edited in transit. A second action claiming an
    // already-claimed output artifact is rejected outright rather than
    // silently overwriting the first producer -- the Nim kernel already
    // rejects this case itself, but issue #8 asks Rust not to simply
    // trust that it did.
    let producer_of = producer_index(
        plan.actions
            .iter()
            .map(|(id, action)| (id.as_str(), action.outputs.as_slice())),
    )?;

    // Every artifact the original PlanningInput actually demanded must
    // be produced by some action in the plan -- a secondary confirmation
    // of what `demand_closure` (checked above, from `input` alone)
    // already guarantees once the action-set/shape checks above pass;
    // kept as a second, independent derivation over the wire plan itself
    // rather than removed, per this function's own "don't just trust one
    // side" stance elsewhere in this same check.
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

    /// "b" declares its own output ("out-b") rather than none, and demand
    /// names only "out-b" -- "a" is pulled in transitively (via "b"'s own
    /// declared input "out-a"), never demanded directly. A review's own
    /// demand-selection fix means an action producing nothing can never
    /// be part of any demand closure (nothing can ever name it), so this
    /// base fixture -- deliberately exercised by most tests below -- has
    /// to give every action a real output to stay reachable.
    fn valid_input() -> PlanningInput {
        PlanningInput::new(
            vec!["out-b".to_string()],
            vec![
                action("a", vec![], vec![ArtifactRef::declared("out-a")]),
                action(
                    "b",
                    vec![ArtifactRef::declared("out-a")],
                    vec![ArtifactRef::declared("out-b")],
                ),
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
            action(
                "b",
                vec![ArtifactRef::declared("out-a")],
                vec![ArtifactRef::declared("out-b")],
            ),
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
        // specifically, not ActionSetMismatch/ActionShapeMismatch. It
        // also needs its own output ("out-c") demanded directly:
        // demand-closure pruning (this round's own fix) would otherwise
        // prune "c" out before its dangling input is ever inspected,
        // since nothing else in this fixture consumes anything "c"
        // produces.
        let c = action(
            "c",
            vec![ArtifactRef::declared("does-not-exist")],
            vec![ArtifactRef::declared("out-c")],
        );
        let mut input = valid_input();
        input.demanded_artifacts.push("out-c".to_string());
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

    /// The exact fix this round makes: a plan that legitimately excludes
    /// an action nothing in the demand closure needs must still validate
    /// -- before this round, `validate` required the plan to declare
    /// *every* action `input.actions` listed, which rejected this
    /// correct, pruned plan outright.
    #[test]
    fn a_plan_that_legitimately_prunes_an_undemanded_action_validates() {
        let mut input = valid_input();
        // "c" produces "out-c", which nothing demands and nothing else
        // consumes -- a legitimately prunable action.
        input
            .actions
            .push(action("c", vec![], vec![ArtifactRef::declared("out-c")]));
        // The plan correctly omits "c" entirely.
        assert_eq!(validate(&valid_plan(), &input), Ok(()));
    }

    /// The flip side of the fix above: a plan that includes an action
    /// *outside* the demand closure -- one nothing demands and nothing
    /// else consumes -- is rejected too, not merely tolerated as "extra
    /// work that happens to also get done."
    #[test]
    fn a_plan_that_includes_an_undemanded_action_is_rejected() {
        let mut input = valid_input();
        input
            .actions
            .push(action("c", vec![], vec![ArtifactRef::declared("out-c")]));
        let mut plan = valid_plan();
        plan.actions.insert(
            "c".to_string(),
            action("c", vec![], vec![ArtifactRef::declared("out-c")]),
        );
        plan.ordered_actions.push("c".to_string());
        assert!(matches!(
            validate(&plan, &input),
            Err(ValidationError::ActionSetMismatch { .. })
        ));
    }

    #[test]
    fn two_actions_claiming_the_same_output_artifact_is_rejected() {
        // Caught by `demand_closure`'s own `producer_index` call (over
        // `input.actions`) before the demand walk even starts -- "c"
        // itself is never demanded and produces nothing anything else
        // consumes, but the duplicate-output conflict with "a" is a
        // structural property of the whole input, independent of demand.
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

    /// A review caught that `ActionShapeMismatch` compared `kind`/
    /// `inputs`/`outputs` but not `compiler_work` -- a plan echoing back
    /// a *mutated* descriptor for an otherwise-unchanged action id would
    /// previously pass this check silently.
    #[test]
    fn a_plan_that_alters_an_actions_compiler_work_descriptor_is_rejected() {
        use crate::compiler_work::{
            transform_function_artifact_id, CompilerWorkDescriptor, ResourceRequest, TransformKind,
            TransformParameters, COMPILER_WORK_SCHEMA_VERSION,
        };

        let descriptor = CompilerWorkDescriptor {
            descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
            operation_version: "0.1.0".to_string(),
            semantic_input_artifact_ids: vec!["out-a".to_string()],
            requested_functions: vec![],
            language: None,
            contract_version: None,
            transform: Some(TransformParameters {
                kind: TransformKind::Checked,
                transform_version: "0.1.0".to_string(),
                caller: "caller".to_string(),
                callee: "callee".to_string(),
            }),
            source_provenance: None,
            test_inputs: vec![],
            resource_request: ResourceRequest::minimal(),
            budget_token: "budget-1".to_string(),
        };
        let id = transform_function_artifact_id(
            "0.1.0",
            "out-a",
            "caller",
            "callee",
            TransformKind::Checked,
            "0.1.0",
        );
        let compiler_work_action = Action {
            id: id.clone(),
            kind: ActionKind::TransformFunction,
            command_identity: "transform_function".to_string(),
            inputs: vec![ArtifactRef::declared("out-a")],
            outputs: vec![ArtifactRef::declared(&id)],
            compiler_work: Some(descriptor.clone()),
        };

        let mut input = valid_input();
        // Demand-closure pruning (this round's own fix) would otherwise
        // exclude this action entirely: nothing else in the base fixture
        // consumes its output, so it must be demanded directly.
        input.demanded_artifacts.push(id.clone());
        input.actions.push(compiler_work_action.clone());
        let mut plan = valid_plan();
        plan.ordered_actions.push(id.clone());

        // The plan echoes back the *same* action id/inputs/outputs, but
        // with `callee` mutated -- everything `ActionShapeMismatch`
        // checked before this fix stays identical.
        let mut tampered_action = compiler_work_action.clone();
        tampered_action
            .compiler_work
            .as_mut()
            .unwrap()
            .transform
            .as_mut()
            .unwrap()
            .callee = "a-different-callee".to_string();
        plan.actions.insert(id, tampered_action);

        assert!(matches!(
            validate(&plan, &input),
            Err(ValidationError::ActionShapeMismatch { .. })
        ));
    }

    /// A well-formed compiler-work action, identical on both sides,
    /// still validates end-to-end -- `validate()` itself, not only
    /// `validate_compiler_work_action` in isolation.
    #[test]
    fn a_plan_with_a_well_formed_compiler_work_action_validates() {
        use crate::compiler_work::{
            transform_function_artifact_id, CompilerWorkDescriptor, ResourceRequest, TransformKind,
            TransformParameters, COMPILER_WORK_SCHEMA_VERSION,
        };

        let descriptor = CompilerWorkDescriptor {
            descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
            operation_version: "0.1.0".to_string(),
            semantic_input_artifact_ids: vec!["out-a".to_string()],
            requested_functions: vec![],
            language: None,
            contract_version: None,
            transform: Some(TransformParameters {
                kind: TransformKind::Checked,
                transform_version: "0.1.0".to_string(),
                caller: "caller".to_string(),
                callee: "callee".to_string(),
            }),
            source_provenance: None,
            test_inputs: vec![],
            resource_request: ResourceRequest::minimal(),
            budget_token: "budget-1".to_string(),
        };
        let id = transform_function_artifact_id(
            "0.1.0",
            "out-a",
            "caller",
            "callee",
            TransformKind::Checked,
            "0.1.0",
        );
        let compiler_work_action = Action {
            id: id.clone(),
            kind: ActionKind::TransformFunction,
            command_identity: "transform_function".to_string(),
            inputs: vec![ArtifactRef::declared("out-a")],
            outputs: vec![ArtifactRef::declared(&id)],
            compiler_work: Some(descriptor),
        };

        let mut input = valid_input();
        // Demand-closure pruning (this round's own fix) would otherwise
        // exclude this action entirely: nothing else in the base fixture
        // consumes its output, so it must be demanded directly.
        input.demanded_artifacts.push(id.clone());
        input.actions.push(compiler_work_action.clone());
        let mut plan = valid_plan();
        plan.actions.insert(id.clone(), compiler_work_action);
        plan.ordered_actions.push(id);

        assert_eq!(validate(&plan, &input), Ok(()));
    }

    /// `validate()` itself propagates a `CompilerWorkContractError`
    /// (wrapped as `ValidationError::CompilerWork`) for a compiler-work
    /// action that is identical on both sides but internally invalid --
    /// here, a `semantic_input_artifact_ids` entry the action's own
    /// `inputs` never declares.
    #[test]
    fn validate_propagates_a_compiler_work_contract_violation() {
        use crate::compiler_work::{
            CompilerWorkContractError, CompilerWorkDescriptor, ResourceRequest, TransformKind,
            TransformParameters, COMPILER_WORK_SCHEMA_VERSION,
        };

        let descriptor = CompilerWorkDescriptor {
            descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
            operation_version: "0.1.0".to_string(),
            // Names "out-a" as a semantic input, but this action's own
            // `inputs` below never declares it.
            semantic_input_artifact_ids: vec!["out-a".to_string()],
            requested_functions: vec![],
            language: None,
            contract_version: None,
            transform: Some(TransformParameters {
                kind: TransformKind::Checked,
                transform_version: "0.1.0".to_string(),
                caller: "caller".to_string(),
                callee: "callee".to_string(),
            }),
            source_provenance: None,
            test_inputs: vec![],
            resource_request: ResourceRequest::minimal(),
            budget_token: "budget-1".to_string(),
        };
        let compiler_work_action = Action {
            id: "hand-picked-id".to_string(),
            kind: ActionKind::TransformFunction,
            command_identity: "transform_function".to_string(),
            inputs: vec![], // does not declare "out-a"
            outputs: vec![ArtifactRef::declared("out-c")],
            compiler_work: Some(descriptor),
        };

        let mut input = valid_input();
        // Demand-closure pruning (this round's own fix) would otherwise
        // exclude this action entirely: nothing else in the base fixture
        // consumes its output, so it must be demanded directly.
        input.demanded_artifacts.push("out-c".to_string());
        input.actions.push(compiler_work_action.clone());
        let mut plan = valid_plan();
        plan.actions
            .insert("hand-picked-id".to_string(), compiler_work_action);
        plan.ordered_actions.push("hand-picked-id".to_string());

        // Since `id` is hand-picked (not the recomputed artifact id),
        // `WorkIdMismatch` fires before `UndeclaredSemanticInput` would --
        // still proves `validate()` itself surfaces a real
        // `CompilerWorkContractError`, not only `validate_compiler_work_action`
        // called directly.
        assert!(matches!(
            validate(&plan, &input),
            Err(ValidationError::CompilerWork(
                CompilerWorkContractError::WorkIdMismatch { .. }
            ))
        ));
    }
}

//! Issue #48 (G1) follow-up, Checkpoint C: a generic, production
//! plan-integrity verifier over a real [`PositiveClosure`] and the
//! [`DependencyResolutionInput`] it was resolved from. This is
//! deliberately not tied to this project's own fixture -- every check
//! here is a structural property any closure/plan pair must have,
//! never a check for a specific package or symbol name.
//!
//! The eight checks this module performs (issue #48's own list):
//!
//! 1. obligation `depends_on` referential integrity (no dangling edge).
//! 2. every action's own input/output identity actually exists (either
//!    a real Checkpoint A source/declared-output identity, or another
//!    action's own declared output).
//! 3. output-producer uniqueness (no two actions claim the same
//!    output).
//! 4. every `discharges` target exists and is a kind that action is
//!    actually allowed to discharge.
//! 5. action `depends_on` referential integrity (no dangling
//!    dependency).
//! 6. the action-dependency graph has no cycle.
//! 7. producer/consumer ordering: if action A consumes an identity B
//!    produces, A must actually depend on B.
//! 8. the final-link action's own inputs are exactly the retained
//!    artifact-production outputs -- no more, no fewer.

use std::collections::{BTreeMap, BTreeSet};

use crate::dependency_graph::{
    discharge_kind_allowed, ArtifactOutputKind, DependencyResolutionInput, ObligationKind,
    PositiveClosure, RequiredAction, RequiredActionKind,
};
use crate::physical_work::{validate_physical_work_contract, PhysicalWorkViolation};

/// One concrete way a plan can fail integrity -- always named with the
/// exact identities involved, never a bare "invalid plan" signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanIntegrityViolation {
    PhysicalWorkContract(PhysicalWorkViolation),
    DanglingObligationEdge {
        obligation_id: String,
        missing_dependency: String,
    },
    DanglingActionDependency {
        action_id: String,
        missing_dependency: String,
    },
    UnknownInputOrOutput {
        action_id: String,
        identity: String,
    },
    DuplicateProducer {
        output_identity: String,
        producers: Vec<String>,
    },
    UnknownDischargeTarget {
        action_id: String,
        obligation_id: String,
    },
    DischargeKindMismatch {
        action_id: String,
        obligation_id: String,
        obligation_kind: ObligationKind,
    },
    ProducerConsumerOrderViolation {
        action_id: String,
        must_depend_on: String,
        shared_identity: String,
    },
    ActionDependencyCycle {
        cycle: Vec<String>,
    },
    FinalLinkInputMismatch {
        missing: Vec<String>,
        unexpected: Vec<String>,
    },
}

/// Every action id transitively reachable from `start` by following
/// real `depends_on` edges -- used to check producer/consumer
/// ordering, since a consumer only needs *some* dependency path to its
/// producer (e.g. through an intermediate action), not necessarily a
/// direct edge, for a topological execution order to still place the
/// producer first.
fn transitive_action_dependencies<'a>(
    start: &'a str,
    by_id: &BTreeMap<&'a str, &'a RequiredAction>,
) -> BTreeSet<&'a str> {
    let mut visited = BTreeSet::new();
    let mut frontier = vec![start];
    while let Some(id) = frontier.pop() {
        if let Some(action) = by_id.get(id) {
            for dep in &action.depends_on {
                if visited.insert(dep.as_str()) {
                    frontier.push(dep.as_str());
                }
            }
        }
    }
    visited
}

/// Depth-first cycle detection over the real action `depends_on` graph.
/// Returns the first cycle found, as the sequence of action ids that
/// closes it.
fn find_action_cycle(actions: &[RequiredAction]) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Visiting,
        Done,
    }

    let by_id: BTreeMap<&str, &RequiredAction> =
        actions.iter().map(|a| (a.id.as_str(), a)).collect();
    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();
    let mut stack: Vec<&str> = Vec::new();

    fn visit<'a>(
        id: &'a str,
        by_id: &BTreeMap<&'a str, &'a RequiredAction>,
        marks: &mut BTreeMap<&'a str, Mark>,
        stack: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        match marks.get(id) {
            Some(Mark::Done) => return None,
            Some(Mark::Visiting) => {
                let start = stack.iter().position(|x| *x == id).unwrap_or(0);
                let mut cycle: Vec<String> = stack[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(id.to_string());
                return Some(cycle);
            }
            None => {}
        }
        marks.insert(id, Mark::Visiting);
        stack.push(id);
        if let Some(action) = by_id.get(id) {
            for dep in &action.depends_on {
                if let Some(cycle) = visit(dep.as_str(), by_id, marks, stack) {
                    return Some(cycle);
                }
            }
        }
        stack.pop();
        marks.insert(id, Mark::Done);
        None
    }

    for id in by_id.keys().copied() {
        if let Some(cycle) = visit(id, &by_id, &mut marks, &mut stack) {
            return Some(cycle);
        }
    }
    None
}

/// Verifies a real, production-resolved [`PositiveClosure`] against the
/// [`DependencyResolutionInput`] it was resolved from. Returns every
/// violation found (empty means the plan is fully self-consistent) --
/// never stops at the first one, so a caller sees the complete picture.
pub fn verify_plan_integrity(
    input: &DependencyResolutionInput,
    closure: &PositiveClosure,
) -> Vec<PlanIntegrityViolation> {
    let mut violations = Vec::new();

    violations.extend(
        validate_physical_work_contract(&closure.physical_work)
            .into_iter()
            .map(PlanIntegrityViolation::PhysicalWorkContract),
    );

    // 1. Obligation depends_on referential integrity.
    for obligation in closure.obligations.values() {
        for dep in &obligation.depends_on {
            if !closure.obligations.contains_key(dep) {
                violations.push(PlanIntegrityViolation::DanglingObligationEdge {
                    obligation_id: obligation.id.clone(),
                    missing_dependency: dep.clone(),
                });
            }
        }
    }

    let action_ids: BTreeSet<&str> = closure
        .required_actions
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    // Source identities appear in action inputs in their obligation-id
    // form (`SourceModule:<id>`, matching `dependency_graph`'s own
    // `source_module_id`), while declared outputs appear by their own
    // bare id -- both are real Checkpoint A identities, just in two
    // different id spaces.
    let known_identities: BTreeSet<String> = input
        .sources
        .iter()
        .map(|s| format!("SourceModule:{}", s.id))
        .chain(input.declared_outputs.iter().map(|o| o.id.clone()))
        .collect();

    let mut output_producers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for action in &closure.required_actions {
        for output in &action.outputs {
            output_producers
                .entry(output.as_str())
                .or_default()
                .push(action.id.as_str());
        }
    }
    // 3. Output-producer uniqueness.
    for (output, producers) in &output_producers {
        if producers.len() > 1 {
            violations.push(PlanIntegrityViolation::DuplicateProducer {
                output_identity: output.to_string(),
                producers: producers.iter().map(|s| s.to_string()).collect(),
            });
        }
    }

    let actions_by_id: BTreeMap<&str, &RequiredAction> = closure
        .required_actions
        .iter()
        .map(|a| (a.id.as_str(), a))
        .collect();

    for action in &closure.required_actions {
        // 2. Input/output identity existence.
        for identity in action.inputs.iter().chain(action.outputs.iter()) {
            if !known_identities.contains(identity.as_str())
                && !output_producers.contains_key(identity.as_str())
            {
                violations.push(PlanIntegrityViolation::UnknownInputOrOutput {
                    action_id: action.id.clone(),
                    identity: identity.clone(),
                });
            }
        }
        // 4. Discharge target existence + kind consistency.
        for discharge in &action.discharges {
            match closure.obligations.get(discharge) {
                None => violations.push(PlanIntegrityViolation::UnknownDischargeTarget {
                    action_id: action.id.clone(),
                    obligation_id: discharge.clone(),
                }),
                Some(obligation) => {
                    if !discharge_kind_allowed(action.kind, obligation.kind) {
                        violations.push(PlanIntegrityViolation::DischargeKindMismatch {
                            action_id: action.id.clone(),
                            obligation_id: discharge.clone(),
                            obligation_kind: obligation.kind,
                        });
                    }
                }
            }
        }
        // 5. Action depends_on referential integrity.
        for dep in &action.depends_on {
            if !action_ids.contains(dep.as_str()) {
                violations.push(PlanIntegrityViolation::DanglingActionDependency {
                    action_id: action.id.clone(),
                    missing_dependency: dep.clone(),
                });
            }
        }
        // 7. Producer/consumer ordering -- a *transitive* dependency
        // path to the producer is enough (e.g. via an intermediate
        // action); only the complete absence of one is a real
        // ordering violation.
        let reachable = transitive_action_dependencies(action.id.as_str(), &actions_by_id);
        for input_id in &action.inputs {
            if let Some(producers) = output_producers.get(input_id.as_str()) {
                for producer in producers {
                    if *producer != action.id && !reachable.contains(producer) {
                        violations.push(PlanIntegrityViolation::ProducerConsumerOrderViolation {
                            action_id: action.id.clone(),
                            must_depend_on: producer.to_string(),
                            shared_identity: input_id.clone(),
                        });
                    }
                }
            }
        }
    }

    // 6. Action-dependency-graph acyclicity.
    if let Some(cycle) = find_action_cycle(&closure.required_actions) {
        violations.push(PlanIntegrityViolation::ActionDependencyCycle { cycle });
    }

    // 8. Final-link input == retained artifact set.
    if let Some(link_action) = closure
        .required_actions
        .iter()
        .find(|a| a.kind == RequiredActionKind::LinkNativeExecutable)
    {
        let native_executable_output_ids: BTreeSet<&str> = input
            .declared_outputs
            .iter()
            .filter(|o| o.kind == ArtifactOutputKind::NativeExecutable)
            .map(|o| o.id.as_str())
            .collect();
        let retained_artifact_outputs: BTreeSet<&str> = closure
            .obligations
            .values()
            .filter(|o| o.kind == ObligationKind::ArtifactProduction && !o.state.is_rejected())
            .filter_map(|o| o.id.strip_prefix("ArtifactProduction:"))
            .filter(|id| !native_executable_output_ids.contains(id))
            .collect();
        let link_inputs: BTreeSet<&str> = link_action.inputs.iter().map(|s| s.as_str()).collect();
        if retained_artifact_outputs != link_inputs {
            violations.push(PlanIntegrityViolation::FinalLinkInputMismatch {
                missing: retained_artifact_outputs
                    .difference(&link_inputs)
                    .map(|s| s.to_string())
                    .collect(),
                unexpected: link_inputs
                    .difference(&retained_artifact_outputs)
                    .map(|s| s.to_string())
                    .collect(),
            });
        }
    }

    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency_graph::{
        resolve, ArtifactOutputFacts, Ecosystem, FfiExportFacts, FfiRequirementFacts,
        LoweringRequirementFacts, PackageCandidateFacts, Role, RuntimeRequirementFacts,
        SourceModuleFacts,
    };

    /// A minimal but real positive input covering all four ecosystems,
    /// mirroring `dependency_graph::tests::minimal_input` -- built here
    /// too rather than shared, since that helper is private to its own
    /// test module and this module verifies a genuinely independent
    /// concern (plan structure, not resolution logic).
    fn minimal_input() -> DependencyResolutionInput {
        DependencyResolutionInput {
            demand_entry_point: "app".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            host_toolchain_id: "nim-2.2.10".to_string(),
            sources: vec![
                SourceModuleFacts {
                    id: "app/src/main.rs".to_string(),
                    ecosystem: Ecosystem::Cargo,
                    package_id: "app".to_string(),
                },
                SourceModuleFacts {
                    id: "nimble/doubler/src/doubler.nim".to_string(),
                    ecosystem: Ecosystem::Nimble,
                    package_id: "doubler".to_string(),
                },
                SourceModuleFacts {
                    id: "c/cadd/v1/cadd.c".to_string(),
                    ecosystem: Ecosystem::C,
                    package_id: "cadd".to_string(),
                },
                SourceModuleFacts {
                    id: "c/cadd/v1/cadd.h".to_string(),
                    ecosystem: Ecosystem::C,
                    package_id: "cadd".to_string(),
                },
            ],
            package_candidates: vec![
                PackageCandidateFacts {
                    ecosystem: Ecosystem::Cargo,
                    package_id: "app".to_string(),
                    version: "0.1.0".to_string(),
                    role: Role::Target,
                    target_triple: "x86_64-unknown-linux-gnu".to_string(),
                    sources: vec!["app/src/main.rs".to_string()],
                    declared_exports: vec![],
                    declared_constraints: vec![],
                },
                PackageCandidateFacts {
                    ecosystem: Ecosystem::Nimble,
                    package_id: "doubler".to_string(),
                    version: "0.1.0".to_string(),
                    role: Role::Target,
                    target_triple: "x86_64-unknown-linux-gnu".to_string(),
                    sources: vec!["nimble/doubler/src/doubler.nim".to_string()],
                    declared_exports: vec![FfiExportFacts {
                        declaring_source: "nimble/doubler/src/doubler.nim".to_string(),
                        symbol: "nim_double".to_string(),
                        abi: "C".to_string(),
                        param_count: 1,
                        return_type: "cint".to_string(),
                    }],
                    declared_constraints: vec![],
                },
                PackageCandidateFacts {
                    ecosystem: Ecosystem::C,
                    package_id: "cadd".to_string(),
                    version: "1.0.0".to_string(),
                    role: Role::Target,
                    target_triple: "x86_64-unknown-linux-gnu".to_string(),
                    sources: vec![
                        "c/cadd/v1/cadd.c".to_string(),
                        "c/cadd/v1/cadd.h".to_string(),
                    ],
                    declared_exports: vec![FfiExportFacts {
                        declaring_source: "c/cadd/v1/cadd.h".to_string(),
                        symbol: "c_add".to_string(),
                        abi: "C".to_string(),
                        param_count: 2,
                        return_type: "int".to_string(),
                    }],
                    declared_constraints: vec![],
                },
            ],
            ffi_requirements: vec![
                FfiRequirementFacts {
                    declaring_source: "app/src/main.rs".to_string(),
                    symbol: "nim_double".to_string(),
                    abi: "C".to_string(),
                    param_count: 1,
                    return_type: "i32".to_string(),
                    expected_provider_package: "doubler".to_string(),
                },
                FfiRequirementFacts {
                    declaring_source: "app/src/main.rs".to_string(),
                    symbol: "c_add".to_string(),
                    abi: "C".to_string(),
                    param_count: 2,
                    return_type: "i32".to_string(),
                    expected_provider_package: "cadd".to_string(),
                },
            ],
            lowering_requirements: vec![
                LoweringRequirementFacts {
                    package_id: "app".to_string(),
                    source_id: "app/src/main.rs".to_string(),
                    description: "Rust application lowering-feasible for target".to_string(),
                },
                LoweringRequirementFacts {
                    package_id: "doubler".to_string(),
                    source_id: "nimble/doubler/src/doubler.nim".to_string(),
                    description: "Nim package lowering-feasible for target".to_string(),
                },
            ],
            abi_constraints: vec![],
            declared_outputs: vec![
                ArtifactOutputFacts {
                    id: "object:app".to_string(),
                    package_id: "app".to_string(),
                    kind: ArtifactOutputKind::RustObject,
                    source_ids: vec![],
                },
                ArtifactOutputFacts {
                    id: "archive:doubler".to_string(),
                    package_id: "doubler".to_string(),
                    kind: ArtifactOutputKind::NimStaticLibrary,
                    source_ids: vec![],
                },
                ArtifactOutputFacts {
                    id: "archive:cadd".to_string(),
                    package_id: "cadd".to_string(),
                    kind: ArtifactOutputKind::CStaticArchive,
                    source_ids: vec![],
                },
                ArtifactOutputFacts {
                    id: "object:cadd".to_string(),
                    package_id: "cadd".to_string(),
                    kind: ArtifactOutputKind::CObject,
                    source_ids: vec![],
                },
                ArtifactOutputFacts {
                    id: "executable:app".to_string(),
                    package_id: "app".to_string(),
                    kind: ArtifactOutputKind::NativeExecutable,
                    source_ids: vec![],
                },
            ],
            runtime_requirements: vec![RuntimeRequirementFacts {
                target_triple: "x86_64-unknown-linux-gnu".to_string(),
                description: "OS ABI/dynamic loader contract".to_string(),
            }],
        }
    }

    /// The verifier accepts a genuinely valid, production-resolved plan
    /// with zero violations -- checked first so every negative case
    /// below is a *minimal* mutation of something already known-good.
    #[test]
    fn a_real_valid_plan_has_no_integrity_violations() {
        let input = minimal_input();
        let closure = resolve(&input).expect("must resolve");
        let violations = verify_plan_integrity(&input, &closure);
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:#?}"
        );
    }

    #[test]
    fn production_integrity_gate_rejects_a_disconnected_physical_work_map() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        closure.physical_work.units.retain(|unit| {
            unit.id
                != crate::physical_work::WorkUnitId::Logical(
                    crate::physical_work::LogicalActivity::N9ExportRegistration,
                )
        });

        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations.iter().any(|violation| matches!(
            violation,
            PlanIntegrityViolation::PhysicalWorkContract(
                PhysicalWorkViolation::MissingLogicalActivity {
                    activity: crate::physical_work::LogicalActivity::N9ExportRegistration
                }
            )
        )));
    }

    #[test]
    fn a_dangling_obligation_edge_is_rejected() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        closure
            .obligations
            .get_mut("Symbol:c_add")
            .unwrap()
            .depends_on
            .push("Nonexistent:obligation".to_string());
        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations
            .iter()
            .any(|v| matches!(v, PlanIntegrityViolation::DanglingObligationEdge { missing_dependency, .. } if missing_dependency == "Nonexistent:obligation")));
    }

    #[test]
    fn a_dangling_action_dependency_is_rejected() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        closure.required_actions[0]
            .depends_on
            .push("nonexistent-action".to_string());
        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations
            .iter()
            .any(|v| matches!(v, PlanIntegrityViolation::DanglingActionDependency { missing_dependency, .. } if missing_dependency == "nonexistent-action")));
    }

    #[test]
    fn an_unknown_source_or_output_identity_is_rejected() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        closure.required_actions[0]
            .inputs
            .push("nonexistent:identity".to_string());
        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations
            .iter()
            .any(|v| matches!(v, PlanIntegrityViolation::UnknownInputOrOutput { identity, .. } if identity == "nonexistent:identity")));
    }

    #[test]
    fn a_duplicate_producer_is_rejected() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        let stolen_output = closure.required_actions[0].outputs[0].clone();
        closure.required_actions[1]
            .outputs
            .push(stolen_output.clone());
        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations
            .iter()
            .any(|v| matches!(v, PlanIntegrityViolation::DuplicateProducer { output_identity, .. } if *output_identity == stolen_output)));
    }

    #[test]
    fn an_unknown_discharge_target_is_rejected() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        closure.required_actions[0]
            .discharges
            .push("Nonexistent:obligation".to_string());
        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations
            .iter()
            .any(|v| matches!(v, PlanIntegrityViolation::UnknownDischargeTarget { obligation_id, .. } if obligation_id == "Nonexistent:obligation")));
    }

    #[test]
    fn a_reversed_producer_consumer_order_is_rejected() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        let link_index = closure
            .required_actions
            .iter()
            .position(|a| a.kind == RequiredActionKind::LinkNativeExecutable)
            .expect("must have a link action");
        // Remove the link action's own dependency on the Rust-object
        // compile action, while it still consumes that action's real
        // output -- a genuine producer/consumer order violation.
        let rust_object_action_id = closure
            .required_actions
            .iter()
            .find(|a| a.kind == RequiredActionKind::CompileRustObject)
            .expect("must have a compile-rust-object action")
            .id
            .clone();
        closure.required_actions[link_index]
            .depends_on
            .retain(|d| *d != rust_object_action_id);
        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations
            .iter()
            .any(|v| matches!(v, PlanIntegrityViolation::ProducerConsumerOrderViolation { must_depend_on, .. } if *must_depend_on == rust_object_action_id)));
    }

    #[test]
    fn a_dependency_cycle_is_rejected() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        let link_index = closure
            .required_actions
            .iter()
            .position(|a| a.kind == RequiredActionKind::LinkNativeExecutable)
            .expect("must have a link action");
        let link_id = closure.required_actions[link_index].id.clone();
        let rust_object_index = closure
            .required_actions
            .iter()
            .position(|a| a.kind == RequiredActionKind::CompileRustObject)
            .expect("must have a compile-rust-object action");
        // The link action already depends (transitively or directly)
        // on the Rust-object action; making the Rust-object action
        // depend back on the link action closes a real cycle.
        closure.required_actions[rust_object_index]
            .depends_on
            .push(link_id);
        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations
            .iter()
            .any(|v| matches!(v, PlanIntegrityViolation::ActionDependencyCycle { .. })));
    }

    #[test]
    fn a_final_link_input_mismatch_is_rejected() {
        let input = minimal_input();
        let mut closure = resolve(&input).expect("must resolve");
        let link_index = closure
            .required_actions
            .iter()
            .position(|a| a.kind == RequiredActionKind::LinkNativeExecutable)
            .expect("must have a link action");
        closure.required_actions[link_index]
            .inputs
            .push("archive:extraneous".to_string());
        let violations = verify_plan_integrity(&input, &closure);
        assert!(violations
            .iter()
            .any(|v| matches!(v, PlanIntegrityViolation::FinalLinkInputMismatch { unexpected, .. } if unexpected.contains(&"archive:extraneous".to_string()))));
    }
}

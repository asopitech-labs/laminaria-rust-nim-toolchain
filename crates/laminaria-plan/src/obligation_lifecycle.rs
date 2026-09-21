//! Versioned production dependency-obligation lifecycle (issue #88).
//!
//! A [`ProductionGraph`] is the publication boundary shared by resolution
//! and execution. It owns one closure, append-only state histories, and the
//! causal operations that justify terminal decisions. Possessing a file or
//! merely planning an action is never publication evidence.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::dependency_graph::{
    DischargeKind, LifecycleViolation, ObligationKind, ObligationState, PositiveClosure,
    RequiredActionKind,
};

pub const OBLIGATION_CONTRACT_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    Succeeded,
    Failed,
    Cancelled,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducedFact {
    pub identity: String,
    pub digest: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEvidence {
    pub operation_id: String,
    pub action_id: String,
    pub action_kind: RequiredActionKind,
    pub run_id: String,
    pub producer_identity: String,
    pub sequence: u64,
    pub inputs: Vec<String>,
    pub outputs: Vec<ProducedFact>,
    pub outcome: OperationOutcome,
    pub structurally_verified: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionEvidence {
    pub operation_id: String,
    pub run_id: String,
    pub producer_identity: String,
    pub sequence: u64,
    pub input_identities: Vec<String>,
    pub resolved_obligations: Vec<String>,
    pub outcome: OperationOutcome,
    pub structurally_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeContractEvidence {
    pub contract_id: String,
    pub operation_id: String,
    pub run_id: String,
    pub producer_identity: String,
    pub sequence: u64,
    pub target_triple: String,
    pub loader_requirement: String,
    pub required_providers: Vec<RuntimeProviderEvidence>,
    pub version_requirement: String,
    pub verification: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeProviderEvidence {
    pub provider_id: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrrelevanceProof {
    pub proof_id: String,
    pub run_id: String,
    pub producer_identity: String,
    pub sequence: u64,
    pub roots: Vec<String>,
    pub examined_edges_digest: String,
    pub no_observable_side_effect: bool,
    pub no_external_contract: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cause", rename_all = "snake_case")]
pub enum TransitionCause {
    Resolution {
        operation_id: String,
    },
    Action {
        operation_id: String,
        action_id: String,
    },
    RuntimeContract {
        contract_id: String,
    },
    ReachabilityProof {
        proof_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateTransition {
    pub sequence: u64,
    pub from: ObligationState,
    pub to: ObligationState,
    pub cause: TransitionCause,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationDecision {
    pub obligation_id: String,
    pub kind: ObligationKind,
    pub state: ObligationState,
    pub history: Vec<StateTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub contract_version: u32,
    pub requested_artifact: ProducedFact,
    pub decisions: Vec<ObligationDecision>,
    pub operations: Vec<OperationEvidence>,
    pub runtime_contracts: Vec<RuntimeContractEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleError {
    IdentityMismatch {
        map_key: String,
        obligation_id: String,
    },
    DuplicateOperation(String),
    UnknownAction(String),
    ProducerIdentityMismatch,
    ActionKindMismatch,
    ActionInputsMismatch,
    ActionOutputsMismatch,
    ActionDependencyNotCompleted {
        action_id: String,
        dependency: String,
    },
    InvalidOperationEvidence(String),
    InvalidResolutionEvidence(String),
    InvalidRuntimeContract(String),
    InvalidIrrelevanceProof(String),
    InvalidTransition {
        obligation_id: String,
        from: ObligationState,
        to: ObligationState,
    },
    Transition(LifecycleViolation),
    PublicationBlocked(Vec<PublicationBlocker>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum PublicationBlocker {
    NoNativeDemand,
    UnclosedObligation {
        obligation_id: String,
        state: ObligationState,
    },
    ReachableRejectedObligation {
        obligation_id: String,
    },
    ReachableProvenIrrelevantObligation {
        obligation_id: String,
    },
    MissingCausalOperation {
        obligation_id: String,
        action_id: String,
    },
    MissingArtifactEvidence {
        artifact_identity: String,
    },
    InvalidHistory {
        obligation_id: String,
    },
}

impl From<LifecycleViolation> for LifecycleError {
    fn from(value: LifecycleViolation) -> Self {
        Self::Transition(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionGraph {
    pub contract_version: u32,
    pub closure: PositiveClosure,
    pub histories: BTreeMap<String, Vec<StateTransition>>,
    pub operations: Vec<OperationEvidence>,
    pub resolution_operations: Vec<ResolutionEvidence>,
    pub runtime_contracts: Vec<RuntimeContractEvidence>,
    pub irrelevance_proofs: Vec<IrrelevanceProof>,
}

impl ProductionGraph {
    fn validate_operation(
        &self,
        evidence: &OperationEvidence,
    ) -> Result<crate::dependency_graph::RequiredAction, LifecycleError> {
        if evidence.operation_id.is_empty()
            || evidence.run_id.is_empty()
            || evidence.producer_identity.is_empty()
            || evidence.detail.is_empty()
            || evidence.outcome != OperationOutcome::Succeeded
            || !evidence.structurally_verified
        {
            return Err(LifecycleError::InvalidOperationEvidence(
                evidence.operation_id.clone(),
            ));
        }
        if self.operation_id_used(&evidence.operation_id) || self.sequence_used(evidence.sequence) {
            return Err(LifecycleError::DuplicateOperation(
                evidence.operation_id.clone(),
            ));
        }
        let action = self
            .closure
            .required_actions
            .iter()
            .find(|action| action.id == evidence.action_id)
            .cloned()
            .ok_or_else(|| LifecycleError::UnknownAction(evidence.action_id.clone()))?;
        if action.kind != evidence.action_kind {
            return Err(LifecycleError::ActionKindMismatch);
        }
        if action.toolchain != evidence.producer_identity {
            return Err(LifecycleError::ProducerIdentityMismatch);
        }
        if as_set(&action.inputs) != as_set(&evidence.inputs) {
            return Err(LifecycleError::ActionInputsMismatch);
        }
        let output_ids: Vec<_> = evidence
            .outputs
            .iter()
            .map(|output| output.identity.clone())
            .collect();
        if as_set(&action.outputs) != as_set(&output_ids)
            || evidence
                .outputs
                .iter()
                .any(|output| output.digest.is_empty() || output.size_bytes == 0)
        {
            return Err(LifecycleError::ActionOutputsMismatch);
        }
        for dependency in &action.depends_on {
            if !self
                .operations
                .iter()
                .any(|operation| operation.action_id == *dependency)
            {
                return Err(LifecycleError::ActionDependencyNotCompleted {
                    action_id: action.id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
        Ok(action)
    }

    pub fn new(closure: PositiveClosure) -> Result<Self, LifecycleError> {
        for (key, obligation) in &closure.obligations {
            if key != &obligation.id {
                return Err(LifecycleError::IdentityMismatch {
                    map_key: key.clone(),
                    obligation_id: obligation.id.clone(),
                });
            }
        }
        Ok(Self {
            contract_version: OBLIGATION_CONTRACT_VERSION,
            closure,
            histories: BTreeMap::new(),
            operations: Vec::new(),
            resolution_operations: Vec::new(),
            runtime_contracts: Vec::new(),
            irrelevance_proofs: Vec::new(),
        })
    }

    fn sequence_used(&self, sequence: u64) -> bool {
        self.operations.iter().any(|e| e.sequence == sequence)
            || self
                .resolution_operations
                .iter()
                .any(|e| e.sequence == sequence)
            || self
                .runtime_contracts
                .iter()
                .any(|e| e.sequence == sequence)
            || self
                .irrelevance_proofs
                .iter()
                .any(|e| e.sequence == sequence)
    }

    fn operation_id_used(&self, id: &str) -> bool {
        self.operations.iter().any(|e| e.operation_id == id)
            || self
                .resolution_operations
                .iter()
                .any(|e| e.operation_id == id)
    }

    fn append_transition(
        &mut self,
        obligation_id: &str,
        sequence: u64,
        from: ObligationState,
        to: ObligationState,
        cause: TransitionCause,
    ) -> Result<(), LifecycleError> {
        let history = self.histories.entry(obligation_id.to_owned()).or_default();
        if history
            .last()
            .is_some_and(|last| last.to != from || last.sequence >= sequence)
        {
            return Err(LifecycleError::InvalidTransition {
                obligation_id: obligation_id.to_owned(),
                from,
                to,
            });
        }
        history.push(StateTransition {
            sequence,
            from,
            to,
            cause,
        });
        Ok(())
    }

    /// Records the resolver decision for every retained obligation. Resolver-
    /// owned facts close here; action-owned facts remain selected/satisfied.
    pub fn record_resolution(
        &mut self,
        evidence: ResolutionEvidence,
    ) -> Result<(), LifecycleError> {
        if evidence.operation_id.is_empty()
            || evidence.run_id.is_empty()
            || evidence.producer_identity.is_empty()
            || evidence.outcome != OperationOutcome::Succeeded
            || !evidence.structurally_verified
            || evidence.resolved_obligations.is_empty()
        {
            return Err(LifecycleError::InvalidResolutionEvidence(
                evidence.operation_id,
            ));
        }
        if self.operation_id_used(&evidence.operation_id) || self.sequence_used(evidence.sequence) {
            return Err(LifecycleError::DuplicateOperation(evidence.operation_id));
        }
        let ids: BTreeSet<_> = evidence.resolved_obligations.iter().cloned().collect();
        if ids.len() != evidence.resolved_obligations.len() {
            return Err(LifecycleError::InvalidResolutionEvidence(
                evidence.operation_id,
            ));
        }
        for id in &evidence.resolved_obligations {
            let obligation = self
                .closure
                .obligations
                .get(id)
                .ok_or_else(|| LifecycleError::InvalidResolutionEvidence(id.clone()))?;
            if !matches!(
                obligation.state,
                ObligationState::Selected | ObligationState::Satisfied
            ) || (obligation.required_action.is_none()
                && !matches!(
                    obligation.kind,
                    ObligationKind::NativeExecutableDemand
                        | ObligationKind::PackageSelection
                        | ObligationKind::SourceModule
                        | ObligationKind::SemanticFfi
                        | ObligationKind::Lowering
                        | ObligationKind::AbiTarget
                        | ObligationKind::Symbol
                ))
            {
                return Err(LifecycleError::InvalidTransition {
                    obligation_id: id.clone(),
                    from: obligation.state,
                    to: ObligationState::Discharged,
                });
            }
        }
        for id in &evidence.resolved_obligations {
            let from = self.closure.obligations[id].state;
            let obligation = self
                .closure
                .obligations
                .get_mut(id)
                .expect("validated above");
            let to = if obligation.required_action.is_some() {
                from
            } else {
                obligation.state = ObligationState::Discharged;
                obligation.discharge_kind = Some(if obligation.kind == ObligationKind::Lowering {
                    DischargeKind::Lowered
                } else {
                    DischargeKind::Specialized
                });
                obligation.evidence =
                    Some(format!("resolution operation {}", evidence.operation_id));
                ObligationState::Discharged
            };
            self.append_transition(
                id,
                evidence.sequence,
                ObligationState::Unresolved,
                to,
                TransitionCause::Resolution {
                    operation_id: evidence.operation_id.clone(),
                },
            )?;
        }
        self.resolution_operations.push(evidence);
        Ok(())
    }

    pub fn record_operation(
        &mut self,
        evidence: OperationEvidence,
        discharge_kind: DischargeKind,
    ) -> Result<(), LifecycleError> {
        let action = self.validate_operation(&evidence)?;
        for obligation_id in &action.discharges {
            let from = self
                .closure
                .obligations
                .get(obligation_id)
                .ok_or_else(|| LifecycleError::InvalidOperationEvidence(obligation_id.clone()))?
                .state;
            self.closure.discharge_obligation(
                obligation_id,
                &action.id,
                discharge_kind,
                format!("operation {}: {}", evidence.operation_id, evidence.detail),
            )?;
            self.append_transition(
                obligation_id,
                evidence.sequence,
                from,
                ObligationState::Discharged,
                TransitionCause::Action {
                    operation_id: evidence.operation_id.clone(),
                    action_id: action.id.clone(),
                },
            )?;
        }
        self.operations.push(evidence);
        Ok(())
    }

    pub fn record_runtime_operation(
        &mut self,
        operation: OperationEvidence,
        contract: RuntimeContractEvidence,
    ) -> Result<(), LifecycleError> {
        let action = self.validate_operation(&operation)?;
        if action.kind != RequiredActionKind::PreflightRuntimeContract
            || action.discharges.len() != 1
            || contract.operation_id != operation.operation_id
            || contract.run_id != operation.run_id
            || contract.producer_identity != operation.producer_identity
            || contract.sequence <= operation.sequence
        {
            return Err(LifecycleError::InvalidRuntimeContract(contract.contract_id));
        }
        let mut staged = self.clone();
        staged.operations.push(operation);
        staged.externalize_runtime(&action.discharges[0], contract)?;
        *self = staged;
        Ok(())
    }

    pub fn externalize_runtime(
        &mut self,
        obligation_id: &str,
        evidence: RuntimeContractEvidence,
    ) -> Result<(), LifecycleError> {
        if evidence.contract_id.is_empty()
            || evidence.operation_id.is_empty()
            || evidence.run_id.is_empty()
            || evidence.producer_identity.is_empty()
            || evidence.target_triple.is_empty()
            || evidence.loader_requirement.is_empty()
            || evidence.required_providers.is_empty()
            || evidence.required_providers.iter().any(|provider| {
                provider.provider_id.is_empty()
                    || provider.path.as_ref().is_some_and(|path| path.is_empty())
            })
            || evidence.version_requirement.is_empty()
            || evidence.verification.is_empty()
            || self.sequence_used(evidence.sequence)
        {
            return Err(LifecycleError::InvalidRuntimeContract(evidence.contract_id));
        }
        let obligation = self
            .closure
            .obligations
            .get(obligation_id)
            .ok_or_else(|| LifecycleError::InvalidRuntimeContract(obligation_id.to_owned()))?;
        if obligation.kind != ObligationKind::Runtime {
            return Err(LifecycleError::InvalidRuntimeContract(
                obligation_id.to_owned(),
            ));
        }
        let from = obligation.state;
        self.closure.externalize_obligation(
            obligation_id,
            format!(
                "runtime contract {}: {}; {}; {}",
                evidence.contract_id,
                evidence.loader_requirement,
                evidence.version_requirement,
                evidence.verification
            ),
        )?;
        let history_from = if self.histories.contains_key(obligation_id) {
            from
        } else {
            ObligationState::Unresolved
        };
        self.append_transition(
            obligation_id,
            evidence.sequence,
            history_from,
            ObligationState::Externalized,
            TransitionCause::RuntimeContract {
                contract_id: evidence.contract_id.clone(),
            },
        )?;
        self.runtime_contracts.push(evidence);
        Ok(())
    }

    pub fn prove_irrelevant(
        &mut self,
        obligation_id: &str,
        evidence: IrrelevanceProof,
    ) -> Result<(), LifecycleError> {
        if evidence.proof_id.is_empty()
            || evidence.run_id.is_empty()
            || evidence.producer_identity.is_empty()
            || evidence.roots.is_empty()
            || evidence.examined_edges_digest.is_empty()
            || !evidence.no_observable_side_effect
            || !evidence.no_external_contract
            || self.sequence_used(evidence.sequence)
        {
            return Err(LifecycleError::InvalidIrrelevanceProof(evidence.proof_id));
        }
        if self.reachable_obligations().contains(obligation_id) {
            return Err(LifecycleError::InvalidIrrelevanceProof(
                obligation_id.to_owned(),
            ));
        }
        let obligation = self
            .closure
            .obligations
            .get_mut(obligation_id)
            .ok_or_else(|| LifecycleError::InvalidIrrelevanceProof(obligation_id.to_owned()))?;
        let from = obligation.state;
        if !matches!(
            from,
            ObligationState::Unresolved | ObligationState::Selected | ObligationState::Satisfied
        ) {
            return Err(LifecycleError::InvalidTransition {
                obligation_id: obligation_id.to_owned(),
                from,
                to: ObligationState::ProvenIrrelevant,
            });
        }
        obligation.state = ObligationState::ProvenIrrelevant;
        obligation.discharge_kind = Some(DischargeKind::ProvenIrrelevant);
        obligation.evidence = Some(format!("reachability proof {}", evidence.proof_id));
        let history_from = if self.histories.contains_key(obligation_id) {
            from
        } else {
            ObligationState::Unresolved
        };
        self.append_transition(
            obligation_id,
            evidence.sequence,
            history_from,
            ObligationState::ProvenIrrelevant,
            TransitionCause::ReachabilityProof {
                proof_id: evidence.proof_id.clone(),
            },
        )?;
        self.irrelevance_proofs.push(evidence);
        Ok(())
    }

    /// The sole production-artifact publication gate.
    pub fn publication_manifest(
        &self,
        requested_artifact: ProducedFact,
    ) -> Result<ArtifactManifest, LifecycleError> {
        let reachable = self.reachable_obligations();
        let mut blockers = Vec::new();
        if reachable.is_empty() {
            blockers.push(PublicationBlocker::NoNativeDemand);
        }
        for (id, obligation) in &self.closure.obligations {
            if reachable.contains(id) {
                match obligation.state {
                    ObligationState::Rejected => {
                        blockers.push(PublicationBlocker::ReachableRejectedObligation {
                            obligation_id: id.clone(),
                        })
                    }
                    ObligationState::ProvenIrrelevant => {
                        blockers.push(PublicationBlocker::ReachableProvenIrrelevantObligation {
                            obligation_id: id.clone(),
                        })
                    }
                    ObligationState::Discharged | ObligationState::Externalized => {}
                    state => blockers.push(PublicationBlocker::UnclosedObligation {
                        obligation_id: id.clone(),
                        state,
                    }),
                }
            }
            if let Some(action_id) = &obligation.required_action {
                if obligation.state == ObligationState::Discharged
                    && !self
                        .operations
                        .iter()
                        .any(|operation| operation.action_id == *action_id)
                {
                    blockers.push(PublicationBlocker::MissingCausalOperation {
                        obligation_id: id.clone(),
                        action_id: action_id.clone(),
                    });
                }
            }
            if let Some(history) = self.histories.get(id) {
                if history
                    .first()
                    .is_some_and(|first| first.from != ObligationState::Unresolved)
                    || history.windows(2).any(|pair| {
                        pair[0].to != pair[1].from || pair[0].sequence >= pair[1].sequence
                    })
                    || history
                        .last()
                        .is_some_and(|last| last.to != obligation.state)
                {
                    blockers.push(PublicationBlocker::InvalidHistory {
                        obligation_id: id.clone(),
                    });
                }
            } else if matches!(
                obligation.state,
                ObligationState::Discharged
                    | ObligationState::Externalized
                    | ObligationState::ProvenIrrelevant
            ) {
                blockers.push(PublicationBlocker::InvalidHistory {
                    obligation_id: id.clone(),
                });
            }
        }
        if requested_artifact.digest.is_empty()
            || requested_artifact.size_bytes == 0
            || !self.operations.iter().any(|operation| {
                operation
                    .outputs
                    .iter()
                    .any(|output| output == &requested_artifact)
            })
        {
            blockers.push(PublicationBlocker::MissingArtifactEvidence {
                artifact_identity: requested_artifact.identity.clone(),
            });
        }
        if !blockers.is_empty() {
            return Err(LifecycleError::PublicationBlocked(blockers));
        }
        let decisions = self
            .closure
            .obligations
            .values()
            .map(|obligation| ObligationDecision {
                obligation_id: obligation.id.clone(),
                kind: obligation.kind,
                state: obligation.state,
                history: self
                    .histories
                    .get(&obligation.id)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        Ok(ArtifactManifest {
            contract_version: self.contract_version,
            requested_artifact,
            decisions,
            operations: self.operations.clone(),
            runtime_contracts: self.runtime_contracts.clone(),
        })
    }

    fn reachable_obligations(&self) -> BTreeSet<String> {
        let mut reachable = BTreeSet::new();
        let mut pending: Vec<String> = self
            .closure
            .obligations
            .values()
            .filter(|obligation| obligation.kind == ObligationKind::NativeExecutableDemand)
            .map(|obligation| obligation.id.clone())
            .collect();
        while let Some(id) = pending.pop() {
            if !reachable.insert(id.clone()) {
                continue;
            }
            if let Some(obligation) = self.closure.obligations.get(&id) {
                pending.extend(obligation.depends_on.iter().cloned());
            }
        }
        reachable
    }
}

fn as_set(values: &[String]) -> BTreeSet<&str> {
    values.iter().map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency_graph::{Ecosystem, Obligation, RequiredAction, Role};

    fn obligation(
        id: &str,
        kind: ObligationKind,
        depends_on: Vec<&str>,
        state: ObligationState,
        required_action: Option<&str>,
    ) -> Obligation {
        Obligation {
            id: id.to_owned(),
            kind,
            ecosystem: Ecosystem::Cross,
            role: Role::Target,
            depends_on: depends_on.into_iter().map(str::to_owned).collect(),
            state,
            discharge_kind: None,
            required_action: required_action.map(str::to_owned),
            rejection: None,
            evidence: Some("real resolver fact".to_owned()),
        }
    }

    fn graph() -> ProductionGraph {
        let obligations = BTreeMap::from([
            (
                "demand".to_owned(),
                obligation(
                    "demand",
                    ObligationKind::NativeExecutableDemand,
                    vec!["artifact", "runtime"],
                    ObligationState::Satisfied,
                    None,
                ),
            ),
            (
                "artifact".to_owned(),
                obligation(
                    "artifact",
                    ObligationKind::ArtifactProduction,
                    vec![],
                    ObligationState::Satisfied,
                    Some("link"),
                ),
            ),
            (
                "runtime".to_owned(),
                obligation(
                    "runtime",
                    ObligationKind::Runtime,
                    vec![],
                    ObligationState::Satisfied,
                    None,
                ),
            ),
            (
                "unused".to_owned(),
                obligation(
                    "unused",
                    ObligationKind::Symbol,
                    vec![],
                    ObligationState::Satisfied,
                    None,
                ),
            ),
        ]);
        ProductionGraph::new(PositiveClosure {
            obligations,
            required_actions: vec![RequiredAction {
                id: "link".to_owned(),
                kind: RequiredActionKind::LinkNativeExecutable,
                target_triple: "x86_64-unknown-linux-gnu".to_owned(),
                role: Role::Target,
                toolchain: "ld@verified".to_owned(),
                inputs: vec!["input-object".to_owned()],
                outputs: vec!["bin:app".to_owned()],
                discharges: vec!["artifact".to_owned()],
                depends_on: vec![],
            }],
            rejected_alternatives: BTreeMap::new(),
        })
        .unwrap()
    }

    fn operation(outcome: OperationOutcome) -> OperationEvidence {
        OperationEvidence {
            operation_id: "op-link".to_owned(),
            action_id: "link".to_owned(),
            action_kind: RequiredActionKind::LinkNativeExecutable,
            run_id: "run-1".to_owned(),
            producer_identity: "ld@verified".to_owned(),
            sequence: 2,
            inputs: vec!["input-object".to_owned()],
            outputs: vec![ProducedFact {
                identity: "bin:app".to_owned(),
                digest: "sha256:abcd".to_owned(),
                size_bytes: 42,
            }],
            outcome,
            structurally_verified: true,
            detail: "link output inspected and executable".to_owned(),
        }
    }

    fn close_non_action_obligations(graph: &mut ProductionGraph) {
        graph
            .record_resolution(ResolutionEvidence {
                operation_id: "op-resolve".to_owned(),
                run_id: "run-1".to_owned(),
                producer_identity: "resolver@sha256:beef".to_owned(),
                sequence: 1,
                input_identities: vec!["demand-input".to_owned()],
                resolved_obligations: vec!["demand".to_owned(), "artifact".to_owned()],
                outcome: OperationOutcome::Succeeded,
                structurally_verified: true,
            })
            .unwrap();
        graph
            .externalize_runtime(
                "runtime",
                RuntimeContractEvidence {
                    contract_id: "glibc-loader".to_owned(),
                    operation_id: "op-runtime-preflight".to_owned(),
                    run_id: "run-1".to_owned(),
                    producer_identity: "runtime-preflight@sha256:cafe".to_owned(),
                    sequence: 3,
                    target_triple: "x86_64-unknown-linux-gnu".to_owned(),
                    loader_requirement: "ld-linux-x86-64.so.2".to_owned(),
                    required_providers: vec![RuntimeProviderEvidence {
                        provider_id: "libc.so.6".to_owned(),
                        path: Some("/lib/libc.so.6".to_owned()),
                    }],
                    version_requirement: "GLIBC >= 2.31".to_owned(),
                    verification: "loader and version inspected".to_owned(),
                },
            )
            .unwrap();
        graph
            .prove_irrelevant(
                "unused",
                IrrelevanceProof {
                    proof_id: "proof-unused".to_owned(),
                    run_id: "run-1".to_owned(),
                    producer_identity: "reachability@sha256:f00d".to_owned(),
                    sequence: 4,
                    roots: vec!["demand".to_owned()],
                    examined_edges_digest: "sha256:edges".to_owned(),
                    no_observable_side_effect: true,
                    no_external_contract: true,
                },
            )
            .unwrap();
    }

    #[test]
    fn complete_causal_graph_publishes_a_manifest_with_the_same_artifact_identity() {
        let mut graph = graph();
        close_non_action_obligations(&mut graph);
        graph
            .record_operation(
                operation(OperationOutcome::Succeeded),
                DischargeKind::StaticallyLinked,
            )
            .unwrap();
        let artifact = operation(OperationOutcome::Succeeded).outputs.remove(0);
        let manifest = graph.publication_manifest(artifact.clone()).unwrap();
        assert_eq!(manifest.contract_version, OBLIGATION_CONTRACT_VERSION);
        assert_eq!(manifest.requested_artifact, artifact);
        assert_eq!(manifest.runtime_contracts, graph.runtime_contracts);
        assert!(manifest.decisions.iter().all(|decision| matches!(
            decision.state,
            ObligationState::Discharged
                | ObligationState::Externalized
                | ObligationState::ProvenIrrelevant
        )));
    }

    #[test]
    fn cancelled_or_mismatched_operation_never_creates_terminal_success() {
        let mut cancelled = graph();
        assert!(matches!(
            cancelled.record_operation(
                operation(OperationOutcome::Cancelled),
                DischargeKind::StaticallyLinked
            ),
            Err(LifecycleError::InvalidOperationEvidence(_))
        ));
        assert_eq!(
            cancelled.closure.obligations["artifact"].state,
            ObligationState::Satisfied
        );

        let mut mismatched = graph();
        let mut evidence = operation(OperationOutcome::Succeeded);
        evidence.outputs[0].identity = "bin:somebody-else".to_owned();
        assert_eq!(
            mismatched.record_operation(evidence, DischargeKind::StaticallyLinked),
            Err(LifecycleError::ActionOutputsMismatch)
        );
        assert_eq!(
            mismatched.closure.obligations["artifact"].state,
            ObligationState::Satisfied
        );

        let mut forged = graph();
        let mut evidence = operation(OperationOutcome::Succeeded);
        evidence.producer_identity = "some-other-linker".to_owned();
        assert_eq!(
            forged.record_operation(evidence, DischargeKind::StaticallyLinked),
            Err(LifecycleError::ProducerIdentityMismatch)
        );
        assert_eq!(
            forged.closure.obligations["artifact"].state,
            ObligationState::Satisfied
        );
    }

    #[test]
    fn publication_is_blocked_before_any_manifest_exists_when_an_obligation_is_open() {
        let graph = graph();
        let artifact = operation(OperationOutcome::Succeeded).outputs.remove(0);
        let error = graph.publication_manifest(artifact).unwrap_err();
        let LifecycleError::PublicationBlocked(blockers) = error else {
            panic!("expected structured publication blockers")
        };
        assert!(blockers.iter().any(|blocker| matches!(
            blocker,
            PublicationBlocker::UnclosedObligation { obligation_id, .. } if obligation_id == "artifact"
        )));
        assert!(blockers
            .iter()
            .any(|blocker| matches!(blocker, PublicationBlocker::MissingArtifactEvidence { .. })));
    }

    #[test]
    fn duplicate_identity_and_contradictory_second_transition_are_rejected() {
        let mut graph = graph();
        graph
            .record_operation(
                operation(OperationOutcome::Succeeded),
                DischargeKind::StaticallyLinked,
            )
            .unwrap();
        assert!(matches!(
            graph.record_operation(
                operation(OperationOutcome::Succeeded),
                DischargeKind::StaticallyLinked
            ),
            Err(LifecycleError::DuplicateOperation(_))
        ));
    }

    #[test]
    fn every_obligation_kind_rejects_a_kind_invalid_terminal_transition() {
        let kinds = [
            ObligationKind::NativeExecutableDemand,
            ObligationKind::PackageSelection,
            ObligationKind::SourceModule,
            ObligationKind::SemanticFfi,
            ObligationKind::Lowering,
            ObligationKind::AbiTarget,
            ObligationKind::ArtifactProduction,
            ObligationKind::Symbol,
            ObligationKind::LinkOrder,
            ObligationKind::FinalLink,
            ObligationKind::Runtime,
            ObligationKind::Provenance,
        ];
        for kind in kinds {
            let id = format!("kind-{kind:?}");
            let closure = PositiveClosure {
                obligations: BTreeMap::from([(
                    id.clone(),
                    obligation(&id, kind, vec![], ObligationState::Satisfied, None),
                )]),
                required_actions: vec![],
                rejected_alternatives: BTreeMap::new(),
            };
            let mut graph = ProductionGraph::new(closure).unwrap();
            let error = if kind == ObligationKind::Runtime {
                graph
                    .record_resolution(ResolutionEvidence {
                        operation_id: format!("invalid-resolution-{kind:?}"),
                        run_id: "run-invalid-transition".to_owned(),
                        producer_identity: "resolver@verified".to_owned(),
                        sequence: 1,
                        input_identities: vec!["input".to_owned()],
                        resolved_obligations: vec![id.clone()],
                        outcome: OperationOutcome::Succeeded,
                        structurally_verified: true,
                    })
                    .unwrap_err()
            } else {
                graph
                    .externalize_runtime(
                        &id,
                        RuntimeContractEvidence {
                            contract_id: format!("invalid-runtime-{kind:?}"),
                            operation_id: "invalid-runtime-operation".to_owned(),
                            run_id: "run-invalid-transition".to_owned(),
                            producer_identity: "runtime-preflight@verified".to_owned(),
                            sequence: 1,
                            target_triple: "x86_64-unknown-linux-gnu".to_owned(),
                            loader_requirement: "loader".to_owned(),
                            required_providers: vec![RuntimeProviderEvidence {
                                provider_id: "provider".to_owned(),
                                path: Some("/provider".to_owned()),
                            }],
                            version_requirement: "version".to_owned(),
                            verification: "verified".to_owned(),
                        },
                    )
                    .unwrap_err()
            };
            assert!(
                matches!(
                    error,
                    LifecycleError::InvalidTransition { .. }
                        | LifecycleError::InvalidRuntimeContract(_)
                ),
                "{kind:?} admitted an invalid terminal transition: {error:?}"
            );
            assert_eq!(
                graph.closure.obligations[&id].state,
                ObligationState::Satisfied
            );
            assert!(!graph.histories.contains_key(&id));
        }
    }
}

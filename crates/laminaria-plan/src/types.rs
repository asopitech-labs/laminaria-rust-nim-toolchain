//! Versioned JSON contract for LAMINARIA's Nim Planning Kernel (issue #8):
//! `plan(PlanningInput) -> ExecutionPlan`, per
//! `docs/research-foundations.md` section 7's naming.
//!
//! This module is the Rust-owned source of truth for the contract;
//! `nim-planner/src/contract.nim` mirrors it field-for-field by hand (see
//! that module's own doc comment) since there is no shared schema
//! generator -- every JSON key here is chosen to match the Nim side
//! exactly. See `docs/self-build.md` for the full protocol and which
//! design choice came from which studied reference project
//! (`.reference/{buck2,bazel,pants,nx}`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::compiler_work::CompilerWorkDescriptor;

/// Bumped 0.1.0 -> 0.2.0 for issue #27's B: four new `ActionKind`
/// variants and `Action`'s new optional `compiler_work` field. Backward
/// compatible on the wire (every existing field/variant is unchanged, the
/// new field is omitted entirely when absent -- see
/// `compiler_work::tests::an_action_with_no_compiler_work_omits_the_field_entirely`),
/// but bumped anyway so a producer/consumer pair that has not been
/// updated together is still caught by the existing schema-version gate
/// (`nim-planner/src/contract.nim`'s `planFromJson`) rather than silently
/// running with a partially-understood contract.
pub const PLAN_SCHEMA_VERSION: &str = "0.2.0";
pub const PRODUCED_BY: &str = "laminaria-nim-planning-kernel";

/// An external, pre-existing input (`Source`, a leaf with no producing
/// action -- the same role a `SourceArtifact` plays in Buck2/Bazel's own
/// artifact model) or a logical artifact id (`Declared`) that some
/// action in the same `PlanningInput` must declare as one of its
/// `outputs`. Dependency edges are derived entirely from matching
/// `Declared` inputs against declared outputs -- there is deliberately
/// no hand-written `depends_on` list (Buck2's `BuildArtifact` model,
/// `app/buck2_artifact/src/artifact/build_artifact.rs` in
/// `.reference/buck2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtifactRef {
    Source { path: String },
    Declared { artifact_id: String },
}

impl ArtifactRef {
    pub fn source(path: impl Into<String>) -> Self {
        ArtifactRef::Source { path: path.into() }
    }

    pub fn declared(artifact_id: impl Into<String>) -> Self {
        ArtifactRef::Declared {
            artifact_id: artifact_id.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    NimBuild,
    CargoBuild,
    Integrate,
    /// Issue #27 B's own compiler-work kinds: LAMINARIA's owned pipeline
    /// (`laminaria-ir::rust_frontend`/`nim_frontend`,
    /// `laminaria-ir::transform`), not an existing compiler/backend
    /// invocation -- carries a [`CompilerWorkDescriptor`] on `Action`.
    /// `docs/compiler-ownership-contract.md` governs this: none of these
    /// four ever shell out to `rustc`/`nim`/`llc`.
    LowerSource,
    ValidateIr,
    TransformFunction,
    EvaluateEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub kind: ActionKind,
    pub command_identity: String,
    pub inputs: Vec<ArtifactRef>,
    pub outputs: Vec<ArtifactRef>,
    /// Present only for `ActionKind::LowerSource`/`ValidateIr`/
    /// `TransformFunction`/`EvaluateEvidence` -- `None` (and omitted from
    /// the wire entirely, never emitted as a `null`) for every existing
    /// delegated-build action kind, so this field's addition changes no
    /// byte of the JSON `laminaria-planner` already produces for those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_work: Option<CompilerWorkDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningInput {
    pub schema_version: String,
    /// Artifact ids the caller actually wants produced -- recorded for
    /// evidence only in this first slice; the self-build's `PlanningInput`
    /// always declares every action needed, so demand-driven pruning
    /// (issue #8's variant-explosion scope) is not implemented yet.
    pub demanded_artifacts: Vec<String>,
    pub actions: Vec<Action>,
}

impl PlanningInput {
    pub fn new(demanded_artifacts: Vec<String>, actions: Vec<Action>) -> Self {
        PlanningInput {
            schema_version: PLAN_SCHEMA_VERSION.to_string(),
            demanded_artifacts,
            actions,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub schema_version: String,
    /// Always `PRODUCED_BY` on a plan that genuinely came out of the Nim
    /// kernel -- `validate::validate` checks this so a plan that didn't
    /// actually come from `laminaria-planner` can never be silently
    /// accepted as a substitute (issue #8: "a Rust replacement planner...
    /// cannot satisfy this slice").
    pub produced_by: String,
    pub producer_version: String,
    /// A structural (non-cryptographic) hash of the canonicalized input,
    /// for lineage/evidence recording only -- see
    /// `nim-planner/src/planning_kernel.nim`'s `computePlanId` doc
    /// comment for why this is an honest LAMINARIA-specific addition,
    /// not a copied idiom from any of the four studied reference
    /// projects (none of which keep a single whole-graph digest as
    /// their primary node identity).
    pub plan_id: String,
    /// A deterministic topological order over `actions`' ids.
    pub ordered_actions: Vec<String>,
    pub actions: BTreeMap<String, Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReasonKind {
    Cycle,
    UnsupportedInput,
    MissingProducer,
    DuplicateProducer,
    InvalidContractVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRejection {
    pub schema_version: String,
    pub reason_kind: RejectionReasonKind,
    pub reason_detail: String,
    /// Populated only when `reason_kind == Cycle`, rendered the same way
    /// Nx's `findCycle` reports one (`a -> b -> c -> a`,
    /// `packages/nx/src/tasks-runner/task-graph-utils.ts` in
    /// `.reference/nx`) -- empty for every other reason kind.
    #[serde(default)]
    pub cycle_path: Vec<String>,
}

/// The top-level answer from `laminaria-planner`'s stdout: either a
/// well-formed `ExecutionPlan` or a well-formed, structured
/// `PlanRejection` -- both are valid, complete answers to "can this be
/// planned?" (issue #8's own acceptance criteria ask for cycles/
/// unsupported input/version mismatches to be *rejected*, not crashed
/// on). Adjacently tagged (`{"outcome": "...", "data": ...}`) to match
/// `nim-planner/src/contract.nim`'s `toJson(PlanOutcome)` exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub enum PlanOutcome {
    Planned(ExecutionPlan),
    Rejected(PlanRejection),
}

impl PlanOutcome {
    pub fn is_planned(&self) -> bool {
        matches!(self, PlanOutcome::Planned(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_ref_round_trips_through_the_exact_wire_shape_nim_produces() {
        let source = ArtifactRef::source("nim-planner/src");
        let json = serde_json::to_value(&source).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "source", "path": "nim-planner/src"})
        );

        let declared = ArtifactRef::declared("planner-bin");
        let json = serde_json::to_value(&declared).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "declared", "artifact_id": "planner-bin"})
        );
    }

    #[test]
    fn plan_outcome_deserializes_the_literal_json_the_nim_binary_emits() {
        // A literal capture of `laminaria-planner`'s real stdout for a
        // one-action plan, not a value re-serialized by this crate --
        // this is what actually guards against the two sides silently
        // drifting apart.
        let raw = r#"{"outcome":"planned","data":{"schema_version":"0.1.0","produced_by":"laminaria-nim-planning-kernel","producer_version":"0.1.0","plan_id":"823b06624fdefe69","ordered_actions":["a"],"actions":{"a":{"id":"a","kind":"nim_build","command_identity":"x","inputs":[],"outputs":[{"kind":"declared","artifact_id":"a"}]}}}}"#;
        let outcome: PlanOutcome = serde_json::from_str(raw).unwrap();
        match outcome {
            PlanOutcome::Planned(plan) => {
                assert_eq!(plan.produced_by, PRODUCED_BY);
                assert_eq!(plan.ordered_actions, vec!["a".to_string()]);
                assert_eq!(plan.actions.len(), 1);
                assert_eq!(plan.actions["a"].kind, ActionKind::NimBuild);
            }
            PlanOutcome::Rejected(_) => panic!("expected Planned"),
        }
    }

    #[test]
    fn plan_outcome_deserializes_a_real_rejection() {
        let raw = r#"{"outcome":"rejected","data":{"schema_version":"0.1.0","reason_kind":"cycle","reason_detail":"cyclic dependency detected: a -> b -> a","cycle_path":["a","b","a"]}}"#;
        let outcome: PlanOutcome = serde_json::from_str(raw).unwrap();
        match outcome {
            PlanOutcome::Rejected(rejection) => {
                assert_eq!(rejection.reason_kind, RejectionReasonKind::Cycle);
                assert_eq!(
                    rejection.cycle_path,
                    vec!["a".to_string(), "b".to_string(), "a".to_string()]
                );
            }
            PlanOutcome::Planned(_) => panic!("expected Rejected"),
        }
    }
}

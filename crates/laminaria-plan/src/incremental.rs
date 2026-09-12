//! Issue #36 T0's accepted session-scoped incremental-planning wire
//! protocol (`docs/design/issue-36-t0-incremental-contract.md`, commit
//! `9d730ec`), §3/§4/§6.4. This is the Rust-owned source of truth;
//! `nim-planner/src/incremental_contract.nim` mirrors it field-for-field
//! by hand, the same convention `crate::types`/`nim-planner/src/
//! contract.nim` already use (see that module's own doc comment).
//!
//! Reuses the existing, unmodified [`crate::types::PlanningInput`]/
//! [`crate::types::Action`]/[`crate::types::RejectionReasonKind`] as-is
//! (T0 §3.1: `StartSession.initial_graph` is a plain `PlanningInput`, no
//! new input schema) -- only the session-scoped commands/responses/
//! events around them are new.
//!
//! JSON shape: every command/response/event is one flat JSON object with
//! an internal `kind` tag (`#[serde(tag = "kind", rename_all =
//! "snake_case")]`), envelope fields (`schema_version`/`session_id`/
//! `command_index`, etc.) repeated on every variant rather than
//! factored out via `#[serde(flatten)]` -- this is a deliberate,
//! simpler encoding choice than the T0 doc's own YAML sketch (which drew
//! the envelope and the `kind`-tagged payload as textually separate),
//! chosen because it matches Nim's own natural `case kind` object-variant
//! shape (`incremental_contract.nim`) exactly (shared fields *outside*
//! the variant, payload fields *inside* it) with no flatten-related
//! serde edge cases to reason about on either side of the hand-mirrored
//! boundary.

use serde::{Deserialize, Serialize};

use crate::types::{Action, PlanningInput, RejectionReasonKind};

pub const INCREMENTAL_PROTOCOL_SCHEMA_VERSION: &str = "0.1.0";
pub const INCREMENTAL_EVENT_SCHEMA_VERSION: &str = "0.1.0";

/// T0 §3.4: `artifact_id`/`requested_by` pair naming one demand -- used
/// both for `StartSession.initial_demands` and `DemandRequested`/
/// `DemandCancelled`/`DependencyDiscovered.new_demands`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemandReference {
    pub artifact_id: String,
    pub requested_by: String,
}

/// T0 §3.4 (second-revision fix#3): pairs an existing, still-active
/// `old_action_id` with one of the same event's `new_actions` that
/// replaces it -- `Action`s are never mutated in place; a changed
/// semantic dependency is always a new, recomputed-id `Action` plus a
/// `Supersession`, never an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Supersession {
    pub old_action_id: String,
    pub new_action_id: String,
}

/// T0 §3.4. Crosses the Nim IPC boundary directly (`ApplyDelta`'s
/// payload) -- the first-revision design that kept this Rust-internal
/// was exactly the P1 the T0 review caught ("increment coordination had
/// regressed into Rust-internal state").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningEvent {
    pub event_id: String,
    pub sequence_number: u64,
    /// The generation the sender believed was current when this event
    /// was assembled -- Nim's own staleness input (T0 §4.2).
    pub planning_generation: u64,
    /// `u64`, not `u128` (the T0 doc's own informal sketch used `u128`):
    /// `serde_json`'s buffered `Content` deserializer, which the
    /// `#[serde(flatten)]` + internally-tagged `kind` combo below forces
    /// it to go through, does not support `u128`/`i128` at all (a real,
    /// concrete failure this module's own test caught -- not a
    /// theoretical concern). `u64` nanoseconds since the epoch is valid
    /// until the year 2554, comfortably enough for this wire field.
    pub emitted_at_unix_ns: u64,
    #[serde(flatten)]
    pub kind: PlanningEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanningEventKind {
    /// T0 §3.4 (second-revision fix#1/#3): `new_actions` is always
    /// non-empty -- a discovery that introduces nothing is not a real
    /// discovery. `supersessions` is empty when every `new_actions`
    /// entry is a genuinely fresh leaf with no existing counterpart.
    DependencyDiscovered {
        discovering_action_id: String,
        new_actions: Vec<Action>,
        #[serde(default)]
        supersessions: Vec<Supersession>,
        #[serde(default)]
        new_demands: Vec<DemandReference>,
    },
    ProducerCompleted {
        artifact_id: String,
        produced_by_action_id: String,
    },
    ProducerFailed {
        artifact_id: String,
        produced_by_action_id: String,
        failure_reason: String,
    },
    DemandRequested {
        artifact_id: String,
        requested_by: String,
    },
    DemandCancelled {
        artifact_id: String,
        requested_by: String,
    },
}

/// T0 §6.1 (second-revision refinement): the 6 states Nim itself tracks
/// as graph operations. `running` is deliberately *not* one of them --
/// it is a Rust-side execution-status overlay Nim never sees or reports
/// (§6.1's own "観測点を分離する" note).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Discovered,
    BlockedDependency,
    Ready,
    Completed,
    Failed,
    Cancelled,
}

/// T0 §3.2. `to_state: None` means this action was retired by a
/// `Supersession` (`retired_because_superseded_by` is then always
/// `Some`); `from_state: None` means this action is appearing in the
/// graph for the first time this delta (`new_action` is then always
/// `Some`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionStateChange {
    pub action_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub from_state: Option<ActionState>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub to_state: Option<ActionState>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub retired_because_superseded_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub new_action: Option<Action>,
}

/// T0 §6.4: a Rust/Nim-observed diagnostic distinct from the existing,
/// unchanged 5-variant [`RejectionReasonKind`] -- these accompany a
/// successfully-applied `PlanDelta` (the event *was* accepted), never a
/// hard `Rejected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncrementalDiagnosticReason {
    StaleGeneration,
    CancelledResult,
    DependencyFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IncrementalPlannerCommand {
    StartSession {
        schema_version: String,
        session_id: String,
        command_index: u64,
        initial_graph: PlanningInput,
        #[serde(default)]
        initial_demands: Vec<DemandReference>,
    },
    ApplyDelta {
        schema_version: String,
        session_id: String,
        command_index: u64,
        event: PlanningEvent,
    },
    CloseSession {
        schema_version: String,
        session_id: String,
        command_index: u64,
    },
}

impl IncrementalPlannerCommand {
    pub fn command_index(&self) -> u64 {
        match self {
            IncrementalPlannerCommand::StartSession { command_index, .. }
            | IncrementalPlannerCommand::ApplyDelta { command_index, .. }
            | IncrementalPlannerCommand::CloseSession { command_index, .. } => *command_index,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IncrementalPlannerResponse {
    PlanDelta {
        schema_version: String,
        session_id: String,
        in_reply_to_command_index: u64,
        planning_generation: u64,
        changed_actions: Vec<ActionStateChange>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        diagnostic: Option<IncrementalDiagnosticReason>,
    },
    Rejected {
        schema_version: String,
        session_id: String,
        in_reply_to_command_index: u64,
        reason_kind: RejectionReasonKind,
        reason_detail: String,
        #[serde(default)]
        cycle_path: Vec<String>,
    },
    SessionClosed {
        schema_version: String,
        session_id: String,
        in_reply_to_command_index: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, ActionKind, ArtifactRef};

    fn sample_action(id: &str) -> Action {
        Action {
            id: id.to_string(),
            kind: ActionKind::LowerSource,
            command_identity: "lower_source".to_string(),
            inputs: vec![ArtifactRef::source("f.rs")],
            outputs: vec![ArtifactRef::declared(id)],
            compiler_work: None,
        }
    }

    #[test]
    fn start_session_round_trips_and_tags_as_snake_case() {
        let cmd = IncrementalPlannerCommand::StartSession {
            schema_version: INCREMENTAL_PROTOCOL_SCHEMA_VERSION.to_string(),
            session_id: "s1".to_string(),
            command_index: 0,
            initial_graph: PlanningInput::new(vec![], vec![sample_action("a")]),
            initial_demands: vec![DemandReference {
                artifact_id: "a".to_string(),
                requested_by: "consumer-a".to_string(),
            }],
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["kind"], "start_session");
        assert_eq!(json["session_id"], "s1");
        let round_tripped: IncrementalPlannerCommand = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, cmd);
    }

    #[test]
    fn apply_delta_dependency_discovered_round_trips_with_empty_supersessions_omitted_on_decode_default(
    ) {
        let event = PlanningEvent {
            event_id: "e1".to_string(),
            sequence_number: 1,
            planning_generation: 0,
            emitted_at_unix_ns: 1,
            kind: PlanningEventKind::DependencyDiscovered {
                discovering_action_id: "discover".to_string(),
                new_actions: vec![sample_action("a")],
                supersessions: vec![],
                new_demands: vec![],
            },
        };
        let cmd = IncrementalPlannerCommand::ApplyDelta {
            schema_version: INCREMENTAL_PROTOCOL_SCHEMA_VERSION.to_string(),
            session_id: "s1".to_string(),
            command_index: 1,
            event,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let round_tripped: IncrementalPlannerCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, cmd);
    }

    #[test]
    fn plan_delta_response_omits_diagnostic_when_none() {
        let resp = IncrementalPlannerResponse::PlanDelta {
            schema_version: INCREMENTAL_PROTOCOL_SCHEMA_VERSION.to_string(),
            session_id: "s1".to_string(),
            in_reply_to_command_index: 0,
            planning_generation: 0,
            changed_actions: vec![ActionStateChange {
                action_id: "a".to_string(),
                from_state: None,
                to_state: Some(ActionState::Ready),
                retired_because_superseded_by: None,
                new_action: Some(sample_action("a")),
            }],
            diagnostic: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("diagnostic").is_none());
        assert_eq!(json["changed_actions"][0]["to_state"], "ready");
        assert!(json["changed_actions"][0].get("from_state").is_none());
    }

    #[test]
    fn rejected_response_round_trips() {
        let resp = IncrementalPlannerResponse::Rejected {
            schema_version: INCREMENTAL_PROTOCOL_SCHEMA_VERSION.to_string(),
            session_id: "s1".to_string(),
            in_reply_to_command_index: 2,
            reason_kind: RejectionReasonKind::Cycle,
            reason_detail: "a -> b -> a".to_string(),
            cycle_path: vec!["a".to_string(), "b".to_string(), "a".to_string()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let round_tripped: IncrementalPlannerResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, resp);
    }

    #[test]
    fn action_state_change_retirement_shape() {
        let change = ActionStateChange {
            action_id: "old".to_string(),
            from_state: Some(ActionState::Ready),
            to_state: None,
            retired_because_superseded_by: Some("new".to_string()),
            new_action: None,
        };
        let json = serde_json::to_value(&change).unwrap();
        assert!(json.get("to_state").is_none());
        assert_eq!(json["retired_because_superseded_by"], "new");
    }
}

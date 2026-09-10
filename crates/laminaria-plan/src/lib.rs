//! `PlanningInput -> ExecutionPlan` contract and Nim Planning Kernel
//! subprocess client for LAMINARIA (issue #8), the Rust half of
//! `nim-planner/`. See `docs/self-build.md` for the full protocol.

pub mod nim_planner_client;
pub mod types;
pub mod validate;

pub use nim_planner_client::{
    call_default_planner, call_planner, find_planner_binary, PlannerCallError,
};
pub use types::{
    Action, ActionKind, ArtifactRef, ExecutionPlan, PlanOutcome, PlanRejection, PlanningInput,
    RejectionReasonKind, PLAN_SCHEMA_VERSION, PRODUCED_BY,
};
pub use validate::{validate, ValidationError};

//! `PlanningInput -> ExecutionPlan` contract and Nim Planning Kernel
//! subprocess client for LAMINARIA (issue #8), the Rust half of
//! `nim-planner/`. See `docs/self-build.md` for the full protocol.

pub mod compiler_work;
pub mod nim_planner_client;
pub mod types;
pub mod validate;

pub use compiler_work::{
    discover_source_dependencies_artifact_id, evaluate_evidence_artifact_id,
    lower_source_artifact_id, transform_function_artifact_id, validate_ir_artifact_id,
    CompilerWorkDescriptor, ResourceRequest, SourceProvenanceRef, TransformKind,
    TransformParameters, COMPILER_WORK_SCHEMA_VERSION,
};
pub use nim_planner_client::{
    call_default_planner, call_planner, find_planner_binary, PlannerCallError,
};
pub use types::{
    Action, ActionKind, ArtifactRef, ExecutionPlan, PlanOutcome, PlanRejection, PlanningInput,
    RejectionReasonKind, PLAN_SCHEMA_VERSION, PRODUCED_BY,
};
pub use validate::{validate, ValidationError};

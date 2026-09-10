//! Issue #27 stage C's first slice: a real, owned in-process executor
//! connecting `laminaria-ir`'s own frontends/transforms/validator/
//! interpreter to a production-Nim-planner-produced, Rust-validated
//! `ExecutionPlan` -- `LowerSource -> ValidateIr -> TransformFunction ->
//! ValidateIr -> EvaluateEvidence`, dispatched sequentially in
//! `ordered_actions` order for this first slice. Concurrency, CPU-budget
//! admission control, memory accounting, and cancellation are the
//! *next*, separate stage-C work (its own "終了試験PR"), deliberately not
//! attempted here -- this slice's own acceptance is the vertical path
//! itself actually running end to end, judged only after issue #27's
//! boundary-contract fixes landed (`laminaria_plan::compiler_work`'s
//! `validate_compiler_work_action`, `laminaria_ir::validate`).
//!
//! **No external compiler fallback anywhere in this module.** Every
//! dispatch arm calls directly into `laminaria_ir`'s own owned logic
//! (`rust_frontend`/`nim_frontend`/`transform`/`validate`/`interpreter`);
//! a dispatch failure is a hard `Err`, never a silent substitute the way
//! `laminaria_run::self_build`/`project_build`'s *delegated*-build
//! executors legitimately shell out to real `cargo`/`nim` (a separate
//! role -- see this crate's own module docs for that boundary).
//!
//! **`ValidatedProgram` is required at this executor's own boundary**:
//! `TransformFunction` and `EvaluateEvidence` dispatch both read their
//! input program from [`ArtifactStore`]'s *validated* store only, never
//! the raw candidate store a `LowerSource`/`TransformFunction` action's
//! own output lands in first -- issue #27 A2's own "未検証IRを
//! executorが黙って実行してはならない" is enforced here by an actual
//! executor, not merely documented as an intention.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use laminaria_ir::diagnostics::Diagnostic;
use laminaria_ir::interpreter::{eval_function, EvalOutcome};
use laminaria_ir::nim_frontend::lower_nim_source;
use laminaria_ir::rust_frontend::lower_rust_source;
use laminaria_ir::transform::anf_insert::anf_insert;
use laminaria_ir::transform::checked_inline::checked_inline;
use laminaria_ir::types::Program;
use laminaria_ir::validate::{validate_program, ProgramValidationError, ValidatedProgram};
use laminaria_plan::compiler_work::{CompilerWorkDescriptor, TransformKind};
use laminaria_plan::{Action, ActionKind, ExecutionPlan};

#[derive(Debug)]
pub enum CompilerWorkExecutionError {
    /// `action.kind` is a compiler-work kind but `action.compiler_work`
    /// is absent -- should already be impossible for a plan that passed
    /// `laminaria_plan::validate::validate`, checked again here as a
    /// defensive boundary rather than an `unwrap`.
    MissingDescriptor {
        action_id: String,
    },
    /// `action.kind` is not one of the four compiler-work kinds this
    /// executor knows how to run -- this executor never falls back to
    /// running a delegated-build action; that stays
    /// `self_build`/`project_build`'s own separate role.
    UnsupportedActionKind {
        action_id: String,
        kind: ActionKind,
    },
    UnsupportedLanguage {
        action_id: String,
        language: String,
    },
    MissingSourceProvenance {
        action_id: String,
    },
    SourceReadFailed {
        action_id: String,
        path: String,
        detail: String,
    },
    LoweringFailed {
        action_id: String,
        diagnostics: Vec<Diagnostic>,
    },
    MissingSemanticInput {
        action_id: String,
    },
    /// `ValidateIr`'s own semantic input names an artifact id this store
    /// has no *candidate* Program for -- either it was never produced, or
    /// (issue #27's own dependency-ready ordering) it hasn't run yet.
    MissingCandidateInput {
        action_id: String,
        artifact_id: String,
    },
    ValidationFailed {
        action_id: String,
        detail: ProgramValidationError,
    },
    /// `TransformFunction`/`EvaluateEvidence`'s own semantic input names
    /// an artifact id this store has no *validated* Program for -- this
    /// executor never reads from the candidate store for these two kinds,
    /// so an unvalidated (or altogether absent) upstream artifact is
    /// always this error, never a silent fallback to the raw candidate.
    MissingValidatedInput {
        action_id: String,
        artifact_id: String,
    },
    MissingTransformParameters {
        action_id: String,
    },
    TransformFailed {
        action_id: String,
        detail: String,
    },
    MissingRequestedFunctionToEvaluate {
        action_id: String,
    },
    MissingTestInputs {
        action_id: String,
    },
    EvaluationFailed {
        action_id: String,
        detail: String,
    },
}

impl std::fmt::Display for CompilerWorkExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingDescriptor { action_id } => {
                write!(f, "action {action_id:?} has no compiler_work descriptor")
            }
            Self::UnsupportedActionKind { action_id, kind } => write!(
                f,
                "action {action_id:?} has kind {kind:?}, which this compiler-work executor does \
                 not dispatch -- delegated-build kinds stay self_build's/project_build's own role"
            ),
            Self::UnsupportedLanguage {
                action_id,
                language,
            } => write!(
                f,
                "action {action_id:?}'s LowerSource descriptor names language {language:?}, \
                 which this executor does not know how to lower"
            ),
            Self::MissingSourceProvenance { action_id } => write!(
                f,
                "action {action_id:?}'s LowerSource descriptor has no source_provenance"
            ),
            Self::SourceReadFailed {
                action_id,
                path,
                detail,
            } => write!(
                f,
                "action {action_id:?} could not read source file {path:?}: {detail}"
            ),
            Self::LoweringFailed {
                action_id,
                diagnostics,
            } => write!(f, "action {action_id:?}'s lowering failed: {diagnostics:?}"),
            Self::MissingSemanticInput { action_id } => write!(
                f,
                "action {action_id:?}'s descriptor has no semantic_input_artifact_ids entry"
            ),
            Self::MissingCandidateInput {
                action_id,
                artifact_id,
            } => write!(
                f,
                "action {action_id:?} names semantic input {artifact_id:?}, which this store has \
                 no candidate Program for"
            ),
            Self::ValidationFailed { action_id, detail } => {
                write!(f, "action {action_id:?}'s ValidateIr failed: {detail}")
            }
            Self::MissingValidatedInput {
                action_id,
                artifact_id,
            } => write!(
                f,
                "action {action_id:?} names semantic input {artifact_id:?}, which this store has \
                 no *validated* Program for -- an unvalidated candidate is never substituted"
            ),
            Self::MissingTransformParameters { action_id } => write!(
                f,
                "action {action_id:?}'s TransformFunction descriptor has no transform parameters"
            ),
            Self::TransformFailed { action_id, detail } => {
                write!(f, "action {action_id:?}'s transform failed: {detail}")
            }
            Self::MissingRequestedFunctionToEvaluate { action_id } => write!(
                f,
                "action {action_id:?}'s EvaluateEvidence descriptor names no function to \
                 evaluate (requested_functions is empty)"
            ),
            Self::MissingTestInputs { action_id } => write!(
                f,
                "action {action_id:?}'s EvaluateEvidence descriptor has no test_inputs"
            ),
            Self::EvaluationFailed { action_id, detail } => {
                write!(f, "action {action_id:?}'s evaluation failed: {detail}")
            }
        }
    }
}

impl std::error::Error for CompilerWorkExecutionError {}

/// The Rust-side in-memory artifact store for `laminaria-ir`'s own
/// values -- issue #27 B's own "IR payloadは初期sliceではRust側の
/// in-memory storeで管理"; the Nim planner never sees any of this, only
/// the descriptor/dependency metadata already round-tripped through it.
///
/// Three separate maps, not one, so a consumer can never accidentally
/// read an unvalidated `Program` through the same lookup a validated one
/// would resolve through -- see this module's own top-level doc comment.
#[derive(Default)]
pub struct ArtifactStore {
    candidates: HashMap<String, Program>,
    validated: HashMap<String, ValidatedProgram>,
    evidence: HashMap<String, Vec<EvalOutcome>>,
}

impl ArtifactStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn candidate_program(&self, artifact_id: &str) -> Option<&Program> {
        self.candidates.get(artifact_id)
    }

    pub fn validated_program(&self, artifact_id: &str) -> Option<&ValidatedProgram> {
        self.validated.get(artifact_id)
    }

    pub fn evidence(&self, artifact_id: &str) -> Option<&[EvalOutcome]> {
        self.evidence.get(artifact_id).map(Vec::as_slice)
    }
}

/// Dispatches every action in `plan.ordered_actions`, in order --
/// sequential for this first slice (issue #27 stage C's own concurrency/
/// resource-accounting/cancellation work is a separate, later PR, not
/// this one). `plan` should already have passed
/// `laminaria_plan::validate::validate` (which itself calls
/// `validate_compiler_work_action` on every action) -- this function does
/// not re-validate the plan's own structural/contract well-formedness,
/// only ever reads artifacts through [`ArtifactStore`]'s own typed
/// accessors, so it cannot silently substitute an unvalidated `Program`
/// where a validated one is required regardless.
pub fn run_compiler_work_plan(
    plan: &ExecutionPlan,
    store: &mut ArtifactStore,
) -> Result<(), CompilerWorkExecutionError> {
    for action_id in &plan.ordered_actions {
        let action = plan
            .actions
            .get(action_id)
            .expect("validate() already checked every ordered_actions id exists");
        dispatch_action(action, store)?;
    }
    Ok(())
}

fn dispatch_action(
    action: &Action,
    store: &mut ArtifactStore,
) -> Result<(), CompilerWorkExecutionError> {
    // Kind checked *before* descriptor presence: a legacy delegated-build
    // action correctly has no `compiler_work` at all by design (that's
    // its normal, well-formed state -- `self_build`'s/`project_build`'s
    // own role), so reporting `MissingDescriptor` for one would be a
    // misleading "something is broken" message for an entirely expected
    // shape. Only a *compiler-work-kinded* action missing its descriptor
    // is the genuine contract violation that error name describes.
    if !matches!(
        action.kind,
        ActionKind::LowerSource
            | ActionKind::ValidateIr
            | ActionKind::TransformFunction
            | ActionKind::EvaluateEvidence
    ) {
        return Err(CompilerWorkExecutionError::UnsupportedActionKind {
            action_id: action.id.clone(),
            kind: action.kind,
        });
    }

    let descriptor = action.compiler_work.as_ref().ok_or_else(|| {
        CompilerWorkExecutionError::MissingDescriptor {
            action_id: action.id.clone(),
        }
    })?;

    match action.kind {
        ActionKind::LowerSource => dispatch_lower_source(action, descriptor, store),
        ActionKind::ValidateIr => dispatch_validate_ir(action, descriptor, store),
        ActionKind::TransformFunction => dispatch_transform_function(action, descriptor, store),
        ActionKind::EvaluateEvidence => dispatch_evaluate_evidence(action, descriptor, store),
        _ => unreachable!("already checked above"),
    }
}

fn dispatch_lower_source(
    action: &Action,
    descriptor: &CompilerWorkDescriptor,
    store: &mut ArtifactStore,
) -> Result<(), CompilerWorkExecutionError> {
    let source_provenance = descriptor.source_provenance.as_ref().ok_or_else(|| {
        CompilerWorkExecutionError::MissingSourceProvenance {
            action_id: action.id.clone(),
        }
    })?;
    let source_path = Path::new(&source_provenance.source_file);
    // Reads the real file from disk at the path the descriptor names --
    // `source_snapshot_id` records a content identity for that text at
    // the time the descriptor was built, but this slice does not yet
    // verify the file on disk still matches it (a real, open follow-up:
    // a source edited between planning and dispatch would silently
    // lower the *new* text under the *old* snapshot id).
    let source_text = fs::read_to_string(source_path).map_err(|e| {
        CompilerWorkExecutionError::SourceReadFailed {
            action_id: action.id.clone(),
            path: source_provenance.source_file.clone(),
            detail: e.to_string(),
        }
    })?;
    let requested: Vec<&str> = descriptor
        .requested_functions
        .iter()
        .map(String::as_str)
        .collect();
    let language = descriptor.language.as_deref().unwrap_or("");
    let lowered = match language {
        "rust" => lower_rust_source(source_path, &source_text, &requested),
        "nim" => lower_nim_source(source_path, &source_text, &requested),
        other => {
            return Err(CompilerWorkExecutionError::UnsupportedLanguage {
                action_id: action.id.clone(),
                language: other.to_string(),
            })
        }
    };
    let program = lowered.map_err(|diagnostics| CompilerWorkExecutionError::LoweringFailed {
        action_id: action.id.clone(),
        diagnostics,
    })?;
    store.candidates.insert(action.id.clone(), program);
    Ok(())
}

fn dispatch_validate_ir(
    action: &Action,
    descriptor: &CompilerWorkDescriptor,
    store: &mut ArtifactStore,
) -> Result<(), CompilerWorkExecutionError> {
    let input_id = descriptor
        .semantic_input_artifact_ids
        .first()
        .ok_or_else(|| CompilerWorkExecutionError::MissingSemanticInput {
            action_id: action.id.clone(),
        })?;
    let candidate = store.candidates.get(input_id).ok_or_else(|| {
        CompilerWorkExecutionError::MissingCandidateInput {
            action_id: action.id.clone(),
            artifact_id: input_id.clone(),
        }
    })?;
    let validated =
        validate_program(candidate).map_err(|e| CompilerWorkExecutionError::ValidationFailed {
            action_id: action.id.clone(),
            detail: e,
        })?;
    store.validated.insert(action.id.clone(), validated);
    Ok(())
}

fn dispatch_transform_function(
    action: &Action,
    descriptor: &CompilerWorkDescriptor,
    store: &mut ArtifactStore,
) -> Result<(), CompilerWorkExecutionError> {
    let input_id = descriptor
        .semantic_input_artifact_ids
        .first()
        .ok_or_else(|| CompilerWorkExecutionError::MissingSemanticInput {
            action_id: action.id.clone(),
        })?;
    // Read exclusively from the *validated* store -- see this module's
    // own top-level doc comment.
    let validated = store.validated.get(input_id).ok_or_else(|| {
        CompilerWorkExecutionError::MissingValidatedInput {
            action_id: action.id.clone(),
            artifact_id: input_id.clone(),
        }
    })?;
    let params = descriptor.transform.as_ref().ok_or_else(|| {
        CompilerWorkExecutionError::MissingTransformParameters {
            action_id: action.id.clone(),
        }
    })?;
    let program = validated.program();
    let transformed = match params.kind {
        TransformKind::Anf => anf_insert(program, &params.caller, &params.callee),
        TransformKind::Checked => checked_inline(program, &params.caller, &params.callee),
    }
    .map_err(|e| CompilerWorkExecutionError::TransformFailed {
        action_id: action.id.clone(),
        detail: format!("{e:?}"),
    })?;
    store.candidates.insert(action.id.clone(), transformed);
    Ok(())
}

fn dispatch_evaluate_evidence(
    action: &Action,
    descriptor: &CompilerWorkDescriptor,
    store: &mut ArtifactStore,
) -> Result<(), CompilerWorkExecutionError> {
    let input_id = descriptor
        .semantic_input_artifact_ids
        .first()
        .ok_or_else(|| CompilerWorkExecutionError::MissingSemanticInput {
            action_id: action.id.clone(),
        })?;
    // Read exclusively from the *validated* store -- see this module's
    // own top-level doc comment.
    let validated = store.validated.get(input_id).ok_or_else(|| {
        CompilerWorkExecutionError::MissingValidatedInput {
            action_id: action.id.clone(),
            artifact_id: input_id.clone(),
        }
    })?;
    // This slice reuses `requested_functions` (documented elsewhere as
    // "LowerSource-only") to also name *which function to evaluate* --
    // a deliberate, minimal reuse rather than a new field, recorded here
    // openly rather than silently assumed; a future contract version may
    // give `EvaluateEvidence` its own dedicated field instead.
    let function_name = descriptor.requested_functions.first().ok_or_else(|| {
        CompilerWorkExecutionError::MissingRequestedFunctionToEvaluate {
            action_id: action.id.clone(),
        }
    })?;
    if descriptor.test_inputs.is_empty() {
        return Err(CompilerWorkExecutionError::MissingTestInputs {
            action_id: action.id.clone(),
        });
    }
    let mut outcomes = Vec::with_capacity(descriptor.test_inputs.len());
    for args in &descriptor.test_inputs {
        let outcome = eval_function(validated.program(), function_name, args).map_err(|e| {
            CompilerWorkExecutionError::EvaluationFailed {
                action_id: action.id.clone(),
                detail: format!("{e:?}"),
            }
        })?;
        outcomes.push(outcome);
    }
    store.evidence.insert(action.id.clone(), outcomes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use laminaria_plan::compiler_work::{
        evaluate_evidence_artifact_id, lower_source_artifact_id, transform_function_artifact_id,
        validate_ir_artifact_id, ResourceRequest, SourceProvenanceRef, TransformParameters,
        COMPILER_WORK_SCHEMA_VERSION,
    };
    use laminaria_plan::{validate, ArtifactRef, PlanOutcome, PlanningInput};
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    /// Builds (once per test binary process) the real `laminaria-planner`
    /// Nim binary via `nim c` directly -- the same pattern (and the same
    /// `OnceLock`-guarded-race reasoning) `self_build.rs`'s and
    /// `laminaria-plan`'s own test modules already use independently; see
    /// either one's doc comment for the concurrent-build race this
    /// guards against.
    #[cfg(unix)]
    fn real_planner_binary() -> PathBuf {
        static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        BUILT
            .get_or_init(|| {
                let nim_planner_dir = repo_root().join("nim-planner");
                let status = std::process::Command::new("nim")
                    .args([
                        "c",
                        "--path:src",
                        "-o:bin/laminaria-planner",
                        "src/laminaria_planner.nim",
                    ])
                    .current_dir(&nim_planner_dir)
                    .status()
                    .expect("failed to invoke nim -- is Nim installed?");
                assert!(status.success(), "nim c failed to build laminaria-planner");
                nim_planner_dir.join("bin/laminaria-planner")
            })
            .clone()
    }

    /// Issue #27 stage C's own first-slice acceptance: the *real*, full
    /// vertical path -- a genuine Nim source file on disk, lowered
    /// through `laminaria_ir::nim_frontend`, ANF-inlined through
    /// `laminaria_ir::transform`, validated at both ends through
    /// `laminaria_ir::validate`, planned and ordered by the *real*
    /// `laminaria-planner` Nim binary, checked by
    /// `laminaria_plan::validate::validate`, and finally evaluated
    /// through `laminaria_ir::interpreter` -- with no step anywhere
    /// invoking `rustc`/`nim` to compute an actual answer (only to
    /// *plan*, which is `laminaria-planner`'s own, equally owned, role).
    #[test]
    #[cfg(unix)]
    fn the_full_lower_validate_transform_validate_evaluate_pipeline_runs_end_to_end() {
        let dir = std::env::temp_dir().join(format!(
            "laminaria-compiler-work-executor-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source_path = dir.join("g.nim");
        let source_text = "proc add(x, y: int32): int32 =\n  x +% y\n\nproc g(x: int32): int32 =\n  add(x, 1'i32)\n";
        std::fs::write(&source_path, source_text).unwrap();
        let source_path_str = source_path.to_str().unwrap().to_string();

        let subset_version = "0.1.0";
        let semantic_contract_version = "0.1.0";
        let transform_version = "0.1.0";
        let observation_contract_version = "0.1.0";
        let operation_version = "0.1.0";

        // 1. LowerSource("add", "g" from g.nim)
        let lower_id = lower_source_artifact_id(
            operation_version,
            "nim",
            &source_path_str, // stand-in content-snapshot id for this test
            &["add", "g"],
            subset_version,
        );
        let lower_action = Action {
            id: lower_id.clone(),
            kind: ActionKind::LowerSource,
            command_identity: "lower_source".to_string(),
            inputs: vec![ArtifactRef::source(&source_path_str)],
            outputs: vec![ArtifactRef::declared(&lower_id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: operation_version.to_string(),
                semantic_input_artifact_ids: vec![],
                requested_functions: vec!["add".to_string(), "g".to_string()],
                language: Some("nim".to_string()),
                contract_version: Some(subset_version.to_string()),
                transform: None,
                source_provenance: Some(SourceProvenanceRef {
                    source_file: source_path_str.clone(),
                    source_snapshot_id: source_path_str.clone(),
                }),
                test_inputs_digest: None,
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        // 2. ValidateIr(lowered candidate)
        let validate1_id =
            validate_ir_artifact_id(operation_version, &lower_id, semantic_contract_version);
        let validate1_action = Action {
            id: validate1_id.clone(),
            kind: ActionKind::ValidateIr,
            command_identity: "validate_ir".to_string(),
            inputs: vec![ArtifactRef::declared(&lower_id)],
            outputs: vec![ArtifactRef::declared(&validate1_id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: operation_version.to_string(),
                semantic_input_artifact_ids: vec![lower_id.clone()],
                requested_functions: vec![],
                language: None,
                contract_version: Some(semantic_contract_version.to_string()),
                transform: None,
                source_provenance: None,
                test_inputs_digest: None,
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        // 3. TransformFunction: ANF-inline "add" into "g"
        let transform_id = transform_function_artifact_id(
            operation_version,
            &validate1_id,
            "g",
            "add",
            TransformKind::Anf,
            transform_version,
        );
        let transform_action = Action {
            id: transform_id.clone(),
            kind: ActionKind::TransformFunction,
            command_identity: "transform_function".to_string(),
            inputs: vec![ArtifactRef::declared(&validate1_id)],
            outputs: vec![ArtifactRef::declared(&transform_id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: operation_version.to_string(),
                semantic_input_artifact_ids: vec![validate1_id.clone()],
                requested_functions: vec![],
                language: None,
                contract_version: None,
                transform: Some(TransformParameters {
                    kind: TransformKind::Anf,
                    transform_version: transform_version.to_string(),
                    caller: "g".to_string(),
                    callee: "add".to_string(),
                }),
                source_provenance: None,
                test_inputs_digest: None,
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        // 4. ValidateIr(transformed candidate)
        let validate2_id =
            validate_ir_artifact_id(operation_version, &transform_id, semantic_contract_version);
        let validate2_action = Action {
            id: validate2_id.clone(),
            kind: ActionKind::ValidateIr,
            command_identity: "validate_ir".to_string(),
            inputs: vec![ArtifactRef::declared(&transform_id)],
            outputs: vec![ArtifactRef::declared(&validate2_id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: operation_version.to_string(),
                semantic_input_artifact_ids: vec![transform_id.clone()],
                requested_functions: vec![],
                language: None,
                contract_version: Some(semantic_contract_version.to_string()),
                transform: None,
                source_provenance: None,
                test_inputs_digest: None,
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        // 5. EvaluateEvidence: run "g" against two finite test inputs.
        let test_inputs: Vec<Vec<i64>> = vec![vec![5], vec![i32::MAX as i64]];
        let test_inputs_digest = "digest-of-2-i32-inputs".to_string();
        let evaluate_id = evaluate_evidence_artifact_id(
            operation_version,
            &validate2_id,
            &test_inputs_digest,
            observation_contract_version,
        );
        let evaluate_action = Action {
            id: evaluate_id.clone(),
            kind: ActionKind::EvaluateEvidence,
            command_identity: "evaluate_evidence".to_string(),
            inputs: vec![ArtifactRef::declared(&validate2_id)],
            outputs: vec![ArtifactRef::declared(&evaluate_id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: operation_version.to_string(),
                semantic_input_artifact_ids: vec![validate2_id.clone()],
                requested_functions: vec!["g".to_string()],
                language: None,
                contract_version: Some(observation_contract_version.to_string()),
                transform: None,
                source_provenance: None,
                test_inputs_digest: Some(test_inputs_digest),
                test_inputs: test_inputs.clone(),
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        let input = PlanningInput::new(
            vec![evaluate_id.clone()],
            vec![
                lower_action,
                validate1_action,
                transform_action,
                validate2_action,
                evaluate_action,
            ],
        );

        let bin = real_planner_binary();
        let outcome = laminaria_plan::call_planner(&bin, &input).unwrap();
        let plan = match outcome {
            PlanOutcome::Planned(plan) => plan,
            PlanOutcome::Rejected(r) => panic!("expected a plan, got a rejection: {r:?}"),
        };
        assert_eq!(
            plan.ordered_actions,
            vec![
                lower_id.clone(),
                validate1_id.clone(),
                transform_id.clone(),
                validate2_id.clone(),
                evaluate_id.clone(),
            ],
            "the real Nim planner must order this chain exactly by its own inputs/outputs \
             dependency derivation"
        );
        validate(&plan, &input).expect("a well-formed compiler-work plan must validate");

        let mut store = ArtifactStore::new();
        run_compiler_work_plan(&plan, &mut store)
            .expect("the full vertical compiler-work path must run end to end");

        let outcomes = store
            .evidence(&evaluate_id)
            .expect("EvaluateEvidence must have recorded outcomes");
        assert_eq!(outcomes.len(), 2);
        // g(x) = add(x, 1) = x +% 1, real wrapping i32 semantics.
        assert_eq!(outcomes[0].value as i32, 6, "g(5) = 5 +% 1 = 6");
        assert_eq!(
            outcomes[1].value as i32,
            i32::MIN,
            "g(i32::MAX) = i32::MAX +% 1 = i32::MIN (wrapping)"
        );

        // The transform actually ran (not a no-op copy): the validated,
        // post-transform Program for "g" no longer calls "add" as a
        // separate function at all.
        let validated_after_transform = store
            .validated_program(&validate2_id)
            .expect("the second ValidateIr must have stored a validated Program");
        let g_body_calls_add = format!("{:?}", validated_after_transform.program().functions["g"])
            .contains("FnId(\"add\")");
        assert!(
            !g_body_calls_add,
            "expected 'add' to have been inlined away by the real ANF transform, found it \
             still called directly"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    fn nim_fact(
        name: &str,
        params: usize,
        body: laminaria_ir::types::Stmt,
    ) -> laminaria_ir::types::FnFact {
        use laminaria_ir::types::{
            IntWidth, Provenance, SourceLanguage, SourcePosition, SourceSpan,
        };
        laminaria_ir::types::FnFact {
            name: name.to_string(),
            params: (0..params)
                .map(|i| (format!("p{i}"), IntWidth::I32))
                .collect(),
            return_width: IntWidth::I32,
            body,
            provenance: Provenance {
                source_file: PathBuf::from("test"),
                span: SourceSpan {
                    start: SourcePosition { line: 1, column: 1 },
                    end: SourcePosition { line: 1, column: 1 },
                },
                language: SourceLanguage::Nim,
            },
        }
    }

    /// Issue #27 A2's own invariant, enforced by an actual executor: a
    /// `TransformFunction` dispatch must never read its input from the
    /// *candidate* store, even when a candidate with the exact same
    /// artifact id genuinely exists there (e.g. left over from an earlier
    /// `LowerSource`/`TransformFunction` step) -- only a *validated*
    /// entry satisfies it.
    #[test]
    fn transform_function_never_reads_an_unvalidated_candidate_even_if_present() {
        use laminaria_ir::types::{
            Expr, Program, Provenance, SourceLanguage, SourcePosition, SourceSpan, Stmt,
        };

        let prov = || Provenance {
            source_file: PathBuf::from("test"),
            span: SourceSpan {
                start: SourcePosition { line: 1, column: 1 },
                end: SourcePosition { line: 1, column: 1 },
            },
            language: SourceLanguage::Nim,
        };
        let mut program = Program::default();
        program.insert(nim_fact(
            "g",
            1,
            Stmt::Return(Expr::Param(0, prov()), prov()),
        ));

        let mut store = ArtifactStore::new();
        // Present only as an *unvalidated* candidate, deliberately never
        // promoted to `validated`.
        store.candidates.insert("prog-1".to_string(), program);

        let action = Action {
            id: "transform-1".to_string(),
            kind: ActionKind::TransformFunction,
            command_identity: "transform_function".to_string(),
            inputs: vec![ArtifactRef::declared("prog-1")],
            outputs: vec![ArtifactRef::declared("transform-1")],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: "0.1.0".to_string(),
                semantic_input_artifact_ids: vec!["prog-1".to_string()],
                requested_functions: vec![],
                language: None,
                contract_version: None,
                transform: Some(TransformParameters {
                    kind: TransformKind::Anf,
                    transform_version: "0.1.0".to_string(),
                    caller: "g".to_string(),
                    callee: "add".to_string(),
                }),
                source_provenance: None,
                test_inputs_digest: None,
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        let result = dispatch_action(&action, &mut store);
        assert!(
            matches!(
                result,
                Err(CompilerWorkExecutionError::MissingValidatedInput { .. })
            ),
            "expected MissingValidatedInput despite a same-id candidate being present, got \
             {result:?}"
        );
    }

    /// This executor never dispatches a legacy delegated-build kind --
    /// that stays `self_build`'s/`project_build`'s own role, and no
    /// external compiler is ever invoked from this module.
    #[test]
    fn a_legacy_delegated_build_action_kind_is_rejected_not_silently_run() {
        let action = Action {
            id: "a".to_string(),
            kind: ActionKind::NimBuild,
            command_identity: "nim c".to_string(),
            inputs: vec![],
            outputs: vec![],
            compiler_work: None,
        };
        let mut store = ArtifactStore::new();
        assert!(matches!(
            dispatch_action(&action, &mut store),
            Err(CompilerWorkExecutionError::UnsupportedActionKind { .. })
        ));
    }

    /// A compiler-work-kinded action with no descriptor at all is a hard
    /// error, never treated as a no-op.
    #[test]
    fn a_compiler_work_action_with_no_descriptor_is_rejected() {
        let action = Action {
            id: "a".to_string(),
            kind: ActionKind::LowerSource,
            command_identity: "lower_source".to_string(),
            inputs: vec![],
            outputs: vec![],
            compiler_work: None,
        };
        let mut store = ArtifactStore::new();
        assert!(matches!(
            dispatch_action(&action, &mut store),
            Err(CompilerWorkExecutionError::MissingDescriptor { .. })
        ));
    }
}

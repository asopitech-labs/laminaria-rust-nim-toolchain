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

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::{Condvar, Mutex};

use laminaria_ir::diagnostics::Diagnostic;
use laminaria_ir::interpreter::{eval_function, EvalOutcome};
use laminaria_ir::nim_frontend::lower_nim_source;
use laminaria_ir::rust_frontend::lower_rust_source;
use laminaria_ir::transform::anf_insert::anf_insert;
use laminaria_ir::transform::checked_inline::checked_inline;
use laminaria_ir::types::Program;
use laminaria_ir::validate::{validate_program, ProgramValidationError, ValidatedProgram};
use laminaria_plan::compiler_work::{CompilerWorkDescriptor, TransformKind};
use laminaria_plan::{Action, ActionKind, ArtifactRef, ExecutionPlan};

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
    /// The real content hash of the source file just read does not match
    /// `source_provenance.source_snapshot_id` -- the file changed between
    /// whenever the descriptor was built and this dispatch, and this
    /// executor refuses to publish the *new* text as an artifact of the
    /// *old*, now-stale claimed snapshot. See
    /// [`compute_source_snapshot_id`]'s own doc comment.
    SourceSnapshotMismatch {
        action_id: String,
        path: String,
        claimed: String,
        actual: String,
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
            Self::SourceSnapshotMismatch {
                action_id,
                path,
                claimed,
                actual,
            } => write!(
                f,
                "action {action_id:?}'s source file {path:?} no longer matches its claimed \
                 snapshot (claimed {claimed:?}, actual content hashes to {actual:?}) -- refusing \
                 to publish the changed text under the stale snapshot id"
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

/// A real SHA-256 content hash of `source_text`, formatted as lowercase
/// hex -- this executor's own canonical `source_snapshot_id` scheme.
/// Whoever builds a `LowerSource` action's `PlanningInput` (a future
/// planning-time caller, or this module's own tests) must compute
/// `source_provenance.source_snapshot_id` the same way, so
/// `dispatch_lower_source`'s own re-check against the file's real,
/// current content at dispatch time actually means something -- matching
/// `laminaria_fingerprint::exec::sha256_file`'s own hex-formatting
/// convention (a file-based sibling of this one), applied here to an
/// already-in-memory string instead of re-reading the file a second time.
pub fn compute_source_snapshot_id(source_text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source_text.as_bytes());
    format!("{:x}", hasher.finalize())
}

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

/// [`ArtifactStore`] behind a single [`Mutex`], with accessors that hold
/// the lock only long enough to clone an input out or insert a result --
/// never across the actual `laminaria_ir` computation itself (lowering/
/// validating/transforming/evaluating). `Program`/`ValidatedProgram`/
/// `EvalOutcome` are all cheap, allocation-owning `Clone` types with no
/// interior mutability, so cloning one out of the lock is exactly the
/// standard "shrink the critical section" pattern -- without it, every
/// concurrent dispatch would serialize on this one lock for its entire
/// runtime, defeating the point of a CPU budget greater than one.
struct SharedStore(Mutex<ArtifactStore>);

impl SharedStore {
    #[cfg(test)]
    fn new(store: ArtifactStore) -> Self {
        SharedStore(Mutex::new(store))
    }

    fn candidate(&self, artifact_id: &str) -> Option<Program> {
        self.0.lock().unwrap().candidates.get(artifact_id).cloned()
    }

    fn validated(&self, artifact_id: &str) -> Option<ValidatedProgram> {
        self.0.lock().unwrap().validated.get(artifact_id).cloned()
    }

    fn insert_candidate(&self, artifact_id: String, program: Program) {
        self.0
            .lock()
            .unwrap()
            .candidates
            .insert(artifact_id, program);
    }

    fn insert_validated(&self, artifact_id: String, validated: ValidatedProgram) {
        self.0
            .lock()
            .unwrap()
            .validated
            .insert(artifact_id, validated);
    }

    fn insert_evidence(&self, artifact_id: String, outcomes: Vec<EvalOutcome>) {
        self.0
            .lock()
            .unwrap()
            .evidence
            .insert(artifact_id, outcomes);
    }
}

/// The static (never changes once computed from `plan`) dependency
/// bookkeeping the scheduler in [`run_compiler_work_plan`] needs: how
/// many not-yet-resolved producers each action still waits on, and which
/// actions become candidates for readiness once a given action resolves.
/// Derived the same way `laminaria_plan::validate`'s own `producer_index`
/// is -- matching `Declared` inputs against declared outputs -- since
/// `plan` has already passed `validate()` and is trusted to have exactly
/// one producer per declared artifact.
fn dependency_graph(
    plan: &ExecutionPlan,
) -> (HashMap<String, usize>, HashMap<String, Vec<String>>) {
    let mut producer_of: HashMap<&str, &str> = HashMap::new();
    for (id, action) in &plan.actions {
        for output in &action.outputs {
            if let ArtifactRef::Declared { artifact_id } = output {
                producer_of.insert(artifact_id.as_str(), id.as_str());
            }
        }
    }

    let mut remaining_deps: HashMap<String, usize> = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    for id in plan.actions.keys() {
        remaining_deps.entry(id.clone()).or_insert(0);
        dependents.entry(id.clone()).or_default();
    }
    for (id, action) in &plan.actions {
        for input in &action.inputs {
            let ArtifactRef::Declared { artifact_id } = input else {
                continue;
            };
            let Some(&producer_id) = producer_of.get(artifact_id.as_str()) else {
                continue; // validate() already guarantees this can't happen
            };
            *remaining_deps.get_mut(id).expect("seeded above") += 1;
            dependents
                .get_mut(producer_id)
                .expect("seeded above")
                .push(id.clone());
        }
    }
    (remaining_deps, dependents)
}

/// Mutex-guarded scheduler bookkeeping, shared across every worker
/// thread in [`run_compiler_work_plan`].
struct SchedulerState {
    ready: VecDeque<String>,
    remaining_deps: HashMap<String, usize>,
    /// Every action id that will never run again: it either already
    /// succeeded, already failed, or was poisoned (a transitive
    /// dependent of a failed action, which can now never reach
    /// `remaining_deps == 0`). The scheduler is done exactly when this
    /// covers every action in the plan.
    resolved: HashSet<String>,
    errors: BTreeMap<String, CompilerWorkExecutionError>,
}

/// Per-action `(action_id, dispatch start, dispatch end)` timestamps,
/// recorded only when a caller opts in via
/// [`run_compiler_work_plan_traced`] -- production dispatch
/// (`run_compiler_work_plan`) always passes `None`, so this costs
/// nothing beyond a branch on that path. Exists so a test can prove two
/// actions' real `laminaria_ir` computations genuinely overlapped in
/// wall-clock time, without depending on an aggregate-duration
/// comparison that a loaded CI runner's own contention can make flaky.
type ActivityLog = Mutex<Vec<(String, std::time::Instant, std::time::Instant)>>;

/// One worker's share of [`run_compiler_work_plan`]'s dependency-ready
/// dispatch loop: repeatedly take a ready action id, dispatch it, and
/// update the shared schedule -- exits once every action is resolved
/// (see [`SchedulerState::resolved`]), which a `Condvar` wakeup around
/// every state change makes every idle worker re-check.
fn worker_loop(
    plan: &ExecutionPlan,
    dependents: &HashMap<String, Vec<String>>,
    total: usize,
    scheduler: &Mutex<SchedulerState>,
    ready_or_done: &Condvar,
    store: &SharedStore,
    activity_log: Option<&ActivityLog>,
) {
    loop {
        let action_id = {
            let mut state = scheduler.lock().unwrap();
            loop {
                if let Some(id) = state.ready.pop_front() {
                    break Some(id);
                }
                if state.resolved.len() >= total {
                    break None;
                }
                state = ready_or_done.wait(state).unwrap();
            }
        };
        let Some(action_id) = action_id else {
            return;
        };

        // The actual `laminaria_ir` computation -- lowering/validating/
        // transforming/evaluating -- runs here, entirely outside any
        // lock, which is what lets a CPU budget greater than one
        // actually overlap real work rather than merely interleave
        // acquisitions of one shared lock.
        let action = &plan.actions[&action_id];
        let started = std::time::Instant::now();
        let result = dispatch_action(action, store);
        if let Some(log) = activity_log {
            log.lock()
                .unwrap()
                .push((action_id.clone(), started, std::time::Instant::now()));
        }

        let mut state = scheduler.lock().unwrap();
        state.resolved.insert(action_id.clone());
        match result {
            Ok(()) => {
                for dependent in dependents.get(&action_id).into_iter().flatten() {
                    if state.resolved.contains(dependent) {
                        continue;
                    }
                    let remaining = state
                        .remaining_deps
                        .get_mut(dependent)
                        .expect("seeded for every action");
                    *remaining -= 1;
                    if *remaining == 0 {
                        state.ready.push_back(dependent.clone());
                    }
                }
            }
            Err(e) => {
                state.errors.insert(action_id.clone(), e);
                // A failed producer's consumer must never start (issue
                // #27's own "producer失敗時にはconsumerを開始しない"):
                // poison every transitive dependent so it is counted
                // resolved -- and therefore never queued -- without ever
                // having `remaining_deps` reach zero via a real
                // completion.
                let mut queue: Vec<String> =
                    dependents.get(&action_id).cloned().unwrap_or_default();
                while let Some(poisoned) = queue.pop() {
                    if state.resolved.insert(poisoned.clone()) {
                        if let Some(further) = dependents.get(&poisoned) {
                            queue.extend(further.iter().cloned());
                        }
                    }
                }
            }
        }
        drop(state);
        ready_or_done.notify_all();
    }
}

/// Dispatches every action in `plan.actions`, in dependency-ready order
/// under a bound of at most `cpu_budget` actions running at once --
/// issue #27's own "依存ready実行とCPU予算": an action starts as soon as
/// every producer its `Declared` inputs name has *successfully*
/// resolved and a CPU slot is free, never in `ordered_actions`'
/// sequential order for its own sake. A failed producer's consumer is
/// never started (see [`worker_loop`]'s own poisoning). `plan` should
/// already have passed `laminaria_plan::validate::validate` (which
/// itself calls `validate_compiler_work_action` on every action) -- this
/// function does not re-validate the plan's own structural/contract
/// well-formedness, only ever reads artifacts through [`SharedStore`]'s
/// own typed accessors, so it cannot silently substitute an unvalidated
/// `Program` where a validated one is required regardless.
///
/// The same scheduling algorithm runs at every `cpu_budget` -- `1`
/// degenerates to one worker draining the ready queue as it's produced,
/// never two actions' `laminaria_ir` computations actually overlapping,
/// while a larger budget lets independent chains' computations run
/// concurrently for real. Because every action's own output is a pure
/// function of its declared inputs (never of scheduling order or wall-
/// clock timing), the *values* two different budgets produce are
/// identical -- only how much of the work happens concurrently differs.
pub fn run_compiler_work_plan(
    plan: &ExecutionPlan,
    store: &mut ArtifactStore,
    cpu_budget: usize,
) -> Result<(), CompilerWorkExecutionError> {
    run_compiler_work_plan_inner(plan, store, cpu_budget, None)
}

/// Test-only entry point that also records every dispatch's real
/// wall-clock interval into `activity_log`, so a concurrency test can
/// directly confirm two independent actions' dispatches genuinely
/// overlapped -- see [`ActivityLog`]'s own doc comment for why this
/// exists instead of comparing aggregate run durations. `#[cfg(unix)]`
/// because its only caller needs the real `laminaria-planner` binary,
/// same as this module's other real-binary tests -- Windows CI never
/// provisions Nim, so an ungated `#[cfg(test)]` alone would leave this
/// with no caller there and fail as dead code.
#[cfg(all(test, unix))]
fn run_compiler_work_plan_traced(
    plan: &ExecutionPlan,
    store: &mut ArtifactStore,
    cpu_budget: usize,
    activity_log: &ActivityLog,
) -> Result<(), CompilerWorkExecutionError> {
    run_compiler_work_plan_inner(plan, store, cpu_budget, Some(activity_log))
}

fn run_compiler_work_plan_inner(
    plan: &ExecutionPlan,
    store: &mut ArtifactStore,
    cpu_budget: usize,
    activity_log: Option<&ActivityLog>,
) -> Result<(), CompilerWorkExecutionError> {
    let cpu_budget = cpu_budget.max(1);
    let total = plan.actions.len();
    let (remaining_deps, dependents) = dependency_graph(plan);

    let ready: VecDeque<String> = remaining_deps
        .iter()
        .filter(|(_, &count)| count == 0)
        .map(|(id, _)| id.clone())
        .collect();

    let scheduler = Mutex::new(SchedulerState {
        ready,
        remaining_deps,
        resolved: HashSet::new(),
        errors: BTreeMap::new(),
    });
    let ready_or_done = Condvar::new();
    let shared_store = SharedStore(Mutex::new(std::mem::take(store)));

    std::thread::scope(|scope| {
        for _ in 0..cpu_budget {
            scope.spawn(|| {
                worker_loop(
                    plan,
                    &dependents,
                    total,
                    &scheduler,
                    &ready_or_done,
                    &shared_store,
                    activity_log,
                )
            });
        }
    });

    *store = shared_store.0.into_inner().unwrap();

    // Deterministic regardless of which worker happened to fail first or
    // how many actions failed: always the smallest failing action id's
    // own error, the same choice at every `cpu_budget`.
    if let Some((_, error)) = scheduler.into_inner().unwrap().errors.into_iter().next() {
        return Err(error);
    }
    Ok(())
}

fn dispatch_action(action: &Action, store: &SharedStore) -> Result<(), CompilerWorkExecutionError> {
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
    store: &SharedStore,
) -> Result<(), CompilerWorkExecutionError> {
    let source_provenance = descriptor.source_provenance.as_ref().ok_or_else(|| {
        CompilerWorkExecutionError::MissingSourceProvenance {
            action_id: action.id.clone(),
        }
    })?;
    let source_path = Path::new(&source_provenance.source_file);
    let source_text = fs::read_to_string(source_path).map_err(|e| {
        CompilerWorkExecutionError::SourceReadFailed {
            action_id: action.id.clone(),
            path: source_provenance.source_file.clone(),
            detail: e.to_string(),
        }
    })?;
    // A review caught a real gap: the file on disk was lowered under
    // whatever `source_snapshot_id` the descriptor *claimed*, never
    // checked against the text actually just read -- a source edited
    // between planning and dispatch would have silently published the
    // *new* text as an artifact of the *old* snapshot id, exactly the
    // "changed source published as an artifact of a stale snapshot"
    // hazard this contract exists to prevent. Recomputed and compared
    // here, hard-failing on any mismatch rather than trusting the claim.
    let actual_snapshot_id = compute_source_snapshot_id(&source_text);
    if actual_snapshot_id != source_provenance.source_snapshot_id {
        return Err(CompilerWorkExecutionError::SourceSnapshotMismatch {
            action_id: action.id.clone(),
            path: source_provenance.source_file.clone(),
            claimed: source_provenance.source_snapshot_id.clone(),
            actual: actual_snapshot_id,
        });
    }
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
    store.insert_candidate(action.id.clone(), program);
    Ok(())
}

fn dispatch_validate_ir(
    action: &Action,
    descriptor: &CompilerWorkDescriptor,
    store: &SharedStore,
) -> Result<(), CompilerWorkExecutionError> {
    let input_id = descriptor
        .semantic_input_artifact_ids
        .first()
        .ok_or_else(|| CompilerWorkExecutionError::MissingSemanticInput {
            action_id: action.id.clone(),
        })?;
    let candidate = store.candidate(input_id).ok_or_else(|| {
        CompilerWorkExecutionError::MissingCandidateInput {
            action_id: action.id.clone(),
            artifact_id: input_id.clone(),
        }
    })?;
    let validated =
        validate_program(&candidate).map_err(|e| CompilerWorkExecutionError::ValidationFailed {
            action_id: action.id.clone(),
            detail: e,
        })?;
    store.insert_validated(action.id.clone(), validated);
    Ok(())
}

fn dispatch_transform_function(
    action: &Action,
    descriptor: &CompilerWorkDescriptor,
    store: &SharedStore,
) -> Result<(), CompilerWorkExecutionError> {
    let input_id = descriptor
        .semantic_input_artifact_ids
        .first()
        .ok_or_else(|| CompilerWorkExecutionError::MissingSemanticInput {
            action_id: action.id.clone(),
        })?;
    // Read exclusively from the *validated* store -- see this module's
    // own top-level doc comment.
    let validated = store.validated(input_id).ok_or_else(|| {
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
    store.insert_candidate(action.id.clone(), transformed);
    Ok(())
}

fn dispatch_evaluate_evidence(
    action: &Action,
    descriptor: &CompilerWorkDescriptor,
    store: &SharedStore,
) -> Result<(), CompilerWorkExecutionError> {
    let input_id = descriptor
        .semantic_input_artifact_ids
        .first()
        .ok_or_else(|| CompilerWorkExecutionError::MissingSemanticInput {
            action_id: action.id.clone(),
        })?;
    // Read exclusively from the *validated* store -- see this module's
    // own top-level doc comment.
    let validated = store.validated(input_id).ok_or_else(|| {
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
    store.insert_evidence(action.id.clone(), outcomes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use laminaria_plan::compiler_work::{
        ResourceRequest, TransformParameters, COMPILER_WORK_SCHEMA_VERSION,
    };
    use laminaria_plan::ArtifactRef;
    // `PathBuf` is used by cross-platform tests too (`nim_fact`'s own
    // `Provenance.source_file`), so it stays ungated -- only the items
    // genuinely exclusive to the real-binary tests below are gated
    // individually, not by wrapping the whole module in `#[cfg(unix)]`
    // the way `laminaria-ir`'s `fixture_parity_tests` does (a module of
    // *only* real-toolchain tests, unlike this one). A review's own CI
    // run caught exactly the "only functions gated, not their shared
    // imports" version of this mistake here: the `windows` job (which
    // never provisions Nim) failed `unused_imports`/`dead_code` under
    // `-D warnings` for the real-binary-only items -- and a second,
    // narrower version of the same mistake surfaced fixing the first:
    // `PathBuf` itself is *not* exclusive to those tests, so gating it
    // too would have broken the `windows` build a different way (a
    // cross-platform test referencing a name only defined on `unix`).
    #[cfg(unix)]
    use laminaria_plan::compiler_work::{
        evaluate_evidence_artifact_id, lower_source_artifact_id, transform_function_artifact_id,
        validate_ir_artifact_id, SourceProvenanceRef,
    };
    #[cfg(unix)]
    use laminaria_plan::{validate, PlanOutcome, PlanningInput};
    use std::path::PathBuf;

    #[cfg(unix)]
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    /// Delegates to `crate::test_support::real_planner_binary`, shared
    /// across every module in this crate that needs the real binary --
    /// this module used to guard its own build with its own private
    /// `OnceLock`, which a review caught (via a real CI failure) could
    /// still race `self_build.rs`'s and `project_build.rs`'s own,
    /// *separate* `OnceLock`s targeting the exact same output path. See
    /// `test_support`'s own doc comment.
    #[cfg(unix)]
    fn real_planner_binary() -> PathBuf {
        crate::test_support::real_planner_binary(&repo_root())
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
        let source_snapshot_id = compute_source_snapshot_id(source_text);
        let lower_id = lower_source_artifact_id(
            operation_version,
            "nim",
            &source_snapshot_id,
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
                    source_snapshot_id: source_snapshot_id.clone(),
                }),
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
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        // 5. EvaluateEvidence: run "g" against two finite test inputs.
        let test_inputs: Vec<Vec<i64>> = vec![vec![5], vec![i32::MAX as i64]];
        let evaluate_id = evaluate_evidence_artifact_id(
            operation_version,
            &validate2_id,
            "g",
            &test_inputs,
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
        // A budget of 2 exercises the dependency-ready scheduler even
        // though this particular chain is purely linear (no two actions
        // are ever simultaneously ready) -- it must produce exactly the
        // same result a sequential run would.
        run_compiler_work_plan(&plan, &mut store, 2)
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
        let store = SharedStore::new(store);

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
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        let result = dispatch_action(&action, &store);
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
        let store = SharedStore::new(ArtifactStore::new());
        assert!(matches!(
            dispatch_action(&action, &store),
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
        let store = SharedStore::new(ArtifactStore::new());
        assert!(matches!(
            dispatch_action(&action, &store),
            Err(CompilerWorkExecutionError::MissingDescriptor { .. })
        ));
    }

    /// A review caught this exact gap: a source file edited between
    /// planning and dispatch used to be silently lowered under the *old*
    /// (now-stale) claimed snapshot id. Writes real content, computes its
    /// real snapshot id, then *changes the file on disk* before
    /// dispatching -- the descriptor still claims the original id.
    #[test]
    fn a_source_file_changed_after_planning_is_rejected_not_silently_lowered() {
        let dir = std::env::temp_dir().join(format!(
            "laminaria-compiler-work-executor-snapshot-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source_path = dir.join("f.nim");
        let original_text = "proc f(x: int32): int32 =\n  x\n";
        std::fs::write(&source_path, original_text).unwrap();
        let claimed_snapshot_id = compute_source_snapshot_id(original_text);
        let source_path_str = source_path.to_str().unwrap().to_string();

        // The file is edited *after* the descriptor above was built --
        // dispatch must see this, not the text the id was computed from.
        std::fs::write(&source_path, "proc f(x: int32): int32 =\n  x +% 1'i32\n").unwrap();

        let action = Action {
            id: "lower-1".to_string(),
            kind: ActionKind::LowerSource,
            command_identity: "lower_source".to_string(),
            inputs: vec![ArtifactRef::source(&source_path_str)],
            outputs: vec![ArtifactRef::declared("lower-1")],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: "0.1.0".to_string(),
                semantic_input_artifact_ids: vec![],
                requested_functions: vec!["f".to_string()],
                language: Some("nim".to_string()),
                contract_version: Some("0.1.0".to_string()),
                transform: None,
                source_provenance: Some(laminaria_plan::compiler_work::SourceProvenanceRef {
                    source_file: source_path_str,
                    source_snapshot_id: claimed_snapshot_id,
                }),
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        };

        let store = SharedStore::new(ArtifactStore::new());
        let result = dispatch_action(&action, &store);
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            matches!(
                result,
                Err(CompilerWorkExecutionError::SourceSnapshotMismatch { .. })
            ),
            "expected SourceSnapshotMismatch, got {result:?}"
        );
    }

    fn minimal_lower_source_action(id: &str, source_file: &str) -> Action {
        Action {
            id: id.to_string(),
            kind: ActionKind::LowerSource,
            command_identity: "lower_source".to_string(),
            inputs: vec![ArtifactRef::source(source_file)],
            outputs: vec![ArtifactRef::declared(id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: "0.1.0".to_string(),
                semantic_input_artifact_ids: vec![],
                requested_functions: vec!["f".to_string()],
                language: Some("nim".to_string()),
                contract_version: Some("0.1.0".to_string()),
                transform: None,
                source_provenance: Some(laminaria_plan::compiler_work::SourceProvenanceRef {
                    source_file: source_file.to_string(),
                    source_snapshot_id: "irrelevant-since-this-read-fails".to_string(),
                }),
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        }
    }

    fn minimal_validate_ir_action(id: &str, input_id: &str) -> Action {
        Action {
            id: id.to_string(),
            kind: ActionKind::ValidateIr,
            command_identity: "validate_ir".to_string(),
            inputs: vec![ArtifactRef::declared(input_id)],
            outputs: vec![ArtifactRef::declared(id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: "0.1.0".to_string(),
                semantic_input_artifact_ids: vec![input_id.to_string()],
                requested_functions: vec![],
                language: None,
                contract_version: Some("0.1.0".to_string()),
                transform: None,
                source_provenance: None,
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        }
    }

    fn plan_of(actions: Vec<Action>) -> ExecutionPlan {
        let ordered_actions: Vec<String> = actions.iter().map(|a| a.id.clone()).collect();
        let actions: BTreeMap<String, Action> =
            actions.into_iter().map(|a| (a.id.clone(), a)).collect();
        ExecutionPlan {
            schema_version: laminaria_plan::PLAN_SCHEMA_VERSION.to_string(),
            produced_by: laminaria_plan::PRODUCED_BY.to_string(),
            producer_version: laminaria_plan::PLAN_SCHEMA_VERSION.to_string(),
            plan_id: "test".to_string(),
            ordered_actions,
            actions,
        }
    }

    /// Issue #27's own "産出者失敗時にはconsumerを開始しない": a
    /// `LowerSource` action that fails to even read its source file must
    /// never let its `ValidateIr` consumer start, at every CPU budget --
    /// the consumer's own `remaining_deps` can only ever reach zero via a
    /// *successful* producer completion (see `worker_loop`'s own
    /// poisoning), so it must stay permanently unresolved and never
    /// appear in the store.
    #[test]
    fn a_failed_producer_leaves_its_consumer_unresolved_and_never_dispatched() {
        let lower = minimal_lower_source_action("lower-fail", "/nonexistent/does-not-exist.nim");
        let validate_consumer = minimal_validate_ir_action("validate-consumer", "lower-fail");
        let plan = plan_of(vec![lower, validate_consumer]);

        for cpu_budget in [1usize, 2usize] {
            let mut store = ArtifactStore::new();
            let result = run_compiler_work_plan(&plan, &mut store, cpu_budget);
            assert!(
                matches!(
                    result,
                    Err(CompilerWorkExecutionError::SourceReadFailed { .. })
                ),
                "cpu_budget {cpu_budget}: expected SourceReadFailed, got {result:?}"
            );
            assert!(
                store.validated_program("validate-consumer").is_none(),
                "cpu_budget {cpu_budget}: the consumer must never have been dispatched after \
                 its producer failed"
            );
        }
    }

    /// Issue #27's own "依存ready実行とCPU予算" acceptance: two entirely
    /// independent chains -- one Rust, one Nim -- planned and dispatched
    /// together. Confirms, at both CPU budget 1 and 2: the evaluated
    /// values and evidence are bit-for-bit identical (scheduling never
    /// changes what a pure computation produces), and only the demanded
    /// chains' six actions ever run (nothing extra). A separate timing
    /// assertion below confirms budget 2 actually overlaps the two
    /// chains' real work, not merely accepts the parameter.
    #[test]
    #[cfg(unix)]
    fn independent_rust_and_nim_chains_agree_across_cpu_budgets_and_run_concurrently_under_budget_two(
    ) {
        use laminaria_plan::compiler_work::SourceProvenanceRef;

        let dir = std::env::temp_dir().join(format!(
            "laminaria-compiler-work-executor-concurrency-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let rust_path = dir.join("f.rs");
        let rust_text = "fn f(x: i32) -> i32 { x }";
        std::fs::write(&rust_path, rust_text).unwrap();
        let rust_path_str = rust_path.to_str().unwrap().to_string();

        let nim_path = dir.join("g.nim");
        let nim_text = "proc g(x: int32): int32 =\n  x\n";
        std::fs::write(&nim_path, nim_text).unwrap();
        let nim_path_str = nim_path.to_str().unwrap().to_string();

        // Many test inputs per chain -- enough real interpreter work
        // (thousands of `eval_function` calls) that two independent
        // chains running concurrently under a CPU budget of 2 finishes
        // in meaningfully less wall-clock time than running them one
        // after another, the same way the fixed function's real cost
        // rather than an artificial delay proves genuine overlap.
        let test_inputs: Vec<Vec<i64>> = (0..60_000i64).map(|n| vec![n]).collect();

        fn chain(
            language: &str,
            source_path: &str,
            source_text: &str,
            function_name: &str,
            test_inputs: &[Vec<i64>],
        ) -> (Vec<Action>, String) {
            let operation_version = "0.1.0";
            let subset_version = "0.1.0";
            let semantic_contract_version = "0.1.0";
            let observation_contract_version = "0.1.0";
            let snapshot_id = compute_source_snapshot_id(source_text);

            let lower_id = laminaria_plan::lower_source_artifact_id(
                operation_version,
                language,
                &snapshot_id,
                &[function_name],
                subset_version,
            );
            let lower_action = Action {
                id: lower_id.clone(),
                kind: ActionKind::LowerSource,
                command_identity: "lower_source".to_string(),
                inputs: vec![ArtifactRef::source(source_path)],
                outputs: vec![ArtifactRef::declared(&lower_id)],
                compiler_work: Some(CompilerWorkDescriptor {
                    descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                    operation_version: operation_version.to_string(),
                    semantic_input_artifact_ids: vec![],
                    requested_functions: vec![function_name.to_string()],
                    language: Some(language.to_string()),
                    contract_version: Some(subset_version.to_string()),
                    transform: None,
                    source_provenance: Some(SourceProvenanceRef {
                        source_file: source_path.to_string(),
                        source_snapshot_id: snapshot_id,
                    }),
                    test_inputs: vec![],
                    resource_request: ResourceRequest::minimal(),
                    budget_token: "budget-1".to_string(),
                }),
            };

            let validate_id = laminaria_plan::validate_ir_artifact_id(
                operation_version,
                &lower_id,
                semantic_contract_version,
            );
            let validate_action = Action {
                id: validate_id.clone(),
                kind: ActionKind::ValidateIr,
                command_identity: "validate_ir".to_string(),
                inputs: vec![ArtifactRef::declared(&lower_id)],
                outputs: vec![ArtifactRef::declared(&validate_id)],
                compiler_work: Some(CompilerWorkDescriptor {
                    descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                    operation_version: operation_version.to_string(),
                    semantic_input_artifact_ids: vec![lower_id.clone()],
                    requested_functions: vec![],
                    language: None,
                    contract_version: Some(semantic_contract_version.to_string()),
                    transform: None,
                    source_provenance: None,
                    test_inputs: vec![],
                    resource_request: ResourceRequest::minimal(),
                    budget_token: "budget-1".to_string(),
                }),
            };

            let evaluate_id = laminaria_plan::evaluate_evidence_artifact_id(
                operation_version,
                &validate_id,
                function_name,
                test_inputs,
                observation_contract_version,
            );
            let evaluate_action = Action {
                id: evaluate_id.clone(),
                kind: ActionKind::EvaluateEvidence,
                command_identity: "evaluate_evidence".to_string(),
                inputs: vec![ArtifactRef::declared(&validate_id)],
                outputs: vec![ArtifactRef::declared(&evaluate_id)],
                compiler_work: Some(CompilerWorkDescriptor {
                    descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                    operation_version: operation_version.to_string(),
                    semantic_input_artifact_ids: vec![validate_id.clone()],
                    requested_functions: vec![function_name.to_string()],
                    language: None,
                    contract_version: Some(observation_contract_version.to_string()),
                    transform: None,
                    source_provenance: None,
                    test_inputs: test_inputs.to_vec(),
                    resource_request: ResourceRequest::minimal(),
                    budget_token: "budget-1".to_string(),
                }),
            };

            (
                vec![lower_action, validate_action, evaluate_action],
                evaluate_id,
            )
        }

        let (rust_actions, rust_evidence_id) =
            chain("rust", &rust_path_str, rust_text, "f", &test_inputs);
        let (nim_actions, nim_evidence_id) =
            chain("nim", &nim_path_str, nim_text, "g", &test_inputs);
        let rust_action_ids: std::collections::HashSet<String> =
            rust_actions.iter().map(|a| a.id.clone()).collect();
        let nim_action_ids: std::collections::HashSet<String> =
            nim_actions.iter().map(|a| a.id.clone()).collect();

        let mut all_actions = rust_actions;
        all_actions.extend(nim_actions);
        let input = laminaria_plan::PlanningInput::new(
            vec![rust_evidence_id.clone(), nim_evidence_id.clone()],
            all_actions,
        );

        let bin = real_planner_binary();
        let outcome = laminaria_plan::call_planner(&bin, &input).unwrap();
        let plan = match outcome {
            PlanOutcome::Planned(plan) => plan,
            PlanOutcome::Rejected(r) => panic!("expected a plan, got a rejection: {r:?}"),
        };
        // Demand-closure pruning: exactly the six actions across both
        // chains, nothing more.
        assert_eq!(plan.actions.len(), 6);
        validate(&plan, &input).expect("a well-formed compiler-work plan must validate");

        let mut store_budget_1 = ArtifactStore::new();
        run_compiler_work_plan(&plan, &mut store_budget_1, 1).expect("budget 1 must succeed");

        let mut store_budget_2 = ArtifactStore::new();
        let activity_log: ActivityLog = Mutex::new(Vec::new());
        run_compiler_work_plan_traced(&plan, &mut store_budget_2, 2, &activity_log)
            .expect("budget 2 must succeed");

        std::fs::remove_dir_all(&dir).ok();

        let evidence_1_rust = store_budget_1.evidence(&rust_evidence_id).unwrap();
        let evidence_2_rust = store_budget_2.evidence(&rust_evidence_id).unwrap();
        let evidence_1_nim = store_budget_1.evidence(&nim_evidence_id).unwrap();
        let evidence_2_nim = store_budget_2.evidence(&nim_evidence_id).unwrap();
        assert_eq!(
            evidence_1_rust, evidence_2_rust,
            "the Rust chain's evidence must be identical regardless of CPU budget"
        );
        assert_eq!(
            evidence_1_nim, evidence_2_nim,
            "the Nim chain's evidence must be identical regardless of CPU budget"
        );
        assert_eq!(evidence_1_rust.len(), test_inputs.len());
        assert_eq!(evidence_1_nim.len(), test_inputs.len());

        // Real overlap, proven directly rather than inferred from an
        // aggregate-duration comparison (which a loaded CI runner's own
        // contention can make flaky regardless of whether the scheduler
        // is genuinely concurrent): under budget 2, some action from the
        // Rust chain and some action from the Nim chain must have
        // dispatches whose real wall-clock intervals actually overlap --
        // impossible for a scheduler that ever serializes all dispatch
        // behind one lock, however fast it runs.
        let intervals = activity_log.into_inner().unwrap();
        let overlaps = intervals.iter().any(|(id_a, start_a, end_a)| {
            rust_action_ids.contains(id_a)
                && intervals.iter().any(|(id_b, start_b, end_b)| {
                    nim_action_ids.contains(id_b) && start_a < end_b && start_b < end_a
                })
        });
        assert!(
            overlaps,
            "expected at least one Rust-chain action and one Nim-chain action to have \
             genuinely overlapping dispatch intervals under a CPU budget of 2, got {intervals:#?}"
        );
    }
}

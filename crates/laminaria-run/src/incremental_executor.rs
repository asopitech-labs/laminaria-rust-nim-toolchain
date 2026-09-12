//! Issue #36 T1: a small, self-contained, real (not simulated) CPU-slot-
//! traced concurrent executor for the incremental session
//! (`incremental_session_client::IncrementalSessionClient`).
//!
//! Deliberately its **own** artifact store and dispatch logic, not a
//! reuse of `compiler_work_executor.rs`'s private `SharedStore`/
//! `dispatch_*` internals: that module's whole design assumes a single,
//! already-complete, static `ExecutionPlan` known upfront (issue #27
//! stage C's own scope), while this module must dispatch actions that
//! arrive incrementally, mid-session, from Nim's own `PlanDelta`
//! responses. Reusing its private structures would have meant bumping
//! visibility across a heavily-reviewed file for a fit that isn't
//! actually the same shape; a small, separately-reviewable module with
//! its own tests is the safer trade, at the cost of some duplication
//! (the source-read/snapshot-check/dispatch-by-kind shape, not the
//! `laminaria_ir` calls themselves, which are the same public functions
//! either module would call).
//!
//! **No external compiler fallback anywhere in this module** (matching
//! `compiler_work_executor.rs`'s own explicit rule): every dispatch arm
//! calls directly into `laminaria_ir`'s owned logic
//! (`rust_frontend`/`discover`/`validate`/`interpreter`); this is
//! verified by [`tests::source_never_spawns_a_subprocess`] below, the
//! same source-text regression-guard technique issue #5 T1's
//! `wasm_target.rs` already uses, applied here to the Rust runtime side
//! of issue #36's own no-fallback requirement (T0 §10 case11).

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};

use laminaria_ir::discover::discover_called_functions;
use laminaria_ir::interpreter::{eval_function, EvalOutcome};
use laminaria_ir::rust_frontend::lower_rust_source;
use laminaria_ir::types::Program;
use laminaria_ir::validate::{validate_program, ValidatedProgram};
use laminaria_plan::incremental::{
    ActionState, DemandReference, IncrementalPlannerResponse, PlanningEvent, PlanningEventKind,
};
use laminaria_plan::{Action, ActionKind};

use crate::compiler_work_executor::compute_source_snapshot_id;
use crate::incremental_session_client::{IncrementalSessionClient, IncrementalSessionError};

#[derive(Debug)]
pub enum ExecutorError {
    Session(IncrementalSessionError),
    UnexpectedRejected { detail: String },
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutorError::Session(e) => write!(f, "{e}"),
            ExecutorError::UnexpectedRejected { detail } => {
                write!(
                    f,
                    "session rejected a delta this executor did not expect: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for ExecutorError {}

impl From<IncrementalSessionError> for ExecutorError {
    fn from(e: IncrementalSessionError) -> Self {
        ExecutorError::Session(e)
    }
}

enum ArtifactValue {
    Candidate(Program),
    Validated(ValidatedProgram),
    Evidence(Vec<EvalOutcome>),
}

#[derive(Default)]
struct IncrementalArtifactStore {
    values: Mutex<HashMap<String, ArtifactValue>>,
}

impl IncrementalArtifactStore {
    fn insert(&self, artifact_id: String, value: ArtifactValue) {
        self.values.lock().unwrap().insert(artifact_id, value);
    }

    pub fn evidence_of(&self, artifact_id: &str) -> Option<Vec<EvalOutcome>> {
        match self.values.lock().unwrap().get(artifact_id) {
            Some(ArtifactValue::Evidence(e)) => Some(e.clone()),
            _ => None,
        }
    }
}

/// The same "peak concurrent real computation" probe
/// `compiler_work_executor.rs::ComputeConcurrencyProbe` uses, applied
/// here to the incremental executor's own worker pool -- this *is* the
/// CPU-slot trace T0 §9 asks for: a slot is "acquired" only for the
/// duration of one dispatch's real `laminaria_ir` call, never while an
/// action merely sits in `blocked_dependency` (which this executor never
/// dispatches at all, so it structurally cannot enter this probe).
#[derive(Default)]
pub struct CpuSlotTrace {
    active: AtomicUsize,
    peak: AtomicUsize,
}

impl CpuSlotTrace {
    fn enter(&self) -> CpuSlotGuard<'_> {
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        CpuSlotGuard(self)
    }

    /// The highest number of actions ever observed genuinely executing
    /// (not merely dispatched/waiting) at the same instant.
    pub fn peak_concurrent(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

struct CpuSlotGuard<'a>(&'a CpuSlotTrace);

impl Drop for CpuSlotGuard<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

enum DispatchOutcome {
    Completed,
    Discovered(Vec<String>),
    Failed(String),
}

fn dispatch(
    action: &Action,
    store: &IncrementalArtifactStore,
    trace: &CpuSlotTrace,
) -> DispatchOutcome {
    let Some(descriptor) = action.compiler_work.as_ref() else {
        return DispatchOutcome::Failed(format!("action {:?} has no compiler_work", action.id));
    };

    match action.kind {
        ActionKind::LowerSource => {
            let Some(provenance) = descriptor.source_provenance.as_ref() else {
                return DispatchOutcome::Failed("missing source_provenance".to_string());
            };
            let source_text = match fs::read_to_string(&provenance.source_file) {
                Ok(t) => t,
                Err(e) => return DispatchOutcome::Failed(format!("source read failed: {e}")),
            };
            let actual_snapshot = compute_source_snapshot_id(&source_text);
            if actual_snapshot != provenance.source_snapshot_id {
                return DispatchOutcome::Failed(format!(
                    "source snapshot mismatch: claimed {:?}, actual {actual_snapshot:?}",
                    provenance.source_snapshot_id
                ));
            }
            let requested: Vec<&str> = descriptor
                .requested_functions
                .iter()
                .map(String::as_str)
                .collect();
            let result = {
                let _slot = trace.enter();
                lower_rust_source(Path::new(&provenance.source_file), &source_text, &requested)
            };
            match result {
                Ok(program) => {
                    store.insert(action.id.clone(), ArtifactValue::Candidate(program));
                    DispatchOutcome::Completed
                }
                Err(diagnostics) => DispatchOutcome::Failed(format!("{diagnostics:?}")),
            }
        }
        ActionKind::DiscoverSourceDependencies => {
            let Some(provenance) = descriptor.source_provenance.as_ref() else {
                return DispatchOutcome::Failed("missing source_provenance".to_string());
            };
            let source_text = match fs::read_to_string(&provenance.source_file) {
                Ok(t) => t,
                Err(e) => return DispatchOutcome::Failed(format!("source read failed: {e}")),
            };
            let known: Vec<&str> = descriptor
                .requested_functions
                .iter()
                .map(String::as_str)
                .collect();
            let result = {
                let _slot = trace.enter();
                discover_called_functions(Path::new(&provenance.source_file), &source_text, &known)
            };
            match result {
                Ok(names) => DispatchOutcome::Discovered(names),
                Err(diagnostics) => DispatchOutcome::Failed(format!("{diagnostics:?}")),
            }
        }
        ActionKind::ValidateIr => {
            let Some(input_id) = descriptor.semantic_input_artifact_ids.first() else {
                return DispatchOutcome::Failed("missing semantic_input_artifact_ids".to_string());
            };
            let candidate = {
                let values = store.values.lock().unwrap();
                match values.get(input_id) {
                    Some(ArtifactValue::Candidate(p)) => p.clone(),
                    _ => {
                        return DispatchOutcome::Failed(format!(
                            "no candidate Program for {input_id:?}"
                        ))
                    }
                }
            };
            let result = {
                let _slot = trace.enter();
                validate_program(&candidate)
            };
            match result {
                Ok(validated) => {
                    store.insert(action.id.clone(), ArtifactValue::Validated(validated));
                    DispatchOutcome::Completed
                }
                Err(e) => DispatchOutcome::Failed(format!("{e}")),
            }
        }
        ActionKind::EvaluateEvidence => {
            let Some(input_id) = descriptor.semantic_input_artifact_ids.first() else {
                return DispatchOutcome::Failed("missing semantic_input_artifact_ids".to_string());
            };
            let Some(function_name) = descriptor.requested_functions.first() else {
                return DispatchOutcome::Failed("missing requested_functions[0]".to_string());
            };
            let validated = {
                let values = store.values.lock().unwrap();
                match values.get(input_id) {
                    Some(ArtifactValue::Validated(v)) => v.clone(),
                    _ => {
                        return DispatchOutcome::Failed(format!(
                            "no validated Program for {input_id:?}"
                        ))
                    }
                }
            };
            let program = validated.program();
            let mut outcomes = Vec::new();
            {
                let _slot = trace.enter();
                for test_input in &descriptor.test_inputs {
                    match eval_function(program, function_name, test_input) {
                        Ok(outcome) => outcomes.push(outcome),
                        Err(e) => return DispatchOutcome::Failed(format!("{e:?}")),
                    }
                }
            }
            store.insert(action.id.clone(), ArtifactValue::Evidence(outcomes));
            DispatchOutcome::Completed
        }
        other => DispatchOutcome::Failed(format!(
            "unsupported action kind for this executor: {other:?}"
        )),
    }
}

enum WorkerReport {
    Completed {
        action_id: String,
    },
    Discovered {
        action_id: String,
        names: Vec<String>,
    },
    Failed {
        action_id: String,
        detail: String,
    },
}

/// What a caller's discovery handler returns: the concrete new actions
/// (and any newly-expressible demands) to introduce for the closed
/// function set the discovery just resolved (T0 §1.8) -- fixture-
/// specific knowledge (e.g. which functions `add_or_double.rs` needs,
/// their real artifact ids) this generic executor cannot have itself.
pub struct DiscoveryResolution {
    pub new_actions: Vec<Action>,
    pub new_demands: Vec<DemandReference>,
}

/// Runs a session to completion: dispatches every `ready` action onto a
/// pool of at most `cpu_budget` worker threads, translates each
/// completion/failure into the matching `PlanningEvent`, feeds it back
/// to `client`, and dispatches whatever becomes newly ready -- until
/// nothing is ready and nothing is in flight. A `DiscoverSourceDependencies`
/// completion is handed to `on_discovered` (the only fixture-specific
/// seam) to build the real `DependencyDiscovered` event.
///
/// Returns the CPU-slot trace and the artifact store's own evidence
/// accessor, so a caller can assert both genuine concurrency (T0 case1:
/// an independent ready branch proceeds while a slower branch is still
/// in flight) and the actual computed result of the work this session
/// ran.
/// Shared worker-pool state, following the exact `Mutex` + `Condvar` +
/// shared queue shape `compiler_work_executor.rs::worker_loop`/
/// `SchedulerState` already use (and this crate's review history already
/// vetted) -- adapted here for a queue that can *grow* mid-run (a
/// discovery introducing new ready actions), which that module's own
/// static-`ExecutionPlan` design never needed to handle.
struct WorkerPoolState {
    ready: VecDeque<Action>,
    /// Set by the supervisor once it knows no further work will ever be
    /// pushed (its own `outstanding` count reached zero) -- the only
    /// signal that tells an idle worker to actually exit, rather than
    /// exiting the instant it merely observes an empty queue (which
    /// would abandon the pool before a later discovery's own new ready
    /// actions ever arrive).
    done: bool,
}

pub fn run_to_completion(
    client: &mut IncrementalSessionClient,
    initial_ready: Vec<Action>,
    cpu_budget: NonZeroUsize,
    mut on_discovered: impl FnMut(&str, &[String]) -> DiscoveryResolution,
) -> Result<(CpuSlotTrace, EvidenceReader), ExecutorError> {
    let store = IncrementalArtifactStore::default();
    let trace = CpuSlotTrace::default();

    let (report_tx, report_rx) = mpsc::channel::<WorkerReport>();
    // Every action this session has ever told us about, by id -- needed
    // because a `PlanDelta` only carries `new_action` the *first* time an
    // action appears (T0 §3.2); a later blocked_dependency -> ready
    // transition for that same, already-known action carries no
    // `new_action` at all, so this executor must remember its
    // definition itself to actually dispatch it once it becomes ready.
    let mut known_actions: HashMap<String, Action> = initial_ready
        .iter()
        .map(|a| (a.id.clone(), a.clone()))
        .collect();
    // `outstanding`: how many actions this executor knows about that have
    // not yet been *fully processed* (reported back to Nim and its
    // response applied) -- owned exclusively by the supervisor thread
    // (no Mutex needed for it), so there is no race between "a worker
    // finished dispatching" and "the supervisor decided nothing is left"
    // the way there would be if termination were instead inferred from a
    // shared in-flight counter (found and fixed during this task's own
    // testing: a worker that finishes very quickly could decrement a
    // shared counter to zero before its own report was received,
    // making the supervisor exit while a real report still sat unread in
    // the channel).
    let mut outstanding = initial_ready.len();
    let pool = std::sync::Condvar::new();
    let state = Mutex::new(WorkerPoolState {
        ready: initial_ready.into_iter().collect(),
        done: false,
    });

    std::thread::scope(|scope| -> Result<(), ExecutorError> {
        for _ in 0..cpu_budget.get() {
            let state = &state;
            let pool = &pool;
            let store = &store;
            let trace = &trace;
            let report_tx = report_tx.clone();
            scope.spawn(move || loop {
                let action = {
                    let mut s = state.lock().unwrap();
                    loop {
                        if let Some(a) = s.ready.pop_front() {
                            break a;
                        }
                        if s.done {
                            return;
                        }
                        s = pool.wait(s).unwrap();
                    }
                };
                let action_id = action.id.clone();
                let outcome = dispatch(&action, store, trace);
                let report = match outcome {
                    DispatchOutcome::Completed => WorkerReport::Completed { action_id },
                    DispatchOutcome::Discovered(names) => {
                        WorkerReport::Discovered { action_id, names }
                    }
                    DispatchOutcome::Failed(detail) => WorkerReport::Failed { action_id, detail },
                };
                if report_tx.send(report).is_err() {
                    return;
                }
            });
        }
        drop(report_tx);

        // Supervisor: this scope's own thread, driving the Nim IPC
        // synchronously (fast/local, never itself a bottleneck for the
        // worker threads' real compute-bound work).
        while outstanding > 0 {
            let Ok(report) = report_rx.recv() else { break };
            outstanding -= 1;

            let mut push_ready = |actions: Vec<Action>| {
                if actions.is_empty() {
                    return;
                }
                outstanding += actions.len();
                let mut s = state.lock().unwrap();
                s.ready.extend(actions);
                drop(s);
                pool.notify_all();
            };

            match report {
                WorkerReport::Completed { action_id } => {
                    let newly_ready = apply_and_dispatch(
                        client,
                        PlanningEvent {
                            event_id: format!("evt-completed-{action_id}"),
                            sequence_number: 0,
                            planning_generation: 0,
                            emitted_at_unix_ns: 0,
                            kind: PlanningEventKind::ProducerCompleted {
                                artifact_id: action_id.clone(),
                                produced_by_action_id: action_id,
                            },
                        },
                        &mut known_actions,
                    )?;
                    push_ready(newly_ready);
                }
                WorkerReport::Failed { action_id, detail } => {
                    let newly_ready = apply_and_dispatch(
                        client,
                        PlanningEvent {
                            event_id: format!("evt-failed-{action_id}"),
                            sequence_number: 0,
                            planning_generation: 0,
                            emitted_at_unix_ns: 0,
                            kind: PlanningEventKind::ProducerFailed {
                                artifact_id: action_id.clone(),
                                produced_by_action_id: action_id,
                                failure_reason: detail,
                            },
                        },
                        &mut known_actions,
                    )?;
                    push_ready(newly_ready);
                }
                WorkerReport::Discovered { action_id, names } => {
                    let newly_ready = apply_and_dispatch(
                        client,
                        PlanningEvent {
                            event_id: format!("evt-completed-{action_id}"),
                            sequence_number: 0,
                            planning_generation: 0,
                            emitted_at_unix_ns: 0,
                            kind: PlanningEventKind::ProducerCompleted {
                                artifact_id: action_id.clone(),
                                produced_by_action_id: action_id.clone(),
                            },
                        },
                        &mut known_actions,
                    )?;
                    push_ready(newly_ready);
                    if !names.is_empty() {
                        let resolution = on_discovered(&action_id, &names);
                        let newly_ready = apply_and_dispatch(
                            client,
                            PlanningEvent {
                                event_id: format!("evt-discovered-{action_id}"),
                                sequence_number: 0,
                                planning_generation: 0,
                                emitted_at_unix_ns: 0,
                                kind: PlanningEventKind::DependencyDiscovered {
                                    discovering_action_id: action_id,
                                    new_actions: resolution.new_actions,
                                    supersessions: vec![],
                                    new_demands: resolution.new_demands,
                                },
                            },
                            &mut known_actions,
                        )?;
                        push_ready(newly_ready);
                    }
                }
            }
        }

        // Every outstanding item has been fully processed -- release
        // every idle worker.
        state.lock().unwrap().done = true;
        pool.notify_all();
        Ok(())
    })?;

    Ok((trace, EvidenceReader(store)))
}

/// A thin, read-only handle so a caller can retrieve one action's
/// computed evidence after [`run_to_completion`] returns, without this
/// module exposing its own internal store type directly.
pub struct EvidenceReader(IncrementalArtifactStore);

impl EvidenceReader {
    pub fn evidence_of(&self, artifact_id: &str) -> Option<Vec<EvalOutcome>> {
        self.0.evidence_of(artifact_id)
    }
}

/// Applies one event to the session and returns every action the
/// resulting `PlanDelta` reports as newly `ready` (its full definition,
/// resolved via `known_actions` when this particular response didn't
/// carry it itself) -- pushing them onto the shared work queue is the
/// caller's job, so it can also update its own `outstanding` bookkeeping
/// atomically with the push.
fn apply_and_dispatch(
    client: &mut IncrementalSessionClient,
    event: PlanningEvent,
    known_actions: &mut HashMap<String, Action>,
) -> Result<Vec<Action>, ExecutorError> {
    let response = client.apply_delta(event)?;
    match response {
        IncrementalPlannerResponse::PlanDelta {
            changed_actions, ..
        } => {
            let mut newly_ready = Vec::new();
            for change in changed_actions {
                if let Some(action) = &change.new_action {
                    known_actions.insert(change.action_id.clone(), action.clone());
                }
                if change.to_state == Some(ActionState::Ready) {
                    let action = change
                        .new_action
                        .or_else(|| known_actions.get(&change.action_id).cloned())
                        .expect(
                            "an action reported ready must have been introduced via new_action \
                             at some point before this",
                        );
                    newly_ready.push(action);
                }
            }
            Ok(newly_ready)
        }
        IncrementalPlannerResponse::Rejected { reason_detail, .. } => {
            Err(ExecutorError::UnexpectedRejected {
                detail: reason_detail,
            })
        }
        IncrementalPlannerResponse::SessionClosed { .. } => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use laminaria_plan::compiler_work::{
        discover_source_dependencies_artifact_id, evaluate_evidence_artifact_id,
        lower_source_artifact_id, validate_ir_artifact_id, CompilerWorkDescriptor, ResourceRequest,
        SourceProvenanceRef,
    };
    use laminaria_plan::{ArtifactRef, PlanningInput};
    use std::num::NonZeroUsize;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    /// Regression guard (issue #5 T1's `wasm_target.rs::
    /// source_never_spawns_a_subprocess` technique, applied to this
    /// module): confirms this executor's own *production* code (the
    /// part before this `#[cfg(test)] mod tests` block) contains no
    /// subprocess-spawning call anywhere -- T0 §10 case11's no-fallback
    /// requirement, checked at the source-text level as an honest
    /// regression guard, not a structural type-level proof (same
    /// distinction issue #5 T0's own §6 correction draws). Scoped to
    /// exclude this test module deliberately: its own tests legitimately
    /// spawn `nim` to build the real test fixture binary, the same way
    /// `nim_planner_client.rs`'s own real-binary tests already do -- that
    /// is test/build tooling, not the production no-fallback claim this
    /// guard actually checks.
    #[test]
    fn source_never_spawns_a_subprocess() {
        let source = include_str!("incremental_executor.rs");
        // A marker with no embedded newline -- found and fixed during
        // this task's own CI run: splitting on a marker that embeds a
        // literal "\n" silently matched nothing at all on Windows, where
        // this file's checked-out line endings are "\r\n", leaving the
        // test module (which legitimately spawns `nim`) inside
        // `production_source` and reintroducing the exact self-matching
        // failure this split exists to avoid.
        let test_module_marker = "#[cfg(test)]";
        let production_source = source
            .split(test_module_marker)
            .next()
            .expect("this file always contains its own test module marker");
        let subprocess_spawn_needle = format!("{}{}", "Command", "::new");
        assert!(
            !production_source.contains(&subprocess_spawn_needle),
            "the incremental executor's production code must never spawn an external compiler \
             process"
        );
    }

    #[cfg(unix)]
    fn real_incremental_planner_binary() -> PathBuf {
        static BUILT: OnceLock<PathBuf> = OnceLock::new();
        BUILT
            .get_or_init(|| {
                let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .canonicalize()
                    .unwrap();
                let nim_planner_dir = repo_root.join("nim-planner");
                let bin = nim_planner_dir.join("bin/laminaria-incremental-planner");
                let status = std::process::Command::new("nim")
                    .args([
                        "c",
                        "--path:src",
                        "--nimcache:nimcache",
                        "-o:bin/laminaria-incremental-planner",
                        "src/laminaria_incremental_planner.nim",
                    ])
                    .current_dir(&nim_planner_dir)
                    .status()
                    .expect("failed to invoke nim -- is Nim installed?");
                assert!(
                    status.success(),
                    "nim c failed to build laminaria-incremental-planner"
                );
                assert!(bin.is_file());
                bin
            })
            .clone()
    }

    fn lower_source_action(
        id: &str,
        language: &str,
        snapshot: &str,
        file: &str,
        functions: &[&str],
    ) -> Action {
        let requested: Vec<String> = functions.iter().map(|s| s.to_string()).collect();
        Action {
            id: id.to_string(),
            kind: ActionKind::LowerSource,
            command_identity: "lower_source".to_string(),
            inputs: vec![ArtifactRef::source(file)],
            outputs: vec![ArtifactRef::declared(id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: "0.1.0".to_string(),
                operation_version: "0.1.0".to_string(),
                semantic_input_artifact_ids: vec![],
                requested_functions: requested,
                language: Some(language.to_string()),
                contract_version: Some("0.1.0".to_string()),
                transform: None,
                source_provenance: Some(SourceProvenanceRef {
                    source_file: file.to_string(),
                    source_snapshot_id: snapshot.to_string(),
                }),
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        }
    }

    fn discover_action(id: &str, snapshot: &str, file: &str, known: &[&str]) -> Action {
        let requested: Vec<String> = known.iter().map(|s| s.to_string()).collect();
        Action {
            id: id.to_string(),
            kind: ActionKind::DiscoverSourceDependencies,
            command_identity: "discover_source_dependencies".to_string(),
            inputs: vec![ArtifactRef::source(file)],
            outputs: vec![ArtifactRef::declared(id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: "0.1.0".to_string(),
                operation_version: "0.1.0".to_string(),
                semantic_input_artifact_ids: vec![],
                requested_functions: requested,
                language: Some("rust".to_string()),
                contract_version: Some("0.1.0".to_string()),
                transform: None,
                source_provenance: Some(SourceProvenanceRef {
                    source_file: file.to_string(),
                    source_snapshot_id: snapshot.to_string(),
                }),
                test_inputs: vec![],
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        }
    }

    fn validate_ir_action(id: &str, input_id: &str) -> Action {
        Action {
            id: id.to_string(),
            kind: ActionKind::ValidateIr,
            command_identity: "validate_ir".to_string(),
            inputs: vec![ArtifactRef::declared(input_id)],
            outputs: vec![ArtifactRef::declared(id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: "0.1.0".to_string(),
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

    fn evaluate_evidence_action(
        id: &str,
        input_id: &str,
        function_name: &str,
        test_inputs: Vec<Vec<i64>>,
    ) -> Action {
        Action {
            id: id.to_string(),
            kind: ActionKind::EvaluateEvidence,
            command_identity: "evaluate_evidence".to_string(),
            inputs: vec![ArtifactRef::declared(input_id)],
            outputs: vec![ArtifactRef::declared(id)],
            compiler_work: Some(CompilerWorkDescriptor {
                descriptor_schema_version: "0.1.0".to_string(),
                operation_version: "0.1.0".to_string(),
                semantic_input_artifact_ids: vec![input_id.to_string()],
                requested_functions: vec![function_name.to_string()],
                language: None,
                contract_version: Some("0.1.0".to_string()),
                transform: None,
                source_provenance: None,
                test_inputs,
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            }),
        }
    }

    /// The end-to-end proof of every outcome this round of issue #36 T1
    /// was asked for, in one real session against the real compiled
    /// Nim binary:
    ///
    /// - an independent ready branch (`f`'s `LowerSource`) genuinely
    ///   overlaps in execution with a slower branch
    ///   (`add_or_double.rs`'s `DiscoverSourceDependencies`) -- T0 case1
    ///   (`peak_concurrent() >= 2`, the same probe technique
    ///   `compiler_work_executor.rs`'s own review-vetted
    ///   `ComputeConcurrencyProbe` uses, entered only around real
    ///   computation).
    /// - discovery resolves the real closed function set (`["add_or_double",
    ///   "double"]`) from the real fixture file, and the resulting
    ///   LowerSource -> ValidateIr -> EvaluateEvidence chain, dispatched
    ///   entirely through real `laminaria_ir` calls, produces the
    ///   D0-confirmed value `add_or_double(3,4,1) == 6`
    ///   (`crates/laminaria-ir/src/rust_frontend.rs:830-835`).
    /// - the whole session runs over the real session-scoped Nim IPC
    ///   boundary (`IncrementalSessionClient`), never a Rust-computed
    ///   substitute.
    #[test]
    #[cfg(unix)]
    fn add_or_double_discovery_runs_concurrently_with_an_independent_branch_and_produces_the_confirmed_value(
    ) {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let f_source = "fn f(x: i32) -> i32 { x }";
        let f_snapshot = compute_source_snapshot_id(f_source);
        let f_path = repo_root.join("target/tmp-incremental-executor-f.rs");
        std::fs::write(&f_path, f_source).unwrap();

        let add_or_double_path = repo_root
            .join("fixtures/laminaria-semantic-substrate-prototype/rust-src/add_or_double.rs");
        let add_or_double_source = std::fs::read_to_string(&add_or_double_path).unwrap();
        let add_or_double_snapshot = compute_source_snapshot_id(&add_or_double_source);

        let lower_f_id = lower_source_artifact_id("0.1.0", "rust", &f_snapshot, &["f"], "0.1.0");
        let lower_f = lower_source_action(
            &lower_f_id,
            "rust",
            &f_snapshot,
            f_path.to_str().unwrap(),
            &["f"],
        );

        let discover_id = discover_source_dependencies_artifact_id(
            "0.1.0",
            "rust",
            &add_or_double_snapshot,
            &["add_or_double"],
            "0.1.0",
        );
        let discover = discover_action(
            &discover_id,
            &add_or_double_snapshot,
            add_or_double_path.to_str().unwrap(),
            &["add_or_double"],
        );

        let initial_graph = PlanningInput::new(
            vec![lower_f_id.clone(), discover_id.clone()],
            vec![lower_f.clone(), discover.clone()],
        );

        let bin = real_incremental_planner_binary();
        let (mut client, start_response) =
            IncrementalSessionClient::start(&bin, "s-case1", initial_graph, vec![]).unwrap();
        let IncrementalPlannerResponse::PlanDelta {
            changed_actions, ..
        } = start_response
        else {
            panic!("expected PlanDelta from StartSession")
        };
        assert_eq!(
            changed_actions.len(),
            2,
            "both actions must start ready (Source-only inputs)"
        );
        let initial_ready = vec![lower_f, discover];

        let add_or_double_path_for_closure = add_or_double_path.clone();
        let add_or_double_snapshot_for_closure = add_or_double_snapshot.clone();
        let mut evidence_action_id_holder: Option<String> = None;

        let (trace, evidence_reader) = run_to_completion(
            &mut client,
            initial_ready,
            NonZeroUsize::new(2).unwrap(),
            |_discovering_action_id, names| {
                assert_eq!(
                    names,
                    ["double".to_string()],
                    "must discover exactly `double`"
                );
                let closed_set = ["add_or_double", "double"];
                let lower_id = lower_source_artifact_id(
                    "0.1.0",
                    "rust",
                    &add_or_double_snapshot_for_closure,
                    &closed_set,
                    "0.1.0",
                );
                let lower = lower_source_action(
                    &lower_id,
                    "rust",
                    &add_or_double_snapshot_for_closure,
                    add_or_double_path_for_closure.to_str().unwrap(),
                    &closed_set,
                );
                let validate_id = validate_ir_artifact_id("0.1.0", &lower_id, "0.1.0");
                let validate = validate_ir_action(&validate_id, &lower_id);
                let evidence_id = evaluate_evidence_artifact_id(
                    "0.1.0",
                    &validate_id,
                    "add_or_double",
                    &[vec![3, 4, 1]],
                    "0.1.0",
                );
                let evidence = evaluate_evidence_action(
                    &evidence_id,
                    &validate_id,
                    "add_or_double",
                    vec![vec![3, 4, 1]],
                );
                evidence_action_id_holder = Some(evidence_id.clone());
                DiscoveryResolution {
                    new_actions: vec![lower, validate, evidence],
                    new_demands: vec![DemandReference {
                        artifact_id: evidence_id,
                        requested_by: "consumer-b".to_string(),
                    }],
                }
            },
        )
        .unwrap();

        assert!(
            trace.peak_concurrent() >= 2,
            "AID_LOWER_F must genuinely overlap with the add_or_double discovery, not merely be \
             dispatched sequentially behind it (T0 case1); observed peak_concurrent={}",
            trace.peak_concurrent()
        );

        let evidence_id = evidence_action_id_holder.expect("discovery closure must have run");
        let evidence = evidence_reader
            .evidence_of(&evidence_id)
            .expect("add_or_double's EvaluateEvidence must have produced evidence");
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            evidence[0].value, 6,
            "add_or_double(3,4,1) must equal the D0-confirmed value 6"
        );

        client.close().unwrap();
        let _ = std::fs::remove_file(&f_path);
    }
}

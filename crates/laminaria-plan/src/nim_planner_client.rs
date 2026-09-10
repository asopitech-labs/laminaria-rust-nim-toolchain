//! Subprocess client for the real `laminaria-planner` Nim binary (issue
//! #8/#4): writes a `PlanningInput` to its stdin as JSON, reads back a
//! `PlanOutcome` from its stdout. This is the *only* way this crate ever
//! produces a `PlanOutcome` -- there is no other code path anywhere in
//! this crate that computes one, so a missing or failing planner binary
//! is always an `Err`, never a silent Rust-computed substitute (issue
//! #8: "a Rust replacement planner... cannot satisfy this slice").
//!
//! Subprocess + JSON, not FFI: sidesteps Nim's ARC/ORC runtime-lifecycle
//! and panic/exception-unwind-boundary questions that direct linking
//! would force (issue #4's own text permits "an explicit validated
//! adapter" as a pinned bootstrap route without settling the wider
//! ABI-free research question) -- see `docs/self-build.md`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::types::{PlanOutcome, PlanningInput};

/// Looks for `laminaria-planner`(`.exe`) next to the currently running
/// executable -- the same sibling-binary convention
/// `laminaria_run::cargo_wrapper::find_rustc_wrapper_binary`/
/// `nim_wrapper::find_cc_wrapper_binary` already use for
/// `laminaria-rustc-wrapper`/`laminaria-cc-wrapper`.
pub fn find_planner_binary() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let dir = current_exe.parent()?;
    let candidate = dir.join(if cfg!(windows) {
        "laminaria-planner.exe"
    } else {
        "laminaria-planner"
    });
    candidate.is_file().then_some(candidate)
}

#[derive(Debug)]
pub enum PlannerCallError {
    /// No `laminaria-planner` binary could be resolved at all -- this
    /// crate refuses to proceed rather than fall back to any other
    /// source of a plan.
    BinaryNotFound,
    Spawn(std::io::Error),
    WriteStdin(std::io::Error),
    ReadStdout(std::io::Error),
    /// The planner process exited nonzero -- per
    /// `nim-planner/src/laminaria_planner.nim`'s own exit-code contract,
    /// this means the input could not even be parsed (a genuine
    /// rejection is a *zero*-exit, well-formed `PlanOutcome::Rejected`,
    /// not a process failure).
    NonZeroExit {
        code: Option<i32>,
        stderr: String,
    },
    /// The process exited zero but its stdout was not a well-formed
    /// `PlanOutcome` -- treated as a hard error, never silently ignored
    /// or replaced with a Rust-computed plan.
    MalformedOutput(serde_json::Error),
}

impl std::fmt::Display for PlannerCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlannerCallError::BinaryNotFound => write!(
                f,
                "could not find the laminaria-planner binary next to the running executable \
                 (expected it built alongside laminaria-cli from nim-planner/)"
            ),
            PlannerCallError::Spawn(e) => write!(f, "failed to spawn laminaria-planner: {e}"),
            PlannerCallError::WriteStdin(e) => {
                write!(
                    f,
                    "failed to write PlanningInput to laminaria-planner's stdin: {e}"
                )
            }
            PlannerCallError::ReadStdout(e) => {
                write!(f, "failed to read laminaria-planner's stdout: {e}")
            }
            PlannerCallError::NonZeroExit { code, stderr } => write!(
                f,
                "laminaria-planner exited with {code:?} (this means it could not even parse its \
                 input -- a genuine plan rejection exits zero): {stderr}"
            ),
            PlannerCallError::MalformedOutput(e) => write!(
                f,
                "laminaria-planner's stdout was not a well-formed PlanOutcome: {e}"
            ),
        }
    }
}

impl std::error::Error for PlannerCallError {}

/// Calls a specific `laminaria-planner` binary (not necessarily the one
/// `find_planner_binary` would resolve -- callers doing a self-build
/// pass the *current generation's own* planner explicitly, since stage0's
/// planner is what must plan stage1, not whichever binary happens to sit
/// next to the currently running process).
pub fn call_planner(
    planner_binary: &Path,
    input: &PlanningInput,
) -> Result<PlanOutcome, PlannerCallError> {
    let input_json = serde_json::to_vec(input).expect("PlanningInput always serializes");

    let mut child = Command::new(planner_binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(PlannerCallError::Spawn)?;

    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(&input_json)
        .map_err(PlannerCallError::WriteStdin)?;

    let output = child
        .wait_with_output()
        .map_err(PlannerCallError::ReadStdout)?;

    if !output.status.success() {
        return Err(PlannerCallError::NonZeroExit {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    serde_json::from_slice(&output.stdout).map_err(PlannerCallError::MalformedOutput)
}

/// Resolves `find_planner_binary()` and calls it -- the convenience path
/// for callers that just want "the planner next to me," as opposed to
/// `call_planner`'s explicit-path form used by self-build generation
/// lineage.
pub fn call_default_planner(input: &PlanningInput) -> Result<PlanOutcome, PlannerCallError> {
    let binary = find_planner_binary().ok_or(PlannerCallError::BinaryNotFound)?;
    call_planner(&binary, input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, ActionKind, ArtifactRef};
    #[cfg(unix)]
    use std::path::PathBuf;

    /// Locates (building first if necessary) the real `laminaria-planner`
    /// binary under `nim-planner/`, so these tests exercise the actual
    /// Nim kernel via the real subprocess boundary -- not a mocked
    /// stand-in for it. Invokes `nim c` directly, not `nimble build`:
    /// `crates/laminaria-run/src/self_build.rs` found that this repo's
    /// `nimble` (v0.22.2) exits `0` even after printing a build-failure
    /// message on a genuine compile error, so `nim c`'s own reliable
    /// exit code is used here too, for the same reason.
    ///
    /// Builds exactly once per test binary process via `OnceLock`, not a
    /// bare `if !bin.is_file()` check -- a real race an external review's
    /// own CI run caught on a fresh checkout: this crate's two real-
    /// binary tests run on separate threads by default, and both saw the
    /// binary missing and raced to `nim c` the *same* output path
    /// simultaneously, so one test's `spawn()` of the half-written
    /// result hit `PermissionDenied`. `OnceLock::get_or_init` guarantees
    /// the build runs exactly once regardless of how many threads call
    /// this concurrently -- callers after the first block until it
    /// finishes, they don't race it. Never observed locally because this
    /// dev machine's binary was already built and cached from earlier in
    /// the same session -- see `crates/laminaria-run/NOTES.md`'s own
    /// "verify on a genuinely fresh environment" lesson, now caught
    /// twice.
    #[cfg(unix)]
    fn real_planner_binary() -> PathBuf {
        static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        BUILT
            .get_or_init(|| {
                let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .canonicalize()
                    .unwrap();
                let nim_planner_dir = repo_root.join("nim-planner");
                let bin = nim_planner_dir.join("bin/laminaria-planner");
                let status = Command::new("nim")
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
                assert!(
                    bin.is_file(),
                    "expected {} to exist after building it",
                    bin.display()
                );
                bin
            })
            .clone()
    }

    fn sample_input() -> PlanningInput {
        PlanningInput::new(
            // "integrate" declares its own output ("generation-root")
            // and demand names only it -- issue #27's own demand-
            // selection fix means an action producing nothing can never
            // be part of any demand closure (nothing can ever name it to
            // demand it); "host-bin"/"planner-bin" are pulled in
            // transitively via "integrate"'s own declared inputs.
            vec!["generation-root".to_string()],
            vec![
                Action {
                    id: "compile-rust-host".to_string(),
                    kind: ActionKind::CargoBuild,
                    command_identity: "cargo build".to_string(),
                    inputs: vec![],
                    outputs: vec![ArtifactRef::declared("host-bin")],
                    compiler_work: None,
                },
                Action {
                    id: "compile-nim-planner".to_string(),
                    kind: ActionKind::NimBuild,
                    command_identity: "nimble build".to_string(),
                    inputs: vec![],
                    outputs: vec![ArtifactRef::declared("planner-bin")],
                    compiler_work: None,
                },
                Action {
                    id: "integrate".to_string(),
                    kind: ActionKind::Integrate,
                    command_identity: "assemble generation root".to_string(),
                    inputs: vec![
                        ArtifactRef::declared("host-bin"),
                        ArtifactRef::declared("planner-bin"),
                    ],
                    outputs: vec![ArtifactRef::declared("generation-root")],
                    compiler_work: None,
                },
            ],
        )
    }

    /// `#[cfg(unix)]`: needs a real, buildable `laminaria-planner`
    /// binary (`real_planner_binary`), and this repo's CI deliberately
    /// never installs Nim on its `windows` job (see that job's own doc
    /// comment in `.github/workflows/ci.yml` -- kept lean/Unix-specific
    /// on purpose), matching how `laminaria-run`'s own real-binary
    /// tests (`self_build.rs`, `reuse.rs`) are gated for the same
    /// reason.
    #[test]
    #[cfg(unix)]
    fn call_planner_against_the_real_binary_produces_a_deterministic_plan() {
        let bin = real_planner_binary();
        let input = sample_input();

        let first = call_planner(&bin, &input).unwrap();
        let second = call_planner(&bin, &input).unwrap();

        assert!(first.is_planned());
        assert_eq!(
            first, second,
            "identical PlanningInput must plan identically"
        );

        match first {
            PlanOutcome::Planned(plan) => {
                assert_eq!(
                    plan.ordered_actions,
                    vec![
                        "compile-nim-planner".to_string(),
                        "compile-rust-host".to_string(),
                        "integrate".to_string(),
                    ]
                );
                assert_eq!(plan.produced_by, crate::types::PRODUCED_BY);
            }
            PlanOutcome::Rejected(r) => panic!("expected a plan, got a rejection: {r:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn call_planner_reports_a_structured_cycle_rejection_from_the_real_binary() {
        let bin = real_planner_binary();
        // Demand names "out-a" directly -- issue #27's own demand-
        // selection fix means an empty demand would prune this whole
        // (cyclic) graph away before the cycle is ever reached, silently
        // returning an empty plan instead of the rejection this test
        // means to exercise.
        let input = PlanningInput::new(
            vec!["out-a".to_string()],
            vec![
                Action {
                    id: "a".to_string(),
                    kind: ActionKind::NimBuild,
                    command_identity: "a".to_string(),
                    inputs: vec![ArtifactRef::declared("out-b")],
                    outputs: vec![ArtifactRef::declared("out-a")],
                    compiler_work: None,
                },
                Action {
                    id: "b".to_string(),
                    kind: ActionKind::NimBuild,
                    command_identity: "b".to_string(),
                    inputs: vec![ArtifactRef::declared("out-a")],
                    outputs: vec![ArtifactRef::declared("out-b")],
                    compiler_work: None,
                },
            ],
        );

        let outcome = call_planner(&bin, &input).unwrap();
        match outcome {
            PlanOutcome::Rejected(rejection) => {
                assert_eq!(
                    rejection.reason_kind,
                    crate::types::RejectionReasonKind::Cycle
                );
                assert_eq!(
                    rejection.cycle_path,
                    vec!["a".to_string(), "b".to_string(), "a".to_string()]
                );
            }
            PlanOutcome::Planned(_) => panic!("expected the cycle to be rejected"),
        }
    }

    /// Issue #27 B's own required alignment check: a `compiler_work`
    /// descriptor sent to the *real* Nim planner binary must come back
    /// byte-for-byte unchanged. The Nim kernel never inspects a
    /// compiler-work action's descriptor contents beyond round-tripping
    /// it -- dependency ordering still comes purely from `inputs`/
    /// `outputs` matching, so mixing one compiler-work action with an
    /// ordinary delegated-build action in the same request must still
    /// order correctly.
    #[test]
    #[cfg(unix)]
    fn a_compiler_work_descriptor_round_trips_through_the_real_planner_binary() {
        use crate::compiler_work::{
            lower_source_artifact_id, transform_function_artifact_id, CompilerWorkDescriptor,
            ResourceRequest, SourceProvenanceRef, TransformKind, TransformParameters,
            COMPILER_WORK_SCHEMA_VERSION,
        };
        use crate::validate::validate;

        let bin = real_planner_binary();

        // Both actions' ids are the *recomputed* artifact id, not an
        // arbitrary label -- exercising `validate_compiler_work_action`'s
        // identity check against a real round trip, not only against
        // hand-constructed unit-test values.
        let lower_id = lower_source_artifact_id("0.1.0", "rust", "hash-abc", &["f"], "0.1.0");
        let lower_descriptor = CompilerWorkDescriptor {
            descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
            operation_version: "0.1.0".to_string(),
            semantic_input_artifact_ids: vec![],
            requested_functions: vec!["f".to_string()],
            language: Some("rust".to_string()),
            contract_version: Some("0.1.0".to_string()),
            transform: None,
            source_provenance: Some(SourceProvenanceRef {
                source_file: "fixtures/f.rs".to_string(),
                source_snapshot_id: "hash-abc".to_string(),
            }),
            test_inputs: vec![],
            resource_request: ResourceRequest::minimal(),
            budget_token: "budget-1".to_string(),
        };
        let transform_id = transform_function_artifact_id(
            "0.1.0",
            &lower_id,
            "caller",
            "callee",
            TransformKind::Checked,
            "0.1.0",
        );
        let transform_descriptor = CompilerWorkDescriptor {
            descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
            operation_version: "0.1.0".to_string(),
            semantic_input_artifact_ids: vec![lower_id.clone()],
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
        let input = PlanningInput::new(
            vec![transform_id.clone()],
            vec![
                Action {
                    id: lower_id.clone(),
                    kind: ActionKind::LowerSource,
                    command_identity: "lower_source".to_string(),
                    inputs: vec![ArtifactRef::source("fixtures/f.rs")],
                    outputs: vec![ArtifactRef::declared(&lower_id)],
                    compiler_work: Some(lower_descriptor),
                },
                Action {
                    id: transform_id.clone(),
                    kind: ActionKind::TransformFunction,
                    command_identity: "transform_function".to_string(),
                    inputs: vec![ArtifactRef::declared(&lower_id)],
                    // A producer's own verified id must be one of its own
                    // published outputs (issue #27 review:
                    // `OutputIdentityNotPublished`) -- not an unrelated
                    // logical name, or a producer's own semantic change
                    // would never propagate to what a consumer resolves.
                    outputs: vec![ArtifactRef::declared(&transform_id)],
                    compiler_work: Some(transform_descriptor.clone()),
                },
            ],
        );

        let outcome = call_planner(&bin, &input).unwrap();
        match outcome {
            PlanOutcome::Planned(plan) => {
                assert_eq!(
                    plan.ordered_actions,
                    vec![lower_id.clone(), transform_id.clone()],
                    "a compiler-work action's dependency must still order purely from \
                     inputs/outputs matching, same as a delegated-build action"
                );
                assert_eq!(
                    plan.actions[&transform_id].compiler_work.as_ref(),
                    Some(&transform_descriptor),
                    "the descriptor must round-trip through the real Nim binary byte-for-byte"
                );
                assert!(
                    validate(&plan, &input).is_ok(),
                    "a real, well-formed compiler-work plan must pass the full contract \
                     (presence-per-kind, schema version, recomputed identity, semantic \
                     dependency correspondence), not just structural round-tripping"
                );
            }
            PlanOutcome::Rejected(r) => panic!("expected a plan, got a rejection: {r:?}"),
        }
    }

    #[test]
    fn call_planner_against_a_missing_binary_is_a_structural_error_never_a_fallback_plan() {
        let missing = std::env::temp_dir().join(format!(
            "laminaria-plan-test-missing-planner-{}",
            std::process::id()
        ));
        let result = call_planner(&missing, &sample_input());
        assert!(
            matches!(result, Err(PlannerCallError::Spawn(_))),
            "a missing planner binary must be a structural Err, never a Rust-computed plan"
        );
    }
}

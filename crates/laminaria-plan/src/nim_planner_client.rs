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
    use std::path::PathBuf;

    /// Locates (building first if necessary) the real `laminaria-planner`
    /// binary under `nim-planner/`, so these tests exercise the actual
    /// Nim kernel via the real subprocess boundary -- not a mocked
    /// stand-in for it. Invokes `nim c` directly, not `nimble build`:
    /// `crates/laminaria-run/src/self_build.rs` found that this repo's
    /// `nimble` (v0.22.2) exits `0` even after printing a build-failure
    /// message on a genuine compile error, so `nim c`'s own reliable
    /// exit code is used here too, for the same reason.
    #[cfg(unix)]
    fn real_planner_binary() -> PathBuf {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let nim_planner_dir = repo_root.join("nim-planner");
        let bin = nim_planner_dir.join("bin/laminaria-planner");
        if !bin.is_file() {
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
        }
        assert!(
            bin.is_file(),
            "expected {} to exist after building it",
            bin.display()
        );
        bin
    }

    fn sample_input() -> PlanningInput {
        PlanningInput::new(
            vec!["host-bin".to_string()],
            vec![
                Action {
                    id: "compile-rust-host".to_string(),
                    kind: ActionKind::CargoBuild,
                    command_identity: "cargo build".to_string(),
                    inputs: vec![],
                    outputs: vec![ArtifactRef::declared("host-bin")],
                },
                Action {
                    id: "compile-nim-planner".to_string(),
                    kind: ActionKind::NimBuild,
                    command_identity: "nimble build".to_string(),
                    inputs: vec![],
                    outputs: vec![ArtifactRef::declared("planner-bin")],
                },
                Action {
                    id: "integrate".to_string(),
                    kind: ActionKind::Integrate,
                    command_identity: "assemble generation root".to_string(),
                    inputs: vec![
                        ArtifactRef::declared("host-bin"),
                        ArtifactRef::declared("planner-bin"),
                    ],
                    outputs: vec![],
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
        let input = PlanningInput::new(
            vec![],
            vec![
                Action {
                    id: "a".to_string(),
                    kind: ActionKind::NimBuild,
                    command_identity: "a".to_string(),
                    inputs: vec![ArtifactRef::declared("out-b")],
                    outputs: vec![ArtifactRef::declared("out-a")],
                },
                Action {
                    id: "b".to_string(),
                    kind: ActionKind::NimBuild,
                    command_identity: "b".to_string(),
                    inputs: vec![ArtifactRef::declared("out-a")],
                    outputs: vec![ArtifactRef::declared("out-b")],
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

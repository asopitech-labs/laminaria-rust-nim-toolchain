//! Issue #36 T1: the Rust-side client for the session-scoped
//! `laminaria-incremental-planner` binary (`nim-planner/src/
//! laminaria_incremental_planner.nim`) -- spawns it once per session,
//! keeps it alive across `StartSession`/`ApplyDelta`/`CloseSession`, one
//! JSON command per stdin line and one JSON response per stdout line,
//! matching `crates/laminaria-plan/src/nim_planner_client.rs`'s own
//! "subprocess + JSON, never a Rust-computed substitute" discipline --
//! the only difference is that this client keeps the same child process
//! alive across many round trips instead of spawning one per call.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use laminaria_plan::incremental::{
    DemandReference, IncrementalPlannerCommand, IncrementalPlannerResponse, PlanningEvent,
    INCREMENTAL_PROTOCOL_SCHEMA_VERSION,
};
use laminaria_plan::PlanningInput;

/// Looks for `laminaria-incremental-planner`(`.exe`) next to the
/// currently running executable -- the same sibling-binary convention
/// `nim_planner_client::find_planner_binary` already uses for the
/// one-shot `laminaria-planner`.
pub fn find_incremental_planner_binary() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let dir = current_exe.parent()?;
    let candidate = dir.join(if cfg!(windows) {
        "laminaria-incremental-planner.exe"
    } else {
        "laminaria-incremental-planner"
    });
    candidate.is_file().then_some(candidate)
}

#[derive(Debug)]
pub enum IncrementalSessionError {
    Spawn(std::io::Error),
    WriteCommand(std::io::Error),
    /// The child exited (or its stdout closed) before a response line
    /// for a command that was actually sent -- never silently treated as
    /// "no response needed."
    ChildExited,
    ReadResponse(std::io::Error),
    MalformedResponse(serde_json::Error),
}

impl std::fmt::Display for IncrementalSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IncrementalSessionError::Spawn(e) => {
                write!(f, "failed to spawn laminaria-incremental-planner: {e}")
            }
            IncrementalSessionError::WriteCommand(e) => {
                write!(
                    f,
                    "failed to write a command to the incremental planner's stdin: {e}"
                )
            }
            IncrementalSessionError::ChildExited => write!(
                f,
                "laminaria-incremental-planner exited before responding to a command that was sent"
            ),
            IncrementalSessionError::ReadResponse(e) => {
                write!(
                    f,
                    "failed to read a response from the incremental planner's stdout: {e}"
                )
            }
            IncrementalSessionError::MalformedResponse(e) => write!(
                f,
                "the incremental planner's stdout line was not a well-formed \
                 IncrementalPlannerResponse: {e}"
            ),
        }
    }
}

impl std::error::Error for IncrementalSessionError {}

/// A live `laminaria-incremental-planner` session: one child process,
/// kept alive from `start` through `close`. `command_index` is owned and
/// incremented here (the same value the child echoes back as
/// `in_reply_to_command_index`), so a caller never has to track it
/// itself.
pub struct IncrementalSessionClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    session_id: String,
    next_command_index: u64,
}

impl IncrementalSessionClient {
    /// Spawns `planner_binary` and immediately sends `StartSession` with
    /// `initial_graph`/`initial_demands` -- the T0 contract's own rule
    /// that a session's very first command is always `StartSession`
    /// (§3.1), so there is no way to construct a client without one.
    pub fn start(
        planner_binary: &Path,
        session_id: impl Into<String>,
        initial_graph: PlanningInput,
        initial_demands: Vec<DemandReference>,
    ) -> Result<(Self, IncrementalPlannerResponse), IncrementalSessionError> {
        let mut child = Command::new(planner_binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(IncrementalSessionError::Spawn)?;

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout was piped"));
        let session_id = session_id.into();

        let mut client = IncrementalSessionClient {
            child,
            stdin,
            stdout,
            session_id,
            next_command_index: 0,
        };
        let response = client.send(IncrementalPlannerCommand::StartSession {
            schema_version: INCREMENTAL_PROTOCOL_SCHEMA_VERSION.to_string(),
            session_id: client.session_id.clone(),
            command_index: client.next_command_index,
            initial_graph,
            initial_demands,
        })?;
        Ok((client, response))
    }

    fn send(
        &mut self,
        command: IncrementalPlannerCommand,
    ) -> Result<IncrementalPlannerResponse, IncrementalSessionError> {
        let mut line =
            serde_json::to_vec(&command).expect("IncrementalPlannerCommand always serializes");
        line.push(b'\n');
        self.stdin
            .write_all(&line)
            .map_err(IncrementalSessionError::WriteCommand)?;
        self.stdin
            .flush()
            .map_err(IncrementalSessionError::WriteCommand)?;

        let mut response_line = String::new();
        let bytes_read = self
            .stdout
            .read_line(&mut response_line)
            .map_err(IncrementalSessionError::ReadResponse)?;
        if bytes_read == 0 {
            return Err(IncrementalSessionError::ChildExited);
        }

        let response: IncrementalPlannerResponse =
            serde_json::from_str(response_line.trim_end())
                .map_err(IncrementalSessionError::MalformedResponse)?;
        Ok(response)
    }

    pub fn apply_delta(
        &mut self,
        event: PlanningEvent,
    ) -> Result<IncrementalPlannerResponse, IncrementalSessionError> {
        self.next_command_index += 1;
        self.send(IncrementalPlannerCommand::ApplyDelta {
            schema_version: INCREMENTAL_PROTOCOL_SCHEMA_VERSION.to_string(),
            session_id: self.session_id.clone(),
            command_index: self.next_command_index,
            event,
        })
    }

    /// Sends `CloseSession` and waits for the child to exit cleanly.
    /// Consumes `self` -- a closed session's client cannot be reused,
    /// matching the wire protocol's own one-way `CloseSession` semantics
    /// (T0 §3.1: the process quits(0) immediately after acknowledging
    /// it).
    pub fn close(mut self) -> Result<IncrementalPlannerResponse, IncrementalSessionError> {
        self.next_command_index += 1;
        let response = self.send(IncrementalPlannerCommand::CloseSession {
            schema_version: INCREMENTAL_PROTOCOL_SCHEMA_VERSION.to_string(),
            session_id: self.session_id.clone(),
            command_index: self.next_command_index,
        })?;
        let _ = self.child.wait();
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use laminaria_plan::{Action, ActionKind, ArtifactRef};
    #[cfg(unix)]
    use std::sync::OnceLock;

    /// Builds (once per test binary process, `OnceLock`-guarded the same
    /// way `nim_planner_client::tests::real_planner_binary` is to avoid
    /// a concurrent-build race a prior review already caught for that
    /// sibling binary) the real `laminaria-incremental-planner` binary.
    #[cfg(unix)]
    fn real_incremental_planner_binary() -> PathBuf {
        static BUILT: OnceLock<PathBuf> = OnceLock::new();
        BUILT
            .get_or_init(|| {
                let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .canonicalize()
                    .unwrap();
                let nim_planner_dir = repo_root.join("nim-planner");
                let bin = nim_planner_dir.join("bin/laminaria-incremental-planner");
                let status = Command::new("nim")
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
                assert!(bin.is_file(), "expected {} to exist", bin.display());
                bin
            })
            .clone()
    }

    #[cfg(unix)]
    fn lower_action(id: &str, inputs: Vec<ArtifactRef>) -> Action {
        Action {
            id: id.to_string(),
            kind: ActionKind::LowerSource,
            command_identity: "lower_source".to_string(),
            inputs,
            outputs: vec![ArtifactRef::declared(id)],
            compiler_work: None,
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_real_session_starts_applies_a_completion_and_closes_cleanly() {
        let bin = real_incremental_planner_binary();
        let lower = lower_action("lower", vec![ArtifactRef::source("f.rs")]);
        let validate = lower_action("validate", vec![ArtifactRef::declared("lower")]);

        let (mut client, start_response) = IncrementalSessionClient::start(
            &bin,
            "s1",
            PlanningInput::new(vec!["validate".to_string()], vec![lower, validate]),
            vec![],
        )
        .unwrap();

        match start_response {
            IncrementalPlannerResponse::PlanDelta {
                changed_actions, ..
            } => {
                assert_eq!(changed_actions.len(), 2);
            }
            other => panic!("expected PlanDelta, got {other:?}"),
        }

        let completed = PlanningEvent {
            event_id: "e1".to_string(),
            sequence_number: 1,
            planning_generation: 0,
            emitted_at_unix_ns: 1,
            kind: laminaria_plan::incremental::PlanningEventKind::ProducerCompleted {
                artifact_id: "lower".to_string(),
                produced_by_action_id: "lower".to_string(),
            },
        };
        let delta_response = client.apply_delta(completed).unwrap();
        match delta_response {
            IncrementalPlannerResponse::PlanDelta {
                changed_actions, ..
            } => {
                assert!(changed_actions.iter().any(|c| c.action_id == "validate"
                    && c.to_state == Some(laminaria_plan::incremental::ActionState::Ready)));
            }
            other => panic!("expected PlanDelta, got {other:?}"),
        }

        let close_response = client.close().unwrap();
        assert!(matches!(
            close_response,
            IncrementalPlannerResponse::SessionClosed { .. }
        ));
    }

    #[test]
    #[cfg(unix)]
    fn a_missing_producer_at_start_session_is_a_real_rejected_response() {
        let bin = real_incremental_planner_binary();
        let broken = lower_action("broken", vec![ArtifactRef::declared("nonexistent")]);
        let (_client, start_response) = IncrementalSessionClient::start(
            &bin,
            "s2",
            PlanningInput::new(vec!["broken".to_string()], vec![broken]),
            vec![],
        )
        .unwrap();
        match start_response {
            IncrementalPlannerResponse::Rejected { reason_kind, .. } => {
                assert_eq!(
                    reason_kind,
                    laminaria_plan::RejectionReasonKind::MissingProducer
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_binary_is_a_structural_error_never_a_fallback_session() {
        let missing = std::env::temp_dir().join(format!(
            "laminaria-run-test-missing-incremental-planner-{}",
            std::process::id()
        ));
        let result = IncrementalSessionClient::start(
            &missing,
            "s-missing",
            PlanningInput::new(vec![], vec![]),
            vec![],
        );
        assert!(matches!(result, Err(IncrementalSessionError::Spawn(_))));
    }
}

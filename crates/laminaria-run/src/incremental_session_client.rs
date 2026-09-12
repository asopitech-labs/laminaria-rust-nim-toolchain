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
    /// The response's own `schema_version`/`session_id`/
    /// `in_reply_to_command_index` didn't match what this client sent --
    /// a review caught that this client accepted *any* well-formed
    /// response without ever checking these against the command it was
    /// actually a reply to, the Rust-side half of the same wire-identity
    /// gap `incremental_kernel.checkEnvelope` closes on the Nim side.
    UnexpectedResponseIdentity {
        detail: String,
    },
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
            IncrementalSessionError::UnexpectedResponseIdentity { detail } => {
                write!(f, "response identity mismatch: {detail}")
            }
        }
    }
}

impl std::error::Error for IncrementalSessionError {}

/// Extracts `(schema_version, session_id, in_reply_to_command_index)`
/// from any response variant -- all three are present on every variant
/// (T0 §3.2), just not through one shared field a `match` can avoid.
fn response_identity(response: &IncrementalPlannerResponse) -> (&str, &str, u64) {
    match response {
        IncrementalPlannerResponse::PlanDelta {
            schema_version,
            session_id,
            in_reply_to_command_index,
            ..
        }
        | IncrementalPlannerResponse::Rejected {
            schema_version,
            session_id,
            in_reply_to_command_index,
            ..
        }
        | IncrementalPlannerResponse::SessionClosed {
            schema_version,
            session_id,
            in_reply_to_command_index,
        } => (schema_version, session_id, *in_reply_to_command_index),
    }
}

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
        let command_index = command.command_index();
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

        // Review-caught gap: verify the response is actually a reply to
        // *this* command, on *this* session, speaking the protocol
        // version this client itself sent -- never trust a well-formed
        // response's own claimed identity without checking it against
        // what was actually sent.
        let (resp_schema_version, resp_session_id, resp_in_reply_to) = response_identity(&response);
        if resp_schema_version != INCREMENTAL_PROTOCOL_SCHEMA_VERSION {
            return Err(IncrementalSessionError::UnexpectedResponseIdentity {
                detail: format!(
                    "expected schema_version {INCREMENTAL_PROTOCOL_SCHEMA_VERSION:?}, got {resp_schema_version:?}"
                ),
            });
        }
        if resp_session_id != self.session_id {
            return Err(IncrementalSessionError::UnexpectedResponseIdentity {
                detail: format!(
                    "expected session_id {:?}, got {resp_session_id:?}",
                    self.session_id
                ),
            });
        }
        if resp_in_reply_to != command_index {
            return Err(IncrementalSessionError::UnexpectedResponseIdentity {
                detail: format!(
                    "expected in_reply_to_command_index {command_index}, got {resp_in_reply_to}"
                ),
            });
        }

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

    /// A raw JSON-lines session, deliberately bypassing
    /// `IncrementalSessionClient`'s own automatic envelope construction
    /// and response-identity checking -- `IncrementalSessionClient`
    /// itself always builds a well-formed envelope, so it cannot be used
    /// to send the deliberately identity-violating commands these tests
    /// need to confirm the *binary* rejects. Sends exactly the raw JSON
    /// text given and returns exactly the raw JSON text received, one
    /// line each -- the same shape as this session's own manual
    /// stdin-piping verification, just automated.
    #[cfg(unix)]
    struct RawSession {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
    }

    #[cfg(unix)]
    impl RawSession {
        fn spawn(bin: &Path) -> Self {
            let mut child = Command::new(bin)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdin = child.stdin.take().unwrap();
            let stdout = BufReader::new(child.stdout.take().unwrap());
            RawSession {
                child,
                stdin,
                stdout,
            }
        }

        fn send_raw(&mut self, json_line: &str) -> serde_json::Value {
            self.stdin.write_all(json_line.as_bytes()).unwrap();
            self.stdin.write_all(b"\n").unwrap();
            self.stdin.flush().unwrap();
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).unwrap();
            assert!(
                n > 0,
                "the binary exited without responding to: {json_line}"
            );
            serde_json::from_str(line.trim_end()).unwrap()
        }
    }

    #[cfg(unix)]
    impl Drop for RawSession {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
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

    // --- Wire-identity tests (issue #36 review: schema_version/session_id/
    // command_index were never actually validated by the real binary --
    // reproduced here against the real binary itself, not just Nim unit
    // tests of the pure checkEnvelope logic, exactly as requested). ---

    #[cfg(unix)]
    const EMPTY_GRAPH: &str = r#"{"schema_version":"0.3.0","demanded_artifacts":[],"actions":[]}"#;

    #[test]
    #[cfg(unix)]
    fn a_wrong_schema_version_is_rejected_as_invalid_contract_version() {
        let bin = real_incremental_planner_binary();
        let mut raw = RawSession::spawn(&bin);
        let response = raw.send_raw(&format!(
            r#"{{"schema_version":"wrong-version","session_id":"s1","command_index":0,"kind":"start_session","initial_graph":{EMPTY_GRAPH},"initial_demands":[]}}"#
        ));
        assert_eq!(response["kind"], "rejected");
        assert_eq!(response["reason_kind"], "invalid_contract_version");
    }

    #[test]
    #[cfg(unix)]
    fn a_start_session_at_a_nonzero_command_index_is_rejected() {
        let bin = real_incremental_planner_binary();
        let mut raw = RawSession::spawn(&bin);
        let response = raw.send_raw(&format!(
            r#"{{"schema_version":"0.1.0","session_id":"s1","command_index":7,"kind":"start_session","initial_graph":{EMPTY_GRAPH},"initial_demands":[]}}"#
        ));
        assert_eq!(response["kind"], "rejected");
        assert_eq!(response["reason_kind"], "unsupported_input");
    }

    #[test]
    #[cfg(unix)]
    fn a_close_session_with_a_different_session_id_is_rejected() {
        let bin = real_incremental_planner_binary();
        let mut raw = RawSession::spawn(&bin);
        let start = raw.send_raw(&format!(
            r#"{{"schema_version":"0.1.0","session_id":"s1","command_index":0,"kind":"start_session","initial_graph":{EMPTY_GRAPH},"initial_demands":[]}}"#
        ));
        assert_eq!(start["kind"], "plan_delta");
        let response = raw.send_raw(
            r#"{"schema_version":"0.1.0","session_id":"s2","command_index":1,"kind":"close_session"}"#,
        );
        assert_eq!(response["kind"], "rejected");
        assert_eq!(response["reason_kind"], "unsupported_input");
        assert!(response["reason_detail"]
            .as_str()
            .unwrap()
            .contains("session_id mismatch"));
    }

    #[test]
    #[cfg(unix)]
    fn a_non_sequential_command_index_is_rejected() {
        let bin = real_incremental_planner_binary();
        let mut raw = RawSession::spawn(&bin);
        let start = raw.send_raw(&format!(
            r#"{{"schema_version":"0.1.0","session_id":"s1","command_index":0,"kind":"start_session","initial_graph":{EMPTY_GRAPH},"initial_demands":[]}}"#
        ));
        assert_eq!(start["kind"], "plan_delta");
        let response = raw.send_raw(
            r#"{"schema_version":"0.1.0","session_id":"s1","command_index":99,"kind":"close_session"}"#,
        );
        assert_eq!(response["kind"], "rejected");
        assert_eq!(response["reason_kind"], "unsupported_input");
        assert!(response["reason_detail"]
            .as_str()
            .unwrap()
            .contains("command_index must be sequential"));
    }

    #[test]
    #[cfg(unix)]
    fn a_second_start_session_for_the_same_session_is_rejected() {
        let bin = real_incremental_planner_binary();
        let mut raw = RawSession::spawn(&bin);
        let start = raw.send_raw(&format!(
            r#"{{"schema_version":"0.1.0","session_id":"s1","command_index":0,"kind":"start_session","initial_graph":{EMPTY_GRAPH},"initial_demands":[]}}"#
        ));
        assert_eq!(start["kind"], "plan_delta");
        let response = raw.send_raw(&format!(
            r#"{{"schema_version":"0.1.0","session_id":"s1","command_index":1,"kind":"start_session","initial_graph":{EMPTY_GRAPH},"initial_demands":[]}}"#
        ));
        assert_eq!(response["kind"], "rejected");
        assert_eq!(response["reason_kind"], "unsupported_input");
    }

    #[test]
    #[cfg(unix)]
    fn a_rejected_envelope_command_does_not_advance_the_expected_command_index() {
        // After a rejected (skipped-ahead) command_index, the *correct*
        // next index is still accepted -- confirms checkEnvelope's own
        // "never mutate on rejection" contract holds through the real
        // binary, not just in the pure-function Nim unit test.
        let bin = real_incremental_planner_binary();
        let mut raw = RawSession::spawn(&bin);
        let start = raw.send_raw(&format!(
            r#"{{"schema_version":"0.1.0","session_id":"s1","command_index":0,"kind":"start_session","initial_graph":{EMPTY_GRAPH},"initial_demands":[]}}"#
        ));
        assert_eq!(start["kind"], "plan_delta");
        let skipped = raw.send_raw(
            r#"{"schema_version":"0.1.0","session_id":"s1","command_index":99,"kind":"close_session"}"#,
        );
        assert_eq!(skipped["kind"], "rejected");
        let correct = raw.send_raw(
            r#"{"schema_version":"0.1.0","session_id":"s1","command_index":1,"kind":"close_session"}"#,
        );
        assert_eq!(correct["kind"], "session_closed");
    }

    #[test]
    #[cfg(unix)]
    fn the_client_itself_rejects_a_response_whose_identity_does_not_match_what_it_sent() {
        // IncrementalSessionClient::send always builds a well-formed
        // command, so this exercises the client's own response-identity
        // check (not the binary's) via a deliberately mismatched
        // self.session_id set up by hand, bypassing `start`'s own
        // (correct) construction.
        let bin = real_incremental_planner_binary();
        let (mut client, _start_response) =
            IncrementalSessionClient::start(&bin, "s1", PlanningInput::new(vec![], vec![]), vec![])
                .unwrap();
        // Simulate a client that (incorrectly) believes it owns a
        // different session than the one it actually started.
        client.session_id = "not-actually-s1".to_string();
        let result = client.apply_delta(PlanningEvent {
            event_id: "e1".to_string(),
            sequence_number: 1,
            planning_generation: 0,
            emitted_at_unix_ns: 1,
            kind: laminaria_plan::incremental::PlanningEventKind::DemandRequested {
                artifact_id: "x".to_string(),
                requested_by: "consumer-a".to_string(),
            },
        });
        assert!(matches!(
            result,
            Err(IncrementalSessionError::UnexpectedResponseIdentity { .. })
        ));
    }
}

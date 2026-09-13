//! Issue #48 (G1): the one seam through which G1's own ecosystem-fact
//! ingestion (`cross_ecosystem_ingest`) ever spawns a process. G1's own
//! fixed decision permits exactly three read-only metadata/version
//! queries and forbids every compiler, archiver, linker, package
//! build, package install, or build-script invocation -- see this
//! module's own [`is_permitted`], the single place that boundary is
//! enforced, and `crates/laminaria-run/src/cross_ecosystem_ingest.rs`'s
//! own top-of-file doc comment for why no other subprocess exists in
//! G1 at all (declared-export facts come from pure text scanning in
//! `laminaria-ir`, never from compiling a candidate and inspecting the
//! result).
//!
//! [`RealCommandRunner`] is what production ingestion uses; it refuses
//! (returns [`IngestError::ForbiddenCommand`] for) anything outside the
//! permitted set, so the boundary holds even if a future edit to this
//! crate tries to call something else. [`RecordingCommandRunner`] is
//! what the required tests inject instead: it delegates permitted
//! commands to the real runner (so ingestion still produces real facts)
//! while recording every attempted command and panicking immediately on
//! a forbidden one, so a regression is caught at the exact call site
//! rather than only by a later assertion.

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

#[derive(Debug)]
pub enum IngestError {
    Io(String),
    Parse(String),
    ForbiddenCommand(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestError::Io(detail) => write!(f, "ingestion I/O error: {detail}"),
            IngestError::Parse(detail) => write!(f, "ingestion parse error: {detail}"),
            IngestError::ForbiddenCommand(detail) => {
                write!(f, "forbidden command in G1 ingestion: {detail}")
            }
        }
    }
}

impl std::error::Error for IngestError {}

/// The permitted-command allowlist issue #48 fixes: `cargo metadata
/// --no-deps ...`, `nimble dump --json`, and `rustc -vV`. Anything else
/// -- `cargo build`/`cargo run`, `nimble build`/`nimble install`,
/// `rustc` with any other flags, `nim c`/`nim cpp`/`nlvm`,
/// `cc`/`clang`/`gcc`/`c++ -c` or equivalent, `ar`, a linker, or `nm` --
/// is forbidden.
///
/// `nimble dump --json` is exactly the command issue #48 names --
/// see `fixtures/cross-ecosystem-native-executable/nimble/doubler/nimble.lock`'s
/// own doc comment (in this repo's fixture README) for why a real
/// `nimble.lock` file is required alongside it: recent nimble
/// ("vnext") versions have `dump` itself perform an implicit
/// "resolve/manage a matching nim toolchain" step to populate its own
/// `nimDir` field. Without a lock file pinning an exact revision, that
/// step can crash (`--offline`/`--disableNimBinaries` were tried and
/// made this *worse*, not better -- they disable the very
/// lock-file-aware resolution path that succeeds); with the lock file
/// present, plain `nimble dump --json` resolves it correctly and
/// reports unchanged manifest fields (verified against real CI
/// output).
pub(crate) fn is_permitted(program: &str, args: &[&str]) -> bool {
    match program {
        "cargo" => {
            args.first() == Some(&"metadata")
                && args.contains(&"--no-deps")
                && args.windows(2).any(|w| w == ["--format-version", "1"])
        }
        "nimble" => args == ["dump", "--json"],
        "rustc" => args == ["-vV"],
        _ => false,
    }
}

/// The one seam G1 ingestion spawns a process through.
pub trait CommandRunner {
    fn run(&self, program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, IngestError>;
}

/// The production `CommandRunner`: executes exactly the permitted
/// metadata/version queries and refuses everything else structurally,
/// not merely by convention.
#[derive(Debug, Default)]
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, IngestError> {
        if !is_permitted(program, args) {
            return Err(IngestError::ForbiddenCommand(format!("{program} {args:?}")));
        }
        let mut cmd = Command::new(program);
        cmd.args(args);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd
            .output()
            .map_err(|e| IngestError::Io(format!("failed to spawn {program}: {e}")))?;
        if !output.status.success() {
            return Err(IngestError::Io(format!(
                "{program} {args:?} exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// The test double required tests inject: records every attempted
/// command (permitted or not) and panics immediately if a forbidden one
/// is ever requested, rather than only failing a later assertion.
/// Permitted commands are still delegated to a real
/// [`RealCommandRunner`], so a test using this runner still exercises
/// production ingestion against real `cargo metadata`/`nimble dump
/// --json`/`rustc -vV` output.
#[derive(Debug, Default)]
pub struct RecordingCommandRunner {
    inner: RealCommandRunner,
    recorded: Mutex<Vec<(String, Vec<String>)>>,
}

impl RecordingCommandRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every command attempted through this runner, in call order.
    pub fn recorded_commands(&self) -> Vec<(String, Vec<String>)> {
        self.recorded
            .lock()
            .expect("recorder mutex poisoned")
            .clone()
    }
}

impl CommandRunner for RecordingCommandRunner {
    fn run(&self, program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, IngestError> {
        self.recorded
            .lock()
            .expect("recorder mutex poisoned")
            .push((
                program.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
        if !is_permitted(program, args) {
            panic!("forbidden command attempted in G1 ingestion: {program} {args:?}");
        }
        self.inner.run(program, args, cwd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_permitted_commands_are_recognized() {
        assert!(is_permitted(
            "cargo",
            &[
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
                "x"
            ]
        ));
        assert!(is_permitted("nimble", &["dump", "--json"]));
        assert!(is_permitted("rustc", &["-vV"]));
    }

    #[test]
    fn every_forbidden_command_the_work_instruction_names_is_rejected() {
        for (program, args) in [
            ("cargo", vec!["build"]),
            ("cargo", vec!["run"]),
            ("nimble", vec!["build"]),
            ("nimble", vec!["install"]),
            ("rustc", vec!["main.rs"]),
            ("nim", vec!["c", "main.nim"]),
            ("nim", vec!["cpp", "main.nim"]),
            ("nlvm", vec!["main.nim"]),
            ("cc", vec!["-c", "a.c"]),
            ("clang", vec!["-c", "a.c"]),
            ("gcc", vec!["-c", "a.c"]),
            ("c++", vec!["-c", "a.cpp"]),
            ("ar", vec!["rcs", "a.a", "a.o"]),
            ("ld", vec!["a.o"]),
            ("nm", vec!["-g", "a.a"]),
        ] {
            assert!(
                !is_permitted(program, &args),
                "{program} {args:?} must be forbidden"
            );
        }
    }

    #[test]
    fn the_real_runner_refuses_a_forbidden_command_without_spawning_it() {
        let runner = RealCommandRunner;
        let result = runner.run("cc", &["-c", "a.c"], None);
        assert!(matches!(result, Err(IngestError::ForbiddenCommand(_))));
    }

    #[test]
    fn the_recording_runner_records_every_attempted_command() {
        let runner = RecordingCommandRunner::new();
        let _ = runner.run("rustc", &["-vV"], None);
        let recorded = runner.recorded_commands();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "rustc");
    }

    #[test]
    #[should_panic(expected = "forbidden command attempted in G1 ingestion")]
    fn the_recording_runner_panics_immediately_on_a_forbidden_command() {
        let runner = RecordingCommandRunner::new();
        let _ = runner.run("cc", &["-c", "a.c"], None);
    }
}

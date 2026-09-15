//! Issue #46 (G2) Checkpoint 1: a first real, production evidence
//! producer wired to issue #48 (G1)'s own obligation-lifecycle
//! transition
//! (`laminaria_plan::dependency_graph::PositiveClosure::discharge_obligation`,
//! `laminaria_plan::dependency_graph::PositiveClosure::externalize_obligation`).
//!
//! Unlike G1's `command_runner::RealCommandRunner` (deliberately
//! forbidden from ever invoking a compiler/archiver/linker -- see that
//! module's own doc comment), this module's whole purpose is to
//! actually execute one of G1's own typed `RequiredAction`s for real
//! and turn its observed, independently-verified output into discharge
//! evidence. Generating a `RequiredAction` is not discharge evidence
//! (issue #48's own words); neither is this module's own act of
//! spawning `cc`/`ar` -- only the real file this module then reads back
//! and inspects (`nm`, a real SHA-256) is.
//!
//! **Scope of this first slice**: only the fixture's C package
//! `cadd@1` -- the `compile-c-object:cadd@1.0.0` ->
//! `archive-static-library:cadd@1.0.0` action pair
//! `laminaria_plan::dependency_graph::resolve` already emits for it.
//! The remaining `RequiredActionKind` variants this fixture's positive
//! plan also requires (`CompileRustObject`, `CompileNimStaticLibrary`,
//! `CompileCppAdapterObject`, `LinkNativeExecutable`,
//! `PreflightRuntimeContract`, `PublishProvenance`) are real, disclosed,
//! not-yet-implemented gaps -- the rest of issue #46's own action
//! chain, deliberately left for a following checkpoint rather than
//! claimed here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use laminaria_plan::dependency_graph::{DischargeKind, LifecycleViolation, PositiveClosure};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum G2Error {
    Io(String),
    ToolFailed {
        program: String,
        detail: String,
    },
    /// The real archive was produced, but a real `nm` read of it does
    /// not show the expected symbol as defined -- the compile/archive
    /// step did not actually produce what G1's own source-declared
    /// facts required.
    ExpectedSymbolNotDefined {
        archive: PathBuf,
        symbol: String,
    },
    Discharge(LifecycleViolation),
}

impl std::fmt::Display for G2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            G2Error::Io(detail) => write!(f, "G2 execution I/O error: {detail}"),
            G2Error::ToolFailed { program, detail } => {
                write!(f, "G2 execution: '{program}' failed: {detail}")
            }
            G2Error::ExpectedSymbolNotDefined { archive, symbol } => write!(
                f,
                "G2 execution: real archive '{}' does not define expected symbol '{symbol}'",
                archive.display()
            ),
            G2Error::Discharge(violation) => write!(f, "G2 execution: {violation}"),
        }
    }
}

impl std::error::Error for G2Error {}

impl From<LifecycleViolation> for G2Error {
    fn from(violation: LifecycleViolation) -> Self {
        G2Error::Discharge(violation)
    }
}

/// Real, observed evidence that a C source was actually compiled and
/// archived -- never a path string alone (issue #48's own "a path
/// string is not artifact evidence").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CObjectArchiveEvidence {
    pub object_path: PathBuf,
    pub archive_path: PathBuf,
    pub archive_size_bytes: u64,
    pub archive_sha256: String,
    /// The exact symbol `nm` confirmed as defined in the real archive.
    pub confirmed_defined_symbol: String,
}

impl CObjectArchiveEvidence {
    fn as_discharge_evidence(&self) -> String {
        format!(
            "real cc+ar: {} -> {} ({} bytes, sha256={}); nm confirms '{}' defined",
            self.object_path.display(),
            self.archive_path.display(),
            self.archive_size_bytes,
            self.archive_sha256,
            self.confirmed_defined_symbol
        )
    }
}

fn run_tool(program: &str, args: &[&str]) -> Result<String, G2Error> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| G2Error::Io(format!("failed to spawn {program}: {e}")))?;
    if !output.status.success() {
        return Err(G2Error::ToolFailed {
            program: program.to_string(),
            detail: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn sha256_file(path: &Path) -> Result<String, G2Error> {
    let bytes = fs::read(path).map_err(|e| G2Error::Io(format!("{}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Real `nm` inspection of a real archive -- G2's own legitimate
/// artifact-inspection boundary (unlike G1, which must never inspect a
/// compiled artifact's symbol table; see `command_runner`'s own doc
/// comment). Returns `Ok(())` only if some line of `nm`'s real output
/// names `symbol` as a *defined* entry (a `T`/`t` -- text/code -- type
/// letter, not `U` undefined) for this specific archive, on the
/// platforms this checkpoint has been exercised on (`nm -Ug`, BSD/macOS
/// and GNU `nm` both support `-g` "external symbols only"; the exact
/// column layout is parsed leniently rather than assumed byte-for-byte
/// identical across platforms).
fn verify_symbol_defined(archive_path: &Path, symbol: &str) -> Result<(), G2Error> {
    let nm_output = run_tool("nm", &["-g", &archive_path.to_string_lossy()])?;
    let underscored = format!("_{symbol}");
    let defined = nm_output.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let _address = fields.next();
        let type_letter = fields.next();
        let name = fields.next();
        matches!(type_letter, Some("T") | Some("t"))
            && matches!(name, Some(n) if n == symbol || n == underscored)
    });
    if defined {
        Ok(())
    } else {
        Err(G2Error::ExpectedSymbolNotDefined {
            archive: archive_path.to_path_buf(),
            symbol: symbol.to_string(),
        })
    }
}

/// Actually executes the real `compile-c-object:cadd@1.0.0` and
/// `archive-static-library:cadd@1.0.0` `RequiredAction` pair
/// (`cc -c` then `ar rcs`) against the fixture's real `cadd@1` C
/// sources, verifies the real result independently (`nm`, a real
/// SHA-256), and discharges the closure's
/// `ArtifactProduction:archive:cadd` obligation through
/// `archive-static-library:cadd@1.0.0` -- the exact action id G1 itself
/// emitted and recorded as that obligation's `required_action`. Fails
/// (and leaves the obligation untouched) if either real command fails,
/// if `nm` does not confirm `c_add` as defined in the real output, or
/// if `PositiveClosure::discharge_obligation` itself refuses the
/// transition (e.g. the obligation was already discharged).
pub fn compile_and_archive_cadd_v1(
    closure: &mut PositiveClosure,
    fixture_root: &Path,
    out_dir: &Path,
) -> Result<CObjectArchiveEvidence, G2Error> {
    fs::create_dir_all(out_dir).map_err(|e| G2Error::Io(format!("{}: {e}", out_dir.display())))?;

    let c_dir = fixture_root.join("c/cadd/v1");
    let c_source = c_dir.join("cadd.c");
    let object_path = out_dir.join("cadd.o");
    let archive_path = out_dir.join("libcadd.a");

    run_tool(
        "cc",
        &[
            "-c",
            &c_source.to_string_lossy(),
            "-I",
            &c_dir.to_string_lossy(),
            "-o",
            &object_path.to_string_lossy(),
        ],
    )?;

    // `ar` refuses to update an archive that already exists with stale
    // members from a previous run of this same function.
    let _ = fs::remove_file(&archive_path);
    run_tool(
        "ar",
        &[
            "rcs",
            &archive_path.to_string_lossy(),
            &object_path.to_string_lossy(),
        ],
    )?;

    verify_symbol_defined(&archive_path, "c_add")?;

    let archive_size_bytes = fs::metadata(&archive_path)
        .map_err(|e| G2Error::Io(format!("{}: {e}", archive_path.display())))?
        .len();
    let archive_sha256 = sha256_file(&archive_path)?;

    let evidence = CObjectArchiveEvidence {
        object_path,
        archive_path,
        archive_size_bytes,
        archive_sha256,
        confirmed_defined_symbol: "c_add".to_string(),
    };

    closure.discharge_obligation(
        "ArtifactProduction:archive:cadd",
        "archive-static-library:cadd@1.0.0",
        DischargeKind::StaticallyLinked,
        evidence.as_discharge_evidence(),
    )?;

    Ok(evidence)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::command_runner::RecordingCommandRunner;
    use crate::cross_ecosystem_ingest::{ingest_fixture_input, FixtureLayout};
    use laminaria_plan::dependency_graph::{resolve, ObligationState};

    /// The end-to-end Checkpoint 1 case: resolve a real G1 positive
    /// closure from the real fixture, then actually execute the real
    /// `cc`/`ar` action pair against real files on disk and discharge
    /// the corresponding obligation with the real, independently
    /// verified result -- not a synthetic evidence string, and not a
    /// test-only `CommandRunner` standing in for the real tool.
    #[test]
    fn g2_really_compiles_and_archives_cadd_v1_and_discharges_its_obligation() {
        let layout = FixtureLayout::discover();
        let runner = RecordingCommandRunner::new();
        let input = ingest_fixture_input(&runner, &layout, &["1.0.0"])
            .expect("must ingest the real positive fixture");
        let mut closure = resolve(&input).expect("must resolve a positive closure");

        let obligation_id = "ArtifactProduction:archive:cadd";
        assert_eq!(
            closure.obligations[obligation_id].state,
            ObligationState::Satisfied,
            "must start from G1's own terminal state"
        );

        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-g2-checkpoint-1-{}-{}",
            std::process::id(),
            "cadd-v1"
        ));
        let evidence = compile_and_archive_cadd_v1(&mut closure, &layout.root, &out_dir)
            .expect("real cc+ar execution and discharge must succeed");

        assert!(
            evidence.object_path.is_file(),
            "the real compiled object must exist on disk"
        );
        assert!(
            evidence.archive_path.is_file(),
            "the real archive must exist on disk"
        );
        assert!(evidence.archive_size_bytes > 0);
        assert_eq!(
            evidence.archive_sha256.len(),
            64,
            "must be a real hex SHA-256"
        );
        assert_eq!(evidence.confirmed_defined_symbol, "c_add");

        let obligation = &closure.obligations[obligation_id];
        assert_eq!(obligation.state, ObligationState::Discharged);
        assert_eq!(
            obligation.discharge_kind,
            Some(DischargeKind::StaticallyLinked)
        );
        assert_eq!(
            obligation.required_action.as_deref(),
            Some("archive-static-library:cadd@1.0.0")
        );
        assert!(obligation
            .evidence
            .as_deref()
            .unwrap()
            .contains(&evidence.archive_sha256));

        let _ = fs::remove_dir_all(&out_dir);
    }

    /// The negative-fixture closure never resolves at all (G1's own
    /// existing guarantee) -- this module's own `discharge` call is
    /// therefore never reachable for `cadd@2`, so there is no
    /// discharge-side test for it; recorded here only so a reader does
    /// not wonder why one is missing.
    #[test]
    fn g2_has_nothing_to_discharge_for_the_rejected_cadd_v2_case() {
        let layout = FixtureLayout::discover();
        let runner = RecordingCommandRunner::new();
        let input = ingest_fixture_input(&runner, &layout, &["2.0.0"])
            .expect("must ingest the real negative fixture");
        assert!(
            resolve(&input).is_err(),
            "cadd@2 alone must still fail to resolve, as G1 itself guarantees"
        );
    }

    /// A real `nm` inspection failing to confirm the expected symbol
    /// (a stale, unrelated archive) must refuse the discharge rather
    /// than trust the caller's own claim.
    #[test]
    fn a_real_archive_missing_the_expected_symbol_refuses_verification() {
        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-g2-checkpoint-1-negative-{}",
            std::process::id()
        ));
        fs::create_dir_all(&out_dir).unwrap();
        let unrelated_c = out_dir.join("unrelated.c");
        fs::write(&unrelated_c, "int unrelated_fn(void) { return 0; }\n").unwrap();
        let object_path = out_dir.join("unrelated.o");
        let archive_path = out_dir.join("libunrelated.a");
        run_tool(
            "cc",
            &[
                "-c",
                &unrelated_c.to_string_lossy(),
                "-o",
                &object_path.to_string_lossy(),
            ],
        )
        .unwrap();
        run_tool(
            "ar",
            &[
                "rcs",
                &archive_path.to_string_lossy(),
                &object_path.to_string_lossy(),
            ],
        )
        .unwrap();

        let result = verify_symbol_defined(&archive_path, "c_add");
        assert!(matches!(
            result,
            Err(G2Error::ExpectedSymbolNotDefined { .. })
        ));

        let _ = fs::remove_dir_all(&out_dir);
    }
}

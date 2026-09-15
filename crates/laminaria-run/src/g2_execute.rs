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
//! **Scope so far**:
//! - Checkpoint 1: the fixture's C package `cadd@1` -- the
//!   `compile-c-object:cadd@1.0.0` -> `archive-static-library:cadd@1.0.0`
//!   action pair.
//! - Checkpoint 2: the fixture's own root Cargo package `app` -- the
//!   single `compile-rust-object:app` action (`rustc --emit=obj`, no
//!   linking).
//! - Checkpoint 3: the fixture's Nimble provider package `doubler` --
//!   the single `compile-nim-static-library:doubler` action
//!   (`nim c --app:staticlib`).
//!
//! Checkpoints 2 and 3 resolve `rustc`/`nim` through issue #18's own
//! verified-toolchain mechanism (`toolchain_resolve::resolve_rustc`/
//! `resolve_nim`, backed by `toolchains.lock.toml`), not a bare `PATH`
//! lookup -- this development machine has more than one Rust and Nim
//! install, and plain `PATH` resolution can silently pick a
//! wrong-architecture one, which is harmless for a checkpoint that only
//! compiles in isolation but would break `LinkNativeExecutable` once it
//! actually links these objects together.
//!
//! The remaining `RequiredActionKind` variants this fixture's positive
//! plan also requires (`CompileCppAdapterObject`,
//! `LinkNativeExecutable`, `PreflightRuntimeContract`,
//! `PublishProvenance`) are real, disclosed, not-yet-implemented gaps
//! -- the rest of issue #46's own action chain, deliberately left for
//! following checkpoints rather than claimed here.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use laminaria_plan::dependency_graph::{DischargeKind, LifecycleViolation, PositiveClosure};
use sha2::{Digest, Sha256};

use crate::cross_ecosystem_ingest::CargoManifestFacts;
use crate::toolchain_resolve::{
    self, resolve_verified_nim, resolve_verified_rust, ToolchainResolutionError,
};

#[derive(Debug)]
pub enum G2Error {
    Io(String),
    ToolFailed {
        program: String,
        detail: String,
    },
    /// The real object/archive was produced, but a real `nm` read of it
    /// does not show the expected symbol as defined -- the
    /// compile/archive step did not actually produce what G1's own
    /// source-declared facts required.
    ExpectedSymbolNotDefined {
        path: PathBuf,
        symbol: String,
    },
    /// The real object was produced, but a real `nm` read of it does
    /// not show the expected symbol as an *undefined* (external)
    /// reference -- either the compile step resolved it some other way
    /// (unexpected for `--emit=obj`, which never links), or the FFI
    /// requirement this checkpoint expected to see was not actually
    /// compiled in at all.
    ExpectedSymbolNotReferenced {
        path: PathBuf,
        symbol: String,
    },
    /// The verified-toolchain resolution issue #18's own `doctor`
    /// mechanism performs (`toolchains.lock.toml` -> exact,
    /// selector-checked executable path) failed or refused to resolve.
    /// G2 uses this rather than a bare `rustc`/`nim` off `PATH`: this
    /// development machine has more than one Rust/Nim install, and
    /// plain `PATH` resolution can silently pick a wrong-architecture
    /// one (Homebrew's x86_64 `rustc`/`nim` ahead of the real,
    /// arm64-native rustup/choosenim ones) -- harmless for a checkpoint
    /// that only compiles in isolation, but fatal once a later
    /// checkpoint actually links these objects together.
    Toolchain(ToolchainResolutionError),
    Discharge(LifecycleViolation),
}

impl std::fmt::Display for G2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            G2Error::Io(detail) => write!(f, "G2 execution I/O error: {detail}"),
            G2Error::ToolFailed { program, detail } => {
                write!(f, "G2 execution: '{program}' failed: {detail}")
            }
            G2Error::ExpectedSymbolNotDefined { path, symbol } => write!(
                f,
                "G2 execution: real file '{}' does not define expected symbol '{symbol}'",
                path.display()
            ),
            G2Error::ExpectedSymbolNotReferenced { path, symbol } => write!(
                f,
                "G2 execution: real file '{}' does not reference expected undefined symbol '{symbol}'",
                path.display()
            ),
            G2Error::Toolchain(err) => write!(f, "G2 execution: toolchain resolution: {err}"),
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

impl From<ToolchainResolutionError> for G2Error {
    fn from(err: ToolchainResolutionError) -> Self {
        G2Error::Toolchain(err)
    }
}

/// Resolves the exact, selector-verified `rustc` executable
/// `toolchains.lock.toml` pins (via `rustup`, never a bare `PATH`
/// lookup) -- see [`G2Error::Toolchain`]'s own doc comment for why.
fn resolve_rustc(repo_root: &Path) -> Result<PathBuf, G2Error> {
    let lock_path = repo_root.join("toolchains.lock.toml");
    let doctor_run = toolchain_resolve::run_doctor(&lock_path, repo_root, true, false)?;
    let (_cargo, rustc) = resolve_verified_rust(&doctor_run)?;
    Ok(rustc)
}

/// The `resolve_rustc` counterpart for the exact, selector-verified
/// `nim` executable `toolchains.lock.toml` pins.
fn resolve_nim(repo_root: &Path) -> Result<PathBuf, G2Error> {
    let lock_path = repo_root.join("toolchains.lock.toml");
    let doctor_run = toolchain_resolve::run_doctor(&lock_path, repo_root, false, true)?;
    let nim = resolve_verified_nim(&doctor_run)?;
    Ok(nim)
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

/// Real `nm` inspection of a real object/archive file -- G2's own
/// legitimate artifact-inspection boundary (unlike G1, which must
/// never inspect a compiled artifact's symbol table; see
/// `command_runner`'s own doc comment). Returns `Ok(())` only if some
/// line of `nm`'s real output names `symbol` as a *defined* entry (a
/// `T`/`t` -- text/code -- type letter, not `U` undefined), on the
/// platforms this checkpoint has been exercised on (`nm -g`, BSD/macOS
/// and GNU `nm` both support `-g` "external symbols only"; the exact
/// column layout is parsed leniently rather than assumed byte-for-byte
/// identical across platforms; a leading `_` -- Mach-O's own C symbol
/// decoration -- is accepted as the same symbol).
fn verify_symbol_defined(path: &Path, symbol: &str) -> Result<(), G2Error> {
    let nm_output = run_tool("nm", &["-g", &path.to_string_lossy()])?;
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
            path: path.to_path_buf(),
            symbol: symbol.to_string(),
        })
    }
}

/// The `verify_symbol_defined` counterpart for an *undefined*
/// (external, `U`) reference -- used to confirm a real object actually
/// requires a given FFI symbol from elsewhere, rather than trusting
/// that source-level `extern "C"` syntax survived compilation
/// unchanged. Same leading-`_` tolerance as `verify_symbol_defined`.
fn verify_symbol_referenced_undefined(path: &Path, symbol: &str) -> Result<(), G2Error> {
    let nm_output = run_tool("nm", &["-g", &path.to_string_lossy()])?;
    let underscored = format!("_{symbol}");
    let referenced = nm_output.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let type_letter = fields.next();
        let name = fields.next();
        matches!(type_letter, Some("U"))
            && matches!(name, Some(n) if n == symbol || n == underscored)
    });
    if referenced {
        Ok(())
    } else {
        Err(G2Error::ExpectedSymbolNotReferenced {
            path: path.to_path_buf(),
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

/// Real, observed evidence that the root Cargo package's own Rust
/// source was actually compiled to an object file -- never a path
/// string alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustObjectEvidence {
    pub object_path: PathBuf,
    pub object_size_bytes: u64,
    pub object_sha256: String,
    /// The real entry-point symbol `nm` confirmed as defined.
    pub confirmed_defined_entry_symbol: String,
    /// The real FFI symbols `nm` confirmed as *referenced but
    /// undefined* -- i.e. genuinely left for `LinkNativeExecutable` to
    /// resolve, not silently already satisfied by something else.
    pub confirmed_referenced_symbols: Vec<String>,
}

impl RustObjectEvidence {
    fn as_discharge_evidence(&self) -> String {
        format!(
            "real rustc --emit=obj: {} ({} bytes, sha256={}); nm confirms '{}' defined and [{}] undefined-referenced",
            self.object_path.display(),
            self.object_size_bytes,
            self.object_sha256,
            self.confirmed_defined_entry_symbol,
            self.confirmed_referenced_symbols.join(", ")
        )
    }
}

/// Actually executes the real `compile-rust-object:app` `RequiredAction`
/// (`rustc --emit=obj`, using the exact edition and real default-
/// activated feature set G1 itself observed via `cargo metadata` --
/// never a hardcoded `--edition`/`--cfg` -- and the exact, verified
/// `rustc` `toolchains.lock.toml` pins, resolved via `resolve_rustc`,
/// never a bare `PATH` lookup) against the fixture's real
/// `app/src/main.rs`, verifies the real result independently (`nm`
/// confirms the real entry point is defined and every real FFI
/// requirement is left as an undefined reference, plus a real
/// SHA-256), and discharges the closure's `ArtifactProduction:object:app`
/// obligation through `compile-rust-object:app` -- the exact action id
/// G1 itself emitted and recorded as that obligation's
/// `required_action`. Deliberately does not link: `--emit=obj` stops
/// before the link step, matching G1's own "G2 executes the typed
/// action chain" model, where `LinkNativeExecutable` is a distinct,
/// later action this checkpoint does not perform.
pub fn compile_rust_object_for_app(
    closure: &mut PositiveClosure,
    fixture_root: &Path,
    repo_root: &Path,
    cargo: &CargoManifestFacts,
    required_extern_symbols: &BTreeSet<String>,
    out_dir: &Path,
) -> Result<RustObjectEvidence, G2Error> {
    fs::create_dir_all(out_dir).map_err(|e| G2Error::Io(format!("{}: {e}", out_dir.display())))?;

    let rustc = resolve_rustc(repo_root)?;
    let main_rs = fixture_root.join("app/src/main.rs");
    let object_path = out_dir.join(format!("{}.o", cargo.package_name));

    let mut args: Vec<String> = vec![
        "--edition".to_string(),
        cargo.edition.clone(),
        "--crate-name".to_string(),
        cargo.package_name.clone(),
        "--crate-type".to_string(),
        "bin".to_string(),
    ];
    for feature in &cargo.active_features {
        args.push("--cfg".to_string());
        args.push(format!("feature=\"{feature}\""));
    }
    args.push("--emit".to_string());
    args.push("obj".to_string());
    args.push("-o".to_string());
    args.push(object_path.to_string_lossy().into_owned());
    args.push(main_rs.to_string_lossy().into_owned());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    run_tool(&rustc.to_string_lossy(), &arg_refs)?;

    verify_symbol_defined(&object_path, "main")?;
    for symbol in required_extern_symbols {
        verify_symbol_referenced_undefined(&object_path, symbol)?;
    }

    let object_size_bytes = fs::metadata(&object_path)
        .map_err(|e| G2Error::Io(format!("{}: {e}", object_path.display())))?
        .len();
    let object_sha256 = sha256_file(&object_path)?;

    let evidence = RustObjectEvidence {
        object_path,
        object_size_bytes,
        object_sha256,
        confirmed_defined_entry_symbol: "main".to_string(),
        confirmed_referenced_symbols: required_extern_symbols.iter().cloned().collect(),
    };

    closure.discharge_obligation(
        &format!("ArtifactProduction:object:{}", cargo.package_name),
        &format!("compile-rust-object:{}", cargo.package_name),
        DischargeKind::Generated,
        evidence.as_discharge_evidence(),
    )?;

    Ok(evidence)
}

/// Real, observed evidence that the Nimble provider package's own Nim
/// source was actually compiled to a static library -- never a path
/// string alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimStaticLibraryEvidence {
    pub archive_path: PathBuf,
    pub archive_size_bytes: u64,
    pub archive_sha256: String,
    /// The exact exported symbol `nm` confirmed as defined in the real
    /// archive.
    pub confirmed_defined_symbol: String,
}

impl NimStaticLibraryEvidence {
    fn as_discharge_evidence(&self) -> String {
        format!(
            "real nim c --app:staticlib: {} ({} bytes, sha256={}); nm confirms '{}' defined",
            self.archive_path.display(),
            self.archive_size_bytes,
            self.archive_sha256,
            self.confirmed_defined_symbol
        )
    }
}

/// Actually executes the real `compile-nim-static-library:doubler`
/// `RequiredAction` (`nim c --app:staticlib`, using the exact,
/// verified `nim` `toolchains.lock.toml` pins -- resolved via
/// `resolve_nim`, never a bare `PATH` lookup, which on this class of
/// development machine can silently resolve a wrong-architecture `nim`
/// ahead of the pinned one) against the fixture's real
/// `nimble/doubler/src/doubler.nim`, verifies the real result
/// independently (`nm` confirms the real exported symbol is defined,
/// plus a real SHA-256), and discharges the closure's
/// `ArtifactProduction:archive:doubler` obligation through
/// `compile-nim-static-library:doubler` -- the exact action id G1
/// itself emitted and recorded as that obligation's `required_action`.
/// One step, unlike the C/C++ compile-then-archive pair: Nim's own
/// `--app:staticlib` mode compiles and archives in a single real
/// invocation.
pub fn compile_nim_static_library_for_doubler(
    closure: &mut PositiveClosure,
    fixture_root: &Path,
    repo_root: &Path,
    exported_symbol: &str,
    out_dir: &Path,
) -> Result<NimStaticLibraryEvidence, G2Error> {
    fs::create_dir_all(out_dir).map_err(|e| G2Error::Io(format!("{}: {e}", out_dir.display())))?;

    let nim = resolve_nim(repo_root)?;
    let doubler_nim = fixture_root.join("nimble/doubler/src/doubler.nim");
    let nimcache_dir = out_dir.join("nimcache");
    let archive_path = out_dir.join("libdoubler.a");

    run_tool(
        &nim.to_string_lossy(),
        &[
            "c",
            "--app:staticlib",
            &format!("--nimcache:{}", nimcache_dir.to_string_lossy()),
            &format!("-o:{}", archive_path.to_string_lossy()),
            &doubler_nim.to_string_lossy(),
        ],
    )?;

    verify_symbol_defined(&archive_path, exported_symbol)?;

    let archive_size_bytes = fs::metadata(&archive_path)
        .map_err(|e| G2Error::Io(format!("{}: {e}", archive_path.display())))?
        .len();
    let archive_sha256 = sha256_file(&archive_path)?;

    let evidence = NimStaticLibraryEvidence {
        archive_path,
        archive_size_bytes,
        archive_sha256,
        confirmed_defined_symbol: exported_symbol.to_string(),
    };

    closure.discharge_obligation(
        "ArtifactProduction:archive:doubler",
        "compile-nim-static-library:doubler",
        DischargeKind::Generated,
        evidence.as_discharge_evidence(),
    )?;

    Ok(evidence)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::command_runner::RecordingCommandRunner;
    use crate::cross_ecosystem_ingest::{
        ingest_cargo_metadata, ingest_fixture_input, FixtureLayout,
    };
    use laminaria_plan::dependency_graph::{resolve, ObligationState};

    /// Same computation `FixtureLayout::discover` uses for its own
    /// `root`, minus the `fixtures/cross-ecosystem-native-executable`
    /// suffix -- the repo root `toolchains.lock.toml` actually lives
    /// in, needed for real toolchain resolution (`resolve_rustc`/
    /// `resolve_nim`), matching `self_build.rs`'s own test helper of
    /// the same name.
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

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

    /// Checkpoint 2's end-to-end case: resolve a real G1 positive
    /// closure, actually run `rustc --emit=obj` against the real
    /// `app/src/main.rs` (using the real edition/active-feature facts
    /// G1 itself observed, never hardcoded), and discharge the
    /// resulting `ArtifactProduction:object:app` obligation with the
    /// real, independently verified result.
    #[test]
    fn g2_really_compiles_app_to_a_rust_object_and_discharges_its_obligation() {
        let layout = FixtureLayout::discover();
        let runner = RecordingCommandRunner::new();
        let input = ingest_fixture_input(&runner, &layout, &["1.0.0"])
            .expect("must ingest the real positive fixture");
        let mut closure = resolve(&input).expect("must resolve a positive closure");
        let cargo = ingest_cargo_metadata(&runner, &layout.app_manifest())
            .expect("must ingest the real app Cargo manifest");
        let required_extern_symbols: BTreeSet<String> = input
            .ffi_requirements
            .iter()
            .map(|r| r.symbol.clone())
            .collect();
        assert!(
            !required_extern_symbols.is_empty(),
            "the real fixture must declare at least one FFI requirement"
        );

        let obligation_id = "ArtifactProduction:object:app";
        assert_eq!(
            closure.obligations[obligation_id].state,
            ObligationState::Satisfied,
            "must start from G1's own terminal state"
        );

        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-g2-checkpoint-2-{}-app",
            std::process::id()
        ));
        let evidence = compile_rust_object_for_app(
            &mut closure,
            &layout.root,
            &repo_root(),
            &cargo,
            &required_extern_symbols,
            &out_dir,
        )
        .expect("real rustc --emit=obj execution and discharge must succeed");

        assert!(
            evidence.object_path.is_file(),
            "the real compiled object must exist on disk"
        );
        assert!(evidence.object_size_bytes > 0);
        assert_eq!(
            evidence.object_sha256.len(),
            64,
            "must be a real hex SHA-256"
        );
        assert_eq!(evidence.confirmed_defined_entry_symbol, "main");
        assert_eq!(
            evidence.confirmed_referenced_symbols.len(),
            required_extern_symbols.len()
        );

        let obligation = &closure.obligations[obligation_id];
        assert_eq!(obligation.state, ObligationState::Discharged);
        assert_eq!(obligation.discharge_kind, Some(DischargeKind::Generated));
        assert_eq!(
            obligation.required_action.as_deref(),
            Some("compile-rust-object:app")
        );
        assert!(obligation
            .evidence
            .as_deref()
            .unwrap()
            .contains(&evidence.object_sha256));

        let _ = fs::remove_dir_all(&out_dir);
    }

    /// A real object genuinely missing an expected undefined reference
    /// (compiled from source with no such `extern "C"` declaration at
    /// all) must refuse verification rather than accept a caller's
    /// unverified claim.
    #[test]
    fn a_real_object_missing_an_expected_undefined_reference_refuses_verification() {
        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-g2-checkpoint-2-negative-{}",
            std::process::id()
        ));
        fs::create_dir_all(&out_dir).unwrap();
        let unrelated_rs = out_dir.join("unrelated.rs");
        fs::write(&unrelated_rs, "fn main() {}\n").unwrap();
        let object_path = out_dir.join("unrelated.o");
        run_tool(
            "rustc",
            &[
                "--edition",
                "2021",
                "--crate-name",
                "unrelated",
                "--crate-type",
                "bin",
                "--emit",
                "obj",
                "-o",
                &object_path.to_string_lossy(),
                &unrelated_rs.to_string_lossy(),
            ],
        )
        .unwrap();

        let result = verify_symbol_referenced_undefined(&object_path, "c_add");
        assert!(matches!(
            result,
            Err(G2Error::ExpectedSymbolNotReferenced { .. })
        ));
        // The entry point itself is still genuinely defined, unaffected
        // by the missing FFI reference above.
        verify_symbol_defined(&object_path, "main")
            .expect("a plain fn main() must still define the real entry symbol");

        let _ = fs::remove_dir_all(&out_dir);
    }

    /// Checkpoint 3's end-to-end case: resolve a real G1 positive
    /// closure, actually run `nim c --app:staticlib` against the real
    /// `nimble/doubler/src/doubler.nim` using the exact, verified `nim`
    /// executable `toolchains.lock.toml` pins (not a bare `PATH`
    /// lookup, which on this development machine can silently resolve
    /// a wrong-architecture `nim`), and discharge the resulting
    /// `ArtifactProduction:archive:doubler` obligation with the real,
    /// independently verified result.
    #[test]
    fn g2_really_compiles_doubler_to_a_nim_static_library_and_discharges_its_obligation() {
        let layout = FixtureLayout::discover();
        let runner = RecordingCommandRunner::new();
        let input = ingest_fixture_input(&runner, &layout, &["1.0.0"])
            .expect("must ingest the real positive fixture");
        let mut closure = resolve(&input).expect("must resolve a positive closure");
        let exported_symbol = input
            .package_candidates
            .iter()
            .find(|c| c.package_id == "doubler")
            .and_then(|c| c.declared_exports.first())
            .map(|e| e.symbol.clone())
            .expect("the real doubler candidate must declare at least one export");

        let obligation_id = "ArtifactProduction:archive:doubler";
        assert_eq!(
            closure.obligations[obligation_id].state,
            ObligationState::Satisfied,
            "must start from G1's own terminal state"
        );

        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-g2-checkpoint-3-{}-doubler",
            std::process::id()
        ));
        let evidence = compile_nim_static_library_for_doubler(
            &mut closure,
            &layout.root,
            &repo_root(),
            &exported_symbol,
            &out_dir,
        )
        .expect("real nim c --app:staticlib execution and discharge must succeed");

        assert!(
            evidence.archive_path.is_file(),
            "the real compiled static library must exist on disk"
        );
        assert!(evidence.archive_size_bytes > 0);
        assert_eq!(
            evidence.archive_sha256.len(),
            64,
            "must be a real hex SHA-256"
        );
        assert_eq!(evidence.confirmed_defined_symbol, exported_symbol);

        let obligation = &closure.obligations[obligation_id];
        assert_eq!(obligation.state, ObligationState::Discharged);
        assert_eq!(obligation.discharge_kind, Some(DischargeKind::Generated));
        assert_eq!(
            obligation.required_action.as_deref(),
            Some("compile-nim-static-library:doubler")
        );
        assert!(obligation
            .evidence
            .as_deref()
            .unwrap()
            .contains(&evidence.archive_sha256));

        let _ = fs::remove_dir_all(&out_dir);
    }
}

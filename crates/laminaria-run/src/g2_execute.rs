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
//! - Checkpoint 4: the fixture's C++ provider package `cppmax` -- the
//!   `compile-cpp-adapter-object:cppmax@1.0.0` ->
//!   `archive-static-library:cppmax@1.0.0` action pair (`c++ -c` then
//!   `ar rcs`, sharing `compile_and_archive_native_source` with
//!   Checkpoint 1's structurally identical C case).
//!
//! Checkpoints 2 and 3 resolve `rustc`/`nim` through issue #18's own
//! verified-toolchain mechanism (`toolchain_resolve::resolve_rustc`/
//! `resolve_nim`, backed by `toolchains.lock.toml`), not a bare `PATH`
//! lookup -- this development machine has more than one Rust and Nim
//! install, and plain `PATH` resolution can silently pick a
//! wrong-architecture one, which is harmless for a checkpoint that only
//! compiles in isolation but would break `LinkNativeExecutable` once it
//! actually links these objects together. `cc`/`c++`/`ar` (Checkpoints
//! 1 and 4) do not have this problem on this machine and stay resolved
//! off `PATH`.
//!
//! With Checkpoint 4, every compile-side `RequiredActionKind` this
//! fixture's positive plan requires was done.
//!
//! - Checkpoint 5: `link-native-executable` -- links the four real
//!   artifacts from Checkpoints 1-4 into one real native executable
//!   (via `rustc` re-invoked on the real `app/src/main.rs` with `-L`
//!   pointed at the real archives -- see
//!   `link_and_run_native_executable`'s own doc comment for exactly
//!   why this is the real link mechanism, not a raw linker
//!   invocation) and **actually runs it**, checking the real exit
//!   code/stdout/stderr against the fixture's own declared expectation
//!   (exit 0, stdout `"9\n"`, empty stderr).
//!
//! `execute_fixed_m1_graph` connects those physical operations to the
//! common obligation lifecycle. It uses the successful execution as the
//! runtime preflight, writes a real provenance statement, admits the graph
//! through the publication gate, and only then publishes the final manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use laminaria_plan::dependency_graph::{
    resolve, DischargeKind, LifecycleViolation, PositiveClosure, RequiredActionKind,
    RuntimeRequirementFacts,
};
use laminaria_plan::obligation_lifecycle::{
    ArtifactManifest, LifecycleError, OperationEvidence, OperationOutcome, ProducedFact,
    ProductionGraph, PublicationBlocker, ResolutionEvidence, RuntimeContractEvidence,
    RuntimeProviderEvidence,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::command_runner::RealCommandRunner;
use crate::cross_ecosystem_ingest::{
    ingest_cargo_metadata, ingest_fixture_input, CargoManifestFacts, FixtureLayout,
};
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
    /// The linked executable was produced and actually run, but its
    /// real exit code/stdout/stderr did not match the fixture's own
    /// declared expected result -- the strongest possible signal that
    /// linking did not actually produce a working program, not merely
    /// that the link command itself exited 0.
    UnexpectedExecutionResult {
        path: PathBuf,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    RuntimeContractUnavailable {
        contract: String,
        detail: String,
    },
    Discharge(LifecycleViolation),
    ProductionLifecycle(LifecycleError),
    PublicationBlocked {
        blockers: Vec<PublicationBlocker>,
        executed_actions: Vec<String>,
        unpublished_paths: Vec<PathBuf>,
    },
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
            G2Error::UnexpectedExecutionResult {
                path,
                exit_code,
                stdout,
                stderr,
            } => write!(
                f,
                "G2 execution: running real executable '{}' gave exit={exit_code:?} stdout={stdout:?} stderr={stderr:?}, not the fixture's declared expected result",
                path.display()
            ),
            G2Error::RuntimeContractUnavailable { contract, detail } => write!(
                f,
                "G2 runtime preflight: contract {contract:?} is unavailable: {detail}"
            ),
            G2Error::Discharge(violation) => write!(f, "G2 execution: {violation}"),
            G2Error::ProductionLifecycle(error) => {
                write!(f, "G2 production lifecycle: {error:?}")
            }
            G2Error::PublicationBlocked {
                blockers,
                executed_actions,
                unpublished_paths,
            } => write!(
                f,
                "G2 publication blocked: {blockers:?}; executed actions: {executed_actions:?}; unpublished paths: {unpublished_paths:?}"
            ),
        }
    }
}

impl std::error::Error for G2Error {}

impl From<LifecycleViolation> for G2Error {
    fn from(violation: LifecycleViolation) -> Self {
        G2Error::Discharge(violation)
    }
}

impl From<LifecycleError> for G2Error {
    fn from(error: LifecycleError) -> Self {
        G2Error::ProductionLifecycle(error)
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

/// Shared by every "compile one translation unit, then archive it"
/// checkpoint (C's `cadd@1` and C++'s `cppmax@1.0.0`, which use
/// different compilers but are otherwise structurally identical real
/// action pairs): actually runs `<compiler> -c` then `ar rcs`,
/// verifies the real result independently (`nm` confirms
/// `expected_defined_symbol`, plus a real SHA-256). Does not discharge
/// anything itself -- each caller knows its own obligation/action ids
/// and calls `PositiveClosure::discharge_obligation` with this
/// function's evidence.
fn compile_and_archive_native_source(
    compiler: &str,
    source_path: &Path,
    include_dir: &Path,
    out_dir: &Path,
    object_filename: &str,
    archive_filename: &str,
    expected_defined_symbol: &str,
) -> Result<CObjectArchiveEvidence, G2Error> {
    fs::create_dir_all(out_dir).map_err(|e| G2Error::Io(format!("{}: {e}", out_dir.display())))?;

    let object_path = out_dir.join(object_filename);
    let archive_path = out_dir.join(archive_filename);

    run_tool(
        compiler,
        &[
            "-c",
            &source_path.to_string_lossy(),
            "-I",
            &include_dir.to_string_lossy(),
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

    verify_symbol_defined(&archive_path, expected_defined_symbol)?;

    let archive_size_bytes = fs::metadata(&archive_path)
        .map_err(|e| G2Error::Io(format!("{}: {e}", archive_path.display())))?
        .len();
    let archive_sha256 = sha256_file(&archive_path)?;

    Ok(CObjectArchiveEvidence {
        object_path,
        archive_path,
        archive_size_bytes,
        archive_sha256,
        confirmed_defined_symbol: expected_defined_symbol.to_string(),
    })
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
    let c_dir = fixture_root.join("c/cadd/v1");
    let evidence = compile_and_archive_native_source(
        "cc",
        &c_dir.join("cadd.c"),
        &c_dir,
        out_dir,
        "cadd.o",
        "libcadd.a",
        "c_add",
    )?;

    closure.discharge_obligation(
        "ArtifactProduction:archive:cadd",
        "archive-static-library:cadd@1.0.0",
        DischargeKind::StaticallyLinked,
        evidence.as_discharge_evidence(),
    )?;

    Ok(evidence)
}

/// Actually executes the real `compile-cpp-adapter-object:cppmax@1.0.0`
/// and `archive-static-library:cppmax@1.0.0` `RequiredAction` pair
/// (`c++ -c` then `ar rcs`) against the fixture's real `cppmax`
/// `extern "C"` adapter source, verifies the real result independently
/// (`nm` confirms `cpp_max_i32` -- the adapter's own exported entry
/// point, never the `max_value<T>` template itself, which has no
/// linkable symbol -- is defined, plus a real SHA-256), and discharges
/// the closure's `ArtifactProduction:archive:cppmax` obligation through
/// `archive-static-library:cppmax@1.0.0`. `c++`/`ar` are resolved off
/// `PATH` like `cc`/`ar` (Checkpoint 1) -- this machine's system Xcode
/// CLT toolchain, unlike its Rust/Nim installs, already resolves to
/// the correct architecture there; see `resolve_rustc`/`resolve_nim`'s
/// own doc comments for the toolchains that do need issue #18's
/// verified resolution instead.
pub fn compile_and_archive_cppmax(
    closure: &mut PositiveClosure,
    fixture_root: &Path,
    out_dir: &Path,
) -> Result<CObjectArchiveEvidence, G2Error> {
    let cpp_dir = fixture_root.join("cpp/cppmax");
    let evidence = compile_and_archive_native_source(
        "c++",
        &cpp_dir.join("cppmax.cpp"),
        &cpp_dir,
        out_dir,
        "cppmax.o",
        "libcppmax.a",
        "cpp_max_i32",
    )?;

    closure.discharge_obligation(
        "ArtifactProduction:archive:cppmax",
        "archive-static-library:cppmax@1.0.0",
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

/// Real, observed evidence that the four ecosystems' real artifacts
/// were actually linked into one native executable *and that executable
/// was actually run* -- the strongest evidence this checkpoint can
/// produce, matching issue #46's own Direct-acceptance box 2 ("produce
/// an ordinary native executable and execute it with the expected
/// observable result").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeExecutableEvidence {
    pub executable_path: PathBuf,
    pub executable_size_bytes: u64,
    pub executable_sha256: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl NativeExecutableEvidence {
    fn as_discharge_evidence(&self) -> String {
        format!(
            "real link + real run: {} ({} bytes, sha256={}); exit={} stdout={:?} stderr={:?}",
            self.executable_path.display(),
            self.executable_size_bytes,
            self.executable_sha256,
            self.exit_code,
            self.stdout,
            self.stderr
        )
    }
}

/// Actually runs `program` with no arguments and captures its real
/// exit code/stdout/stderr -- deliberately does not treat a non-zero
/// exit as an `Err` (unlike `run_tool`): checking the *real* result
/// against the fixture's own declared expectation is this function's
/// caller's job, not this function's.
fn run_and_capture(program: &Path) -> Result<(Option<i32>, String, String), G2Error> {
    let output = Command::new(program)
        .output()
        .map_err(|e| G2Error::Io(format!("failed to run {}: {e}", program.display())))?;
    Ok((
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Evidence that the exact linked bytes run without a build environment and
/// that every retained platform dependency has an observed runtime provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePreflightEvidence {
    pub clean_executable_path: PathBuf,
    pub executable_sha256: String,
    pub target_triple: String,
    pub interpreter: PathBuf,
    pub shared_library_bindings: Vec<RuntimeProviderEvidence>,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl RuntimePreflightEvidence {
    fn as_operation_detail(&self) -> String {
        format!(
            "exact sha256={} ran from artifact-only directory with an empty environment; target={}; interpreter={}; shared libraries=[{}]; exit={} stdout={:?} stderr={:?}",
            self.executable_sha256,
            self.target_triple,
            self.interpreter.display(),
            self.shared_library_bindings
                .iter()
                .map(|provider| match &provider.path {
                    Some(path) => format!("{}=>{}", provider.provider_id, path),
                    None => format!("{}=>kernel", provider.provider_id),
                })
                .collect::<Vec<_>>()
                .join(", "),
            self.exit_code,
            self.stdout,
            self.stderr
        )
    }
}

fn require_runtime_path(path: &Path, contract: &str) -> Result<(), G2Error> {
    if path.is_file() {
        Ok(())
    } else {
        Err(G2Error::RuntimeContractUnavailable {
            contract: contract.to_owned(),
            detail: format!("required runtime file '{}' is missing", path.display()),
        })
    }
}

#[cfg(target_os = "linux")]
fn inspect_linux_runtime(
    executable: &Path,
) -> Result<(PathBuf, Vec<RuntimeProviderEvidence>), G2Error> {
    let program_headers = run_tool(
        "readelf",
        &["--program-headers", &executable.to_string_lossy()],
    )?;
    let interpreter = program_headers
        .lines()
        .find_map(|line| {
            let marker = "Requesting program interpreter: ";
            let start = line.find(marker)? + marker.len();
            Some(PathBuf::from(line[start..].trim().trim_end_matches(']')))
        })
        .ok_or_else(|| G2Error::RuntimeContractUnavailable {
            contract: "OS ABI/dynamic loader".to_owned(),
            detail: "ELF program headers declare no interpreter".to_owned(),
        })?;
    require_runtime_path(&interpreter, "ELF program interpreter")?;

    let loader_output = run_tool("ldd", &[&executable.to_string_lossy()])?;
    if let Some(missing) = loader_output
        .lines()
        .find(|line| line.contains("not found"))
    {
        return Err(G2Error::RuntimeContractUnavailable {
            contract: "ELF shared-library closure".to_owned(),
            detail: missing.trim().to_owned(),
        });
    }
    let mut bindings = Vec::new();
    for line in loader_output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some((name, provider_and_address)) = line.split_once("=>") {
            let provider = provider_and_address
                .split_whitespace()
                .next()
                .ok_or_else(|| G2Error::RuntimeContractUnavailable {
                    contract: "ELF shared-library closure".to_owned(),
                    detail: format!("ldd reported no provider for {name:?}"),
                })?;
            require_runtime_path(Path::new(provider), name.trim())?;
            bindings.push(RuntimeProviderEvidence {
                provider_id: name.trim().to_owned(),
                path: Some(provider.to_owned()),
            });
        } else {
            let provider = line.split_whitespace().next().unwrap_or_default();
            if provider.starts_with('/') {
                require_runtime_path(Path::new(provider), "ELF runtime provider")?;
                bindings.push(RuntimeProviderEvidence {
                    provider_id: provider.to_owned(),
                    path: Some(provider.to_owned()),
                });
            } else if provider == "linux-vdso.so.1" {
                bindings.push(RuntimeProviderEvidence {
                    provider_id: "linux-vdso.so.1".to_owned(),
                    path: None,
                });
            }
        }
    }
    if bindings.is_empty() {
        return Err(G2Error::RuntimeContractUnavailable {
            contract: "ELF shared-library closure".to_owned(),
            detail: "ldd reported no runtime bindings".to_owned(),
        });
    }
    Ok((interpreter, bindings))
}

/// Copies only the exact production executable into a fresh consumption
/// directory, proves its digest is unchanged, inspects the Linux loader
/// closure, and runs it with an empty environment and that directory as cwd.
/// Cargo, Nimble, compilers, sources, archives, and package caches are absent
/// from both the directory and the process environment.
#[cfg(target_os = "linux")]
pub fn preflight_runtime_contract(
    executable: &NativeExecutableEvidence,
    requirements: &[RuntimeRequirementFacts],
    out_dir: &Path,
) -> Result<RuntimePreflightEvidence, G2Error> {
    let requirement = requirements
        .first()
        .ok_or_else(|| G2Error::RuntimeContractUnavailable {
            contract: "declared runtime contract".to_owned(),
            detail: "the resolved graph declared no runtime requirement".to_owned(),
        })?;
    if requirements
        .iter()
        .any(|candidate| candidate.target_triple != requirement.target_triple)
    {
        return Err(G2Error::RuntimeContractUnavailable {
            contract: "target runtime contract".to_owned(),
            detail: "resolved runtime requirements disagree on target triple".to_owned(),
        });
    }

    let clean_dir = out_dir.join("clean-runtime");
    fs::create_dir_all(&clean_dir)
        .map_err(|error| G2Error::Io(format!("{}: {error}", clean_dir.display())))?;
    let clean_executable_path = clean_dir.join("app");
    fs::copy(&executable.executable_path, &clean_executable_path).map_err(|error| {
        G2Error::Io(format!(
            "copy {} to {}: {error}",
            executable.executable_path.display(),
            clean_executable_path.display()
        ))
    })?;
    let entries: Vec<PathBuf> = fs::read_dir(&clean_dir)
        .map_err(|error| G2Error::Io(format!("{}: {error}", clean_dir.display())))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .map_err(|error| G2Error::Io(format!("{}: {error}", clean_dir.display())))?;
    if entries != vec![clean_executable_path.clone()] {
        return Err(G2Error::RuntimeContractUnavailable {
            contract: requirement.description.clone(),
            detail: format!("clean consumption directory contains unexpected inputs: {entries:?}"),
        });
    }
    let executable_sha256 = sha256_file(&clean_executable_path)?;
    if executable_sha256 != executable.executable_sha256 {
        return Err(G2Error::RuntimeContractUnavailable {
            contract: "exact production artifact identity".to_owned(),
            detail: format!(
                "copied artifact digest {executable_sha256} differs from linked digest {}",
                executable.executable_sha256
            ),
        });
    }
    let (interpreter, shared_library_bindings) = inspect_linux_runtime(&clean_executable_path)?;
    let output = Command::new(&clean_executable_path)
        .env_clear()
        .current_dir(&clean_dir)
        .output()
        .map_err(|error| G2Error::RuntimeContractUnavailable {
            contract: requirement.description.clone(),
            detail: format!("failed to execute exact artifact: {error}"),
        })?;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if exit_code != Some(0) || stdout != "9\n" || !stderr.is_empty() {
        return Err(G2Error::UnexpectedExecutionResult {
            path: clean_executable_path,
            exit_code,
            stdout,
            stderr,
        });
    }
    Ok(RuntimePreflightEvidence {
        clean_executable_path,
        executable_sha256,
        target_triple: requirement.target_triple.clone(),
        interpreter,
        shared_library_bindings,
        exit_code: 0,
        stdout,
        stderr,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn preflight_runtime_contract(
    _executable: &NativeExecutableEvidence,
    requirements: &[RuntimeRequirementFacts],
    _out_dir: &Path,
) -> Result<RuntimePreflightEvidence, G2Error> {
    Err(G2Error::RuntimeContractUnavailable {
        contract: requirements
            .first()
            .map(|requirement| requirement.description.clone())
            .unwrap_or_else(|| "declared runtime contract".to_owned()),
        detail: "the fixed M1 clean-runtime inspector currently supports ELF/Linux targets"
            .to_owned(),
    })
}

/// Actually executes the real `link-native-executable` `RequiredAction`
/// against the four real artifacts Checkpoints 1, 2, 3, and 4 already
/// produced and independently verified (`archive:cadd`, `object:app`,
/// `archive:doubler`, `archive:cppmax`), then actually **runs** the
/// resulting executable and checks its real, observed exit
/// code/stdout/stderr against the fixture's own declared expectation
/// (exit 0, stdout `"9\n"`, empty stderr) -- not merely that the link
/// command itself exited 0.
///
/// **How this reuses the real prior artifacts, and why it does not
/// literally hand a `.o`/`.a` file to a raw linker**: `rustc`'s CLI has
/// no clean way to link an externally-provided object plus
/// automatically resolve Rust's own runtime (`std`/`core`, panic
/// handling, the platform C runtime) without also compiling a crate --
/// hand-deriving those low-level, per-platform linker flags would
/// duplicate, fragilely, exactly what `rustc` itself already does
/// correctly. Instead, this function re-invokes `rustc` on the real
/// `app/src/main.rs` (the exact same real edition/active-feature facts
/// as `compile_rust_object_for_app`, via the exact same
/// `resolve_rustc`) with `-L` pointed at the real, already-built
/// `cadd`/`cppmax`/`doubler` archive directories -- `main.rs`'s own
/// `#[link(name = "...", kind = "static")]` attributes (already present
/// in the fixture, read by `rustc` itself) are what actually pull the
/// three real archives into the link, and `rustc`'s own internal linker
/// invocation is what resolves Rust's own runtime, exactly as a normal
/// `cargo build` would. This does re-run codegen for `app` (rather than
/// reusing Checkpoint 2's literal `object:app` file byte-for-byte), a
/// deliberate, disclosed choice, not an oversight -- the same real
/// source, edition, and features, and the resulting linked program's
/// own actual execution is this function's real evidence, not the
/// object file's identity.
///
/// The three foreign static archives `link_and_run_native_executable`
/// needs `-L` search directories for -- grouped into one type instead
/// of three separate parameters (each is a distinct real file this
/// checkpoint's own caller already produced via Checkpoints 1, 3, and
/// 4's own evidence).
pub struct NativeArchives<'a> {
    pub cadd: &'a Path,
    pub cppmax: &'a Path,
    pub doubler: &'a Path,
}

/// Discharges all three obligations `link-native-executable` is
/// declared to discharge: `FinalLink:native_executable`,
/// `LinkOrder:native_executable`, and `ArtifactProduction:executable:app`.
pub fn link_and_run_native_executable(
    closure: &mut PositiveClosure,
    fixture_root: &Path,
    repo_root: &Path,
    cargo: &CargoManifestFacts,
    archives: &NativeArchives<'_>,
    out_dir: &Path,
) -> Result<NativeExecutableEvidence, G2Error> {
    fs::create_dir_all(out_dir).map_err(|e| G2Error::Io(format!("{}: {e}", out_dir.display())))?;

    let rustc = resolve_rustc(repo_root)?;
    let main_rs = fixture_root.join("app/src/main.rs");
    let executable_path = out_dir.join(&cargo.package_name);

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    for archive_path in [archives.cadd, archives.cppmax, archives.doubler] {
        let dir = archive_path
            .parent()
            .ok_or_else(|| {
                G2Error::Io(format!(
                    "archive path '{}' has no parent directory",
                    archive_path.display()
                ))
            })?
            .to_path_buf();
        if !search_dirs.contains(&dir) {
            search_dirs.push(dir);
        }
    }

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
    for dir in &search_dirs {
        args.push("-L".to_string());
        args.push(dir.to_string_lossy().into_owned());
    }
    args.push("-o".to_string());
    args.push(executable_path.to_string_lossy().into_owned());
    args.push(main_rs.to_string_lossy().into_owned());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    run_tool(&rustc.to_string_lossy(), &arg_refs)?;

    let (exit_code, stdout, stderr) = run_and_capture(&executable_path)?;
    if exit_code != Some(0) || stdout != "9\n" || !stderr.is_empty() {
        return Err(G2Error::UnexpectedExecutionResult {
            path: executable_path,
            exit_code,
            stdout,
            stderr,
        });
    }

    let executable_size_bytes = fs::metadata(&executable_path)
        .map_err(|e| G2Error::Io(format!("{}: {e}", executable_path.display())))?
        .len();
    let executable_sha256 = sha256_file(&executable_path)?;

    let evidence = NativeExecutableEvidence {
        executable_path,
        executable_size_bytes,
        executable_sha256,
        exit_code: exit_code.expect("checked Some(0) above"),
        stdout,
        stderr,
    };

    for obligation_id in [
        "FinalLink:native_executable",
        "LinkOrder:native_executable",
        "ArtifactProduction:executable:app",
    ] {
        closure.discharge_obligation(
            obligation_id,
            "link-native-executable",
            DischargeKind::StaticallyLinked,
            evidence.as_discharge_evidence(),
        )?;
    }

    Ok(evidence)
}

/// Result of the fixed M1 production entry point: the exact executable
/// evidence and the publication manifest that closes the same graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedGraphPublication {
    pub executable: NativeExecutableEvidence,
    pub runtime_preflight: RuntimePreflightEvidence,
    pub manifest: ArtifactManifest,
    pub provenance_path: PathBuf,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct ProvenanceStatement<'a> {
    contract_version: u32,
    subject_artifact: &'a ProducedFact,
    completed_operations: &'a [OperationEvidence],
    runtime_contracts: &'a [RuntimeContractEvidence],
    obligation_histories:
        &'a BTreeMap<String, Vec<laminaria_plan::obligation_lifecycle::StateTransition>>,
    publication_action_id: &'a str,
    publication_discharges: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeededGap {
    None,
    #[cfg(test)]
    Omit(RequiredActionKind),
}

impl SeededGap {
    fn omits(self, _kind: RequiredActionKind) -> bool {
        match self {
            Self::None => false,
            #[cfg(test)]
            Self::Omit(omitted) => omitted == _kind,
        }
    }
}

/// Executes the fixed Cargo/Nimble/C/C++ M1 graph and admits the executable
/// only through issue #88's common lifecycle gate.
///
/// The existing real compilers/linker remain the only artifact producers.
/// This function adds the lossless operation records and terminal-closure
/// check around them; it does not duplicate their implementation.
pub fn execute_fixed_m1_graph(
    repo_root: &Path,
    out_dir: &Path,
) -> Result<FixedGraphPublication, G2Error> {
    execute_fixed_m1_graph_with_seed(repo_root, out_dir, SeededGap::None)
}

fn execute_fixed_m1_graph_with_seed(
    repo_root: &Path,
    out_dir: &Path,
    seeded_gap: SeededGap,
) -> Result<FixedGraphPublication, G2Error> {
    let layout = FixtureLayout::discover();
    let runner = RealCommandRunner;
    let input = ingest_fixture_input(&runner, &layout, &["1.0.0"])
        .map_err(|error| G2Error::Io(format!("fixture ingestion: {error}")))?;
    let closure = resolve(&input)
        .map_err(|error| G2Error::Io(format!("dependency resolution: {error:?}")))?;
    let mut execution_closure = closure.clone();
    let mut production = ProductionGraph::new(closure)?;
    let cargo = ingest_cargo_metadata(&runner, &layout.app_manifest())
        .map_err(|error| G2Error::Io(format!("Cargo metadata ingestion: {error}")))?;
    let required_extern_symbols: BTreeSet<String> = input
        .ffi_requirements
        .iter()
        .map(|requirement| requirement.symbol.clone())
        .collect();
    let doubler_symbol = input
        .package_candidates
        .iter()
        .find(|candidate| candidate.package_id == "doubler")
        .and_then(|candidate| candidate.declared_exports.first())
        .map(|export| export.symbol.clone())
        .ok_or_else(|| G2Error::Io("doubler has no declared export".to_owned()))?;

    fs::create_dir_all(out_dir)
        .map_err(|error| G2Error::Io(format!("{}: {error}", out_dir.display())))?;
    let cadd = compile_and_archive_cadd_v1(&mut execution_closure, &layout.root, out_dir)?;
    let cppmax = compile_and_archive_cppmax(&mut execution_closure, &layout.root, out_dir)?;
    let doubler = compile_nim_static_library_for_doubler(
        &mut execution_closure,
        &layout.root,
        repo_root,
        &doubler_symbol,
        out_dir,
    )?;
    let rust = compile_rust_object_for_app(
        &mut execution_closure,
        &layout.root,
        repo_root,
        &cargo,
        &required_extern_symbols,
        out_dir,
    )?;
    let executable = link_and_run_native_executable(
        &mut execution_closure,
        &layout.root,
        repo_root,
        &cargo,
        &NativeArchives {
            cadd: &cadd.archive_path,
            cppmax: &cppmax.archive_path,
            doubler: &doubler.archive_path,
        },
        out_dir,
    )?;
    let runtime_preflight =
        preflight_runtime_contract(&executable, &input.runtime_requirements, out_dir)?;

    let resolver_owned: Vec<String> = production
        .closure
        .obligations
        .values()
        .filter(|obligation| {
            matches!(
                obligation.state,
                laminaria_plan::dependency_graph::ObligationState::Selected
                    | laminaria_plan::dependency_graph::ObligationState::Satisfied
            )
        })
        .map(|obligation| obligation.id.clone())
        .collect();
    production.record_resolution(ResolutionEvidence {
        operation_id: "m1-resolution".to_owned(),
        run_id: "m1-fixed-graph".to_owned(),
        producer_identity: "laminaria dependency resolver".to_owned(),
        sequence: 1,
        input_identities: input
            .sources
            .iter()
            .map(|source| source.id.clone())
            .collect(),
        resolved_obligations: resolver_owned,
        outcome: OperationOutcome::Succeeded,
        structurally_verified: true,
    })?;

    let physical_facts = BTreeMap::from([
        (
            "compile-c-object:cadd@1.0.0",
            observed_file(&cadd.object_path)?,
        ),
        (
            "archive-static-library:cadd@1.0.0",
            ProducedFact {
                identity: String::new(),
                digest: cadd.archive_sha256.clone(),
                size_bytes: cadd.archive_size_bytes,
            },
        ),
        (
            "compile-cpp-adapter-object:cppmax@1.0.0",
            observed_file(&cppmax.object_path)?,
        ),
        (
            "archive-static-library:cppmax@1.0.0",
            ProducedFact {
                identity: String::new(),
                digest: cppmax.archive_sha256.clone(),
                size_bytes: cppmax.archive_size_bytes,
            },
        ),
        (
            "compile-nim-static-library:doubler",
            ProducedFact {
                identity: String::new(),
                digest: doubler.archive_sha256.clone(),
                size_bytes: doubler.archive_size_bytes,
            },
        ),
        (
            "compile-rust-object:app",
            ProducedFact {
                identity: String::new(),
                digest: rust.object_sha256.clone(),
                size_bytes: rust.object_size_bytes,
            },
        ),
        (
            "link-native-executable",
            ProducedFact {
                identity: String::new(),
                digest: executable.executable_sha256.clone(),
                size_bytes: executable.executable_size_bytes,
            },
        ),
    ]);

    let provenance_path = out_dir.join("laminaria-provenance.json");
    let staged_provenance_path = out_dir.join(".laminaria-provenance.json.staging");
    let manifest_path = out_dir.join("laminaria-artifact-manifest.json");
    let staged_manifest_path = out_dir.join(".laminaria-artifact-manifest.json.staging");
    let actions = production.closure.required_actions.clone();
    let mut requested_artifact = None;
    let mut sequence = 2_u64;
    for action in actions {
        if seeded_gap.omits(action.kind) {
            continue;
        }
        let base_fact = if action.kind == RequiredActionKind::PublishProvenance {
            let subject_artifact = requested_artifact.as_ref().ok_or_else(|| {
                G2Error::Io("provenance publication has no linked artifact identity".to_owned())
            })?;
            let statement = ProvenanceStatement {
                contract_version: production.contract_version,
                subject_artifact,
                completed_operations: &production.operations,
                runtime_contracts: &production.runtime_contracts,
                obligation_histories: &production.histories,
                publication_action_id: &action.id,
                publication_discharges: &action.discharges,
            };
            let bytes = serde_json::to_vec_pretty(&statement)
                .map_err(|error| G2Error::Io(format!("serialize provenance: {error}")))?;
            write_staged_file(&staged_provenance_path, &bytes)?;
            observed_file(&staged_provenance_path)?
        } else if action.outputs.is_empty() {
            ProducedFact {
                identity: String::new(),
                digest: "no-material-output".to_owned(),
                size_bytes: 1,
            }
        } else {
            physical_facts
                .get(action.id.as_str())
                .cloned()
                .ok_or_else(|| {
                    G2Error::Io(format!(
                        "production action '{}' has no observed output evidence",
                        action.id
                    ))
                })?
        };
        let outputs: Vec<ProducedFact> = action
            .outputs
            .iter()
            .map(|identity| ProducedFact {
                identity: identity.clone(),
                digest: base_fact.digest.clone(),
                size_bytes: base_fact.size_bytes,
            })
            .collect();
        if action.kind == RequiredActionKind::LinkNativeExecutable {
            requested_artifact = outputs.first().cloned();
        }
        let discharge_kind = match action.kind {
            RequiredActionKind::CompileRustObject
            | RequiredActionKind::CompileNimStaticLibrary
            | RequiredActionKind::CompileCObject
            | RequiredActionKind::CompileCppAdapterObject => DischargeKind::Generated,
            RequiredActionKind::ArchiveStaticLibrary | RequiredActionKind::LinkNativeExecutable => {
                DischargeKind::StaticallyLinked
            }
            RequiredActionKind::PreflightRuntimeContract => DischargeKind::ExternalRuntimeContract,
            RequiredActionKind::PublishProvenance => DischargeKind::Embedded,
        };
        let operation = OperationEvidence {
            operation_id: format!("m1-run/{}", action.id),
            action_id: action.id.clone(),
            action_kind: action.kind,
            run_id: "m1-fixed-graph".to_owned(),
            producer_identity: action.toolchain.clone(),
            sequence,
            inputs: action.inputs.clone(),
            outputs,
            outcome: OperationOutcome::Succeeded,
            structurally_verified: true,
            detail: match action.kind {
                RequiredActionKind::PreflightRuntimeContract => {
                    runtime_preflight.as_operation_detail()
                }
                RequiredActionKind::PublishProvenance => {
                    "serialized operation trace and obligation histories".to_owned()
                }
                _ => "real output digest and structure verified".to_owned(),
            },
        };
        if action.kind == RequiredActionKind::PreflightRuntimeContract {
            let contract_sequence = sequence + 1;
            production.record_runtime_operation(
                operation.clone(),
                RuntimeContractEvidence {
                    contract_id: format!("runtime:elf:{}", runtime_preflight.target_triple),
                    operation_id: operation.operation_id.clone(),
                    run_id: operation.run_id.clone(),
                    producer_identity: operation.producer_identity.clone(),
                    sequence: contract_sequence,
                    target_triple: runtime_preflight.target_triple.clone(),
                    loader_requirement: runtime_preflight
                        .interpreter
                        .to_string_lossy()
                        .into_owned(),
                    required_providers: runtime_preflight.shared_library_bindings.clone(),
                    version_requirement: format!(
                        "OS ABI compatible with {}",
                        runtime_preflight.target_triple
                    ),
                    verification: "independently observable ELF interpreter and provider paths"
                        .to_owned(),
                },
            )?;
            sequence += 2;
        } else {
            production.record_operation(operation, discharge_kind)?;
            sequence += 1;
        }
    }
    let requested_artifact = requested_artifact
        .ok_or_else(|| G2Error::Io("link action produced no executable identity".to_owned()))?;
    let manifest = match production.publication_manifest(requested_artifact) {
        Ok(manifest) => manifest,
        Err(LifecycleError::PublicationBlocked(blockers)) => {
            remove_staging_file(&staged_provenance_path)?;
            remove_staging_file(&staged_manifest_path)?;
            return Err(G2Error::PublicationBlocked {
                blockers,
                executed_actions: production
                    .operations
                    .iter()
                    .map(|operation| operation.action_id.clone())
                    .collect(),
                unpublished_paths: vec![provenance_path, manifest_path],
            });
        }
        Err(error) => return Err(error.into()),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| G2Error::Io(format!("serialize artifact manifest: {error}")))?;
    write_staged_file(&staged_manifest_path, &manifest_bytes)?;
    publish_staged_file(&staged_provenance_path, &provenance_path)?;
    publish_staged_file(&staged_manifest_path, &manifest_path)?;
    Ok(FixedGraphPublication {
        executable,
        runtime_preflight,
        manifest,
        provenance_path,
        manifest_path,
    })
}

fn write_staged_file(path: &Path, bytes: &[u8]) -> Result<(), G2Error> {
    fs::write(path, bytes).map_err(|error| G2Error::Io(format!("{}: {error}", path.display())))?;
    let file = fs::File::open(path)
        .map_err(|error| G2Error::Io(format!("{}: {error}", path.display())))?;
    file.sync_all()
        .map_err(|error| G2Error::Io(format!("{}: {error}", path.display())))
}

fn remove_staging_file(path: &Path) -> Result<(), G2Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(G2Error::Io(format!("{}: {error}", path.display()))),
    }
}

fn publish_staged_file(staged: &Path, published: &Path) -> Result<(), G2Error> {
    fs::rename(staged, published).map_err(|error| {
        G2Error::Io(format!(
            "publish {} as {}: {error}",
            staged.display(),
            published.display()
        ))
    })
}

fn observed_file(path: &Path) -> Result<ProducedFact, G2Error> {
    let size_bytes = fs::metadata(path)
        .map_err(|error| G2Error::Io(format!("{}: {error}", path.display())))?
        .len();
    Ok(ProducedFact {
        identity: String::new(),
        digest: sha256_file(path)?,
        size_bytes,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::command_runner::RecordingCommandRunner;
    use crate::cross_ecosystem_ingest::{
        ingest_cargo_metadata, ingest_fixture_input, FixtureLayout,
    };
    use laminaria_plan::dependency_graph::{resolve, ObligationKind, ObligationState};

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

    #[test]
    fn fixed_m1_production_entry_point_publishes_only_the_fully_closed_exact_artifact() {
        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-issue-88-production-{}",
            std::process::id()
        ));
        let publication = execute_fixed_m1_graph(&repo_root(), &out_dir)
            .expect("the fixed production graph must execute and close");
        assert_eq!(publication.executable.exit_code, 0);
        assert_eq!(publication.executable.stdout, "9\n");
        assert_eq!(
            publication.runtime_preflight.executable_sha256,
            publication.executable.executable_sha256
        );
        assert_eq!(publication.runtime_preflight.exit_code, 0);
        assert_eq!(publication.runtime_preflight.stdout, "9\n");
        assert_eq!(
            fs::read_dir(
                publication
                    .runtime_preflight
                    .clean_executable_path
                    .parent()
                    .unwrap()
            )
            .unwrap()
            .count(),
            1,
            "the clean consumption directory must contain only the exact executable"
        );
        assert!(publication.runtime_preflight.interpreter.is_file());
        assert!(!publication
            .runtime_preflight
            .shared_library_bindings
            .is_empty());
        assert_eq!(
            publication.manifest.requested_artifact.digest,
            publication.executable.executable_sha256
        );
        assert!(publication.provenance_path.is_file());
        assert!(publication.manifest_path.is_file());
        let persisted_manifest: ArtifactManifest = serde_json::from_slice(
            &fs::read(&publication.manifest_path).expect("published manifest must be readable"),
        )
        .expect("published manifest must be valid JSON");
        assert_eq!(persisted_manifest, publication.manifest);
        let provenance_operation = publication
            .manifest
            .operations
            .iter()
            .find(|operation| operation.action_kind == RequiredActionKind::PublishProvenance)
            .expect("publication must have causal provenance evidence");
        let mut observed_provenance = observed_file(&publication.provenance_path)
            .expect("published provenance must be observable");
        observed_provenance.identity = provenance_operation.outputs[0].identity.clone();
        assert_eq!(provenance_operation.outputs, vec![observed_provenance]);
        assert!(publication
            .manifest
            .decisions
            .iter()
            .all(|decision| matches!(
                decision.state,
                ObligationState::Discharged
                    | ObligationState::Externalized
                    | ObligationState::ProvenIrrelevant
            )));
        let runtime_decision = publication
            .manifest
            .decisions
            .iter()
            .find(|decision| decision.kind == ObligationKind::Runtime)
            .expect("published manifest must contain the runtime obligation");
        assert_eq!(runtime_decision.state, ObligationState::Externalized);
        assert_eq!(publication.manifest.runtime_contracts.len(), 1);
        assert_eq!(
            publication.manifest.runtime_contracts[0].operation_id,
            "m1-run/preflight-runtime-contract"
        );
        let observed_kinds: BTreeSet<ObligationKind> = publication
            .manifest
            .decisions
            .iter()
            .map(|decision| decision.kind)
            .collect();
        assert_eq!(
            observed_kinds,
            BTreeSet::from([
                ObligationKind::NativeExecutableDemand,
                ObligationKind::PackageSelection,
                ObligationKind::SourceModule,
                ObligationKind::SemanticFfi,
                ObligationKind::Lowering,
                ObligationKind::AbiTarget,
                ObligationKind::ArtifactProduction,
                ObligationKind::Symbol,
                ObligationKind::LinkOrder,
                ObligationKind::FinalLink,
                ObligationKind::Runtime,
                ObligationKind::Provenance,
            ])
        );
        assert!(publication.manifest.decisions.iter().all(|decision| {
            decision
                .history
                .last()
                .is_some_and(|transition| transition.to == decision.state)
        }));
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn fixed_m1_production_entry_point_blocks_a_seeded_missing_provenance_operation() {
        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-issue-88-negative-{}",
            std::process::id()
        ));
        let layout = FixtureLayout::discover();
        let input = ingest_fixture_input(&RealCommandRunner, &layout, &["1.0.0"])
            .expect("must ingest the fixed production graph");
        let closure = resolve(&input).expect("must resolve the fixed production graph");
        let expected_actions: Vec<String> = closure
            .required_actions
            .iter()
            .filter(|action| action.kind != RequiredActionKind::PublishProvenance)
            .map(|action| action.id.clone())
            .collect();

        let error = execute_fixed_m1_graph_with_seed(
            &repo_root(),
            &out_dir,
            SeededGap::Omit(RequiredActionKind::PublishProvenance),
        )
        .expect_err("missing provenance must block publication");
        let G2Error::PublicationBlocked {
            blockers,
            executed_actions,
            unpublished_paths,
        } = error
        else {
            panic!("expected a structured publication failure, got {error:?}")
        };
        assert_eq!(
            blockers,
            vec![PublicationBlocker::UnclosedObligation {
                obligation_id: "Provenance:native_executable".to_owned(),
                state: ObligationState::Satisfied,
            }]
        );
        assert_eq!(executed_actions, expected_actions);
        assert_eq!(
            unpublished_paths,
            vec![
                out_dir.join("laminaria-provenance.json"),
                out_dir.join("laminaria-artifact-manifest.json")
            ]
        );
        assert!(unpublished_paths.iter().all(|path| !path.exists()));
        assert!(!out_dir.join(".laminaria-provenance.json.staging").exists());
        assert!(!out_dir
            .join(".laminaria-artifact-manifest.json.staging")
            .exists());
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn runtime_preflight_rejects_a_missing_externalized_provider() {
        let missing = Path::new("/laminaria-missing-runtime/ld-does-not-exist.so");
        let error = require_runtime_path(missing, "seeded missing dynamic loader")
            .expect_err("a missing runtime provider must fail preflight");
        assert!(matches!(
            error,
            G2Error::RuntimeContractUnavailable { contract, detail }
                if contract == "seeded missing dynamic loader"
                    && detail.contains("ld-does-not-exist.so")
        ));
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

    /// Checkpoint 4's end-to-end case: resolve a real G1 positive
    /// closure, actually run `c++ -c` then `ar rcs` against the real
    /// `cpp/cppmax/cppmax.cpp` adapter source, and discharge the
    /// resulting `ArtifactProduction:archive:cppmax` obligation with
    /// the real, independently verified result. `c++`/`ar` resolve off
    /// `PATH` (no `toolchain_resolve` needed, unlike Checkpoints 2/3).
    #[test]
    fn g2_really_compiles_and_archives_cppmax_and_discharges_its_obligation() {
        let layout = FixtureLayout::discover();
        let runner = RecordingCommandRunner::new();
        let input = ingest_fixture_input(&runner, &layout, &["1.0.0"])
            .expect("must ingest the real positive fixture");
        let mut closure = resolve(&input).expect("must resolve a positive closure");

        let obligation_id = "ArtifactProduction:archive:cppmax";
        assert_eq!(
            closure.obligations[obligation_id].state,
            ObligationState::Satisfied,
            "must start from G1's own terminal state"
        );

        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-g2-checkpoint-4-{}-cppmax",
            std::process::id()
        ));
        let evidence = compile_and_archive_cppmax(&mut closure, &layout.root, &out_dir)
            .expect("real c++ + ar execution and discharge must succeed");

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
        assert_eq!(evidence.confirmed_defined_symbol, "cpp_max_i32");

        let obligation = &closure.obligations[obligation_id];
        assert_eq!(obligation.state, ObligationState::Discharged);
        assert_eq!(
            obligation.discharge_kind,
            Some(DischargeKind::StaticallyLinked)
        );
        assert_eq!(
            obligation.required_action.as_deref(),
            Some("archive-static-library:cppmax@1.0.0")
        );
        assert!(obligation
            .evidence
            .as_deref()
            .unwrap()
            .contains(&evidence.archive_sha256));

        let _ = fs::remove_dir_all(&out_dir);
    }

    /// Checkpoint 5's end-to-end case, and this whole module's
    /// capstone: resolve a real G1 positive closure, actually run every
    /// prior checkpoint's real compile/archive action in order (exactly
    /// as a real G2 pipeline would), then actually link and **run** the
    /// resulting native executable, asserting the real, observed result
    /// matches the fixture's own declared expectation
    /// (`c_add(nim_double(cpp_max_i32(3, 4)), 1)` == 9) -- and that all
    /// three obligations `link-native-executable` discharges reach
    /// `Discharged` with real, consistent evidence.
    #[test]
    fn g2_really_links_and_runs_the_native_executable_with_the_expected_result() {
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
        let doubler_exported_symbol = input
            .package_candidates
            .iter()
            .find(|c| c.package_id == "doubler")
            .and_then(|c| c.declared_exports.first())
            .map(|e| e.symbol.clone())
            .expect("the real doubler candidate must declare at least one export");

        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-g2-checkpoint-5-{}-link",
            std::process::id()
        ));

        let cadd_evidence = compile_and_archive_cadd_v1(&mut closure, &layout.root, &out_dir)
            .expect("Checkpoint 1's own real compile+archive must still succeed");
        let cppmax_evidence = compile_and_archive_cppmax(&mut closure, &layout.root, &out_dir)
            .expect("Checkpoint 4's own real compile+archive must still succeed");
        let doubler_evidence = compile_nim_static_library_for_doubler(
            &mut closure,
            &layout.root,
            &repo_root(),
            &doubler_exported_symbol,
            &out_dir,
        )
        .expect("Checkpoint 3's own real nim compile must still succeed");
        compile_rust_object_for_app(
            &mut closure,
            &layout.root,
            &repo_root(),
            &cargo,
            &required_extern_symbols,
            &out_dir,
        )
        .expect("Checkpoint 2's own real rustc --emit=obj compile must still succeed");

        for obligation_id in [
            "FinalLink:native_executable",
            "LinkOrder:native_executable",
            "ArtifactProduction:executable:app",
        ] {
            assert_eq!(
                closure.obligations[obligation_id].state,
                ObligationState::Satisfied,
                "must start from G1's own terminal state for '{obligation_id}'"
            );
        }

        let evidence = link_and_run_native_executable(
            &mut closure,
            &layout.root,
            &repo_root(),
            &cargo,
            &NativeArchives {
                cadd: &cadd_evidence.archive_path,
                cppmax: &cppmax_evidence.archive_path,
                doubler: &doubler_evidence.archive_path,
            },
            &out_dir,
        )
        .expect("real link + real run of the native executable must succeed");

        assert!(
            evidence.executable_path.is_file(),
            "the real linked executable must exist on disk"
        );
        assert!(evidence.executable_size_bytes > 0);
        assert_eq!(
            evidence.executable_sha256.len(),
            64,
            "must be a real hex SHA-256"
        );
        assert_eq!(evidence.exit_code, 0);
        assert_eq!(evidence.stdout, "9\n");
        assert_eq!(evidence.stderr, "");

        for obligation_id in [
            "FinalLink:native_executable",
            "LinkOrder:native_executable",
            "ArtifactProduction:executable:app",
        ] {
            let obligation = &closure.obligations[obligation_id];
            assert_eq!(obligation.state, ObligationState::Discharged);
            assert_eq!(
                obligation.discharge_kind,
                Some(DischargeKind::StaticallyLinked)
            );
            assert_eq!(
                obligation.required_action.as_deref(),
                Some("link-native-executable")
            );
            assert!(obligation
                .evidence
                .as_deref()
                .unwrap()
                .contains(&evidence.executable_sha256));
        }

        let _ = fs::remove_dir_all(&out_dir);
    }

    /// A real, successfully linked executable that actually runs but
    /// produces the wrong observable result must be refused by
    /// `link_and_run_native_executable` itself -- not merely detectable
    /// by some separate check -- exercised end-to-end with a genuinely
    /// different `cadd` implementation (`a * b` instead of `a + b`, same
    /// exported symbol, same header, so it links cleanly) standing in
    /// for the real one.
    #[test]
    fn a_real_but_wrong_executable_result_is_refused_by_link_and_run() {
        let layout = FixtureLayout::discover();
        let runner = RecordingCommandRunner::new();
        let input = ingest_fixture_input(&runner, &layout, &["1.0.0"])
            .expect("must ingest the real positive fixture");
        let cargo = ingest_cargo_metadata(&runner, &layout.app_manifest())
            .expect("must ingest the real app Cargo manifest");

        let out_dir = std::env::temp_dir().join(format!(
            "laminaria-g2-checkpoint-5-negative-{}",
            std::process::id()
        ));
        fs::create_dir_all(&out_dir).unwrap();

        // A real, linkable `cadd` that exports the same `c_add` symbol
        // but computes a different result -- links cleanly (same
        // symbol, same signature), only the *observed* result differs.
        let wrong_cadd_dir = out_dir.join("wrong-cadd");
        fs::create_dir_all(&wrong_cadd_dir).unwrap();
        fs::copy(
            layout.root.join("c/cadd/v1/cadd.h"),
            wrong_cadd_dir.join("cadd.h"),
        )
        .unwrap();
        fs::write(
            wrong_cadd_dir.join("cadd.c"),
            "#include \"cadd.h\"\nint c_add(int a, int b) { return a * b; }\n",
        )
        .unwrap();
        let wrong_cadd_evidence = compile_and_archive_native_source(
            "cc",
            &wrong_cadd_dir.join("cadd.c"),
            &wrong_cadd_dir,
            &out_dir,
            "wrong-cadd.o",
            "libcadd.a",
            "c_add",
        )
        .expect("the deliberately wrong but still valid cadd must still compile and archive");

        let mut closure = resolve(&input).expect("must resolve a positive closure");
        let cppmax_evidence = compile_and_archive_cppmax(&mut closure, &layout.root, &out_dir)
            .expect("the real cppmax archive must still build");
        let doubler_exported_symbol = input
            .package_candidates
            .iter()
            .find(|c| c.package_id == "doubler")
            .and_then(|c| c.declared_exports.first())
            .map(|e| e.symbol.clone())
            .expect("the real doubler candidate must declare at least one export");
        let doubler_evidence = compile_nim_static_library_for_doubler(
            &mut closure,
            &layout.root,
            &repo_root(),
            &doubler_exported_symbol,
            &out_dir,
        )
        .expect("the real doubler archive must still build");

        let result = link_and_run_native_executable(
            &mut closure,
            &layout.root,
            &repo_root(),
            &cargo,
            &NativeArchives {
                cadd: &wrong_cadd_evidence.archive_path,
                cppmax: &cppmax_evidence.archive_path,
                doubler: &doubler_evidence.archive_path,
            },
            &out_dir,
        );

        match result {
            Err(G2Error::UnexpectedExecutionResult {
                exit_code, stdout, ..
            }) => {
                // cpp_max_i32(3, 4) = 4, nim_double(4) = 8, wrong
                // c_add(8, 1) = 8 * 1 = 8, not 9 -- main.rs's own real
                // `if result == 9 { 0 } else { 1 }` makes exit 1.
                assert_eq!(exit_code, Some(1));
                assert_eq!(stdout, "8\n");
            }
            other => panic!("expected UnexpectedExecutionResult, got {other:?}"),
        }
        // A refused link+run must never discharge anything.
        assert_eq!(
            closure.obligations["FinalLink:native_executable"].state,
            ObligationState::Satisfied
        );

        let _ = fs::remove_dir_all(&out_dir);
    }
}

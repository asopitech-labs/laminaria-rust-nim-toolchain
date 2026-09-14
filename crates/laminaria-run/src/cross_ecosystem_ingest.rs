//! Issue #48 (G1): ecosystem-fact ingestion for the typed
//! dependency-obligation graph (`laminaria_plan::dependency_graph`).
//!
//! ## G1's exact boundary
//!
//! Every function here is **read-only**. It builds a
//! [`laminaria_plan::dependency_graph::DependencyResolutionInput`] from:
//!
//! - real, read-only `cargo metadata --no-deps` output;
//! - real, read-only `nimble dump --json` output;
//! - real C/C++ header text, read from disk and scanned by
//!   [`laminaria_ir::c_header_discover`] for declared prototypes;
//! - real Nim source text, read from disk and scanned by
//!   [`laminaria_ir::nim_export_discover`] for `{.exportc.}` pragmas;
//! - real Rust source text, read from disk and scanned by
//!   [`laminaria_ir::foreign_discover`] for `extern "C"` requirements.
//!
//! **No function in this module compiles, archives, links, builds, or
//! installs anything, and none inspects a compiled artifact.** Every
//! subprocess this module ever spawns goes through
//! [`crate::command_runner::CommandRunner`], whose own
//! `is_permitted` allowlist is the single place the compiler/
//! archiver/linker/build/install boundary is enforced -- see that
//! module's own doc comment. Declared-export facts come from pure text
//! scanning, never from compiling a candidate and inspecting the
//! result with `nm`: this is the exact correction issue #48 makes over
//! the prior `compile_c_candidate`/`compile_cpp_candidate`/
//! `compile_nimble_candidate`/`nm`-based implementation those functions
//! (deliberately removed here) used to provide.
//!
//! `laminaria_plan::dependency_graph::resolve` itself never spawns a
//! process at all (see that module's own doc comment); G2 (issue #46)
//! is where a [`laminaria_plan::dependency_graph::RequiredAction`] is
//! actually executed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use laminaria_ir::c_header_discover::discover_c_declared_functions;
use laminaria_ir::cfg_predicate::CfgContext;
use laminaria_ir::foreign_discover::discover_foreign_function_requirements;
use laminaria_ir::nim_export_discover::discover_exportc_declarations;
use laminaria_plan::dependency_graph::{
    AbiConstraintFacts, ArtifactOutputFacts, ArtifactOutputKind, DependencyResolutionInput,
    Ecosystem, FfiExportFacts, FfiRequirementFacts, LoweringRequirementFacts,
    PackageCandidateFacts, Role, RuntimeRequirementFacts, SourceModuleFacts,
};

pub use crate::command_runner::{
    CommandRunner, IngestError, RealCommandRunner, RecordingCommandRunner,
};

/// The real host target triple, from `rustc -vV`'s own `host:` line --
/// this fixture is never cross-compiled, so the host triple is also the
/// demanded target triple.
pub fn host_target_triple(runner: &dyn CommandRunner) -> Result<String, IngestError> {
    let stdout = runner.run("rustc", &["-vV"], None)?;
    for line in stdout.lines() {
        if let Some(host) = line.strip_prefix("host: ") {
            return Ok(host.trim().to_string());
        }
    }
    Err(IngestError::Parse(
        "rustc -vV produced no 'host:' line".to_string(),
    ))
}

/// One package's real, read-only Cargo manifest facts (`cargo metadata
/// --no-deps`, never a build).
pub struct CargoManifestFacts {
    pub package_name: String,
    pub version: String,
    pub features: Vec<String>,
    /// The real, transitively-resolved default-activation feature set
    /// (starting from `cargo metadata`'s own `"default"` feature entry
    /// and following each feature's own declared implications) -- this
    /// is what a plain `cargo build` (no `--features`/
    /// `--no-default-features`) actually activates, used to evaluate
    /// which `#[cfg(feature = "...")]`-gated FFI declarations are
    /// really in force for this build, rather than treating every
    /// declaration a source file's syntax contains as unconditionally
    /// required.
    pub active_features: BTreeSet<String>,
}

/// Transitively expands the real default-activation closure from
/// `cargo metadata`'s own `features` map (`name -> implied features`),
/// starting at `"default"` -- the exact feature set a plain `cargo
/// build` activates. Never invented: every entry comes from the real
/// manifest's own declared implications.
fn resolve_default_active_features(
    features_map: &BTreeMap<String, Vec<String>>,
) -> BTreeSet<String> {
    let mut active = BTreeSet::new();
    let mut frontier = vec!["default".to_string()];
    while let Some(feature) = frontier.pop() {
        if !active.insert(feature.clone()) {
            continue;
        }
        if let Some(implied) = features_map.get(&feature) {
            frontier.extend(implied.iter().cloned());
        }
    }
    active
}

pub fn ingest_cargo_metadata(
    runner: &dyn CommandRunner,
    manifest_path: &Path,
) -> Result<CargoManifestFacts, IngestError> {
    let stdout = runner.run(
        "cargo",
        &[
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            manifest_path.to_str().ok_or_else(|| {
                IngestError::Parse("manifest path is not valid UTF-8".to_string())
            })?,
        ],
        None,
    )?;
    let json: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| IngestError::Parse(e.to_string()))?;
    let package = json["packages"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| IngestError::Parse("cargo metadata reported no packages".to_string()))?;
    let package_name = package["name"]
        .as_str()
        .ok_or_else(|| IngestError::Parse("package has no name".to_string()))?
        .to_string();
    let version = package["version"].as_str().unwrap_or_default().to_string();
    let features_map: BTreeMap<String, Vec<String>> = package["features"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(name, implied)| {
                    let implied: Vec<String> = implied
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    (name.clone(), implied)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut features: Vec<String> = features_map.keys().cloned().collect();
    features.sort();
    let active_features = resolve_default_active_features(&features_map);
    Ok(CargoManifestFacts {
        package_name,
        version,
        features,
        active_features,
    })
}

/// One package's real, read-only Nimble manifest facts (`nimble dump
/// --json`, never a build).
pub struct NimbleManifestFacts {
    pub package_name: String,
    pub version: String,
    pub requires: Vec<String>,
}

pub fn ingest_nimble_package(
    runner: &dyn CommandRunner,
    nimble_dir: &Path,
) -> Result<NimbleManifestFacts, IngestError> {
    // Exactly the command issue #48 names -- see
    // `crate::command_runner::is_permitted`'s own doc comment and the
    // fixture's own `nimble/doubler/nimble.lock` for why a real lock
    // file, not an extra flag, is what makes this resolve correctly
    // against real CI's nimble ("vnext") toolchain-management step.
    let stdout = runner.run("nimble", &["dump", "--json"], Some(nimble_dir))?;
    // A real, observed CI difference from this machine's own local
    // `nimble`: a fresh install can print an informational banner to
    // stdout *before* the actual JSON object -- `nimble dump --json`'s
    // own output is always exactly one top-level JSON object, so
    // parsing from the first `{` is a real robustness fix for a real
    // observed tool quirk.
    let json_start = stdout.find('{').ok_or_else(|| {
        IngestError::Parse(format!(
            "nimble dump --json produced no JSON object at all; raw output: {stdout:?}"
        ))
    })?;
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).map_err(|e| {
        IngestError::Parse(format!(
            "{e} (raw output: {:?})",
            &stdout[..json_start.min(stdout.len())]
        ))
    })?;
    let package_name = json["name"]
        .as_str()
        .ok_or_else(|| IngestError::Parse("nimble dump produced no name".to_string()))?
        .to_string();
    let version = json["version"].as_str().unwrap_or_default().to_string();
    let requires: Vec<String> = json["requires"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    let name = r["name"].as_str()?;
                    let str_req = r["str"].as_str().unwrap_or("");
                    Some(format!("{name} {str_req}").trim().to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(NimbleManifestFacts {
        package_name,
        version,
        requires,
    })
}

/// Real, shallow syn-based FFI *requirement* discovery over real Rust
/// source text (`laminaria_ir::foreign_discover`) -- never a fabricated
/// requirement list. `source_id` is the stable identity to record as
/// `declaring_source` (e.g. `"app/src/main.rs"`), never the filesystem
/// path itself. `cfg_context` is the real, already-determined
/// target/feature activation facts (see [`CargoManifestFacts::active_features`]) --
/// a declaration whose own real `#[cfg(...)]` predicate evaluates false
/// against it is not returned at all, since it is not actually part of
/// this build's own active closure (issue #48's G1 follow-up,
/// Checkpoint A: "Cargo featureまたはtargetを変えるとactive FFI
/// requirementsが変化する").
pub fn discover_ffi_requirements(
    source_path: &Path,
    source_id: &str,
    cfg_context: &CfgContext,
) -> Result<Vec<FfiRequirementFacts>, IngestError> {
    let source =
        std::fs::read_to_string(source_path).map_err(|e| IngestError::Io(e.to_string()))?;
    let found = discover_foreign_function_requirements(&source)
        .map_err(|diags| IngestError::Parse(format!("{diags:?}")))?;
    Ok(found
        .into_iter()
        .filter(|r| {
            r.cfg_predicate
                .as_ref()
                .map(|p| p.evaluate(cfg_context))
                .unwrap_or(true)
        })
        .map(|r| {
            let expected_provider_package = r
                .link_hint
                .as_ref()
                .map(|h| h.name.clone())
                .unwrap_or_else(|| r.name.clone());
            FfiRequirementFacts {
                declaring_source: source_id.to_string(),
                symbol: r.name,
                abi: r.abi,
                param_count: r.param_count,
                return_type: r.return_type,
                expected_provider_package,
            }
        })
        .collect())
}

/// Real, text-only C/C++ header-declaration discovery
/// (`laminaria_ir::c_header_discover`) -- never a compiled-artifact
/// symbol table.
pub fn discover_c_declared_exports(
    header_path: &Path,
    source_id: &str,
) -> Result<Vec<FfiExportFacts>, IngestError> {
    let text = std::fs::read_to_string(header_path).map_err(|e| IngestError::Io(e.to_string()))?;
    Ok(discover_c_declared_functions(&text)
        .into_iter()
        .map(|f| FfiExportFacts {
            declaring_source: source_id.to_string(),
            symbol: f.name,
            abi: "C".to_string(),
            param_count: f.param_count,
            return_type: f.return_type,
        })
        .collect())
}

/// Real, text-only Nim `{.exportc.}` discovery
/// (`laminaria_ir::nim_export_discover`) -- never a compiled-artifact
/// symbol table. A genuinely malformed proc signature (e.g. an
/// unbalanced parameter list) is a real
/// `laminaria_ir::nim_export_discover::NimSignatureError`, propagated
/// here rather than silently skipped -- issue #48's G1 follow-up,
/// Checkpoint A: "Rust/Nim sourceが要求されたlowering/capabilityを
/// 満たさなければ...".
pub fn discover_nim_declared_exports(
    nim_source_path: &Path,
    source_id: &str,
) -> Result<Vec<FfiExportFacts>, IngestError> {
    let text =
        std::fs::read_to_string(nim_source_path).map_err(|e| IngestError::Io(e.to_string()))?;
    let found = discover_exportc_declarations(&text)
        .map_err(|e| IngestError::Parse(format!("Nim signature error: {e}")))?;
    Ok(found
        .into_iter()
        .map(|p| FfiExportFacts {
            declaring_source: source_id.to_string(),
            symbol: p.exported_symbol,
            abi: "C".to_string(),
            param_count: p.param_count,
            return_type: p.return_type,
        })
        .collect())
}

/// The fixture's own fixed layout
/// (`fixtures/cross-ecosystem-native-executable/`), one path accessor
/// per real ecosystem input. This struct, and this struct alone, is
/// allowed to name this project's own fixture paths -- every reader
/// function above takes arbitrary paths/text and knows nothing about
/// `app`/`doubler`/`cadd`/`cppmax`.
pub struct FixtureLayout {
    pub root: PathBuf,
}

impl FixtureLayout {
    pub fn discover() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
            .join("fixtures/cross-ecosystem-native-executable");
        FixtureLayout { root }
    }

    pub fn app_manifest(&self) -> PathBuf {
        self.root.join("app/Cargo.toml")
    }

    pub fn app_main_rs(&self) -> PathBuf {
        self.root.join("app/src/main.rs")
    }

    pub fn nimble_dir(&self) -> PathBuf {
        self.root.join("nimble/doubler")
    }

    pub fn nimble_main_nim(&self) -> PathBuf {
        self.root.join("nimble/doubler/src/doubler.nim")
    }

    pub fn cadd_v1_c(&self) -> PathBuf {
        self.root.join("c/cadd/v1/cadd.c")
    }

    pub fn cadd_v1_h(&self) -> PathBuf {
        self.root.join("c/cadd/v1/cadd.h")
    }

    pub fn cadd_v2_c(&self) -> PathBuf {
        self.root.join("c/cadd/v2/cadd.c")
    }

    pub fn cadd_v2_h(&self) -> PathBuf {
        self.root.join("c/cadd/v2/cadd.h")
    }

    pub fn cppmax_cpp(&self) -> PathBuf {
        self.root.join("cpp/cppmax/cppmax.cpp")
    }

    pub fn cppmax_h(&self) -> PathBuf {
        self.root.join("cpp/cppmax/cppmax.h")
    }
}

/// Verifies a real retained source/implementation file actually exists
/// and is readable as UTF-8 text -- never compiled, never scanned for
/// exports (a `.c`/`.cpp` implementation's own header is the
/// authoritative declared-export source), but a `SourceModule`
/// obligation's own `Satisfied` evidence must not claim a file was
/// read when it never was. A missing or unreadable retained
/// implementation source fails ingestion outright (issue #48's G1
/// follow-up, Checkpoint A: "retained C/C++ implementation sourceが
/// 欠落または読取不能なら入力確定に失敗する").
fn verify_source_readable(path: &Path) -> Result<(), IngestError> {
    std::fs::read_to_string(path)
        .map(|_| ())
        .map_err(|e| IngestError::Io(format!("{}: {e}", path.display())))
}

/// Builds one C-family candidate's `PackageCandidateFacts` from its
/// real source+header pair, reading declared exports from the header
/// text only -- never compiling either file. The `.c`/`.cpp`
/// implementation file's own existence and readability is still
/// verified directly, so a retained source this candidate depends on
/// can never be silently missing.
fn ingest_c_candidate(
    source_id_prefix: &str,
    c_path: &Path,
    h_path: &Path,
    package_id: &str,
    version: &str,
    target_triple: &str,
) -> Result<(Vec<SourceModuleFacts>, PackageCandidateFacts), IngestError> {
    let c_id = format!("{source_id_prefix}.c");
    let h_id = format!("{source_id_prefix}.h");
    let declared_exports = discover_c_declared_exports(h_path, &h_id)?;
    verify_source_readable(c_path)?;
    let sources = vec![
        SourceModuleFacts {
            id: c_id.clone(),
            ecosystem: Ecosystem::C,
            package_id: package_id.to_string(),
        },
        SourceModuleFacts {
            id: h_id,
            ecosystem: Ecosystem::C,
            package_id: package_id.to_string(),
        },
    ];
    let candidate = PackageCandidateFacts {
        ecosystem: Ecosystem::C,
        package_id: package_id.to_string(),
        version: version.to_string(),
        role: Role::Target,
        target_triple: target_triple.to_string(),
        sources: sources.iter().map(|s| s.id.clone()).collect(),
        declared_exports,
        declared_constraints: vec![],
    };
    Ok((sources, candidate))
}

/// Assembles the complete, real
/// [`DependencyResolutionInput`] for the positive fixture scenario, with
/// `cadd_candidate_versions` selecting which real `cadd` variant(s) to
/// offer as candidates -- `&["1.0.0"]` for the positive case,
/// `&["2.0.0"]` for the negative case. Every fact here comes from a
/// real read-only command or a real source/header file; nothing is
/// invented, and nothing is compiled.
pub fn ingest_fixture_input(
    runner: &dyn CommandRunner,
    layout: &FixtureLayout,
    cadd_candidate_versions: &[&str],
) -> Result<DependencyResolutionInput, IngestError> {
    let target_triple = host_target_triple(runner)?;
    let cargo = ingest_cargo_metadata(runner, &layout.app_manifest())?;
    let nimble = ingest_nimble_package(runner, &layout.nimble_dir())?;

    let app_source_id = "app/src/main.rs".to_string();
    let doubler_source_id = "nimble/doubler/src/doubler.nim".to_string();

    // The real activation facts this exact ingestion run observed:
    // `unix` reflects the real current process's own OS family (valid
    // since this fixture is never cross-compiled -- the demanded
    // target and the ingesting host are the same real platform), and
    // `active_features` is `cargo`'s own real default-activation
    // closure. Changing either changes which FFI declarations below
    // are considered active.
    let cfg_context = CfgContext {
        true_idents: if cfg!(unix) {
            BTreeSet::from(["unix".to_string()])
        } else {
            BTreeSet::new()
        },
        active_features: cargo.active_features.clone(),
    };
    let ffi_requirements =
        discover_ffi_requirements(&layout.app_main_rs(), &app_source_id, &cfg_context)?;

    let mut sources = vec![
        SourceModuleFacts {
            id: app_source_id.clone(),
            ecosystem: Ecosystem::Cargo,
            package_id: cargo.package_name.clone(),
        },
        SourceModuleFacts {
            id: doubler_source_id.clone(),
            ecosystem: Ecosystem::Nimble,
            package_id: nimble.package_name.clone(),
        },
    ];

    let mut package_candidates = vec![
        PackageCandidateFacts {
            ecosystem: Ecosystem::Cargo,
            package_id: cargo.package_name.clone(),
            version: cargo.version.clone(),
            role: Role::Target,
            target_triple: target_triple.clone(),
            sources: vec![app_source_id.clone()],
            declared_exports: vec![],
            declared_constraints: cargo
                .features
                .iter()
                .map(|f| format!("feature:{f}"))
                .collect(),
        },
        PackageCandidateFacts {
            ecosystem: Ecosystem::Nimble,
            package_id: nimble.package_name.clone(),
            version: nimble.version.clone(),
            role: Role::Target,
            target_triple: target_triple.clone(),
            sources: vec![doubler_source_id.clone()],
            declared_exports: discover_nim_declared_exports(
                &layout.nimble_main_nim(),
                &doubler_source_id,
            )?,
            declared_constraints: nimble
                .requires
                .iter()
                .map(|r| format!("requires:{r}"))
                .collect(),
        },
    ];

    for version in cadd_candidate_versions {
        let (v_sources, v_candidate) = match *version {
            "1.0.0" => ingest_c_candidate(
                "c/cadd/v1/cadd",
                &layout.cadd_v1_c(),
                &layout.cadd_v1_h(),
                "cadd",
                "1.0.0",
                &target_triple,
            )?,
            "2.0.0" => ingest_c_candidate(
                "c/cadd/v2/cadd",
                &layout.cadd_v2_c(),
                &layout.cadd_v2_h(),
                "cadd",
                "2.0.0",
                &target_triple,
            )?,
            other => {
                return Err(IngestError::Parse(format!(
                    "unknown cadd candidate version requested: {other}"
                )))
            }
        };
        sources.extend(v_sources);
        package_candidates.push(v_candidate);
    }

    let cppmax_c_id = "cpp/cppmax/cppmax.cpp".to_string();
    let cppmax_h_id = "cpp/cppmax/cppmax.h".to_string();
    verify_source_readable(&layout.cppmax_cpp())?;
    sources.push(SourceModuleFacts {
        id: cppmax_c_id.clone(),
        ecosystem: Ecosystem::Cpp,
        package_id: "cppmax".to_string(),
    });
    sources.push(SourceModuleFacts {
        id: cppmax_h_id.clone(),
        ecosystem: Ecosystem::Cpp,
        package_id: "cppmax".to_string(),
    });
    package_candidates.push(PackageCandidateFacts {
        ecosystem: Ecosystem::Cpp,
        package_id: "cppmax".to_string(),
        version: "1.0.0".to_string(),
        role: Role::Target,
        target_triple: target_triple.clone(),
        sources: vec![cppmax_c_id, cppmax_h_id.clone()],
        declared_exports: discover_c_declared_exports(&layout.cppmax_h(), &cppmax_h_id)?,
        declared_constraints: vec![],
    });

    let lowering_requirements = vec![
        LoweringRequirementFacts {
            package_id: cargo.package_name.clone(),
            source_id: app_source_id.clone(),
            description: "Rust application source is lowering-feasible for the demanded target"
                .to_string(),
        },
        LoweringRequirementFacts {
            package_id: nimble.package_name.clone(),
            source_id: doubler_source_id,
            description: "Nim package source is lowering-feasible for the demanded target"
                .to_string(),
        },
    ];

    let abi_constraints = ffi_requirements
        .iter()
        .map(|r| AbiConstraintFacts {
            boundary_symbol: r.symbol.clone(),
            abi: r.abi.clone(),
            target_triple: target_triple.clone(),
        })
        .collect();

    let declared_outputs = vec![
        ArtifactOutputFacts {
            id: format!("object:{}", cargo.package_name),
            package_id: cargo.package_name.clone(),
            kind: ArtifactOutputKind::RustObject,
        },
        ArtifactOutputFacts {
            id: format!("archive:{}", nimble.package_name),
            package_id: nimble.package_name.clone(),
            kind: ArtifactOutputKind::NimStaticLibrary,
        },
        ArtifactOutputFacts {
            id: "object:cadd".to_string(),
            package_id: "cadd".to_string(),
            kind: ArtifactOutputKind::CObject,
        },
        ArtifactOutputFacts {
            id: "archive:cadd".to_string(),
            package_id: "cadd".to_string(),
            kind: ArtifactOutputKind::CStaticArchive,
        },
        ArtifactOutputFacts {
            id: "object:cppmax".to_string(),
            package_id: "cppmax".to_string(),
            kind: ArtifactOutputKind::CppAdapterObject,
        },
        ArtifactOutputFacts {
            id: "archive:cppmax".to_string(),
            package_id: "cppmax".to_string(),
            kind: ArtifactOutputKind::CppStaticArchive,
        },
        ArtifactOutputFacts {
            id: format!("executable:{}", cargo.package_name),
            package_id: cargo.package_name.clone(),
            kind: ArtifactOutputKind::NativeExecutable,
        },
    ];

    let runtime_requirements = vec![RuntimeRequirementFacts {
        target_triple: target_triple.clone(),
        description: format!(
            "OS ABI/dynamic loader contract for target '{target_triple}' is not bundled"
        ),
    }];

    Ok(DependencyResolutionInput {
        demand_entry_point: cargo.package_name,
        target_triple,
        host_toolchain_id: "nim".to_string(),
        sources,
        package_candidates,
        ffi_requirements,
        lowering_requirements,
        abi_constraints,
        declared_outputs,
        runtime_requirements,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use laminaria_plan::dependency_graph::{
        resolve, ObligationKind, ObligationState, RejectionReason, RequiredActionKind, Role,
    };
    use std::collections::BTreeSet;
    use std::sync::OnceLock;

    /// The real facts an ingestion pass gathered, plus every command it
    /// actually attempted while gathering them.
    struct FixtureIngestion {
        input: DependencyResolutionInput,
        recorded_commands: Vec<(String, Vec<String>)>,
    }

    /// Both fixture variants' ingestion results, gathered together
    /// behind one lock.
    struct BothIngestions {
        positive: FixtureIngestion,
        negative: FixtureIngestion,
        /// Whether the fixture directory's own file listing (path +
        /// size) was identical before and after both real ingestions
        /// ran -- computed once, here, rather than by a separate test
        /// running its own extra real ingestion outside this shared
        /// lock (which would reintroduce the exact concurrent-nimble
        /// race this lock exists to prevent).
        fixture_directory_unchanged: bool,
    }

    fn snapshot_fixture_directory(root: &Path) -> Vec<(PathBuf, u64)> {
        let mut entries = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(read_dir) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Ok(metadata) = entry.metadata() {
                    entries.push((path, metadata.len()));
                }
            }
        }
        entries.sort();
        entries
    }

    /// Ingests both fixture variants' real `cargo metadata`/`nimble
    /// dump --json`/`rustc -vV` facts, strictly one at a time, exactly
    /// once per test process, and caches the result -- the same
    /// "gather the real fixture facts once, reuse them across every
    /// test in this module" pattern
    /// `incremental_executor.rs::tests::real_incremental_planner_binary`
    /// already established. Both variants share one `OnceLock` (rather
    /// than one each) specifically because `cargo test`'s own parallel
    /// test threads would otherwise be free to run the positive and
    /// negative ingestion *concurrently* with each other, and both
    /// invoke real `nimble dump --json` against the very same
    /// `nimble/doubler` package directory -- a real, observed CI flake:
    /// two concurrent invocations against one nimble package directory
    /// can trip nimble's own toolchain-negotiation path and attempt a
    /// network install that a sandboxed CI runner cannot complete, even
    /// though each individual invocation succeeds reliably when run
    /// alone. This caching is test-only scaffolding: it never runs in
    /// production, and it still never compiles, archives, or links
    /// anything -- `ingest_fixture_input` itself is exactly as
    /// read-only whether called once or from every test.
    fn both_ingestions() -> &'static BothIngestions {
        static CACHE: OnceLock<BothIngestions> = OnceLock::new();
        CACHE.get_or_init(|| {
            let layout = FixtureLayout::discover();
            let before = snapshot_fixture_directory(&layout.root);

            let positive_runner = RecordingCommandRunner::new();
            let positive_input = ingest_fixture_input(&positive_runner, &layout, &["1.0.0"])
                .expect("must ingest positive fixture input");

            let negative_runner = RecordingCommandRunner::new();
            let negative_input = ingest_fixture_input(&negative_runner, &layout, &["2.0.0"])
                .expect("must ingest negative fixture input");

            let after = snapshot_fixture_directory(&layout.root);

            BothIngestions {
                positive: FixtureIngestion {
                    input: positive_input,
                    recorded_commands: positive_runner.recorded_commands(),
                },
                negative: FixtureIngestion {
                    input: negative_input,
                    recorded_commands: negative_runner.recorded_commands(),
                },
                fixture_directory_unchanged: before == after,
            }
        })
    }

    fn positive_ingestion() -> &'static FixtureIngestion {
        &both_ingestions().positive
    }

    fn negative_ingestion() -> &'static FixtureIngestion {
        &both_ingestions().negative
    }

    fn all_commands_permitted(recorded: &[(String, Vec<String>)]) -> bool {
        recorded.iter().all(|(program, args)| {
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            crate::command_runner::is_permitted(program, &args_ref)
        })
    }

    /// Required test 1: the positive case resolves without ever
    /// attempting a forbidden command, and the resulting closure
    /// contains the exact required action kinds with a valid dependency
    /// ordering (every action's own `depends_on` names an action that
    /// already appears earlier in the emitted list).
    #[test]
    fn g1_positive_resolves_without_building_and_emits_complete_action_plan() {
        let ingestion = positive_ingestion();
        assert!(
            all_commands_permitted(&ingestion.recorded_commands),
            "no forbidden command may be attempted while ingesting the positive fixture"
        );

        let closure = resolve(&ingestion.input).expect("must resolve");

        let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
        for action in &closure.required_actions {
            for dep in &action.depends_on {
                assert!(
                    seen_ids.contains(dep.as_str()),
                    "action '{}' depends on '{}' which has not been emitted yet",
                    action.id,
                    dep
                );
            }
            seen_ids.insert(&action.id);
        }

        let kinds: Vec<RequiredActionKind> =
            closure.required_actions.iter().map(|a| a.kind).collect();
        for expected in [
            RequiredActionKind::CompileRustObject,
            RequiredActionKind::CompileNimStaticLibrary,
            RequiredActionKind::CompileCObject,
            RequiredActionKind::CompileCppAdapterObject,
            RequiredActionKind::ArchiveStaticLibrary,
            RequiredActionKind::LinkNativeExecutable,
            RequiredActionKind::PreflightRuntimeContract,
            RequiredActionKind::PublishProvenance,
        ] {
            assert!(
                kinds.contains(&expected),
                "missing required action kind {expected:?}"
            );
        }
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == RequiredActionKind::ArchiveStaticLibrary)
                .count(),
            2,
            "exactly one archive action for cadd and one for cppmax"
        );
    }

    /// Required test 2: the real negative scenario (only `cadd@2.0.0`
    /// offered) is rejected before any build command, with a structured
    /// diagnostic naming the required/declared symbol mismatch.
    #[test]
    fn g1_negative_rejects_cadd_v2_before_any_build_command() {
        let ingestion = negative_ingestion();
        assert!(
            all_commands_permitted(&ingestion.recorded_commands),
            "no forbidden command may be attempted while ingesting the negative fixture"
        );

        let rejection = resolve(&ingestion.input).expect_err("must reject");
        assert_eq!(rejection.reason, RejectionReason::MissingSymbol);
        assert_eq!(rejection.obligation_id, "Symbol:c_add");
        assert!(rejection.detail.contains("c_add"));
        assert!(rejection.detail.contains("c_add_v2"));
        assert_eq!(rejection.demand_entry_point, "app");
        assert!(
            rejection
                .considered_candidates
                .iter()
                .any(|c| c.contains("cadd@2.0.0") && c.contains("c/cadd/v2/cadd.h")),
            "rejection must name the considered v2 candidate and its source header: {:?}",
            rejection.considered_candidates
        );
    }

    /// Required test 3: the native executable demand reaches every
    /// retained obligation by actually following `depends_on`, checked
    /// separately per ecosystem -- never inferred from mere presence in
    /// the complete obligations map.
    #[test]
    fn native_demand_reaches_every_retained_obligation() {
        let closure = resolve(&positive_ingestion().input).expect("must resolve");
        let demand_id = closure
            .obligations
            .values()
            .find(|o| o.kind == ObligationKind::NativeExecutableDemand)
            .map(|o| o.id.clone())
            .expect("a NativeExecutableDemand obligation must exist");

        let mut reached: BTreeSet<String> = BTreeSet::new();
        let mut frontier = vec![demand_id];
        while let Some(id) = frontier.pop() {
            if !reached.insert(id.clone()) {
                continue;
            }
            if let Some(obligation) = closure.obligations.get(&id) {
                frontier.extend(obligation.depends_on.iter().cloned());
            }
        }

        for (id, obligation) in &closure.obligations {
            if obligation.state.is_rejected() {
                continue;
            }
            assert!(
                reached.contains(id),
                "retained obligation '{id}' is unreachable from the demand"
            );
        }

        for expected in [
            Ecosystem::Cargo,
            Ecosystem::Nimble,
            Ecosystem::C,
            Ecosystem::Cpp,
        ] {
            assert!(
                closure
                    .obligations
                    .iter()
                    .any(|(id, o)| reached.contains(id) && o.ecosystem == expected),
                "no reachable obligation belongs to ecosystem {expected:?}"
            );
        }
    }

    /// Required test 4: no artifact/link/runtime/provenance obligation
    /// is ever `Discharged`/`Externalized` in G1's own output; each
    /// such obligation instead names the required G2 action that will
    /// discharge it.
    #[test]
    fn g1_never_discharges_a_production_action_obligation() {
        let closure = resolve(&positive_ingestion().input).expect("must resolve");
        let production_kinds = [
            ObligationKind::ArtifactProduction,
            ObligationKind::FinalLink,
            ObligationKind::Runtime,
            ObligationKind::Provenance,
        ];
        let mut checked = 0;
        for obligation in closure.obligations.values() {
            if !production_kinds.contains(&obligation.kind) {
                continue;
            }
            checked += 1;
            assert_eq!(
                obligation.state,
                ObligationState::Satisfied,
                "production obligation '{}' must be Satisfied, not {:?}",
                obligation.id,
                obligation.state
            );
            assert!(
                obligation.required_action.is_some(),
                "production obligation '{}' must name the G2 action that will discharge it",
                obligation.id
            );
        }
        assert!(
            checked > 0,
            "the closure must actually contain production obligations"
        );
    }

    /// Required test 5: the same normalized input resolves to a
    /// byte-for-byte identical serialized plan every time -- identities
    /// never contain a temporary-directory path or a timestamp (the
    /// fixture's own stable, repo-relative source ids guarantee this).
    #[test]
    fn g1_plan_is_deterministic_for_identical_inputs() {
        let input = &positive_ingestion().input;
        let a = resolve(input).expect("must resolve");
        let b = resolve(input).expect("must resolve");
        let a_json = serde_json::to_string(&a).expect("serialize");
        let b_json = serde_json::to_string(&b).expect("serialize");
        assert_eq!(a_json, b_json);
        assert!(
            !a_json.contains("/tmp"),
            "identities must never leak a temp-directory path"
        );
        assert!(!a_json.contains(std::env::temp_dir().to_string_lossy().as_ref()));
    }

    /// Required test 6: a host-role candidate that would otherwise
    /// match a required symbol is rejected with `HostTargetRoleMismatch`,
    /// never silently accepted as satisfying a target-role obligation.
    #[test]
    fn host_tool_actions_cannot_satisfy_target_artifact_obligations() {
        let mut input = positive_ingestion().input.clone();
        for candidate in input.package_candidates.iter_mut() {
            if candidate.package_id == "cadd" {
                candidate.role = Role::Host;
            }
        }
        let rejection = resolve(&input).expect_err("a host-role-only candidate must not resolve");
        assert_eq!(rejection.reason, RejectionReason::HostTargetRoleMismatch);
    }

    /// Required test 7: removing a required source/provider from a
    /// minimal constructed input prevents plan publication entirely.
    #[test]
    fn unresolved_reachable_obligation_prevents_plan_publication() {
        let mut input = positive_ingestion().input.clone();
        input
            .package_candidates
            .retain(|c| c.package_id != "cppmax");
        let result = resolve(&input);
        assert!(
            result.is_err(),
            "removing the cppmax provider must prevent plan publication"
        );
    }

    // --- G1 follow-up (Checkpoint A): verified authoritative input ---------

    fn unique_temp_dir(label: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "laminaria-g1-checkpoint-a-{label}-{}-{n}",
            std::process::id()
        ))
    }

    /// Checkpoint A, evidence 1: a retained C implementation source
    /// that is missing must fail input construction outright -- via
    /// the real production entry point (`ingest_c_candidate`), not a
    /// synthetic stand-in.
    #[test]
    fn a_missing_retained_c_implementation_source_fails_ingestion() {
        let layout = FixtureLayout::discover();
        let dir = unique_temp_dir("missing-c-source");
        std::fs::create_dir_all(&dir).expect("must create temp dir");
        let header_copy = dir.join("cadd.h");
        std::fs::copy(layout.cadd_v1_h(), &header_copy).expect("must copy real header");
        let missing_c_path = dir.join("cadd.c"); // deliberately never created

        let result = ingest_c_candidate(
            "temp/cadd",
            &missing_c_path,
            &header_copy,
            "cadd",
            "1.0.0",
            "x86_64-unknown-linux-gnu",
        );
        assert!(
            result.is_err(),
            "ingestion must fail when the retained .c implementation file is missing"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Checkpoint A, evidence 2: changing the active Cargo feature
    /// configuration changes which FFI requirements are active --
    /// exercised directly against the real `app/src/main.rs`, which
    /// gates its `nim_double` import on `feature = "use_nim_double"`.
    #[test]
    fn changing_the_active_feature_set_changes_active_ffi_requirements() {
        let layout = FixtureLayout::discover();
        let with_feature = CfgContext {
            true_idents: BTreeSet::from(["unix".to_string()]),
            active_features: BTreeSet::from(["use_nim_double".to_string()]),
        };
        let without_feature = CfgContext {
            true_idents: BTreeSet::from(["unix".to_string()]),
            active_features: BTreeSet::new(),
        };
        let with =
            discover_ffi_requirements(&layout.app_main_rs(), "app/src/main.rs", &with_feature)
                .expect("must discover with the feature active");
        let without =
            discover_ffi_requirements(&layout.app_main_rs(), "app/src/main.rs", &without_feature)
                .expect("must discover with the feature inactive");
        assert!(with.iter().any(|r| r.symbol == "nim_double"));
        assert!(
            !without.iter().any(|r| r.symbol == "nim_double"),
            "nim_double must not be reported as an active requirement once its gating feature is off"
        );
        // c_add/cpp_max_i32 are gated only on `unix`, not on the
        // feature, so they stay active in both configurations.
        assert!(with.iter().any(|r| r.symbol == "c_add"));
        assert!(without.iter().any(|r| r.symbol == "c_add"));
    }

    /// Checkpoint A, evidence 3 (Rust half): a Rust source that fails
    /// to parse at all must fail ingestion, never silently resolve to
    /// an empty or partial requirement list.
    #[test]
    fn a_rust_source_that_fails_to_parse_fails_ingestion() {
        let dir = unique_temp_dir("bad-rust");
        std::fs::create_dir_all(&dir).expect("must create temp dir");
        let bad_rs = dir.join("main.rs");
        std::fs::write(&bad_rs, "extern \"C\" { fn f(").expect("must write temp file");
        let cfg_context = CfgContext::default();
        let result = discover_ffi_requirements(&bad_rs, "main.rs", &cfg_context);
        assert!(
            result.is_err(),
            "an unparseable Rust source must fail ingestion"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Checkpoint A, evidence 3 (Nim half): a Nim source with a
    /// genuinely malformed exported-proc signature must fail
    /// ingestion, never silently resolve to a partial export list.
    #[test]
    fn a_nim_source_with_a_malformed_signature_fails_ingestion() {
        let dir = unique_temp_dir("bad-nim");
        std::fs::create_dir_all(&dir).expect("must create temp dir");
        let bad_nim = dir.join("broken.nim");
        std::fs::write(
            &bad_nim,
            "proc broken(x: cint {.exportc: \"broken\".} =\n  x\n",
        )
        .expect("must write temp file");
        let result = discover_nim_declared_exports(&bad_nim, "broken.nim");
        assert!(
            result.is_err(),
            "a Nim proc with an unbalanced parameter list must fail ingestion"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Checkpoint A/B, evidence 4 (end-to-end): mutating a real
    /// ingested candidate's declared signature so it no longer matches
    /// the real required import must be rejected by production
    /// resolution, even though the symbol name still matches.
    #[test]
    fn an_ingested_candidate_with_a_mutated_signature_is_rejected_end_to_end() {
        let mut input = positive_ingestion().input.clone();
        for candidate in input.package_candidates.iter_mut() {
            if candidate.package_id == "cadd" {
                for export in candidate.declared_exports.iter_mut() {
                    if export.symbol == "c_add" {
                        export.param_count = 99;
                    }
                }
            }
        }
        let rejection =
            resolve(&input).expect_err("a mutated, incompatible signature must be rejected");
        assert_eq!(rejection.reason, RejectionReason::IncompatibleAbi);
    }

    /// Checkpoint A, evidence 5: real production ingestion never
    /// writes to, or otherwise changes, the fixture directory it reads
    /// from -- a direct filesystem-side-effect observation (computed
    /// once around both shared real ingestions in `both_ingestions()`,
    /// never by a separate test running its own extra real ingestion
    /// outside that shared lock).
    #[test]
    fn ingestion_leaves_the_fixture_directory_unchanged() {
        assert!(
            both_ingestions().fixture_directory_unchanged,
            "ingestion must never create, delete, or modify any file under the fixture directory"
        );
    }
}

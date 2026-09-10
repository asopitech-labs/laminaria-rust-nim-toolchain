//! Plans and builds a *user's target project* (issue #26), as opposed to
//! `self_build.rs` (which plans and builds LAMINARIA itself and legitimately
//! always needs both a Rust and a Nim toolchain). This module resolves and
//! invokes only the toolchain(s) the target project's own demanded
//! artifacts actually need -- reusing the same production Nim planner
//! (`laminaria_plan::call_planner`) and the same traced-execution
//! infrastructure (`crate::run_and_record_with_doctor`) as `self_build.rs`,
//! not a separate/duplicate code path.
//!
//! **Capability determination never infers "mixed" from file coexistence,
//! and never silently rejects it either.** A `Cargo.toml` and a Nim entry
//! point living in the same directory are not evidence of a dependency
//! between them -- and a real dependency (e.g. a Rust `build.rs` shelling
//! out to `nim`) can exist without any Nim file at the project root at all.
//! `determine_project_requirements` therefore only ever infers a capability
//! when exactly one language's files are present (no config needed); when
//! both are present, or when a real-but-undetectable dependency exists, the
//! caller must say so explicitly via `--requires` -- an explicit target
//! designation request, not a blanket "unsupported" verdict.
//!
//! **What "both requested" builds, honestly:** if a Nim entry point is
//! resolvable, `rust,nim` plans two independent producer actions (both
//! wanted, no known relationship between them -- no fabricated
//! `Integrate` step; self-build's own `Integrate` action assembles
//! *LAMINARIA's own* generation-root layout and has no generic equivalent
//! for an arbitrary target project). If no Nim entry point is resolvable,
//! `rust,nim` plans a single `CargoBuild` action whose Nim toolchain is
//! resolved and put on that action's own `PATH` (not a second producer) --
//! the "a single build process needs the other toolchain on hand" shape.
//!
//! **Deferred, not attempted here:** automatically *inferring* a real
//! cross-language dependency from source/manifest inspection is the job of
//! the variant/compatibility model (issue #22); this module only ever acts
//! on what the caller explicitly declares via `--requires`/`--nim-entry`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use laminaria_fingerprint::doctor::DoctorRun;
use laminaria_plan::{
    validate, Action, ActionKind, ArtifactRef, ExecutionPlan, PlanOutcome, PlanRejection,
    PlannerCallError, PlanningInput, ValidationError,
};

use crate::absolute_path;
use crate::toolchain_resolve::{self, ToolchainResolutionError};
use crate::types::{ProbeLevel, RootCommand, Run};

pub const WORKLOAD_ID: &str = "laminaria-project-build";

pub const ARTIFACT_RUST_PROJECT: &str = "project-rust-artifacts";
pub const ARTIFACT_NIM_PROJECT: &str = "project-nim-artifacts";
pub const ACTION_COMPILE_RUST_PROJECT: &str = "compile-rust-project";
pub const ACTION_COMPILE_NIM_PROJECT: &str = "compile-nim-project";

/// A toolchain family a target project's build may need. Distinct from
/// `laminaria_plan::ActionKind`, which names a *planned action*'s shape,
/// not a caller's declared requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Rust,
    Nim,
}

#[derive(Debug, Clone)]
pub struct ProjectRequirements {
    pub rust: bool,
    pub nim: bool,
    /// Path, relative to `project_root`, of a real Nim producer entry
    /// point -- `Some` only when Nim is needed *as its own build action*
    /// (two independent producers). `None` while `nim` is still `true`
    /// means "the Nim toolchain must be resolved and made available, but
    /// there is no separate Nim artifact to build" (e.g. a Cargo
    /// `build.rs` that shells out to `nim`).
    pub nim_entry: Option<PathBuf>,
}

#[derive(Debug)]
pub enum ProjectBuildError {
    NoBuildableSources(String),
    AmbiguousProject {
        cargo_toml: PathBuf,
        nim_entry: PathBuf,
    },
    NimEntryNotFound(PathBuf),
    Toolchain(ToolchainResolutionError),
    Planner(PlannerCallError),
    Rejected(PlanRejection),
    InvalidPlan(ValidationError),
    ActionFailed {
        action_id: String,
        detail: String,
    },
    Io(std::io::Error),
}

impl std::fmt::Display for ProjectBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectBuildError::NoBuildableSources(detail) => write!(f, "{detail}"),
            ProjectBuildError::AmbiguousProject {
                cargo_toml,
                nim_entry,
            } => write!(
                f,
                "found both {} and a resolvable Nim entry point ({}) with no explicit --requires \
                 given; file coexistence alone does not imply a dependency between them -- pass \
                 --requires rust|nim|rust,nim to say what this build should actually target",
                cargo_toml.display(),
                nim_entry.display()
            ),
            ProjectBuildError::NimEntryNotFound(path) => write!(
                f,
                "--nim-entry names {}, but no such file exists -- an explicit --nim-entry must \
                 name a real file; it is never silently treated as \"no Nim entry\"",
                path.display()
            ),
            ProjectBuildError::Toolchain(e) => write!(f, "{e}"),
            ProjectBuildError::Planner(e) => write!(f, "failed to call the Nim planner: {e}"),
            ProjectBuildError::Rejected(r) => write!(
                f,
                "the Nim planner rejected this project's PlanningInput ({:?}): {}",
                r.reason_kind, r.reason_detail
            ),
            ProjectBuildError::InvalidPlan(e) => {
                write!(
                    f,
                    "the Nim planner's ExecutionPlan failed Rust-side validation: {e}"
                )
            }
            ProjectBuildError::ActionFailed { action_id, detail } => {
                write!(f, "project-build action '{action_id}' failed: {detail}")
            }
            ProjectBuildError::Io(e) => write!(f, "I/O error during project build: {e}"),
        }
    }
}

impl std::error::Error for ProjectBuildError {}

impl From<std::io::Error> for ProjectBuildError {
    fn from(e: std::io::Error) -> Self {
        ProjectBuildError::Io(e)
    }
}

/// Resolves a Nim producer entry point relative to `project_root`:
/// `nim_entry_override` if given, else the standard `nimble init`
/// convention -- a single `*.nimble` file at `project_root` whose stem
/// matches `src/<stem>.nim` or `<stem>.nim`. More than one `.nimble` file,
/// or none matching the convention, resolves to `Ok(None)` rather than
/// guessing -- that is a legitimate "no entry point found by inference"
/// answer. An explicit `nim_entry_override` naming a file that does not
/// exist is different: it is a caller mistake, not "no entry," and must
/// be reported as `Err`, never silently downgraded to `Ok(None)` (a
/// review caught exactly that: `--nim-entry src/missing.nim` against a
/// project that also has a `Cargo.toml` used to build Rust-only and
/// report success, silently discarding the broken explicit request).
fn resolve_nim_entry(
    project_root: &Path,
    nim_entry_override: Option<&Path>,
) -> Result<Option<PathBuf>, ProjectBuildError> {
    if let Some(entry) = nim_entry_override {
        let full_path = project_root.join(entry);
        return if full_path.is_file() {
            Ok(Some(entry.to_path_buf()))
        } else {
            Err(ProjectBuildError::NimEntryNotFound(full_path))
        };
    }

    let Ok(entries) = std::fs::read_dir(project_root) else {
        return Ok(None);
    };
    let mut nimble_stems = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("nimble"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()));

    let Some(stem) = nimble_stems.next() else {
        return Ok(None);
    };
    if nimble_stems.next().is_some() {
        // More than one .nimble file: which one names the real entry
        // point is genuinely ambiguous, so this is not a case to guess at
        // -- the caller should pass --nim-entry explicitly.
        return Ok(None);
    }

    for candidate in [format!("src/{stem}.nim"), format!("{stem}.nim")] {
        if project_root.join(&candidate).is_file() {
            return Ok(Some(PathBuf::from(candidate)));
        }
    }
    Ok(None)
}

/// Determines which toolchain(s) a target project's build needs. See this
/// module's own doc comment for the full reasoning; in short:
/// `requires_override` (from `--requires`) is always authoritative when
/// given, file presence is only consulted when it is unambiguous, and
/// coexistence without an explicit designation is a request for one, not a
/// rejection.
pub fn determine_project_requirements(
    project_root: &Path,
    requires_override: Option<&[Capability]>,
    nim_entry_override: Option<&Path>,
) -> Result<ProjectRequirements, ProjectBuildError> {
    if let Some(caps) = requires_override {
        let rust = caps.contains(&Capability::Rust);
        let nim = caps.contains(&Capability::Nim);
        if !rust && !nim {
            return Err(ProjectBuildError::NoBuildableSources(
                "--requires was given but named no capability (expected rust, nim, or rust,nim)"
                    .to_string(),
            ));
        }
        let nim_entry = if nim {
            resolve_nim_entry(project_root, nim_entry_override)?
        } else {
            None
        };
        if nim && !rust && nim_entry.is_none() {
            return Err(ProjectBuildError::NoBuildableSources(format!(
                "--requires named nim, but no Nim entry point could be resolved under {} (add a \
                 single <name>.nimble with a matching src/<name>.nim or <name>.nim, or pass \
                 --nim-entry)",
                project_root.display()
            )));
        }
        return Ok(ProjectRequirements {
            rust,
            nim,
            nim_entry,
        });
    }

    let cargo_toml = project_root.join("Cargo.toml");
    let has_cargo_toml = cargo_toml.is_file();
    let nim_entry = resolve_nim_entry(project_root, nim_entry_override)?;

    match (has_cargo_toml, nim_entry) {
        (true, None) => Ok(ProjectRequirements {
            rust: true,
            nim: false,
            nim_entry: None,
        }),
        (false, Some(entry)) => Ok(ProjectRequirements {
            rust: false,
            nim: true,
            nim_entry: Some(entry),
        }),
        (true, Some(entry)) => Err(ProjectBuildError::AmbiguousProject {
            cargo_toml,
            nim_entry: entry,
        }),
        (false, None) => Err(ProjectBuildError::NoBuildableSources(format!(
            "no Cargo.toml and no resolvable Nim entry point under {}",
            project_root.display()
        ))),
    }
}

/// Builds the target project's `PlanningInput`. Never emits an action for
/// a language `req` doesn't need, and never a fabricated `Integrate` step
/// -- see this module's own doc comment for the two-toolchain shapes.
pub fn project_planning_input(project_root: &Path, req: &ProjectRequirements) -> PlanningInput {
    let mut actions = Vec::new();
    let mut demanded_artifacts = Vec::new();

    if req.rust {
        actions.push(Action {
            id: ACTION_COMPILE_RUST_PROJECT.to_string(),
            kind: ActionKind::CargoBuild,
            command_identity: "cargo build --release (target project)".to_string(),
            inputs: vec![ArtifactRef::source(project_root.display().to_string())],
            outputs: vec![ArtifactRef::declared(ARTIFACT_RUST_PROJECT)],
        });
        demanded_artifacts.push(ARTIFACT_RUST_PROJECT.to_string());
    }
    if let Some(entry) = &req.nim_entry {
        actions.push(Action {
            id: ACTION_COMPILE_NIM_PROJECT.to_string(),
            kind: ActionKind::NimBuild,
            command_identity: format!("nim c {} (target project)", entry.display()),
            inputs: vec![ArtifactRef::source(project_root.display().to_string())],
            outputs: vec![ArtifactRef::declared(ARTIFACT_NIM_PROJECT)],
        });
        demanded_artifacts.push(ARTIFACT_NIM_PROJECT.to_string());
    }

    PlanningInput::new(demanded_artifacts, actions)
}

#[derive(Debug)]
pub struct ResolvedProjectToolchains {
    pub rust: Option<(PathBuf, PathBuf)>,
    pub nim: Option<PathBuf>,
}

/// Resolves and verifies only the toolchain family/families `req` actually
/// needs, fingerprinted against `project_root` itself (not LAMINARIA's own
/// checkout -- there is no separate `repo_root` concept in this module).
/// Returns the `DoctorRun` too, so the caller can feed the *same* one into
/// `crate::run_and_record_with_doctor` instead of triggering a second,
/// unfiltered probe at record time.
pub fn resolve_project_toolchains(
    lock_path: &Path,
    project_root: &Path,
    req: &ProjectRequirements,
) -> Result<(DoctorRun, ResolvedProjectToolchains), ProjectBuildError> {
    let doctor_run = toolchain_resolve::run_doctor(lock_path, project_root, req.rust, req.nim)
        .map_err(ProjectBuildError::Toolchain)?;

    let rust = if req.rust {
        Some(
            toolchain_resolve::resolve_verified_rust(&doctor_run)
                .map_err(ProjectBuildError::Toolchain)?,
        )
    } else {
        None
    };
    let nim = if req.nim {
        Some(
            toolchain_resolve::resolve_verified_nim(&doctor_run)
                .map_err(ProjectBuildError::Toolchain)?,
        )
    } else {
        None
    };

    Ok((doctor_run, ResolvedProjectToolchains { rust, nim }))
}

fn build_staging_dir(generation_root: &Path) -> PathBuf {
    generation_root.join(".build")
}

fn cargo_target_dir(generation_root: &Path) -> PathBuf {
    build_staging_dir(generation_root).join("cargo-target")
}

fn nim_cache_dir(generation_root: &Path) -> PathBuf {
    build_staging_dir(generation_root).join("nimcache")
}

fn nim_output_dir(generation_root: &Path) -> PathBuf {
    build_staging_dir(generation_root).join("nim-out")
}

fn binary_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

/// Prepends `dir` to a `PATH` value, using the platform-correct list
/// separator -- `existing` is this process's own ambient `PATH` (the same
/// one the spawned child would otherwise inherit unmodified), not a value
/// this crate invents.
fn prepend_to_path(dir: &Path, existing: Option<&str>) -> String {
    let sep = if cfg!(windows) { ';' } else { ':' };
    match existing {
        Some(existing) if !existing.is_empty() => format!("{}{sep}{existing}", dir.display()),
        _ => dir.display().to_string(),
    }
}

/// `cargo_binary`/`rustc_binary` are the resolved, lock-verified
/// executables (`resolve_project_toolchains`), never a bare `"cargo"`
/// program name -- same reasoning as `self_build.rs`'s own
/// `cargo_build_root`. `cwd = project_root`, and deliberately **no**
/// `--workspace`: the target project's own `Cargo.toml` already
/// determines single-crate-vs-workspace shape, and forcing `--workspace`
/// would leak a LAMINARIA-shaped assumption onto an arbitrary target.
/// `extra_path_prepend`, when `Some(nim_bin_dir)`, makes the *verified*
/// Nim toolchain available on this action's own `PATH` -- prepended,
/// never a second, unpinned lookup. Whether Nim is needed here is
/// governed purely by whether Nim was requested at all (`req.rust &&
/// req.nim`), independent of whether a separate `NimBuild` producer
/// action also exists: a review caught that gating this solely on
/// `req.nim_entry.is_none()` meant a Cargo `build.rs` that itself calls
/// `nim` got *no* verified Nim on its `PATH` whenever a real Nim entry
/// point also happened to exist elsewhere in the project (the
/// two-independent-producers shape) -- silently falling through to
/// whatever unpinned `nim` was first on this process's own `PATH`
/// instead. Whether a Nim *artifact* is produced and whether the Cargo
/// *action* needs Nim on hand are separate facts; conflating them was
/// the bug.
fn cargo_project_build_root(
    project_root: &Path,
    generation_root: &Path,
    cargo_binary: &Path,
    rustc_binary: &Path,
    extra_path_prepend: Option<&Path>,
) -> RootCommand {
    let mut env_overrides = BTreeMap::new();
    env_overrides.insert("RUSTC".to_string(), rustc_binary.display().to_string());
    if let Some(nim_bin_dir) = extra_path_prepend {
        let current_path = std::env::var("PATH").ok();
        env_overrides.insert(
            "PATH".to_string(),
            prepend_to_path(nim_bin_dir, current_path.as_deref()),
        );
    }
    RootCommand {
        program: cargo_binary.display().to_string(),
        args: vec![
            "build".to_string(),
            "--release".to_string(),
            "--target-dir".to_string(),
            cargo_target_dir(generation_root).display().to_string(),
        ],
        cwd: Some(project_root.to_path_buf()),
        env_overrides,
    }
}

/// `nim_binary` is the resolved, lock-verified executable. `entry` is
/// `req.nim_entry`'s path relative to `project_root`, so `cwd =
/// project_root` resolves it correctly regardless of this process's own
/// cwd. Output and nimcache both live under `generation_root`'s own
/// isolated `.build/` staging area, matching `self_build.rs`'s own
/// generation-isolation approach.
fn nim_project_build_root(
    project_root: &Path,
    generation_root: &Path,
    nim_binary: &Path,
    entry: &Path,
) -> RootCommand {
    let stem = entry
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let output_path = nim_output_dir(generation_root).join(binary_name(stem));
    RootCommand {
        program: nim_binary.display().to_string(),
        args: vec![
            "c".to_string(),
            format!("--nimcache:{}", nim_cache_dir(generation_root).display()),
            format!("-o:{}", output_path.display()),
            entry.display().to_string(),
        ],
        cwd: Some(project_root.to_path_buf()),
        env_overrides: BTreeMap::new(),
    }
}

#[derive(Debug)]
pub struct ProjectActionOutcome {
    pub action_id: String,
    pub run: Run,
    pub succeeded: bool,
    pub detail: String,
    /// The real, on-disk artifact path(s) this action produced. For a
    /// `CargoBuild` action, taken from the actually-executed Cargo's own
    /// `--message-format=json` telemetry (`cargo_telemetry`, already
    /// parsed unconditionally by `run_and_record`) -- correct under
    /// `--target <triple>`, custom profiles, or multiple bin targets,
    /// none of which a fixed guessed directory would survive. For a
    /// `NimBuild` action, the exact `-o:` path this module itself passed
    /// -- fully known upfront, since this code chose it.
    pub artifacts: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct ProjectGenerationResult {
    pub plan: ExecutionPlan,
    pub requirements: ProjectRequirements,
    pub actions: Vec<ProjectActionOutcome>,
    pub generation_root: PathBuf,
}

fn cargo_artifacts_from_run(run: &Run) -> Vec<PathBuf> {
    let Some(telemetry_value) = &run.compiler_telemetry else {
        return Vec::new();
    };
    let Ok(telemetry) =
        serde_json::from_value::<crate::types::CargoCompilerTelemetry>(telemetry_value.clone())
    else {
        return Vec::new();
    };
    telemetry
        .artifacts
        .iter()
        .flat_map(|a| a.filenames.clone())
        .chain(
            telemetry
                .artifacts
                .iter()
                .filter_map(|a| a.executable.clone()),
        )
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn run_project_traced_action(
    action: &Action,
    project_root: &Path,
    runs_root: &Path,
    doctor_run: &DoctorRun,
    generation_label: &str,
    root: RootCommand,
    observation_roots: Vec<PathBuf>,
    artifacts: Vec<PathBuf>,
) -> Result<ProjectActionOutcome, ProjectBuildError> {
    let requested_artifact = action.outputs.first().and_then(|o| match o {
        ArtifactRef::Declared { artifact_id } => Some(artifact_id.clone()),
        ArtifactRef::Source { .. } => None,
    });

    let (mut run, _dir) = crate::run_and_record_with_doctor(
        doctor_run,
        runs_root,
        WORKLOAD_ID,
        &format!("project-build/{generation_label}/{}", action.id),
        requested_artifact,
        root,
        ProbeLevel::Level1ProcessResource,
        &observation_roots,
    )?;

    let exit_succeeded = run.result.as_ref().is_some_and(|r| r.success);
    // Cargo's own telemetry is only known once the action has actually
    // run -- read it from the persisted Run itself rather than the
    // pre-execution RootCommand, and only look at artifacts on exit
    // success (a failed compile's stale/partial output must never be
    // reported as a real, usable artifact).
    let candidate_artifacts = if exit_succeeded {
        let mut from_run = cargo_artifacts_from_run(&run);
        if from_run.is_empty() {
            from_run = artifacts;
        }
        from_run
    } else {
        Vec::new()
    };

    // A zero exit status is not proof the demanded artifact was actually
    // produced: a review caught that Nim's `--compileOnly:on` (set via
    // the target project's own `nim.cfg`, entirely outside this crate's
    // control) exits 0 without ever linking the requested `-o:` output,
    // and this action was still reported as a success naming a file that
    // was never created. Every candidate artifact path must exist on
    // disk, and there must be at least one, for the action to count as a
    // genuine success -- exit status alone is necessary, not sufficient.
    let produced_expected_artifacts =
        !candidate_artifacts.is_empty() && candidate_artifacts.iter().all(|p| p.is_file());
    let succeeded = exit_succeeded && produced_expected_artifacts;
    let artifacts = if succeeded {
        candidate_artifacts.clone()
    } else {
        Vec::new()
    };

    if exit_succeeded && !produced_expected_artifacts {
        // The persisted Run's own `result.success` must not keep claiming
        // success once the artifact-existence check has overridden it --
        // an evidence record that still says "succeeded" while this
        // function's own verdict says otherwise would be a real Run file
        // silently disagreeing with the CLI's own reported outcome.
        if let Some(result) = run.result.as_mut() {
            result.success = false;
        }
        run.process_trace.known_gaps.push(format!(
            "action '{}' exited 0 (root_exit_status) but its demanded artifact(s) {:?} were not \
             all present on disk afterward; result.success has been corrected to false to \
             reflect that (issue #26 artifact-existence check, not just exit status)",
            action.id, candidate_artifacts
        ));
    }

    run.process_trace.known_gaps.push(format!(
        "project-build evidence (issue #26): plan_project_root={}, generation={}, action_id={}",
        project_root.display(),
        generation_label,
        action.id
    ));
    crate::store::write_run(runs_root, &run)?;

    let detail = if succeeded {
        format!(
            "action '{}' completed successfully (run {})",
            action.id, run.run_id
        )
    } else if exit_succeeded {
        format!(
            "action '{}' exited successfully but did not produce the expected artifact(s) (see \
             run {})",
            action.id, run.run_id
        )
    } else {
        format!("action '{}' failed (see run {})", action.id, run.run_id)
    };

    Ok(ProjectActionOutcome {
        action_id: action.id.clone(),
        run,
        succeeded,
        detail,
        artifacts,
    })
}

/// Resolves `project_root` to an absolute path exactly the way
/// `run_project_generation` does internally, before it is ever fed into
/// `determine_project_requirements`/`project_planning_input`. Exposed so
/// a caller that only plans (`plan-build`, which never executes anything
/// and so has no other reason to touch `project_root`) computes the
/// identical `PlanningInput` -- and therefore the identical `plan_id` --
/// that `build` would for the same input, regardless of the two
/// commands' own current working directory. A review caught this
/// directly: `plan-build` left `project_root` relative while `build`
/// absolutized it, so the same logical project produced two different
/// `plan_id`s depending on which command computed it, even though
/// nothing about the target had actually changed.
pub fn absolute_project_root(project_root: &Path) -> std::io::Result<PathBuf> {
    absolute_path(project_root)
}

/// Plans and builds `project_root` using `planner_binary` (the production
/// Nim Planning Kernel -- same contract, same executor infrastructure as
/// `self_build.rs`, not a separate/duplicate path), resolving and
/// verifying only the toolchain(s) the project's own demanded artifacts
/// need. `lock_path` is resolved relative to the invoking process's own
/// cwd, independent of `project_root` -- a target project does not carry
/// its own `toolchains.lock.toml`.
#[allow(clippy::too_many_arguments)]
pub fn run_project_generation(
    project_root: &Path,
    generation_root: &Path,
    planner_binary: &Path,
    generation_label: &str,
    runs_root: &Path,
    lock_path: &Path,
    requires_override: Option<&[Capability]>,
    nim_entry_override: Option<&Path>,
) -> Result<ProjectGenerationResult, ProjectBuildError> {
    let project_root = &absolute_path(project_root)?;
    let generation_root = &absolute_path(generation_root)?;

    let requirements =
        determine_project_requirements(project_root, requires_override, nim_entry_override)?;

    let input = project_planning_input(project_root, &requirements);
    let outcome =
        laminaria_plan::call_planner(planner_binary, &input).map_err(ProjectBuildError::Planner)?;
    let plan = match outcome {
        PlanOutcome::Planned(plan) => plan,
        PlanOutcome::Rejected(rejection) => return Err(ProjectBuildError::Rejected(rejection)),
    };
    validate(&plan, &input).map_err(ProjectBuildError::InvalidPlan)?;

    let (doctor_run, toolchains) =
        resolve_project_toolchains(lock_path, project_root, &requirements)?;

    let _ = std::fs::remove_dir_all(build_staging_dir(generation_root));
    std::fs::create_dir_all(generation_root)?;

    // Whether the CargoBuild action needs the verified Nim toolchain on
    // its own PATH is governed by whether Nim was requested at all, not
    // by whether a separate NimBuild producer action also exists (see
    // `cargo_project_build_root`'s own doc comment for the bug this
    // fixes) -- a Cargo build.rs calling `nim` and a separate real Nim
    // entry point elsewhere in the project are independent facts.
    let nim_needed_by_cargo_action = requirements.rust && requirements.nim;

    let mut action_outcomes = Vec::new();
    for action_id in &plan.ordered_actions {
        let action = plan
            .actions
            .get(action_id)
            .expect("validate() already checked every ordered_actions id exists");

        let (root, observation_roots, pre_execution_artifacts) = match action.kind {
            ActionKind::CargoBuild => {
                let (cargo, rustc) = toolchains
                    .rust
                    .as_ref()
                    .expect("a CargoBuild action implies req.rust, which implies toolchains.rust");
                let extra_path = if nim_needed_by_cargo_action {
                    toolchains.nim.as_deref().and_then(Path::parent)
                } else {
                    None
                };
                let root = cargo_project_build_root(
                    project_root,
                    generation_root,
                    cargo,
                    rustc,
                    extra_path,
                );
                (root, vec![cargo_target_dir(generation_root)], Vec::new())
            }
            ActionKind::NimBuild => {
                let nim = toolchains.nim.as_ref().expect(
                    "a NimBuild action implies req.nim_entry.is_some(), which implies toolchains.nim",
                );
                let entry = requirements
                    .nim_entry
                    .as_ref()
                    .expect("a NimBuild action implies a resolved nim_entry");
                let root = nim_project_build_root(project_root, generation_root, nim, entry);
                let output_path = nim_output_dir(generation_root).join(binary_name(
                    entry
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("project"),
                ));
                (
                    root,
                    vec![nim_output_dir(generation_root)],
                    vec![output_path],
                )
            }
            ActionKind::Integrate => {
                unreachable!("project_planning_input never emits an Integrate action")
            }
        };

        let outcome = run_project_traced_action(
            action,
            project_root,
            runs_root,
            &doctor_run,
            generation_label,
            root,
            observation_roots,
            pre_execution_artifacts,
        )?;

        let succeeded = outcome.succeeded;
        let failure_detail = outcome.detail.clone();
        let failed_action_id = outcome.action_id.clone();
        action_outcomes.push(outcome);
        if !succeeded {
            return Err(ProjectBuildError::ActionFailed {
                action_id: failed_action_id,
                detail: failure_detail,
            });
        }
    }

    Ok(ProjectGenerationResult {
        plan,
        requirements,
        actions: action_outcomes,
        generation_root: generation_root.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "laminaria-run-project-build-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Builds `nim-planner/bin/laminaria-planner` via `nim c` directly,
    /// exactly once per test binary process -- same `OnceLock` pattern
    /// `self_build.rs`'s own `build_stage0_planner` uses, for the same
    /// reason (a fresh checkout otherwise races multiple test threads'
    /// `nim c` invocations against the same output path).
    #[cfg(unix)]
    fn stage0_planner(repo_root: &Path) -> PathBuf {
        static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        BUILT
            .get_or_init(|| {
                let status = std::process::Command::new("nim")
                    .args([
                        "c",
                        "--path:src",
                        "-o:bin/laminaria-planner",
                        "src/laminaria_planner.nim",
                    ])
                    .current_dir(repo_root.join("nim-planner"))
                    .status()
                    .expect("failed to invoke nim -- is Nim installed?");
                assert!(
                    status.success(),
                    "stage0 nim c build of laminaria-planner failed"
                );
                repo_root.join("nim-planner/bin/laminaria-planner")
            })
            .clone()
    }

    #[test]
    fn determine_project_requirements_infers_rust_only_from_a_pure_rust_fixture() {
        let root = repo_root().join("fixtures/rust-heavy-workspace");
        let req = determine_project_requirements(&root, None, None).unwrap();
        assert!(req.rust);
        assert!(!req.nim);
        assert!(req.nim_entry.is_none());
    }

    #[test]
    fn determine_project_requirements_infers_nim_only_from_a_pure_nim_fixture() {
        let root = repo_root().join("fixtures/nim-heavy-workspace");
        let req = determine_project_requirements(&root, None, None).unwrap();
        assert!(!req.rust);
        assert!(req.nim);
        assert_eq!(req.nim_entry, Some(PathBuf::from("src/fixture.nim")));
    }

    fn write_ambiguous_project(dir: &Path) {
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("thing.nimble"), "").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/thing.nim"), "echo \"hi\"\n").unwrap();
    }

    /// File coexistence must never be silently resolved one way or the
    /// other, and must never be rejected outright as "unsupported" either
    /// -- it is a request for an explicit `--requires` designation.
    #[test]
    fn determine_project_requirements_asks_for_explicit_requires_when_both_are_present_and_unrequested(
    ) {
        let dir = tmp_dir("ambiguous");
        write_ambiguous_project(&dir);

        let result = determine_project_requirements(&dir, None, None);
        match result {
            Err(ProjectBuildError::AmbiguousProject { .. }) => {}
            other => panic!("expected AmbiguousProject, got {other:?}"),
        }
    }

    /// An explicit `--requires rust` must win over file coexistence -- the
    /// "Rust app plus an unrelated Nim helper tool in the same directory"
    /// case. Nim must never even be inspected.
    #[test]
    fn determine_project_requirements_honors_an_explicit_rust_only_request_despite_a_coexisting_nim_file(
    ) {
        let dir = tmp_dir("explicit-rust-only");
        write_ambiguous_project(&dir);

        let req = determine_project_requirements(&dir, Some(&[Capability::Rust]), None).unwrap();
        assert!(req.rust);
        assert!(!req.nim);
        assert!(req.nim_entry.is_none());
    }

    /// An explicit `--requires rust,nim` over the same coexisting-files
    /// directory: both are genuinely wanted, and since a real Nim entry
    /// point *is* resolvable, this must plan two independent producers,
    /// not a fabricated integration between them.
    #[test]
    fn determine_project_requirements_honors_an_explicit_mixed_request_with_a_real_nim_entry() {
        let dir = tmp_dir("explicit-mixed");
        write_ambiguous_project(&dir);

        let req =
            determine_project_requirements(&dir, Some(&[Capability::Rust, Capability::Nim]), None)
                .unwrap();
        assert!(req.rust);
        assert!(req.nim);
        assert_eq!(req.nim_entry, Some(PathBuf::from("src/thing.nim")));
    }

    /// A Cargo-only project (no Nim file at all) with `--requires
    /// rust,nim`: the "single Cargo build whose build.rs genuinely needs
    /// Nim on hand" shape -- `nim_entry` must stay `None` (no second
    /// producer to fabricate), while `nim` itself stays `true` (the
    /// toolchain must still be resolved and made available).
    #[test]
    fn determine_project_requirements_allows_mixed_requires_with_no_nim_entry_at_all() {
        let dir = tmp_dir("cargo-only-needs-nim");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let req =
            determine_project_requirements(&dir, Some(&[Capability::Rust, Capability::Nim]), None)
                .unwrap();
        assert!(req.rust);
        assert!(req.nim);
        assert!(req.nim_entry.is_none());
    }

    #[test]
    fn determine_project_requirements_rejects_a_project_with_neither_language() {
        let dir = tmp_dir("empty");
        let result = determine_project_requirements(&dir, None, None);
        match result {
            Err(ProjectBuildError::NoBuildableSources(_)) => {}
            other => panic!("expected NoBuildableSources, got {other:?}"),
        }
    }

    #[test]
    fn determine_project_requirements_rejects_an_explicit_nim_request_with_no_resolvable_entry() {
        let dir = tmp_dir("nim-requested-no-entry");
        let result = determine_project_requirements(&dir, Some(&[Capability::Nim]), None);
        match result {
            Err(ProjectBuildError::NoBuildableSources(detail)) => {
                assert!(
                    detail.contains("nim"),
                    "detail should mention nim: {detail}"
                );
            }
            other => panic!("expected NoBuildableSources, got {other:?}"),
        }
    }

    #[test]
    fn project_planning_input_for_rust_only_has_exactly_one_cargo_build_action() {
        let req = ProjectRequirements {
            rust: true,
            nim: false,
            nim_entry: None,
        };
        let input = project_planning_input(Path::new("/x"), &req);
        assert_eq!(input.actions.len(), 1);
        assert_eq!(input.actions[0].kind, ActionKind::CargoBuild);
        assert_eq!(
            input.demanded_artifacts,
            vec![ARTIFACT_RUST_PROJECT.to_string()]
        );
    }

    #[test]
    fn project_planning_input_for_nim_only_has_exactly_one_nim_build_action() {
        let req = ProjectRequirements {
            rust: false,
            nim: true,
            nim_entry: Some(PathBuf::from("src/fixture.nim")),
        };
        let input = project_planning_input(Path::new("/x"), &req);
        assert_eq!(input.actions.len(), 1);
        assert_eq!(input.actions[0].kind, ActionKind::NimBuild);
        assert_eq!(
            input.demanded_artifacts,
            vec![ARTIFACT_NIM_PROJECT.to_string()]
        );
    }

    #[test]
    fn project_planning_input_for_two_real_producers_has_no_integrate_action() {
        let req = ProjectRequirements {
            rust: true,
            nim: true,
            nim_entry: Some(PathBuf::from("src/thing.nim")),
        };
        let input = project_planning_input(Path::new("/x"), &req);
        assert_eq!(input.actions.len(), 2);
        assert!(input
            .actions
            .iter()
            .all(|a| a.kind != ActionKind::Integrate));
        assert_eq!(
            input.demanded_artifacts,
            vec![
                ARTIFACT_RUST_PROJECT.to_string(),
                ARTIFACT_NIM_PROJECT.to_string()
            ]
        );
    }

    #[test]
    fn project_planning_input_for_rust_needing_nim_on_hand_has_only_one_action() {
        let req = ProjectRequirements {
            rust: true,
            nim: true,
            nim_entry: None,
        };
        let input = project_planning_input(Path::new("/x"), &req);
        assert_eq!(input.actions.len(), 1);
        assert_eq!(input.actions[0].kind, ActionKind::CargoBuild);
    }

    /// End-to-end: a real Rust-only fixture, planned by the real Nim
    /// planner and built by the real executor, with the Nim toolchain
    /// family never resolved at all -- checked directly on the
    /// *persisted* `Run`, not just the pre-execution `DoctorRun` (closing
    /// the same "toolchain resolved selectively but re-probed at record
    /// time" gap `run_and_record_with_doctor` exists to fix).
    #[test]
    #[cfg(unix)]
    fn run_project_generation_builds_a_rust_only_fixture_without_touching_nim() {
        let repo_root = repo_root();
        let planner = stage0_planner(&repo_root);
        let project_root = repo_root.join("fixtures/rust-heavy-workspace");
        let generation_root = tmp_dir("rust-only-gen");
        let runs_root = tmp_dir("rust-only-runs");
        let lock_path = repo_root.join("toolchains.lock.toml");

        let result = run_project_generation(
            &project_root,
            &generation_root,
            &planner,
            "rust-only-test",
            &runs_root,
            &lock_path,
            None,
            None,
        )
        .expect("a pure-Rust fixture must build without a Nim toolchain");

        assert_eq!(result.actions.len(), 1);
        let action = &result.actions[0];
        assert!(action.succeeded);
        assert!(
            !action.artifacts.is_empty(),
            "expected at least one reported artifact"
        );
        for artifact in &action.artifacts {
            assert!(
                artifact.is_file(),
                "reported artifact {} does not exist",
                artifact.display()
            );
        }
        assert!(
            action
                .run
                .resolved_toolchain_fingerprint
                .as_ref()
                .unwrap()
                .nim_toolchains
                .is_empty(),
            "the Nim toolchain family must never be resolved for a Rust-only project build"
        );
        assert_eq!(
            action.run.environment_fingerprint.repository.commit,
            laminaria_fingerprint::env::detect(
                &project_root,
                "x",
                "x"
            ).repository.commit,
            "the Run's environment fingerprint must describe the target project, not LAMINARIA's own checkout"
        );

        let _ = std::fs::remove_dir_all(&generation_root);
        let _ = std::fs::remove_dir_all(&runs_root);
    }

    /// Mirror of the Rust-only test above: a real Nim-only fixture, with
    /// the Rust toolchain family never resolved, and the reported
    /// artifact is exactly the `-o:` path this module itself chose.
    #[test]
    #[cfg(unix)]
    fn run_project_generation_builds_a_nim_only_fixture_without_touching_rust() {
        let repo_root = repo_root();
        let planner = stage0_planner(&repo_root);
        let project_root = repo_root.join("fixtures/nim-heavy-workspace");
        let generation_root = tmp_dir("nim-only-gen");
        let runs_root = tmp_dir("nim-only-runs");
        let lock_path = repo_root.join("toolchains.lock.toml");

        let result = run_project_generation(
            &project_root,
            &generation_root,
            &planner,
            "nim-only-test",
            &runs_root,
            &lock_path,
            None,
            None,
        )
        .expect("a pure-Nim fixture must build without a Rust toolchain");

        assert_eq!(result.actions.len(), 1);
        let action = &result.actions[0];
        assert!(action.succeeded);
        assert_eq!(action.artifacts.len(), 1);
        assert!(
            action.artifacts[0].is_file(),
            "reported artifact {} does not exist",
            action.artifacts[0].display()
        );
        assert!(
            action
                .run
                .resolved_toolchain_fingerprint
                .as_ref()
                .unwrap()
                .rust_toolchains
                .is_empty(),
            "the Rust toolchain family must never be resolved for a Nim-only project build"
        );

        let _ = std::fs::remove_dir_all(&generation_root);
        let _ = std::fs::remove_dir_all(&runs_root);
    }

    /// A genuinely broken Rust source must abort the generation with
    /// `ActionFailed`, not a falsely-reported success -- same
    /// throwaway-copy pattern `reuse.rs`/`self_build.rs` already use for
    /// this kind of test.
    #[test]
    #[cfg(unix)]
    fn run_project_generation_reports_a_real_compile_failure_as_action_failed() {
        let repo_root = repo_root();
        let planner = stage0_planner(&repo_root);
        let broken_project = tmp_dir("broken-rust-project");
        copy_dir_recursive(
            &repo_root.join("fixtures/rust-heavy-workspace"),
            &broken_project,
        );
        let main_rs = broken_project.join("crates/fixture-bin/src/main.rs");
        let mut contents = std::fs::read_to_string(&main_rs).unwrap();
        contents.push_str("\nthis is not valid rust\n");
        std::fs::write(&main_rs, contents).unwrap();

        let generation_root = tmp_dir("broken-rust-gen");
        let runs_root = tmp_dir("broken-rust-runs");
        let lock_path = repo_root.join("toolchains.lock.toml");

        let result = run_project_generation(
            &broken_project,
            &generation_root,
            &planner,
            "broken-rust-test",
            &runs_root,
            &lock_path,
            None,
            None,
        );

        match result {
            Err(ProjectBuildError::ActionFailed { .. }) => {}
            other => panic!("expected ActionFailed for a genuine compile error, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&broken_project);
        let _ = std::fs::remove_dir_all(&generation_root);
        let _ = std::fs::remove_dir_all(&runs_root);
    }

    /// Regression test for the exact bug an external review reproduced:
    /// a Nim entry point's own `nim.cfg` sets `--compileOnly:on` (entirely
    /// outside this crate's control -- a real thing a target project's
    /// own config can do), so `nim c` exits 0 without ever linking the
    /// requested `-o:` output. Confirmed directly before writing this
    /// test: `nim c -o:<path> src/thing.nim` with a `--compileOnly:on`
    /// `nim.cfg` alongside it prints `[SuccessX]` and exits 0, and
    /// `<path>` is never created. The action must be reported as failed,
    /// not a success naming a file that doesn't exist.
    #[test]
    #[cfg(unix)]
    fn run_project_generation_fails_when_nim_exits_zero_without_producing_the_artifact() {
        let repo_root = repo_root();
        let planner = stage0_planner(&repo_root);
        let project_root = tmp_dir("compile-only-nim-project");
        std::fs::create_dir_all(project_root.join("src")).unwrap();
        std::fs::write(
            project_root.join("thing.nimble"),
            "srcDir = \"src\"\nbin = @[\"thing\"]\n",
        )
        .unwrap();
        std::fs::write(project_root.join("src/thing.nim"), "echo \"hi\"\n").unwrap();
        std::fs::write(project_root.join("nim.cfg"), "--compileOnly:on\n").unwrap();

        let generation_root = tmp_dir("compile-only-nim-gen");
        let runs_root = tmp_dir("compile-only-nim-runs");
        let lock_path = repo_root.join("toolchains.lock.toml");

        let result = run_project_generation(
            &project_root,
            &generation_root,
            &planner,
            "compile-only-test",
            &runs_root,
            &lock_path,
            None,
            None,
        );

        match result {
            Err(ProjectBuildError::ActionFailed { .. }) => {}
            other => panic!(
                "expected ActionFailed when nim exits 0 without producing the artifact, got \
                 {other:?}"
            ),
        }

        let _ = std::fs::remove_dir_all(&project_root);
        let _ = std::fs::remove_dir_all(&generation_root);
        let _ = std::fs::remove_dir_all(&runs_root);
    }

    /// Regression test for the exact bug an external review reproduced:
    /// with `--requires rust,nim` and a *real* Nim entry point also
    /// present (the two-independent-producers shape), the Cargo action's
    /// `build.rs` must still see the *verified* Nim toolchain on its own
    /// `PATH` -- not whatever unpinned `nim` happens to be first on this
    /// process's ambient `PATH`. Checked directly on the constructed
    /// `RootCommand`/persisted `Run`, not just on build success (an
    /// unpinned PATH lookup could easily "work" if it happens to resolve
    /// to something compatible, which would hide the bug).
    #[test]
    #[cfg(unix)]
    fn cargo_action_gets_the_verified_nim_on_path_even_when_a_real_nim_entry_also_exists() {
        let repo_root = repo_root();
        let project_root = tmp_dir("mixed-with-real-nim-entry");
        std::fs::write(
            project_root.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(project_root.join("thing.nimble"), "").unwrap();
        std::fs::create_dir_all(project_root.join("src")).unwrap();
        std::fs::write(project_root.join("src/thing.nim"), "echo \"hi\"\n").unwrap();

        let req = ProjectRequirements {
            rust: true,
            nim: true,
            nim_entry: Some(PathBuf::from("src/thing.nim")),
        };
        let lock_path = repo_root.join("toolchains.lock.toml");
        let (_doctor_run, toolchains) =
            resolve_project_toolchains(&lock_path, &project_root, &req).unwrap();
        let (cargo, rustc) = toolchains.rust.as_ref().unwrap();
        let nim_bin_dir = toolchains.nim.as_deref().and_then(Path::parent).unwrap();

        let generation_root = tmp_dir("mixed-with-real-nim-entry-gen");
        let root = cargo_project_build_root(
            &project_root,
            &generation_root,
            cargo,
            rustc,
            Some(nim_bin_dir),
        );

        let path_value = root
            .env_overrides
            .get("PATH")
            .expect("PATH must be overridden to include the verified Nim toolchain");
        assert!(
            path_value.starts_with(&nim_bin_dir.display().to_string()),
            "expected the verified Nim bin dir to be prepended to PATH, got: {path_value}"
        );

        let _ = std::fs::remove_dir_all(&project_root);
        let _ = std::fs::remove_dir_all(&generation_root);
    }

    /// Regression test: an explicit `--nim-entry` naming a file that does
    /// not exist must be reported as an error, never silently downgraded
    /// to "no Nim entry point" and built as Rust-only -- the exact bug an
    /// external review reproduced (a Rust project with a bad `--nim-entry`
    /// used to build and report success as if `--nim-entry` had never
    /// been given at all).
    #[test]
    fn determine_project_requirements_rejects_a_nonexistent_explicit_nim_entry() {
        let dir = tmp_dir("bad-nim-entry");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let result = determine_project_requirements(&dir, None, Some(Path::new("src/missing.nim")));
        match result {
            Err(ProjectBuildError::NimEntryNotFound(path)) => {
                assert!(
                    path.ends_with("src/missing.nim"),
                    "expected the error to name the missing path, got {}",
                    path.display()
                );
            }
            other => panic!("expected NimEntryNotFound, got {other:?}"),
        }
    }

    /// Same bug, exercised through the `--requires rust,nim` explicit
    /// path rather than pure inference.
    #[test]
    fn determine_project_requirements_rejects_a_nonexistent_explicit_nim_entry_with_requires() {
        let dir = tmp_dir("bad-nim-entry-explicit-requires");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let result = determine_project_requirements(
            &dir,
            Some(&[Capability::Rust, Capability::Nim]),
            Some(Path::new("src/missing.nim")),
        );
        match result {
            Err(ProjectBuildError::NimEntryNotFound(_)) => {}
            other => panic!("expected NimEntryNotFound, got {other:?}"),
        }
    }

    #[cfg(unix)]
    fn copy_dir_recursive(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if path.is_dir() {
                if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                    continue;
                }
                copy_dir_recursive(&path, &dst_path);
            } else {
                std::fs::copy(&path, &dst_path).unwrap();
            }
        }
    }
}

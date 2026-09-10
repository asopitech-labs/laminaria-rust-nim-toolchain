//! LAMINARIA's own self-build (issues #8/#6/#4's shared first slice):
//! builds the self-build's `PlanningInput`, calls the real Nim planner
//! (`laminaria_plan::call_planner`), validates the returned
//! `ExecutionPlan`, and executes its `ordered_actions` sequentially --
//! each `nim_build`/`cargo_build` action as a real, traced `Run`
//! (reusing this crate's own RUSTC-wrapper/CC-wrapper tracer paths via
//! `run_and_record`, never a bare `Command::output()`), then assembles
//! the results into one generation root. See `docs/self-build.md` for
//! the full stage0/stage1 protocol.
//!
//! **Concurrency bound: 1** (strictly sequential), stated explicitly as
//! this first slice's conservative choice -- issue #6 permits this
//! ("state the local concurrency/resource bound; begin conservatively if
//! needed"). Nested Cargo/`nim c` parallelism is left at each tool's own
//! default and named, not hidden, in each action's own known_gaps.
//!
//! **No caching/reuse** (issues #7/#12) is wired in here -- every
//! `run_generation` call fully rebuilds both the Nim planner and the
//! Rust host from source, per this slice's explicit scope. This is
//! actually enforced, not just claimed: an external review caught that
//! an earlier version built into `repo_root`'s own shared `target/`
//! and `nim-planner/bin/`, so a *second* generation silently inherited
//! the first generation's already-fresh Cargo/Nim outputs and reported
//! success without recompiling anything. Every generation now gets its
//! own isolated `<generation_root>/.build/` staging area (a fresh
//! Cargo `--target-dir` and Nim `--nimcache`), wiped before each
//! `run_generation` call.

use std::path::{Path, PathBuf};

use laminaria_plan::{
    validate, Action, ActionKind, ArtifactRef, ExecutionPlan, PlanOutcome, PlanRejection,
    PlannerCallError, PlanningInput, ValidationError,
};

use crate::types::{ProbeLevel, RootCommand, Run};

pub const SELF_BUILD_WORKLOAD_ID: &str = "laminaria-self-build";

pub const ARTIFACT_PLANNER_BIN: &str = "laminaria-planner-bin";
pub const ARTIFACT_HOST_BINS: &str = "laminaria-host-bins";
pub const ARTIFACT_GENERATION_ROOT: &str = "generation-root";

pub const ACTION_COMPILE_NIM_PLANNER: &str = "compile-nim-planner";
pub const ACTION_COMPILE_RUST_HOST: &str = "compile-rust-host";
pub const ACTION_INTEGRATE: &str = "integrate";

/// The Rust binaries `cargo build --workspace --release` produces that
/// must end up alongside `laminaria-planner` in the generation root --
/// every one of `find_planner_binary`/`find_rustc_wrapper_binary`/
/// `find_cc_wrapper_binary`'s sibling-binary lookups depends on all four
/// binaries living in the same directory as whichever one is currently
/// running.
const HOST_BINARY_NAMES: &[&str] = &[
    "laminaria",
    "laminaria-rustc-wrapper",
    "laminaria-cc-wrapper",
];
const PLANNER_BINARY_NAME: &str = "laminaria-planner";

fn binary_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

/// Builds the self-build's `PlanningInput`: compile the Nim planner from
/// source, compile the Rust host workspace from source, and integrate
/// both into one generation root. Demand always covers the whole
/// self-build in this first slice -- no demand-driven pruning yet
/// (issue #8's variant-explosion scope is out of scope here).
pub fn self_build_planning_input() -> PlanningInput {
    PlanningInput::new(
        vec![ARTIFACT_GENERATION_ROOT.to_string()],
        vec![
            Action {
                id: ACTION_COMPILE_NIM_PLANNER.to_string(),
                kind: ActionKind::NimBuild,
                command_identity: "nim c src/laminaria_planner.nim (nim-planner)".to_string(),
                inputs: vec![ArtifactRef::source("nim-planner")],
                outputs: vec![ArtifactRef::declared(ARTIFACT_PLANNER_BIN)],
            },
            Action {
                id: ACTION_COMPILE_RUST_HOST.to_string(),
                kind: ActionKind::CargoBuild,
                command_identity: "cargo build --workspace --release".to_string(),
                inputs: vec![
                    ArtifactRef::source("crates"),
                    ArtifactRef::source("Cargo.toml"),
                ],
                outputs: vec![ArtifactRef::declared(ARTIFACT_HOST_BINS)],
            },
            Action {
                id: ACTION_INTEGRATE.to_string(),
                kind: ActionKind::Integrate,
                command_identity: "assemble generation root".to_string(),
                inputs: vec![
                    ArtifactRef::declared(ARTIFACT_PLANNER_BIN),
                    ArtifactRef::declared(ARTIFACT_HOST_BINS),
                ],
                outputs: vec![ArtifactRef::declared(ARTIFACT_GENERATION_ROOT)],
            },
        ],
    )
}

#[derive(Debug)]
pub enum SelfBuildError {
    Planner(PlannerCallError),
    Rejected(PlanRejection),
    InvalidPlan(ValidationError),
    /// The toolchain lock file could not be loaded, or does not resolve
    /// both a Rust and a Nim toolchain with a real executable path --
    /// checked and resolved *before* any action executes, so self-build
    /// never silently falls back to running whatever `cargo`/`nim`
    /// happen to be on `PATH` (an external review caught this: pointing
    /// `--lock` at a nonexistent file previously still "succeeded,"
    /// with an empty resolved-toolchain list in the evidence and PATH's
    /// own `cargo`/`nim` actually invoked).
    ToolchainUnresolved(String),
    ActionFailed {
        action_id: String,
        detail: String,
    },
    Io(std::io::Error),
}

impl std::fmt::Display for SelfBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelfBuildError::Planner(e) => write!(f, "failed to call the Nim planner: {e}"),
            SelfBuildError::Rejected(r) => write!(
                f,
                "the Nim planner rejected this self-build's PlanningInput ({:?}): {}",
                r.reason_kind, r.reason_detail
            ),
            SelfBuildError::InvalidPlan(e) => {
                write!(
                    f,
                    "the Nim planner's ExecutionPlan failed Rust-side validation: {e}"
                )
            }
            SelfBuildError::ToolchainUnresolved(detail) => write!(
                f,
                "could not resolve a verified Rust and Nim toolchain before executing: {detail}"
            ),
            SelfBuildError::ActionFailed { action_id, detail } => {
                write!(f, "self-build action '{action_id}' failed: {detail}")
            }
            SelfBuildError::Io(e) => write!(f, "I/O error during self-build: {e}"),
        }
    }
}

impl std::error::Error for SelfBuildError {}

impl From<std::io::Error> for SelfBuildError {
    fn from(e: std::io::Error) -> Self {
        SelfBuildError::Io(e)
    }
}

/// One action's execution result. `run` is `None` only for
/// `ActionKind::Integrate`, which is filesystem assembly, not a
/// compiler invocation -- it does not produce a traced `Run` because
/// there is no subprocess to trace; wrapping it in a synthetic one would
/// misrepresent it as compiler evidence it isn't. `NimBuild`/
/// `CargoBuild` actions always produce a real `Run`, which is what issue
/// #6's "not a hidden `Command::output()`" concern is actually about.
#[derive(Debug)]
pub struct ActionOutcome {
    pub action_id: String,
    pub run: Option<Run>,
    pub succeeded: bool,
    pub detail: String,
}

#[derive(Debug)]
pub struct GenerationResult {
    pub plan: ExecutionPlan,
    pub actions: Vec<ActionOutcome>,
    pub generation_root: PathBuf,
}

/// Runs one self-build generation: `repo_root`'s own Nim/Rust sources,
/// planned by `planner_binary` (the *current* generation's own planner
/// -- stage0's planner plans stage1, never a different generation's),
/// executed action-by-action, and assembled into `generation_root`.
///
/// `generation_label` is a human-readable lineage tag (e.g.
/// `"stage0-to-stage1"`) recorded into each action's `Run` evidence
/// alongside the plan's `plan_id` and `planner_binary`'s own resolved
/// digest -- issue #6's "recording the invoked build-driver identity,
/// planner identity, plan ID, action results and generation lineage."
#[allow(clippy::too_many_arguments)]
pub fn run_generation(
    repo_root: &Path,
    generation_root: &Path,
    planner_binary: &Path,
    generation_label: &str,
    runs_root: &Path,
    lock_path: &Path,
) -> Result<GenerationResult, SelfBuildError> {
    let input = self_build_planning_input();
    let outcome =
        laminaria_plan::call_planner(planner_binary, &input).map_err(SelfBuildError::Planner)?;
    let plan = match outcome {
        PlanOutcome::Planned(plan) => plan,
        PlanOutcome::Rejected(rejection) => return Err(SelfBuildError::Rejected(rejection)),
    };
    validate(&plan, &input).map_err(SelfBuildError::InvalidPlan)?;

    // Resolve and verify the Rust/Nim toolchains *before* executing
    // anything -- a missing/unreadable lock file, or one that resolves
    // no usable toolchain, must fail closed here rather than letting
    // execution silently fall through to whatever `cargo`/`nim` happen
    // to be on `PATH`.
    let toolchain = resolve_verified_toolchain(lock_path, repo_root)?;

    // Wipe this generation's own build staging area first, so a
    // previous call into the *same* `generation_root` (or, before this
    // fix, `repo_root`'s own shared `target/`/`nim-planner/bin/`) can
    // never make this call's actions spuriously report "already fresh,
    // nothing to do" -- every `run_generation` call is a genuine
    // from-scratch rebuild, not just documented as one.
    let _ = std::fs::remove_dir_all(build_staging_dir(generation_root));
    std::fs::create_dir_all(generation_root)?;

    let planner_digest = laminaria_fingerprint::exec::sha256_file(planner_binary);
    let evidence = ActionEvidence {
        plan_id: plan.plan_id.clone(),
        planner_digest,
        generation_label: generation_label.to_string(),
    };

    let mut action_outcomes = Vec::new();
    for action_id in &plan.ordered_actions {
        let action = plan
            .actions
            .get(action_id)
            .expect("validate() already checked every ordered_actions id exists");

        let result = match action.kind {
            ActionKind::NimBuild => run_traced_action(
                action,
                repo_root,
                runs_root,
                lock_path,
                &evidence,
                nim_build_root(repo_root, generation_root, &toolchain.nim),
                vec![nim_output_dir(generation_root)],
            ),
            ActionKind::CargoBuild => run_traced_action(
                action,
                repo_root,
                runs_root,
                lock_path,
                &evidence,
                cargo_build_root(repo_root, generation_root, &toolchain.cargo),
                vec![cargo_target_dir(generation_root).join("release")],
            ),
            ActionKind::Integrate => run_integrate_action(action, generation_root),
        };

        let outcome = result?;
        let succeeded = outcome.succeeded;
        let failure_detail = outcome.detail.clone();
        let failed_action_id = outcome.action_id.clone();
        action_outcomes.push(outcome);
        if !succeeded {
            // A failed action aborts everything depending on it -- since
            // execution follows `ordered_actions`'s dependency order
            // strictly, simply stopping here means no dependent action
            // ever runs; no partial generation is treated as valid input
            // to the next stage (the caller sees `Err`, not a
            // `GenerationResult` claiming success).
            return Err(SelfBuildError::ActionFailed {
                action_id: failed_action_id,
                detail: failure_detail,
            });
        }
    }

    Ok(GenerationResult {
        plan,
        actions: action_outcomes,
        generation_root: generation_root.to_path_buf(),
    })
}

struct ActionEvidence {
    plan_id: String,
    planner_digest: Option<String>,
    generation_label: String,
}

/// The exact, resolved-from-the-lock-file `cargo`/`nim` executables
/// self-build must invoke -- never a bare `"cargo"`/`"nim"` program name
/// resolved implicitly from `PATH` at spawn time (an external review
/// caught this: with no verification step, a nonexistent `--lock`
/// resulted in an empty resolved-toolchain list in the evidence while
/// execution silently used PATH's own `cargo`/`nim` anyway).
struct VerifiedToolchain {
    cargo: PathBuf,
    nim: PathBuf,
}

/// Loads `lock_path`, and requires it to resolve at least one Rust
/// toolchain and one Nim toolchain, each with a real executable path
/// (`laminaria_fingerprint`'s own `doctor::build`, reused directly
/// rather than re-deriving toolchain resolution here). This project's
/// `toolchains.lock.toml` declares exactly one of each today, so simply
/// taking the first resolved entry is unambiguous; a lock file
/// declaring more than one Rust or Nim toolchain would need an explicit
/// `--rust-toolchain`/`--nim-toolchain` selector this first slice does
/// not yet have -- named as an open gap, not silently guessed at.
fn resolve_verified_toolchain(
    lock_path: &Path,
    repo_root: &Path,
) -> Result<VerifiedToolchain, SelfBuildError> {
    let doctor_run = laminaria_fingerprint::doctor::build(lock_path, repo_root);
    if let Some(load_error) = &doctor_run.lock_load_error {
        return Err(SelfBuildError::ToolchainUnresolved(format!(
            "failed to load toolchain lock file {}: {load_error:?}",
            lock_path.display()
        )));
    }

    let rust_toolchain = doctor_run.report.rust_toolchains.first().ok_or_else(|| {
        SelfBuildError::ToolchainUnresolved(format!(
            "toolchain lock file {} declares no Rust toolchain",
            lock_path.display()
        ))
    })?;
    let nim_toolchain = doctor_run.report.nim_toolchains.first().ok_or_else(|| {
        SelfBuildError::ToolchainUnresolved(format!(
            "toolchain lock file {} declares no Nim toolchain",
            lock_path.display()
        ))
    })?;
    let cargo = rust_toolchain.cargo.path.clone().ok_or_else(|| {
        SelfBuildError::ToolchainUnresolved(format!(
            "Rust toolchain '{}' has no resolved cargo executable (see its own resolution notes \
             from `laminaria doctor`)",
            rust_toolchain.logical_name
        ))
    })?;
    let nim = nim_toolchain.nim.path.clone().ok_or_else(|| {
        SelfBuildError::ToolchainUnresolved(format!(
            "Nim toolchain '{}' has no resolved nim executable (see its own resolution notes \
             from `laminaria doctor`)",
            nim_toolchain.logical_name
        ))
    })?;

    Ok(VerifiedToolchain { cargo, nim })
}

/// `<generation_root>/.build/` -- everything a generation's own
/// compilation needs that is *not* part of the final assembled layout
/// (unlike the four sibling binaries `integrate` copies to the top
/// level of `generation_root`). Wiped at the start of every
/// `run_generation` call.
fn build_staging_dir(generation_root: &Path) -> PathBuf {
    generation_root.join(".build")
}

fn nim_output_dir(generation_root: &Path) -> PathBuf {
    build_staging_dir(generation_root).join("nim-out")
}

fn nim_cache_dir(generation_root: &Path) -> PathBuf {
    build_staging_dir(generation_root).join("nimcache")
}

fn cargo_target_dir(generation_root: &Path) -> PathBuf {
    build_staging_dir(generation_root).join("cargo-target")
}

/// Invokes `nim c` directly, *not* `nimble build`: a real bug this
/// crate's own compile-failure test caught -- `nimble build`'s wrapper
/// around `nim c` prints "Error: Build failed for the package" on a
/// genuine compile error but still exits `0` (reproduced directly with
/// this repo's own `nimble v0.22.2`), which would have silently treated
/// a broken Nim source tree as a successful action. `nim c`'s own exit
/// code correctly reflects compile success/failure. This also means the
/// action is recognized by `is_nim_c_command` in this crate's own
/// `lib.rs`, so it gets real CC-wrapper per-invocation tracing --
/// `nimble build` would have hidden its internal `nim c` invocation from
/// that tracer entirely, which `nimble build` would not have.
///
/// `nim_binary` is the resolved, lock-verified executable
/// (`resolve_verified_toolchain`), not a bare `"nim"` program name --
/// `is_nim_c_command`'s own file-stem check still recognizes it (it
/// compares `Path::new(&root.program).file_stem()`, not the full path)
/// so CC-wrapper substitution still engages. Output and nimcache both
/// live under `generation_root`'s own isolated `.build/` staging area
/// (this module's own doc comment: never `repo_root`'s shared
/// `nim-planner/bin/`, which let one generation's build spuriously
/// inherit an earlier one's freshness).
fn nim_build_root(repo_root: &Path, generation_root: &Path, nim_binary: &Path) -> RootCommand {
    let output_path = nim_output_dir(generation_root).join(binary_name(PLANNER_BINARY_NAME));
    RootCommand {
        program: nim_binary.display().to_string(),
        args: vec![
            "c".to_string(),
            "--path:src".to_string(),
            format!("--nimcache:{}", nim_cache_dir(generation_root).display()),
            format!("-o:{}", output_path.display()),
            "src/laminaria_planner.nim".to_string(),
        ],
        cwd: Some(repo_root.join("nim-planner")),
        env_overrides: Default::default(),
    }
}

/// `cargo_binary` is the resolved, lock-verified executable, same
/// reasoning as `nim_build_root`'s own doc comment (`is_cargo_command`
/// also compares only the file stem). `--target-dir` points at this
/// generation's own isolated staging area, not `repo_root`'s shared
/// `target/` -- the actual fix for the cross-generation cache-sharing
/// bug this module's own doc comment describes.
fn cargo_build_root(repo_root: &Path, generation_root: &Path, cargo_binary: &Path) -> RootCommand {
    RootCommand {
        program: cargo_binary.display().to_string(),
        args: vec![
            "build".to_string(),
            "--workspace".to_string(),
            "--release".to_string(),
            "--target-dir".to_string(),
            cargo_target_dir(generation_root).display().to_string(),
        ],
        cwd: Some(repo_root.to_path_buf()),
        env_overrides: Default::default(),
    }
}

/// Executes one `NimBuild`/`CargoBuild` action as a real, traced `Run`
/// via `crate::run_and_record` -- the same RUSTC-wrapper/CC-wrapper
/// per-invocation tracer paths every other traced command in this crate
/// already goes through, so even this first slice's coarse ("whole
/// `cargo build`"/whole `nimble build`") actions are backed by real
/// per-compiler-invocation evidence, not a bare `Command::output()`.
/// Patches the returned `Run` with this action's plan/planner/generation
/// lineage evidence and re-persists it, the same patch-then-rewrite
/// pattern `scenario::run_scenario_once` already uses for its own
/// post-hoc `preparation_record`/`cache_state` corrections.
fn run_traced_action(
    action: &Action,
    repo_root: &Path,
    runs_root: &Path,
    lock_path: &Path,
    evidence: &ActionEvidence,
    root: RootCommand,
    observation_roots: Vec<PathBuf>,
) -> Result<ActionOutcome, SelfBuildError> {
    let requested_artifact = action.outputs.first().and_then(|o| match o {
        ArtifactRef::Declared { artifact_id } => Some(artifact_id.clone()),
        ArtifactRef::Source { .. } => None,
    });

    let (mut run, _dir) = crate::run_and_record(
        runs_root,
        SELF_BUILD_WORKLOAD_ID,
        &format!("self-build/{}/{}", evidence.generation_label, action.id),
        requested_artifact,
        lock_path,
        repo_root,
        root,
        ProbeLevel::Level1ProcessResource,
        &observation_roots,
    )?;

    run.process_trace.known_gaps.push(format!(
        "self-build lineage evidence (issue #6): plan_id={}, planner_digest_sha256={:?}, \
         generation={}, action_id={}",
        evidence.plan_id, evidence.planner_digest, evidence.generation_label, action.id
    ));
    crate::store::write_run(runs_root, &run)?;

    let succeeded = run.result.as_ref().is_some_and(|r| r.success);
    let detail = if succeeded {
        format!(
            "action '{}' completed successfully (run {})",
            action.id, run.run_id
        )
    } else {
        format!("action '{}' failed (see run {})", action.id, run.run_id)
    };

    Ok(ActionOutcome {
        action_id: action.id.clone(),
        run: Some(run),
        succeeded,
        detail,
    })
}

/// Executes the `Integrate` action: not a compiler invocation, so it
/// produces no traced `Run` (see `ActionOutcome::run`'s own doc
/// comment) -- it copies the two upstream actions' produced binaries
/// into one generation root so every sibling-binary lookup
/// (`find_planner_binary`/`find_rustc_wrapper_binary`/
/// `find_cc_wrapper_binary`) finds what it needs next to whichever
/// binary ends up running.
fn run_integrate_action(
    action: &Action,
    generation_root: &Path,
) -> Result<ActionOutcome, SelfBuildError> {
    std::fs::create_dir_all(generation_root)?;

    let mut missing = Vec::new();

    let planner_src = nim_output_dir(generation_root).join(binary_name(PLANNER_BINARY_NAME));
    if planner_src.is_file() {
        copy_executable(
            &planner_src,
            &generation_root.join(binary_name(PLANNER_BINARY_NAME)),
        )?;
    } else {
        missing.push(planner_src.display().to_string());
    }

    let cargo_release_dir = cargo_target_dir(generation_root).join("release");
    for name in HOST_BINARY_NAMES {
        let src = cargo_release_dir.join(binary_name(name));
        if src.is_file() {
            copy_executable(&src, &generation_root.join(binary_name(name)))?;
        } else {
            missing.push(src.display().to_string());
        }
    }

    if !missing.is_empty() {
        return Ok(ActionOutcome {
            action_id: action.id.clone(),
            run: None,
            succeeded: false,
            detail: format!(
                "integrate: expected upstream action output(s) not found: {}",
                missing.join(", ")
            ),
        });
    }

    Ok(ActionOutcome {
        action_id: action.id.clone(),
        run: None,
        succeeded: true,
        detail: format!(
            "integrate: assembled generation root at {}",
            generation_root.display()
        ),
    })
}

#[cfg(unix)]
fn copy_executable(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::copy(src, dst)?;
    let mut perms = std::fs::metadata(dst)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(dst, perms)
}

#[cfg(not(unix))]
fn copy_executable(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::copy(src, dst).map(|_| ())
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

    /// Builds `nim-planner/bin/laminaria-planner` via `nim c` directly --
    /// *not* `nimble build`, which this same test module already found
    /// exits `0` on a genuine compile failure (see `nim_build_root`'s own
    /// doc comment). A second, independent reason surfaced in CI: on
    /// Ubuntu, `apt`'s packaged `nim`/`nimble` fail `nimble build`'s own
    /// dependency check outright (`Error: Unsatisfied dependency: nim
    /// (>= 2.0.0)`, even though the installed `nim --version` genuinely
    /// satisfies it) -- `nim c` sidesteps nimble's dependency resolution
    /// entirely, matching production's own `nim_build_root`.
    #[cfg(unix)]
    fn build_stage0_planner(repo_root: &Path) {
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
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "laminaria-run-self-build-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn run_generation_against_a_missing_planner_binary_fails_without_producing_a_generation() {
        let repo_root = repo_root();
        let generation_root = tmp_dir("missing-planner-gen");
        let runs_root = tmp_dir("missing-planner-runs");
        let lock_path = repo_root.join("toolchains.lock.toml");
        let missing_planner = tmp_dir("nonexistent-planner-parent").join("laminaria-planner");

        let result = run_generation(
            &repo_root,
            &generation_root,
            &missing_planner,
            "test",
            &runs_root,
            &lock_path,
        );
        assert!(
            matches!(result, Err(SelfBuildError::Planner(_))),
            "a missing planner binary must be a structural error, never a silent fallback plan"
        );
        assert!(!generation_root.join(binary_name("laminaria")).exists());

        let _ = std::fs::remove_dir_all(&generation_root);
        let _ = std::fs::remove_dir_all(&runs_root);
    }

    /// The real end-to-end proof this issue #8/#6/#4 slice asks for:
    /// stage0 (built here by an ordinary, un-planned `nimble build`/
    /// `cargo build --workspace --release` -- the "external tool"
    /// bootstrap seed) drives `run_generation` to produce stage1 through
    /// the real Nim-planned, Rust-executed pipeline, and stage1's own
    /// freshly built planner is then independently invoked and proven to
    /// actually plan -- not merely present on disk.
    #[test]
    #[cfg(unix)]
    fn stage0_produces_a_stage1_whose_own_planner_actually_works() {
        let repo_root = repo_root();

        // stage0: an ordinary, external-tool build -- deliberately not
        // going through run_generation/the Nim planner at all.
        build_stage0_planner(&repo_root);
        let cargo_status = std::process::Command::new("cargo")
            .args(["build", "--workspace", "--release"])
            .current_dir(&repo_root)
            .status()
            .expect("failed to invoke cargo");
        assert!(cargo_status.success(), "stage0 cargo build failed");

        let stage0_planner = repo_root
            .join("nim-planner/bin")
            .join(binary_name(PLANNER_BINARY_NAME));
        assert!(stage0_planner.is_file());

        let generation_root = tmp_dir("stage1-gen");
        let runs_root = tmp_dir("stage1-runs");
        let lock_path = repo_root.join("toolchains.lock.toml");

        let result = run_generation(
            &repo_root,
            &generation_root,
            &stage0_planner,
            "stage0-to-stage1",
            &runs_root,
            &lock_path,
        )
        .expect("stage0 -> stage1 self-build should succeed");

        assert_eq!(result.plan.produced_by, laminaria_plan::PRODUCED_BY);
        assert_eq!(result.actions.len(), 3);
        assert!(result.actions.iter().all(|a| a.succeeded));

        let stage1_planner = generation_root.join(binary_name(PLANNER_BINARY_NAME));
        assert!(
            stage1_planner.is_file(),
            "stage1 must contain its own laminaria-planner binary"
        );
        for name in HOST_BINARY_NAMES {
            assert!(
                generation_root.join(binary_name(name)).is_file(),
                "stage1 must contain {name}"
            );
        }

        // The actual proof this isn't "linked but unused": independently
        // invoke stage1's *own* freshly built planner binary and confirm
        // it really plans, using the same self-build PlanningInput.
        let outcome = laminaria_plan::call_planner(&stage1_planner, &self_build_planning_input())
            .expect("stage1's own planner binary must be independently invocable");
        match outcome {
            PlanOutcome::Planned(plan) => {
                assert_eq!(plan.produced_by, laminaria_plan::PRODUCED_BY);
                assert_eq!(plan.ordered_actions.len(), 3);
            }
            PlanOutcome::Rejected(r) => {
                panic!("stage1's planner rejected a well-formed input: {r:?}")
            }
        }

        let _ = std::fs::remove_dir_all(&generation_root);
        let _ = std::fs::remove_dir_all(&runs_root);
    }

    /// Portable recursive copy of just what a self-build needs
    /// (`Cargo.toml`/`Cargo.lock`, `crates/`, `nim-planner/`), skipping
    /// build-output directories (`target`, `bin`, `nimcache`, `.git`) --
    /// never the tracked repo itself, so this test can break a source
    /// file without ever leaving the real repo dirty. Same reasoning as
    /// `reuse.rs`'s own `copy_dir_recursive` test helper: a plain
    /// `std::fs` walk, not `cp -r`, to stay portable.
    fn copy_workspace_sources(src_root: &Path, dst_root: &Path) {
        std::fs::create_dir_all(dst_root).unwrap();
        for name in ["Cargo.toml", "Cargo.lock", "toolchains.lock.toml"] {
            let src = src_root.join(name);
            if src.is_file() {
                std::fs::copy(&src, dst_root.join(name)).unwrap();
            }
        }
        copy_dir_filtered(&src_root.join("crates"), &dst_root.join("crates"));
        copy_dir_filtered(&src_root.join("nim-planner"), &dst_root.join("nim-planner"));
    }

    fn copy_dir_filtered(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if matches!(
                name.to_string_lossy().as_ref(),
                "target" | "bin" | "nimcache" | ".git"
            ) {
                continue;
            }
            let dst_path = dst.join(&name);
            if entry.file_type().unwrap().is_dir() {
                copy_dir_filtered(&entry.path(), &dst_path);
            } else {
                std::fs::copy(entry.path(), dst_path).unwrap();
            }
        }
    }

    /// The other half of this slice's completion criterion (alongside
    /// `stage0_produces_a_stage1_whose_own_planner_actually_works`):
    /// "circular ... invalid input, and compile failures are not treated
    /// as success." A genuine Nim compile error in a throwaway copy of
    /// `nim-planner/` must abort the generation at exactly that action,
    /// never run `integrate`, and never leave a stage output binary
    /// behind.
    #[test]
    #[cfg(unix)]
    fn a_compile_failure_aborts_the_generation_without_producing_a_stage_output() {
        let repo_root = repo_root();
        let stage0_planner = repo_root
            .join("nim-planner/bin")
            .join(binary_name(PLANNER_BINARY_NAME));
        if !stage0_planner.is_file() {
            build_stage0_planner(&repo_root);
        }

        let broken_repo = tmp_dir("broken-repo");
        copy_workspace_sources(&repo_root, &broken_repo);

        let kernel_path = broken_repo.join("nim-planner/src/planning_kernel.nim");
        let mut content = std::fs::read_to_string(&kernel_path).unwrap();
        content.push_str("\nthis is not valid Nim syntax (((\n");
        std::fs::write(&kernel_path, content).unwrap();

        let generation_root = tmp_dir("broken-compile-gen");
        let runs_root = tmp_dir("broken-compile-runs");
        let lock_path = broken_repo.join("toolchains.lock.toml");

        let result = run_generation(
            &broken_repo,
            &generation_root,
            &stage0_planner,
            "broken-compile",
            &runs_root,
            &lock_path,
        );

        match result {
            Err(SelfBuildError::ActionFailed { action_id, .. }) => {
                assert_eq!(action_id, ACTION_COMPILE_NIM_PLANNER);
            }
            other => panic!(
                "expected the broken nim-planner compile action to fail generation, got {other:?}"
            ),
        }
        assert!(
            !generation_root.join(binary_name("laminaria")).exists(),
            "a compile failure must never produce a stage output binary -- integrate must not \
             have run"
        );
        assert!(
            !generation_root
                .join(binary_name(PLANNER_BINARY_NAME))
                .exists(),
            "a compile failure in compile-nim-planner must leave no planner binary in the \
             generation root either"
        );

        let _ = std::fs::remove_dir_all(&broken_repo);
        let _ = std::fs::remove_dir_all(&generation_root);
        let _ = std::fs::remove_dir_all(&runs_root);
    }
}

//! Scenario execution, repetition, and noise-floor-aware comparison --
//! issue #21's first slice: "turn raw Run records into reproducible
//! baseline experiments that distinguish cold/warm/no-op/incremental
//! states and prevent single noisy wall-clock samples from driving
//! architecture decisions."
//!
//! **Deliberately scoped for this first slice** (issue #21 has ten
//! acceptance criteria; this is one independently-reviewable unit, not
//! the whole issue):
//!
//! - Only the four scenarios issue #21 names as the ones to start with
//!   (`docs/measurement-foundation.md`'s own required list): cold build,
//!   true no-op, a Rust implementation-only edit, a Nim
//!   implementation-only edit. Backend/config-only, link-only,
//!   worktree-relocation, and every ThinLTO/Wasm-later-extension scenario
//!   are not attempted here.
//! - Scenarios are built by this module's own preset constructors
//!   (`rust_heavy_workspace_scenario`/`nim_heavy_workspace_scenario`),
//!   targeting this repo's own `fixtures/rust-heavy-workspace`/
//!   `fixtures/nim-heavy-workspace` fixtures specifically -- not a
//!   general scenario-authoring DSL.
//! - Cache-state tracking is a three-value label (`CacheStateLabel`), not
//!   the full per-subsystem contract (Cargo target dir, compiler
//!   incremental state, sccache, ThinLTO cache, filesystem/page-cache
//!   policy) issue #21 asks for -- named as an open gap in `NOTES.md`,
//!   not silently treated as complete.
//! - Noise-floor comparison uses one scenario's own measured sample
//!   standard deviation as its noise floor -- not the "do not use one
//!   universal percentage threshold" mistake issue #21 explicitly warns
//!   against, but also not a rigorous statistical test (no confidence
//!   interval, no correction for small sample counts). A first, honest
//!   slice, not a finished statistics engine.
//! - Cross-environment/cross-toolchain comparison rejection (issue #21's
//!   acceptance criterion 7) checks `workload_id`, `EnvironmentFingerprint`
//!   comparability (`laminaria_fingerprint::comparability`, reused
//!   directly, not reinvented), and toolchain identity (reusing
//!   `reuse::toolchain_identity_from_run`) -- but only using the *first*
//!   repetition's own identity as each report's representative value, not
//!   cross-validating that every repetition inside one report agrees.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store;
use crate::types::{CacheState, PreparationRecord, ProbeLevel, RootCommand, Run};

pub const SCENARIO_SCHEMA_VERSION: &str = "0.1.0";

/// The three cache states this first slice actually distinguishes --
/// issue #21's own required "clean/cold", "warm rebuild", "true no-op"
/// scenarios. `Warm` covers both an implementation edit and any other
/// non-cold, non-true-no-op state; this module's own scenario presets
/// only ever produce `Cold`, `TrueNoop`, or `Warm` (for an edit).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheStateLabel {
    Cold,
    Warm,
    TrueNoop,
}

/// One scenario: a preparation step (recorded, but explicitly excluded
/// from the timed interval -- issue #21's "preparation actions must be
/// recorded but separated from the timed execution interval"), the
/// measured root command, which directories to snapshot for an artifact
/// inventory, and the cache-state label this scenario is expected to
/// produce.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub id: String,
    pub workload_id: String,
    pub prepare: Vec<RootCommand>,
    pub root: RootCommand,
    pub observation_roots: Vec<PathBuf>,
    pub cache_state_label: CacheStateLabel,
}

fn cargo_root(manifest_path: &Path) -> RootCommand {
    RootCommand {
        program: "cargo".to_string(),
        args: vec![
            "build".to_string(),
            "--manifest-path".to_string(),
            manifest_path.display().to_string(),
            "--workspace".to_string(),
        ],
        cwd: None,
        env_overrides: Default::default(),
    }
}

/// `path.display()`, with backslashes normalized to forward slashes.
/// `rm -rf`/`touch` below both resolve to MSYS-built coreutils on
/// Windows (native `cmd.exe` has neither), and MSYS's own C runtime
/// reinterprets backslashes in argv as escape sequences on the way in --
/// a real bug this crate's own CI caught (a Windows `PathBuf`'s
/// backslashes passed straight to `rm`/`cp`/`touch`, corrupted before
/// the program ever saw the intended path). Forward slashes are a
/// harmless no-op on Unix and are what MSYS itself actually expects.
fn portable_arg_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn rm_rf(path: &Path) -> RootCommand {
    RootCommand {
        program: "rm".to_string(),
        args: vec!["-rf".to_string(), portable_arg_path(path)],
        cwd: None,
        env_overrides: Default::default(),
    }
}

fn touch(path: &Path) -> RootCommand {
    RootCommand {
        program: "touch".to_string(),
        args: vec![portable_arg_path(path)],
        cwd: None,
        env_overrides: Default::default(),
    }
}

/// A cold build, a true no-op rebuild, or an implementation-only edit
/// rebuild of `fixtures/rust-heavy-workspace` -- `edited_source_path` is
/// only used (and must be `Some`) for `CacheStateLabel::Warm`; `Cold` and
/// `TrueNoop` don't take one, since neither scenario touches source.
pub fn rust_heavy_workspace_scenario(
    kind: CacheStateLabel,
    manifest_path: &Path,
    target_dir: &Path,
    edited_source_path: Option<&Path>,
) -> Scenario {
    let prepare = match kind {
        CacheStateLabel::Cold => vec![rm_rf(target_dir)],
        CacheStateLabel::TrueNoop => vec![],
        CacheStateLabel::Warm => vec![touch(
            edited_source_path
                .expect("rust_heavy_workspace_scenario(Warm, ...) requires edited_source_path"),
        )],
    };
    Scenario {
        id: match kind {
            CacheStateLabel::Cold => "cold-build".to_string(),
            CacheStateLabel::TrueNoop => "true-noop".to_string(),
            CacheStateLabel::Warm => "rust-implementation-edit".to_string(),
        },
        workload_id: "rust-heavy-workspace".to_string(),
        prepare,
        root: cargo_root(manifest_path),
        observation_roots: vec![target_dir.to_path_buf()],
        cache_state_label: kind,
    }
}

fn nim_root(nimcache_dir: &Path, out_path: &Path, main_nim: &Path) -> RootCommand {
    RootCommand {
        program: "nim".to_string(),
        args: vec![
            "c".to_string(),
            format!("--nimcache:{}", nimcache_dir.display()),
            format!("-o:{}", out_path.display()),
            main_nim.display().to_string(),
        ],
        cwd: None,
        env_overrides: Default::default(),
    }
}

/// The Nim-side counterpart to `rust_heavy_workspace_scenario`, targeting
/// `fixtures/nim-heavy-workspace`. `Cold` clears both the nimcache
/// directory and the previously-linked output binary (Nim's own
/// equivalent of a `target/` directory is split across the two).
pub fn nim_heavy_workspace_scenario(
    kind: CacheStateLabel,
    main_nim: &Path,
    nimcache_dir: &Path,
    out_path: &Path,
    edited_source_path: Option<&Path>,
) -> Scenario {
    let prepare = match kind {
        CacheStateLabel::Cold => vec![rm_rf(nimcache_dir), rm_rf(out_path)],
        CacheStateLabel::TrueNoop => vec![],
        CacheStateLabel::Warm => vec![touch(
            edited_source_path
                .expect("nim_heavy_workspace_scenario(Warm, ...) requires edited_source_path"),
        )],
    };
    Scenario {
        id: match kind {
            CacheStateLabel::Cold => "cold-build".to_string(),
            CacheStateLabel::TrueNoop => "true-noop".to_string(),
            CacheStateLabel::Warm => "nim-implementation-edit".to_string(),
        },
        workload_id: "nim-heavy-workspace".to_string(),
        prepare,
        root: nim_root(nimcache_dir, out_path, main_nim),
        observation_roots: vec![nimcache_dir.to_path_buf(), out_path.to_path_buf()],
        cache_state_label: kind,
    }
}

/// Runs `scenario.prepare` (portable spawn/wait only -- not through
/// `tracer`, since preparation is explicitly excluded from the timed
/// interval issue #21 asks for), returning a human-readable description
/// of each step for `PreparationRecord::steps`. A prep step failing is a
/// real error (not swallowed): if `rm -rf target` or `touch` itself fails,
/// the scenario's own precondition wasn't actually established, so the
/// timed run that would follow can't be trusted either.
fn run_prepare_steps(steps: &[RootCommand]) -> std::io::Result<Vec<String>> {
    let mut descriptions = Vec::new();
    for step in steps {
        let description = format!("{} {}", step.program, step.args.join(" "));
        let status = std::process::Command::new(&step.program)
            .args(&step.args)
            .status()?;
        if !status.success() {
            return Err(std::io::Error::other(format!(
                "preparation step failed ({description}): exit status {status}"
            )));
        }
        descriptions.push(description);
    }
    Ok(descriptions)
}

/// Runs `scenario` once: preparation (untimed, recorded into
/// `PreparationRecord`), then the timed measured command via
/// `crate::run_and_record`. Patches the returned `Run`'s
/// `preparation_record`/`cache_state` (both always-default in
/// `run_and_record` itself -- see `types::CacheState`'s own doc comment)
/// with what this scenario actually did, and re-persists the corrected
/// `Run` via `store::write_run` so the written `run.json` matches what
/// this function returns, not the pre-patch version.
#[allow(clippy::too_many_arguments)]
pub fn run_scenario_once(
    scenario: &Scenario,
    runs_root: &Path,
    lock_path: &Path,
    repo_root: &Path,
) -> std::io::Result<Run> {
    let prep_descriptions = run_prepare_steps(&scenario.prepare)?;
    let cache_clears: Vec<String> = prep_descriptions
        .iter()
        .filter(|d| d.starts_with("rm "))
        .cloned()
        .collect();

    let (mut run, _dir) = crate::run_and_record(
        runs_root,
        &scenario.workload_id,
        &scenario.id,
        None,
        lock_path,
        repo_root,
        scenario.root.clone(),
        ProbeLevel::Level1ProcessResource,
        &scenario.observation_roots,
    )?;

    run.preparation_record = PreparationRecord {
        steps: prep_descriptions,
        cache_clears,
    };
    run.cache_state = CacheState {
        notes: vec![format!("{:?}", scenario.cache_state_label)],
    };
    store::write_run(runs_root, &run)?;

    Ok(run)
}

/// Summary statistics over a `Vec<f64>` of retained raw samples -- issue
/// #21's required "sample count, min, median/p50, p90, mean, variance/
/// standard deviation" fields. `p90`/`p50` use nearest-rank on the sorted
/// samples (no interpolation) -- adequate for the small (single/low
/// double-digit) repetition counts this first slice is exercised with;
/// not claimed accurate for large-N statistical work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub sample_count: usize,
    pub samples: Vec<f64>,
    pub min: f64,
    pub p50: f64,
    pub p90: f64,
    pub mean: f64,
    pub stddev: f64,
}

impl Stats {
    pub fn from_samples(mut samples: Vec<f64>) -> Self {
        assert!(
            !samples.is_empty(),
            "Stats::from_samples requires at least one sample"
        );
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let sample_count = samples.len();
        let min = samples[0];
        let p50 = samples[nearest_rank(sample_count, 0.50)];
        let p90 = samples[nearest_rank(sample_count, 0.90)];
        let mean = samples.iter().sum::<f64>() / sample_count as f64;
        let variance = if sample_count > 1 {
            samples.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / (sample_count - 1) as f64
        } else {
            0.0
        };
        let stddev = variance.sqrt();
        Stats {
            sample_count,
            samples,
            min,
            p50,
            p90,
            mean,
            stddev,
        }
    }
}

fn nearest_rank(count: usize, fraction: f64) -> usize {
    let idx = (fraction * count as f64).ceil() as usize;
    idx.saturating_sub(1).min(count - 1)
}

/// A minimal per-Run extract this module actually aggregates across
/// repetitions -- not the whole `Run`, since a `ScenarioReport` must stay
/// small enough to write/read/diff by hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunExtract {
    run_id: String,
    success: bool,
    wall_seconds: f64,
    process_count: usize,
    artifact_created: usize,
    artifact_modified: usize,
    artifact_deleted: usize,
    artifact_unchanged: usize,
}

fn extract(run: &Run) -> RunExtract {
    let wall_seconds = run
        .process_trace
        .processes
        .first()
        .and_then(|p| {
            p.end_elapsed_ns
                .map(|end| (end - p.start_elapsed_ns) as f64 / 1e9)
        })
        .unwrap_or(0.0);
    let process_count = run.process_trace.processes.len();

    let mut created = 0;
    let mut modified = 0;
    let mut deleted = 0;
    let mut unchanged = 0;
    if let Some(delta) = &run.artifact_delta {
        if let Some(records) = delta.get("records").and_then(|r| r.as_array()) {
            for record in records {
                match record.get("state").and_then(|s| s.as_str()) {
                    Some("Created") => created += 1,
                    Some("Modified") => modified += 1,
                    Some("Deleted") => deleted += 1,
                    Some("Unchanged") => unchanged += 1,
                    _ => {}
                }
            }
        }
    }

    RunExtract {
        run_id: run.run_id.clone(),
        success: run.result.as_ref().is_some_and(|r| r.success),
        wall_seconds,
        process_count,
        artifact_created: created,
        artifact_modified: modified,
        artifact_deleted: deleted,
        artifact_unchanged: unchanged,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioReport {
    pub schema_version: String,
    pub scenario_id: String,
    pub workload_id: String,
    pub run_ids: Vec<String>,
    /// One entry per `run_ids`, in order -- issue #21's "a correct final
    /// binary produced by rebuilding everything does not satisfy
    /// incremental-work acceptance criteria" cuts both ways: a *failed*
    /// repetition must never be silently folded into `wall_seconds` as if
    /// it were a normal, fast sample. `run_ids`/`process_counts`/
    /// `artifact_*` below still cover every repetition regardless of
    /// success -- failure evidence is retained, not discarded -- but
    /// `wall_seconds` is computed from successful repetitions only (see
    /// `build_report`).
    pub success: Vec<bool>,
    pub wall_seconds: Stats,
    /// One process count per repetition, in `run_ids` order -- kept
    /// alongside `wall_seconds` (not reduced to a single summary number)
    /// specifically so `compare_reports` can flag "the action count
    /// itself changed" as a distinct signal from "wall time changed",
    /// per issue #21's "regression is not wall time alone" requirement.
    pub process_counts: Vec<usize>,
    pub artifact_created: Vec<usize>,
    pub artifact_modified: Vec<usize>,
    pub artifact_deleted: Vec<usize>,
    pub artifact_unchanged: Vec<usize>,
    /// This report's own comparability identity -- issue #21's acceptance
    /// criterion 7 ("cross-environment comparison is rejected or
    /// explicitly marked non-comparable by default"), a real gap an
    /// external review caught: without recording *any* environment/
    /// toolchain identity here, `compare_reports` had nothing to check
    /// two reports against, and happily produced a numeric verdict for
    /// two entirely different workloads. Taken from the *first*
    /// repetition only -- not cross-validated against the rest of this
    /// same report's own repetitions.
    pub environment_fingerprint: laminaria_fingerprint::EnvironmentFingerprint,
    /// The actually-used toolchain's own digest (`reuse::
    /// toolchain_identity_from_run`) -- `None` when it couldn't be
    /// identified (matches that function's own fail-closed behavior, see
    /// its doc comment), which `compare_reports` treats as "not
    /// comparable," never as "assume it matches."
    pub toolchain_digest_sha256: Option<String>,
}

/// Builds a `ScenarioReport` from `runs`. `wall_seconds` (and its own
/// `Stats::sample_count`) are computed from **successful repetitions
/// only** -- a real bug an external review caught: a scenario whose
/// command fails partway through (a real, deliberately-triggered failure,
/// or an environment regression) was being aggregated as a normal,
/// often-fast sample, making an early failure look like a speedup rather
/// than the invalid measurement it is. Failure evidence itself is not
/// discarded: `run_ids`/`success`/`process_counts`/`artifact_*` still
/// cover every repetition, successful or not, and each failed run's own
/// full evidence remains on disk at `run.json` as always. Returns an
/// error when *every* repetition failed, since there is then no valid
/// wall-time sample to report at all -- not a zero-length `Stats`
/// (`Stats::from_samples` itself refuses an empty input), and not a
/// silently fabricated one either.
fn build_report(
    scenario_id: &str,
    workload_id: &str,
    runs: &[Run],
) -> std::io::Result<ScenarioReport> {
    let extracts: Vec<RunExtract> = runs.iter().map(extract).collect();
    let successful_wall_seconds: Vec<f64> = extracts
        .iter()
        .filter(|e| e.success)
        .map(|e| e.wall_seconds)
        .collect();
    if successful_wall_seconds.is_empty() {
        return Err(std::io::Error::other(format!(
            "scenario {scenario_id:?}: all {} repetition(s) failed -- no valid wall-time sample \
             to report (evidence preserved in each repetition's own run.json: {:?})",
            extracts.len(),
            extracts.iter().map(|e| &e.run_id).collect::<Vec<_>>()
        )));
    }
    let first_run = runs.first().ok_or_else(|| {
        std::io::Error::other(format!("scenario {scenario_id:?}: no repetitions given"))
    })?;
    let is_nim = is_nim_root_program(&first_run.root_command.program);
    let (toolchain_digest_sha256, _) = crate::reuse::toolchain_identity_from_run(first_run, is_nim);
    Ok(ScenarioReport {
        schema_version: SCENARIO_SCHEMA_VERSION.to_string(),
        scenario_id: scenario_id.to_string(),
        workload_id: workload_id.to_string(),
        run_ids: extracts.iter().map(|e| e.run_id.clone()).collect(),
        success: extracts.iter().map(|e| e.success).collect(),
        wall_seconds: Stats::from_samples(successful_wall_seconds),
        process_counts: extracts.iter().map(|e| e.process_count).collect(),
        artifact_created: extracts.iter().map(|e| e.artifact_created).collect(),
        artifact_modified: extracts.iter().map(|e| e.artifact_modified).collect(),
        artifact_deleted: extracts.iter().map(|e| e.artifact_deleted).collect(),
        artifact_unchanged: extracts.iter().map(|e| e.artifact_unchanged).collect(),
        environment_fingerprint: first_run.environment_fingerprint.clone(),
        toolchain_digest_sha256,
    })
}

fn is_nim_root_program(program: &str) -> bool {
    Path::new(program).file_stem().and_then(|s| s.to_str()) == Some("nim")
}

/// Runs `scenario` `repeat_count` times (each a full `run_scenario_once`,
/// preparation included), retaining every raw sample -- issue #21's "store
/// every raw sample" requirement -- and returns the aggregated
/// `ScenarioReport`. Every repetition's own `run.json` is already on disk
/// (via `run_scenario_once`'s call to `store::write_run`) before this
/// function returns, so `regenerate_report_from_disk` can rebuild the same
/// report later without rerunning anything.
pub fn run_scenario_repeated(
    scenario: &Scenario,
    repeat_count: usize,
    runs_root: &Path,
    lock_path: &Path,
    repo_root: &Path,
) -> std::io::Result<ScenarioReport> {
    assert!(repeat_count > 0, "repeat_count must be at least 1");
    let mut runs = Vec::with_capacity(repeat_count);
    for _ in 0..repeat_count {
        runs.push(run_scenario_once(
            scenario, runs_root, lock_path, repo_root,
        )?);
    }
    build_report(&scenario.id, &scenario.workload_id, &runs)
}

/// Rebuilds a `ScenarioReport` purely from already-written `run.json`
/// files, without rerunning anything -- issue #21's "all raw samples are
/// retained and reports are regenerable" acceptance criterion, exercised
/// as a real disk round trip, not just claimed.
pub fn regenerate_report_from_disk(
    runs_root: &Path,
    scenario_id: &str,
    run_ids: &[String],
) -> std::io::Result<ScenarioReport> {
    let mut runs = Vec::with_capacity(run_ids.len());
    for run_id in run_ids {
        runs.push(store::read_run(runs_root, run_id)?);
    }
    let workload_id = runs
        .first()
        .map(|r| r.workload_id.clone())
        .unwrap_or_default();
    build_report(scenario_id, &workload_id, &runs)
}

/// How many standard deviations of separation between two reports' mean
/// wall time counts as "above the noise floor" -- a fixed multiplier, not
/// a universal *percentage* threshold (issue #21 explicitly warns against
/// the latter, since the same percentage means different things on a
/// noisy vs. quiet environment; this multiplier is applied to each
/// comparison's *own* measured spread instead).
const NOISE_FLOOR_STDDEV_MULTIPLIER: f64 = 2.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WallTimeVerdict {
    AboveNoise,
    WithinNoise,
    BelowNoise,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub baseline_scenario_id: String,
    pub candidate_scenario_id: String,
    pub wall_seconds_relative_diff: f64,
    pub wall_time_verdict: WallTimeVerdict,
    pub process_count_changed: bool,
    pub artifact_profile_changed: bool,
    /// Set when `process_count_changed` or `artifact_profile_changed` --
    /// issue #21's "a result is not an improvement merely because wall
    /// time fell" requirement: a caller must not read `wall_time_verdict`
    /// alone as a valid regression/improvement result when this is
    /// non-empty, since the two runs executed structurally different work.
    pub confounding_notes: Vec<String>,
}

/// The min and max of a `&[usize]` slice, as a single comparable value --
/// used instead of a bare minimum (see `compare_reports`'s own doc
/// comment for why a bare minimum was a real bug).
fn range(values: &[usize]) -> (usize, usize) {
    (
        values.iter().min().copied().unwrap_or(0),
        values.iter().max().copied().unwrap_or(0),
    )
}

/// Compares `candidate` against `baseline`, using `baseline`'s own
/// measured `stddev` as the noise floor (see
/// `NOISE_FLOOR_STDDEV_MULTIPLIER`).
///
/// **Rejects the comparison outright** (`Err`, before computing any
/// numeric verdict at all) unless `workload_id` matches, the two reports'
/// `EnvironmentFingerprint`s are comparable
/// (`laminaria_fingerprint::comparability::environments_comparable`,
/// reused directly), and both sides have the *same, resolved* toolchain
/// digest (`reuse::toolchain_identity_from_run`'s fail-closed rule
/// applies here too: an unresolved digest on either side is never treated
/// as a match). A real bug an external review caught: comparing two
/// reports from different workloads (with no environment/toolchain
/// identity recorded on either at all, prior to this fix) produced an
/// ordinary-looking `BelowNoise` verdict with zero `confounding_notes` --
/// exactly the kind of not-actually-comparable result issue #21's
/// acceptance criterion 7 exists to prevent.
///
/// Once eligible, also checks whether the two reports' process counts or
/// artifact create/modify/delete profiles actually match -- using each
/// report's own `(min, max)` **range**, not a bare minimum (another real
/// bug: a baseline of `[1]` and a candidate of `[1, 50, 50]` share the
/// same minimum, silently hiding that two of the three candidate
/// repetitions did far more work). If the ranges don't match, the two
/// scenarios did structurally different work, and `wall_time_verdict`
/// alone must not be read as a valid regression/improvement result.
pub fn compare_reports(
    baseline: &ScenarioReport,
    candidate: &ScenarioReport,
) -> Result<Comparison, String> {
    if baseline.workload_id != candidate.workload_id {
        return Err(format!(
            "not comparable: workload_id differs (baseline={:?}, candidate={:?})",
            baseline.workload_id, candidate.workload_id
        ));
    }
    if let Err(reasons) = laminaria_fingerprint::comparability::environments_comparable(
        &baseline.environment_fingerprint,
        &candidate.environment_fingerprint,
    ) {
        return Err(format!(
            "not comparable: environment differs: {}",
            reasons.join("; ")
        ));
    }
    match (
        &baseline.toolchain_digest_sha256,
        &candidate.toolchain_digest_sha256,
    ) {
        (Some(a), Some(b)) if a == b => {}
        (a, b) => {
            return Err(format!(
                "not comparable: toolchain digest differs or unresolved (baseline={a:?}, \
                 candidate={b:?}) -- an unresolved digest on either side is never treated as a \
                 match"
            ));
        }
    }

    let diff = candidate.wall_seconds.mean - baseline.wall_seconds.mean;
    let relative_diff = if baseline.wall_seconds.mean != 0.0 {
        diff / baseline.wall_seconds.mean
    } else {
        0.0
    };
    let noise_floor = NOISE_FLOOR_STDDEV_MULTIPLIER * baseline.wall_seconds.stddev;
    let wall_time_verdict = if diff.abs() <= noise_floor {
        WallTimeVerdict::WithinNoise
    } else if diff > 0.0 {
        WallTimeVerdict::AboveNoise
    } else {
        WallTimeVerdict::BelowNoise
    };

    let mut confounding_notes = Vec::new();

    let baseline_process_range = range(&baseline.process_counts);
    let candidate_process_range = range(&candidate.process_counts);
    let process_count_changed = baseline_process_range != candidate_process_range;
    if process_count_changed {
        confounding_notes.push(format!(
            "process count range differs (baseline={baseline_process_range:?}, \
             candidate={candidate_process_range:?}) -- more or fewer compiler/backend actions \
             were executed in at least one repetition, so a wall-time difference here reflects a \
             different amount of work, not purely a speed change"
        ));
    }

    let baseline_artifact_profile = (
        range(&baseline.artifact_created),
        range(&baseline.artifact_modified),
        range(&baseline.artifact_deleted),
    );
    let candidate_artifact_profile = (
        range(&candidate.artifact_created),
        range(&candidate.artifact_modified),
        range(&candidate.artifact_deleted),
    );
    let artifact_profile_changed = baseline_artifact_profile != candidate_artifact_profile;
    if artifact_profile_changed {
        confounding_notes.push(format!(
            "artifact create/modify/delete range differs (baseline={baseline_artifact_profile:?}, \
             candidate={candidate_artifact_profile:?}) -- the two scenarios did not consistently \
             produce/change the same set of artifacts across repetitions"
        ));
    }

    Ok(Comparison {
        baseline_scenario_id: baseline.scenario_id.clone(),
        candidate_scenario_id: candidate.scenario_id.clone(),
        wall_seconds_relative_diff: relative_diff,
        wall_time_verdict,
        process_count_changed,
        artifact_profile_changed,
        confounding_notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ExitStatusRecord, ProbeLevel, ProcessRecord, ProcessTrace, ResourceUsage, RunResult,
        SCHEMA_VERSION,
    };
    use std::collections::BTreeMap;

    fn sample_run(run_id: &str, success: bool, wall_seconds: f64) -> Run {
        let end_elapsed_ns = (wall_seconds * 1e9) as u64;
        Run {
            run_id: run_id.to_string(),
            schema_version: SCHEMA_VERSION.to_string(),
            workload_id: "test-workload".to_string(),
            scenario_id: "test-scenario".to_string(),
            requested_artifact: None,
            environment_fingerprint: laminaria_fingerprint::EnvironmentFingerprint {
                schema_version: "0.1.0".to_string(),
                captured_at_unix: 0,
                os: "test-os".to_string(),
                os_version: None,
                kernel: None,
                architecture: "test-arch".to_string(),
                cpu_model: None,
                cpu_physical_cores: None,
                cpu_logical_cores: None,
                memory_bytes: None,
                filesystem_type: None,
                environment_class: "unknown".to_string(),
                repository: laminaria_fingerprint::RepositoryState {
                    commit: None,
                    dirty: None,
                },
                sdk_path: None,
                measurement_harness: "test".to_string(),
                architecture_notice: None,
                self_process_translation_notice: None,
                path_toolchain_shadow: None,
                allowed_environment_variables: BTreeMap::new(),
                unobserved_fields: Vec::new(),
            },
            requested_toolchain_selector: None,
            resolved_toolchain_fingerprint: None,
            preparation_record: crate::types::PreparationRecord::default(),
            cache_state: crate::types::CacheState::default(),
            root_command: RootCommand {
                program: "true".to_string(),
                args: Vec::new(),
                cwd: None,
                env_overrides: BTreeMap::new(),
            },
            run_started_at_unix_ns: 0,
            run_ended_at_unix_ns: Some(end_elapsed_ns as u128),
            result: Some(RunResult {
                success,
                root_exit_status: ExitStatusRecord {
                    success,
                    code: Some(if success { 0 } else { 1 }),
                    signal: None,
                },
            }),
            process_trace: ProcessTrace {
                processes: vec![ProcessRecord {
                    pid: Some(1),
                    parent_pid: Some(1),
                    executable: None,
                    argv: vec!["true".to_string()],
                    cwd: None,
                    start_elapsed_ns: 0,
                    end_elapsed_ns: Some(end_elapsed_ns),
                    exit_status: Some(ExitStatusRecord {
                        success,
                        code: Some(if success { 0 } else { 1 }),
                        signal: None,
                    }),
                    resource_usage: ResourceUsage::default(),
                    probe_level: ProbeLevel::Level1ProcessResource,
                    coverage_note: "test".to_string(),
                }],
                known_gaps: Vec::new(),
            },
            compiler_telemetry: None,
            artifact_delta: None,
            measurement_overhead: None,
        }
    }

    /// The exact bug an external review caught: a failed repetition's own
    /// (often short) wall time must never be silently averaged in as if
    /// it were a normal, fast, successful sample.
    #[test]
    fn a_failed_repetition_is_excluded_from_wall_seconds_but_its_evidence_is_kept() {
        let runs = vec![
            sample_run("run-fast-fail", false, 0.01),
            sample_run("run-ok-1", true, 10.0),
            sample_run("run-ok-2", true, 10.2),
        ];
        let report = build_report("edit", "test-workload", &runs).unwrap();

        assert_eq!(
            report.run_ids.len(),
            3,
            "every repetition's evidence must be kept"
        );
        assert_eq!(report.success, vec![false, true, true]);
        assert_eq!(
            report.wall_seconds.sample_count, 2,
            "only the two successful repetitions may contribute to the wall-time statistics"
        );
        assert!(
            report.wall_seconds.min >= 9.0,
            "the failed run's fast 0.01s must not pull the reported minimum down, got {}",
            report.wall_seconds.min
        );
    }

    #[test]
    fn a_scenario_where_every_repetition_failed_reports_no_wall_time_sample() {
        let runs = vec![
            sample_run("run-fail-1", false, 0.01),
            sample_run("run-fail-2", false, 0.02),
        ];
        let result = build_report("edit", "test-workload", &runs);
        assert!(
            result.is_err(),
            "an all-failed scenario must not produce a ScenarioReport with a fabricated or \
             empty wall_seconds Stats"
        );
    }

    #[test]
    fn stats_computes_min_p50_p90_mean_stddev_from_a_small_sample() {
        let stats = Stats::from_samples(vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(stats.sample_count, 5);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.p50, 3.0);
        assert_eq!(stats.p90, 5.0);
        assert_eq!(stats.mean, 3.0);
        assert!((stats.stddev - 1.5811).abs() < 0.001);
    }

    #[test]
    fn stats_handles_a_single_sample_without_dividing_by_zero() {
        let stats = Stats::from_samples(vec![2.0]);
        assert_eq!(stats.sample_count, 1);
        assert_eq!(stats.stddev, 0.0);
        assert_eq!(stats.mean, 2.0);
    }

    fn sample_environment_fingerprint() -> laminaria_fingerprint::EnvironmentFingerprint {
        laminaria_fingerprint::EnvironmentFingerprint {
            schema_version: "0.1.0".to_string(),
            captured_at_unix: 0,
            os: "test-os".to_string(),
            os_version: None,
            kernel: None,
            architecture: "test-arch".to_string(),
            cpu_model: None,
            cpu_physical_cores: None,
            cpu_logical_cores: None,
            memory_bytes: None,
            filesystem_type: None,
            environment_class: "unknown".to_string(),
            repository: laminaria_fingerprint::RepositoryState {
                commit: None,
                dirty: None,
            },
            sdk_path: None,
            measurement_harness: "test".to_string(),
            architecture_notice: None,
            self_process_translation_notice: None,
            path_toolchain_shadow: None,
            allowed_environment_variables: BTreeMap::new(),
            unobserved_fields: Vec::new(),
        }
    }

    /// Two reports built with this same helper are, by default, eligible
    /// for comparison (same workload/environment/toolchain) -- tests that
    /// specifically exercise `compare_reports`'s eligibility rejection
    /// override `workload_id`/`environment_fingerprint`/
    /// `toolchain_digest_sha256` explicitly instead.
    fn report(
        scenario_id: &str,
        wall_samples: Vec<f64>,
        process_counts: Vec<usize>,
    ) -> ScenarioReport {
        ScenarioReport {
            schema_version: SCENARIO_SCHEMA_VERSION.to_string(),
            scenario_id: scenario_id.to_string(),
            workload_id: "test-workload".to_string(),
            run_ids: (0..wall_samples.len())
                .map(|i| format!("run-{i}"))
                .collect(),
            success: vec![true; wall_samples.len()],
            wall_seconds: Stats::from_samples(wall_samples),
            process_counts,
            artifact_created: vec![0],
            artifact_modified: vec![0],
            artifact_deleted: vec![0],
            artifact_unchanged: vec![0],
            environment_fingerprint: sample_environment_fingerprint(),
            toolchain_digest_sha256: Some("test-toolchain-digest".to_string()),
        }
    }

    #[test]
    fn a_small_timing_difference_within_the_noise_floor_is_not_flagged_as_a_regression() {
        // Baseline has real jitter (stddev > 0); candidate's mean is only
        // slightly higher than baseline's, well inside 2 stddev.
        let baseline = report("baseline", vec![1.0, 1.1, 0.9, 1.05, 0.95], vec![4]);
        let candidate = report("candidate", vec![1.02, 1.08, 1.0], vec![4]);

        let comparison = compare_reports(&baseline, &candidate).unwrap();

        assert_eq!(comparison.wall_time_verdict, WallTimeVerdict::WithinNoise);
        assert!(comparison.confounding_notes.is_empty());
    }

    #[test]
    fn a_large_timing_difference_beyond_the_noise_floor_is_flagged_above_noise() {
        let baseline = report("baseline", vec![1.0, 1.02, 0.98, 1.01, 0.99], vec![4]);
        let candidate = report("candidate", vec![5.0, 5.1, 4.9], vec![4]);

        let comparison = compare_reports(&baseline, &candidate).unwrap();

        assert_eq!(comparison.wall_time_verdict, WallTimeVerdict::AboveNoise);
        assert!(comparison.wall_seconds_relative_diff > 3.0);
    }

    #[test]
    fn a_process_count_change_is_flagged_even_when_wall_time_looks_like_an_improvement() {
        // The exact scenario issue #21's acceptance criteria warn about:
        // a deliberately path-divergent run (fewer processes -- e.g. a
        // fallback path skipped real compilation work) must not be
        // silently read as a valid "improvement" from its lower wall time
        // alone.
        let baseline = report("baseline", vec![10.0, 10.1, 9.9], vec![6]);
        let candidate = report("candidate", vec![1.0, 1.1, 0.9], vec![1]);

        let comparison = compare_reports(&baseline, &candidate).unwrap();

        assert_eq!(comparison.wall_time_verdict, WallTimeVerdict::BelowNoise);
        assert!(comparison.process_count_changed);
        assert!(!comparison.confounding_notes.is_empty());
    }

    /// The exact bug an external review caught: a baseline whose every
    /// repetition used exactly 1 process shares a *minimum* of 1 with a
    /// candidate where two of three repetitions spiked to 50 -- a
    /// min-only comparison would miss that entirely.
    #[test]
    fn a_process_count_spike_in_some_but_not_all_repetitions_is_still_flagged() {
        let baseline = report("baseline", vec![1.0, 1.0, 1.0], vec![1]);
        let candidate = report("candidate", vec![1.0, 1.0, 1.0], vec![1, 50, 50]);

        let comparison = compare_reports(&baseline, &candidate).unwrap();

        assert!(
            comparison.process_count_changed,
            "a min-only comparison would miss this: both sides share a minimum of 1"
        );
        assert!(!comparison.confounding_notes.is_empty());
    }

    /// The exact bug an external review caught: comparing two reports
    /// from genuinely different workloads (or with no recorded
    /// environment/toolchain identity at all) must be rejected outright,
    /// not silently reduced to a wall-time-only verdict.
    #[test]
    fn comparing_different_workloads_is_rejected_before_any_numeric_verdict() {
        let mut baseline = report("baseline", vec![10.0, 10.1, 9.9], vec![4]);
        baseline.workload_id = "rust-heavy-workspace".to_string();
        let mut candidate = report("candidate", vec![5.0, 5.1, 4.9], vec![4]);
        candidate.workload_id = "nim-heavy-workspace".to_string();

        let result = compare_reports(&baseline, &candidate);

        assert!(
            result.is_err(),
            "comparing two different workloads must be rejected, not silently scored as \
             below_noise with zero confounding_notes"
        );
    }

    #[test]
    fn comparing_reports_with_an_unresolved_toolchain_on_either_side_is_rejected() {
        let mut baseline = report("baseline", vec![10.0, 10.1, 9.9], vec![4]);
        baseline.toolchain_digest_sha256 = None;
        let candidate = report("candidate", vec![5.0, 5.1, 4.9], vec![4]);

        let result = compare_reports(&baseline, &candidate);

        assert!(
            result.is_err(),
            "an unresolved toolchain digest on either side must never be treated as a match"
        );
    }

    #[test]
    fn comparing_reports_with_different_toolchain_digests_is_rejected() {
        let baseline = report("baseline", vec![10.0, 10.1, 9.9], vec![4]);
        let mut candidate = report("candidate", vec![5.0, 5.1, 4.9], vec![4]);
        candidate.toolchain_digest_sha256 = Some("a-different-toolchain-digest".to_string());

        let result = compare_reports(&baseline, &candidate);

        assert!(result.is_err());
    }

    #[test]
    fn comparing_reports_from_different_environments_is_rejected() {
        let baseline = report("baseline", vec![10.0, 10.1, 9.9], vec![4]);
        let mut candidate = report("candidate", vec![5.0, 5.1, 4.9], vec![4]);
        candidate.environment_fingerprint.os = "a-different-os".to_string();

        let result = compare_reports(&baseline, &candidate);

        assert!(result.is_err());
    }

    #[test]
    fn an_artifact_profile_change_is_flagged_independent_of_the_timing_verdict() {
        let baseline = report("baseline", vec![1.0, 1.0, 1.0], vec![4]);
        let mut candidate = report("candidate", vec![1.0, 1.0, 1.0], vec![4]);
        candidate.artifact_created = vec![5];

        let comparison = compare_reports(&baseline, &candidate).unwrap();

        assert_eq!(comparison.wall_time_verdict, WallTimeVerdict::WithinNoise);
        assert!(comparison.artifact_profile_changed);
        assert!(!comparison.confounding_notes.is_empty());
    }
}

//! Issue #50: a real, subprocess-measured resource baseline for the P0
//! cross-layer pruning hypothesis ("does a fact obtained only from Rust
//! source semantics let LAMINARIA safely omit work before Cargo/rustc/
//! LLVM ever run").
//!
//! **What this is not a repeat of**: `d1b2_new_fixtures::run_m8_small`
//! (issue #28) already proves that *Cargo's own* `--bin fixture-bin`
//! unit-graph resolution avoids compiling `unused-pkg-*` in this same
//! fixture -- that is Cargo's own resolver doing its job, not a LAMINARIA
//! result, and the issue #50 P0 text is explicit that reproducing a
//! resolver/cache/link capability that already has a strong prior
//! implementation is not evidence for LAMINARIA's own hypothesis.
//!
//! **What this measures instead**: `laminaria_ir::rust_dependency_discover`
//! (an owned `syn` AST walk, no Cargo invocation) and
//! `laminaria_plan::rust_cross_layer::plan_rust_cross_layer` (a pure,
//! in-process selection kernel, issue #50) together decide which package
//! candidates are needed, using only Rust source facts -- never calling
//! Cargo's resolver. This module then spends that owned decision on a
//! real `cargo build` invocation scoped to exactly the selected packages
//! (`-p <pkg>` per `plan.selected_packages`), and compares it, via
//! `laminaria_run::tracer`'s real `wait4`-based Level 1 resource
//! accounting and Cargo's own real `--message-format=json` Level 2
//! telemetry, against an eager baseline (`cargo build --workspace`) that
//! is handed every locked candidate with no pruning at all.
//!
//! **Honest limits, not silently assumed away**: this exercises exactly
//! one fixture (`fixtures/many-unrequested-targets/small`), whose
//! `fixture-bin/src/main.rs` deliberately calls `used_core::value()`/
//! `used_util::value()` via fully-qualified two-segment paths --
//! `rust_dependency_discover`'s own `visit_expr_call`-only implementation
//! does not yet resolve `use`-imported short names, aliased imports, or
//! macro-embedded references (see that module's own doc comment); a
//! source file written in the far more common `use x::y; y()` idiom would
//! not be detected, and this baseline would then *not* be evidence of
//! safe pruning for that code. This module's `feedback_pruning_is_supported_by_raw_evidence`
//! is scoped to exactly what it checks: that the raw resource evidence
//! backs the specific plan this fixture's source actually produced, not
//! a general claim about arbitrary Rust source.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use laminaria_ir::rust_dependency_discover::discover_external_crate_references;
use laminaria_plan::rust_cross_layer::{
    plan_rust_cross_layer, RustCrossLayerInput, RustCrossLayerPlan,
};
use laminaria_run::scenario::{self, CacheStateLabel, Comparison, Scenario, ScenarioReport, Stats};
use laminaria_run::types::{CargoCompilerTelemetry, RootCommand, Run};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "0.1.0";
pub const WORKLOAD_ID: &str = "issue50-rust-cross-layer-pruning@many-unrequested-targets-small-v1";
const EAGER_SCENARIO_ID: &str = "eager-full-workspace";
const FEEDBACK_SCENARIO_ID: &str = "feedback-pruned";

fn fixture_root() -> PathBuf {
    crate::planner_binary::repo_root().join("fixtures/many-unrequested-targets/small")
}

/// Cargo.lock package names -- mirrors
/// `crates/laminaria-run/tests/rust_cross_layer_test.rs`'s own helper of
/// the same name (kept independently here, same reasoning `m3_baseline.rs`/
/// `m8_baseline.rs` each keep their own small helpers rather than share a
/// speculative common module for a two-caller need).
fn locked_packages(lock: &Path) -> Result<BTreeSet<String>, String> {
    let text = std::fs::read_to_string(lock)
        .map_err(|e| format!("failed to read {}: {e}", lock.display()))?;
    Ok(text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("name = \"")?.strip_suffix('"'))
        .map(str::to_string)
        .collect())
}

fn declared_dependencies(manifest: &Path) -> Result<BTreeSet<String>, String> {
    let content = std::fs::read_to_string(manifest)
        .map_err(|e| format!("failed to read {}: {e}", manifest.display()))?;
    let mut in_dependencies = false;
    Ok(content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('[') {
                in_dependencies = line == "[dependencies]";
                return None;
            }
            in_dependencies
                .then(|| line.split_once('=').map(|(name, _)| name.trim().to_string()))
                .flatten()
        })
        .collect())
}

/// Runs the owned discovery + planning path for real: reads
/// `fixture-bin/src/main.rs` from disk, walks it with
/// `discover_external_crate_references` (a `syn` AST visit, never a
/// Cargo invocation), and feeds the result into `plan_rust_cross_layer`
/// alongside the fixture's real `Cargo.lock`/`Cargo.toml`.
fn compute_plan() -> Result<RustCrossLayerPlan, String> {
    let root = fixture_root();
    let source_path = root.join("crates/fixture-bin/src/main.rs");
    let source = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("failed to read {}: {e}", source_path.display()))?;
    let semantic_references = discover_external_crate_references(&source)
        .map_err(|e| format!("failed to parse {}: {e}", source_path.display()))?;

    plan_rust_cross_layer(&RustCrossLayerInput {
        requested_artifact: "fixture-bin".to_string(),
        entry_package: "fixture-bin".to_string(),
        package_candidates: locked_packages(&root.join("Cargo.lock"))?,
        declared_dependency_packages: declared_dependencies(
            &root.join("crates/fixture-bin/Cargo.toml"),
        )?,
        semantic_references,
    })
    .map_err(|e| format!("plan_rust_cross_layer rejected the fixture's own real source: {e:?}"))
}

fn cargo_scenario(id: &str, target_dir: &Path, manifest_path: &Path, package_args: &[String]) -> Scenario {
    let mut args = vec![
        "build".to_string(),
        "--manifest-path".to_string(),
        manifest_path.display().to_string(),
        "--target-dir".to_string(),
        target_dir.display().to_string(),
    ];
    args.extend(package_args.iter().cloned());
    Scenario {
        id: id.to_string(),
        workload_id: WORKLOAD_ID.to_string(),
        prepare: vec![RootCommand {
            program: "rm".to_string(),
            args: vec!["-rf".to_string(), target_dir.display().to_string()],
            cwd: None,
            env_overrides: Default::default(),
        }],
        root: RootCommand {
            program: "cargo".to_string(),
            args,
            cwd: None,
            env_overrides: Default::default(),
        },
        observation_roots: vec![target_dir.to_path_buf()],
        // Every repetition clears target_dir first (this scenario's own
        // `prepare`), so every repetition of both scenarios is a cold
        // build -- comparing a warm eager build against a cold feedback
        // build (or vice versa) would confound the pruning signal with an
        // unrelated cache-state difference.
        cache_state_label: CacheStateLabel::Cold,
    }
}

/// `--workspace`: every locked candidate is handed to Cargo, with no
/// pruning at all -- the P0 text's own "no early pruning" baseline.
fn eager_scenario(target_dir: &Path, manifest_path: &Path) -> Scenario {
    cargo_scenario(
        EAGER_SCENARIO_ID,
        target_dir,
        manifest_path,
        &["--workspace".to_string()],
    )
}

/// `-p <pkg>` once per `plan.selected_packages` -- Cargo is only ever
/// told about the packages LAMINARIA's own owned planner already decided
/// are needed; the pruned candidates are never named on this command
/// line at all.
fn feedback_scenario(target_dir: &Path, manifest_path: &Path, selected: &BTreeSet<String>) -> Scenario {
    let mut package_args = Vec::with_capacity(selected.len() * 2);
    for package in selected {
        package_args.push("-p".to_string());
        package_args.push(package.clone());
    }
    cargo_scenario(FEEDBACK_SCENARIO_ID, target_dir, manifest_path, &package_args)
}

/// Cargo's real `--message-format=json` `package_id` field, verified
/// directly against this repo's own pinned toolchain (`cargo 1.97.1`,
/// `toolchains.lock.toml`) rather than assumed from older documentation:
/// a `PackageIdSpec` URL, `path+file:///abs/path/to/crates/<dir>#<version>`
/// for every package in this fixture (all path dependencies, no registry
/// source). The package directory name equals the Cargo package name for
/// every crate in this fixture (checked directly) -- not a general
/// parser for arbitrary `package_id` shapes, only for this fixture's own
/// verified format. Returns `None` (never panics) on a shape this parser
/// doesn't recognize, so a future Cargo format change shows up as a
/// missing name rather than a wrong one.
fn package_name_from_package_id(package_id: &str) -> Option<String> {
    let without_version = package_id.split('#').next()?;
    let name = without_version.trim_end_matches('/').rsplit('/').next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// The set of package names Cargo's own Level 2 telemetry says this run
/// actually compiled (`fresh == false`, i.e. not merely reported as
/// already up to date) -- read back from `Run.compiler_telemetry`, the
/// same real JSON Cargo wrote to `stdout.log` during the run, not
/// inferred from wall time or process counts.
fn extract_compiled_package_names(run: &Run) -> BTreeSet<String> {
    let Some(value) = &run.compiler_telemetry else {
        return BTreeSet::new();
    };
    let Ok(telemetry) = serde_json::from_value::<CargoCompilerTelemetry>(value.clone()) else {
        return BTreeSet::new();
    };
    telemetry
        .artifacts
        .iter()
        .filter(|artifact| !artifact.fresh)
        .filter_map(|artifact| package_name_from_package_id(&artifact.package_id))
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepetitionResourceRecord {
    pub run_id: String,
    pub success: bool,
    /// `wait4`-derived (`laminaria_run::tracer`), cumulative over the
    /// `cargo build` subtree (Cargo itself, every `rustc`/linker
    /// invocation it spawned and reaped).
    pub user_cpu_seconds: Option<f64>,
    pub system_cpu_seconds: Option<f64>,
    pub peak_rss_bytes: Option<u64>,
    pub compiled_package_names: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResourceOutcome {
    pub scenario_id: String,
    pub repetitions: Vec<RepetitionResourceRecord>,
    pub scenario_report: ScenarioReport,
    /// `user_cpu_seconds + system_cpu_seconds` per successful repetition
    /// -- `ScenarioReport` itself only aggregates `wall_seconds`, so this
    /// is computed here directly from each `Run`'s own
    /// `process_trace.processes[0].resource_usage`, the same raw field
    /// `laminaria_run::tracer` populates from `wait4`.
    pub cpu_seconds_stats: Option<Stats>,
    pub peak_rss_bytes_stats: Option<Stats>,
}

fn build_resource_outcome(
    scenario: &Scenario,
    runs: &[Run],
    runs_root: &Path,
) -> Result<ScenarioResourceOutcome, String> {
    let run_ids: Vec<String> = runs.iter().map(|r| r.run_id.clone()).collect();
    let scenario_report = scenario::regenerate_report_from_disk(runs_root, &scenario.id, &run_ids)
        .map_err(|e| format!("failed to build a ScenarioReport for {:?}: {e}", scenario.id))?;

    let mut repetitions = Vec::with_capacity(runs.len());
    let mut cpu_samples = Vec::new();
    let mut rss_samples = Vec::new();
    for run in runs {
        let success = run.result.as_ref().is_some_and(|r| r.success);
        let root_process = run.process_trace.processes.first();
        let user_cpu_seconds = root_process.and_then(|p| p.resource_usage.user_cpu_seconds);
        let system_cpu_seconds = root_process.and_then(|p| p.resource_usage.system_cpu_seconds);
        let peak_rss_bytes = root_process.and_then(|p| p.resource_usage.peak_rss_bytes);
        if success {
            if let (Some(user), Some(system)) = (user_cpu_seconds, system_cpu_seconds) {
                cpu_samples.push(user + system);
            }
            if let Some(rss) = peak_rss_bytes {
                rss_samples.push(rss as f64);
            }
        }
        repetitions.push(RepetitionResourceRecord {
            run_id: run.run_id.clone(),
            success,
            user_cpu_seconds,
            system_cpu_seconds,
            peak_rss_bytes,
            compiled_package_names: extract_compiled_package_names(run),
        });
    }

    Ok(ScenarioResourceOutcome {
        scenario_id: scenario.id.clone(),
        repetitions,
        scenario_report,
        cpu_seconds_stats: (!cpu_samples.is_empty()).then(|| Stats::from_samples(cpu_samples)),
        peak_rss_bytes_stats: (!rss_samples.is_empty()).then(|| Stats::from_samples(rss_samples)),
    })
}

fn run_scenario_n_times(
    scenario: &Scenario,
    repetitions: usize,
    runs_root: &Path,
    lock_path: &Path,
    repo_root: &Path,
) -> Result<Vec<Run>, String> {
    let mut runs = Vec::with_capacity(repetitions);
    for _ in 0..repetitions {
        let run = scenario::run_scenario_once(scenario, runs_root, lock_path, repo_root)
            .map_err(|e| format!("scenario {:?} repetition failed: {e}", scenario.id))?;
        runs.push(run);
    }
    Ok(runs)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustCrossLayerBaselineReport {
    pub schema_version: String,
    pub workload_id: String,
    pub runs_root: PathBuf,
    /// `plan_rust_cross_layer`'s own real output over the fixture's real
    /// source/lock/manifest -- the owned decision this module then spends
    /// on the `feedback` scenario's `-p` arguments.
    pub plan_selected_packages: BTreeSet<String>,
    pub plan_pruned_packages: BTreeSet<String>,
    pub eager: ScenarioResourceOutcome,
    pub feedback: ScenarioResourceOutcome,
    /// `Err` whenever the two reports aren't eligible for
    /// `laminaria_run::scenario::compare_reports`'s ordinary noise-floor
    /// wall-time verdict -- expected here, not a failure: eager and
    /// feedback deliberately compile a different set of packages, so
    /// `compare_reports` itself is expected to flag
    /// `artifact_profile_changed`/`process_count_changed` rather than
    /// hand back a "same work, different speed" verdict. Kept as raw
    /// evidence either way, not discarded.
    pub wall_time_comparison: Result<Comparison, String>,
    /// Whether the eager (`--workspace`) build's own real Level 2
    /// telemetry shows it actually compiled at least one package
    /// `plan_pruned_packages` names -- proof the eager side is not a
    /// strawman that happened to skip that work anyway.
    pub eager_build_compiled_a_pruned_package: bool,
    /// Whether the feedback (`-p <selected>`) build's own real Level 2
    /// telemetry shows it never compiled any package
    /// `plan_pruned_packages` names, in any repetition.
    pub feedback_build_never_compiled_a_pruned_package: bool,
}

impl RustCrossLayerBaselineReport {
    /// The P0 experiment's own pass criterion, computed entirely from
    /// this already-recorded, disk-backed evidence: the owned plan
    /// actually pruned something, the eager baseline genuinely would have
    /// compiled it (so the comparison isn't vacuous), and honoring the
    /// plan's own selection kept the real `cargo build` invocation from
    /// ever compiling it. This is a correctness/causality check, not a
    /// speed claim -- see `wall_time_comparison`'s own doc comment for
    /// why a clean noise-floor speed verdict is not expected here.
    pub fn feedback_pruning_is_supported_by_raw_evidence(&self) -> bool {
        !self.plan_pruned_packages.is_empty()
            && self.eager_build_compiled_a_pruned_package
            && self.feedback_build_never_compiled_a_pruned_package
    }
}

/// Runs the full issue #50 P0 baseline for real: resolves the owned plan
/// from the fixture's actual source/lock/manifest, then runs `warmup`
/// discarded repetitions followed by `repetitions` measured repetitions
/// of both the eager and feedback `cargo build` scenarios, each a cold
/// build (`prepare` clears its own `--target-dir` first). Every
/// repetition is persisted as a real `Run` under `runs/<run_id>/`
/// (`scenario::run_scenario_once`'s own `store::write_run` call) before
/// this function returns, so `regenerate` can rebuild the same report
/// later without rerunning anything.
pub fn run(warmup: usize, repetitions: usize) -> Result<RustCrossLayerBaselineReport, String> {
    assert!(repetitions > 0, "repetitions must be at least 1");

    let plan = compute_plan()?;
    let repo_root = crate::planner_binary::repo_root();
    let lock_path = repo_root.join("toolchains.lock.toml");
    let fixture = fixture_root();
    let manifest_path = fixture.join("Cargo.toml");
    let runs_root = repo_root.join("runs");

    // Both under `target/`, not `target-eager`/`target-feedback` siblings
    // of the fixture root -- `.gitignore`'s bare `target` pattern only
    // matches a directory literally named `target` at any depth, so a
    // differently-named sibling would show up as untracked litter in
    // `git status`.
    let eager_target_dir = fixture.join("target/issue50-eager");
    let feedback_target_dir = fixture.join("target/issue50-feedback");

    let eager = eager_scenario(&eager_target_dir, &manifest_path);
    let feedback = feedback_scenario(&feedback_target_dir, &manifest_path, &plan.selected_packages);

    for _ in 0..warmup {
        scenario::run_scenario_once(&eager, &runs_root, &lock_path, &repo_root)
            .map_err(|e| format!("eager warmup repetition failed: {e}"))?;
        scenario::run_scenario_once(&feedback, &runs_root, &lock_path, &repo_root)
            .map_err(|e| format!("feedback warmup repetition failed: {e}"))?;
    }

    let eager_runs = run_scenario_n_times(&eager, repetitions, &runs_root, &lock_path, &repo_root)?;
    let feedback_runs =
        run_scenario_n_times(&feedback, repetitions, &runs_root, &lock_path, &repo_root)?;

    build_report(plan, &runs_root, &eager, &eager_runs, &feedback, &feedback_runs)
}

fn build_report(
    plan: RustCrossLayerPlan,
    runs_root: &Path,
    eager_scenario: &Scenario,
    eager_runs: &[Run],
    feedback_scenario: &Scenario,
    feedback_runs: &[Run],
) -> Result<RustCrossLayerBaselineReport, String> {
    let eager = build_resource_outcome(eager_scenario, eager_runs, runs_root)?;
    let feedback = build_resource_outcome(feedback_scenario, feedback_runs, runs_root)?;

    let eager_build_compiled_a_pruned_package = eager.repetitions.iter().any(|r| {
        r.compiled_package_names
            .iter()
            .any(|name| plan.pruned_packages.contains(name))
    });
    let feedback_build_never_compiled_a_pruned_package = feedback.repetitions.iter().all(|r| {
        r.compiled_package_names
            .iter()
            .all(|name| !plan.pruned_packages.contains(name))
    });

    let wall_time_comparison = scenario::compare_reports(&eager.scenario_report, &feedback.scenario_report);

    Ok(RustCrossLayerBaselineReport {
        schema_version: SCHEMA_VERSION.to_string(),
        workload_id: WORKLOAD_ID.to_string(),
        runs_root: runs_root.to_path_buf(),
        plan_selected_packages: plan.selected_packages,
        plan_pruned_packages: plan.pruned_packages,
        eager,
        feedback,
        wall_time_comparison,
        eager_build_compiled_a_pruned_package,
        feedback_build_never_compiled_a_pruned_package,
    })
}

/// Rebuilds a `RustCrossLayerBaselineReport` purely from already-written
/// `run.json` files plus a fresh call to `compute_plan()` (the plan
/// itself is cheap, pure, and deterministic over the fixture's own
/// on-disk source/lock/manifest, so it is recomputed rather than trusted
/// from `report`'s own claims) -- mirrors `m3_baseline::regenerate`/
/// `m8_baseline::regenerate`'s own "discard whatever the input report
/// claimed, rebuild from disk" discipline.
pub fn regenerate(
    report: RustCrossLayerBaselineReport,
) -> Result<RustCrossLayerBaselineReport, String> {
    if report.workload_id != WORKLOAD_ID {
        return Err(format!(
            "workload_id {:?} is not {WORKLOAD_ID:?} -- refusing to regenerate a report for a \
             different workload",
            report.workload_id
        ));
    }

    let plan = compute_plan()?;
    let runs_root = report.runs_root.clone();

    let read_runs = |run_ids: &[String]| -> Result<Vec<Run>, String> {
        run_ids
            .iter()
            .map(|run_id| {
                laminaria_run::store::read_run(&runs_root, run_id)
                    .map_err(|e| format!("failed to read run {run_id}: {e}"))
            })
            .collect()
    };

    let eager_run_ids: Vec<String> = report.eager.repetitions.iter().map(|r| r.run_id.clone()).collect();
    let feedback_run_ids: Vec<String> = report
        .feedback
        .repetitions
        .iter()
        .map(|r| r.run_id.clone())
        .collect();
    let eager_runs = read_runs(&eager_run_ids)?;
    let feedback_runs = read_runs(&feedback_run_ids)?;

    let eager_scenario_stub = Scenario {
        id: EAGER_SCENARIO_ID.to_string(),
        workload_id: WORKLOAD_ID.to_string(),
        prepare: Vec::new(),
        root: eager_runs
            .first()
            .map(|r| r.root_command.clone())
            .ok_or_else(|| "no eager repetitions to regenerate from".to_string())?,
        observation_roots: Vec::new(),
        cache_state_label: CacheStateLabel::Cold,
    };
    let feedback_scenario_stub = Scenario {
        id: FEEDBACK_SCENARIO_ID.to_string(),
        workload_id: WORKLOAD_ID.to_string(),
        prepare: Vec::new(),
        root: feedback_runs
            .first()
            .map(|r| r.root_command.clone())
            .ok_or_else(|| "no feedback repetitions to regenerate from".to_string())?,
        observation_roots: Vec::new(),
        cache_state_label: CacheStateLabel::Cold,
    };

    build_report(
        plan,
        &runs_root,
        &eager_scenario_stub,
        &eager_runs,
        &feedback_scenario_stub,
        &feedback_runs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `cargo build` subprocesses, real `wait4` resource accounting,
    /// real Cargo JSON telemetry -- no mocking, per this module's own
    /// purpose (raw resource evidence, not a simulated stand-in for it).
    /// Kept to warmup=1, repetitions=2 per scenario (4 real cold builds of
    /// a 3-7 crate `opt-level=0` fixture total) to stay fast.
    #[test]
    fn feedback_pruning_is_supported_by_real_measured_evidence() {
        let report = run(1, 2).expect("the real P0 baseline must run to completion");

        assert_eq!(
            report.plan_selected_packages,
            ["fixture-bin", "used-core", "used-util"]
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>(),
            "the owned plan over this fixture's real source must select exactly these packages"
        );
        assert_eq!(report.plan_pruned_packages.len(), 4);

        assert!(
            report.eager_build_compiled_a_pruned_package,
            "the eager (--workspace) build's own real Cargo telemetry must show it actually \
             compiled at least one unused-pkg-*, or this comparison would be vacuous"
        );
        assert!(
            report.feedback_build_never_compiled_a_pruned_package,
            "the feedback (-p <selected>) build's own real Cargo telemetry must never show a \
             pruned package compiled, in any repetition"
        );
        assert!(
            report.feedback_pruning_is_supported_by_raw_evidence(),
            "the P0 pass criterion must hold from this run's own recorded evidence"
        );

        for outcome in [&report.eager, &report.feedback] {
            assert!(
                outcome.repetitions.iter().all(|r| r.success),
                "scenario {:?}: every repetition must succeed",
                outcome.scenario_id
            );
            assert!(
                outcome.cpu_seconds_stats.is_some(),
                "scenario {:?}: real wait4 CPU accounting must be present",
                outcome.scenario_id
            );
            assert!(
                outcome.peak_rss_bytes_stats.is_some(),
                "scenario {:?}: real wait4 peak-RSS accounting must be present",
                outcome.scenario_id
            );
        }

        // regenerate() must reproduce the same judgment purely from disk,
        // not merely echo the in-memory report back.
        let regenerated = regenerate(report).expect("regenerate must rebuild from disk");
        assert!(regenerated.feedback_pruning_is_supported_by_raw_evidence());
    }

    #[test]
    fn package_name_from_package_id_parses_this_fixture_s_real_path_plus_file_spec() {
        assert_eq!(
            package_name_from_package_id(
                "path+file:///home/x/fixtures/many-unrequested-targets/small/crates/used-core#0.1.0"
            ),
            Some("used-core".to_string())
        );
        assert_eq!(package_name_from_package_id(""), None);
    }
}

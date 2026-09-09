use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// CLI-facing mirror of `laminaria_run::types::ProbeLevel`'s two
/// implemented variants. A separate enum (not a re-export) so clap's
/// `ValueEnum` derive stays in this crate rather than requiring `clap` as
/// a dependency of `laminaria-run` just for its CLI string spelling.
#[derive(Clone, Copy, ValueEnum)]
enum ProbeLevelArg {
    /// Portable lifecycle tracing only: exit status and wall timestamps,
    /// no resource accounting, no Cargo/Nim wrapper substitution -- the
    /// minimal-wrapper baseline to compare Level 1's own overhead against
    /// (docs/measurement-foundation.md section 12, issue #19 Experiment 6).
    Level0,
    /// Full `wait4`-based resource accounting plus Cargo/Nim wrapper
    /// substitution where applicable. The default.
    Level1,
}

impl From<ProbeLevelArg> for laminaria_run::types::ProbeLevel {
    fn from(value: ProbeLevelArg) -> Self {
        match value {
            ProbeLevelArg::Level0 => laminaria_run::types::ProbeLevel::Level0Lifecycle,
            ProbeLevelArg::Level1 => laminaria_run::types::ProbeLevel::Level1ProcessResource,
        }
    }
}

#[derive(Parser)]
#[command(
    name = "laminaria",
    about = "LAMINARIA Rust runtime scheduler CLI",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Report the resolved EnvironmentFingerprint and ToolchainFingerprints
    /// for every entry in toolchains.lock.toml (issue #18).
    Doctor {
        /// Emit machine-readable JSON instead of a human-readable summary.
        #[arg(long)]
        json: bool,
        /// Path to the repository-owned multi-toolchain lock file.
        #[arg(long, default_value = "toolchains.lock.toml")]
        lock: PathBuf,
        /// Repository root used for commit/dirty-state and filesystem detection.
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
    },
    /// Wrap a command as one Run, recording its versioned Run envelope and
    /// Level 0/1 process trace under `runs/<run-id>/` (issue #19).
    Run {
        /// Logical identity of the thing being measured (e.g. a fixture
        /// name). Free-form; not yet validated against a registry.
        #[arg(long)]
        workload_id: String,
        /// Which pre-state/change/requested-artifact scenario this Run
        /// represents (docs/measurement-foundation.md section 9). Free-form
        /// for now -- the scenario state machine itself is not yet enforced.
        #[arg(long)]
        scenario_id: String,
        /// Optional logical path/identity of the artifact this Run is
        /// ultimately measuring the production of.
        #[arg(long)]
        requested_artifact: Option<String>,
        /// Directory raw Run evidence is written under, as
        /// `<runs-root>/<run-id>/`.
        #[arg(long, default_value = "runs")]
        runs_root: PathBuf,
        /// Path to the repository-owned multi-toolchain lock file, used to
        /// populate this Run's environment/toolchain fingerprint.
        #[arg(long, default_value = "toolchains.lock.toml")]
        lock: PathBuf,
        /// Repository root used for commit/dirty-state and filesystem detection.
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
        /// Emit the written Run as machine-readable JSON instead of a
        /// human-readable summary.
        #[arg(long)]
        json: bool,
        /// Tracing depth: `level1` (default, full resource accounting plus
        /// Cargo/Nim wrapper substitution) or `level0` (portable lifecycle
        /// only -- the minimal-wrapper baseline to compare level1's own
        /// overhead against, issue #19 Experiment 6).
        #[arg(long, value_enum, default_value_t = ProbeLevelArg::Level1)]
        probe_level: ProbeLevelArg,
        /// Directory to snapshot before and after the traced command, to
        /// capture an artifact create/modify/delete/unchanged inventory
        /// (issue #20). Repeatable. Not auto-detected -- e.g. pass
        /// `--observe target` for a Cargo build's own target directory.
        /// Omit for no artifact inventory at all.
        #[arg(long)]
        observe: Vec<PathBuf>,
        /// The root command to trace, e.g. `-- cargo build --release`.
        #[arg(required = true, num_args = 1.., last = true)]
        command: Vec<String>,
    },
    /// Rebuild `runs/<run-id>/summary.json` from that Run's already-written
    /// `run.json`, without rerunning the workload (issue #19).
    RegenerateSummary {
        #[arg(long, default_value = "runs")]
        runs_root: PathBuf,
        run_id: String,
    },
    /// Runs one of the four preset baseline scenarios (issue #21: cold
    /// build, true no-op, or an implementation-only edit) against
    /// `fixtures/rust-heavy-workspace` or `fixtures/nim-heavy-workspace`,
    /// repeated `--repeat` times, and reports aggregated statistics.
    ScenarioRun {
        #[arg(long, value_enum)]
        workload: ScenarioWorkloadArg,
        #[arg(long, value_enum)]
        kind: ScenarioKindArg,
        /// Required only for `--kind edit`: the source file to `touch`
        /// before each repetition.
        #[arg(long)]
        edited_source_path: Option<PathBuf>,
        /// `fixtures/rust-heavy-workspace/Cargo.toml`-relative or
        /// `fixtures/nim-heavy-workspace/src/fixture.nim`-relative,
        /// depending on `--workload`; defaults match this repo's own
        /// fixture layout.
        #[arg(long)]
        manifest_path: Option<PathBuf>,
        #[arg(long)]
        target_dir: Option<PathBuf>,
        #[arg(long)]
        nimcache_dir: Option<PathBuf>,
        #[arg(long)]
        out_path: Option<PathBuf>,
        #[arg(long, default_value_t = 1)]
        repeat: usize,
        #[arg(long, default_value = "runs")]
        runs_root: PathBuf,
        #[arg(long, default_value = "toolchains.lock.toml")]
        lock: PathBuf,
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Rebuilds a `ScenarioReport` purely from already-written `run.json`
    /// files (issue #21's "reports are regenerable" acceptance
    /// criterion), without rerunning anything.
    ScenarioRegenerate {
        #[arg(long, default_value = "runs")]
        runs_root: PathBuf,
        #[arg(long)]
        scenario_id: String,
        /// Repeatable: every run_id that belongs to this scenario's
        /// repetition set.
        #[arg(long = "run-id", required = true)]
        run_ids: Vec<String>,
    },
    /// Compares two already-generated `ScenarioReport` JSON files
    /// (`laminaria scenario-run --json > report.json`), noise-floor-aware
    /// and flagging process-count/artifact-profile changes separately
    /// from the wall-time verdict (issue #21).
    ScenarioCompare {
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ScenarioWorkloadArg {
    RustHeavyWorkspace,
    NimHeavyWorkspace,
}

#[derive(Clone, Copy, ValueEnum)]
enum ScenarioKindArg {
    Cold,
    Noop,
    Edit,
}

impl From<ScenarioKindArg> for laminaria_run::scenario::CacheStateLabel {
    fn from(value: ScenarioKindArg) -> Self {
        match value {
            ScenarioKindArg::Cold => laminaria_run::scenario::CacheStateLabel::Cold,
            ScenarioKindArg::Noop => laminaria_run::scenario::CacheStateLabel::TrueNoop,
            ScenarioKindArg::Edit => laminaria_run::scenario::CacheStateLabel::Warm,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Commands::Doctor {
            json,
            lock,
            repo_root,
        } => laminaria_fingerprint::doctor::run(&lock, &repo_root, json),
        Commands::Run {
            workload_id,
            scenario_id,
            requested_artifact,
            runs_root,
            lock,
            repo_root,
            json,
            probe_level,
            observe,
            command,
        } => run_command(
            workload_id,
            scenario_id,
            requested_artifact,
            runs_root,
            lock,
            repo_root,
            json,
            probe_level,
            observe,
            command,
        ),
        Commands::RegenerateSummary { runs_root, run_id } => {
            regenerate_summary_command(runs_root, run_id)
        }
        Commands::ScenarioRun {
            workload,
            kind,
            edited_source_path,
            manifest_path,
            target_dir,
            nimcache_dir,
            out_path,
            repeat,
            runs_root,
            lock,
            repo_root,
            json,
        } => scenario_run_command(
            workload,
            kind,
            edited_source_path,
            manifest_path,
            target_dir,
            nimcache_dir,
            out_path,
            repeat,
            runs_root,
            lock,
            repo_root,
            json,
        ),
        Commands::ScenarioRegenerate {
            runs_root,
            scenario_id,
            run_ids,
        } => scenario_regenerate_command(runs_root, scenario_id, run_ids),
        Commands::ScenarioCompare {
            baseline,
            candidate,
        } => scenario_compare_command(baseline, candidate),
    };
    std::process::exit(code);
}

#[allow(clippy::too_many_arguments)]
fn scenario_run_command(
    workload: ScenarioWorkloadArg,
    kind: ScenarioKindArg,
    edited_source_path: Option<PathBuf>,
    manifest_path: Option<PathBuf>,
    target_dir: Option<PathBuf>,
    nimcache_dir: Option<PathBuf>,
    out_path: Option<PathBuf>,
    repeat: usize,
    runs_root: PathBuf,
    lock: PathBuf,
    repo_root: PathBuf,
    json: bool,
) -> i32 {
    let cache_state_label = kind.into();
    if matches!(kind, ScenarioKindArg::Edit) && edited_source_path.is_none() {
        eprintln!("laminaria scenario-run: --edited-source-path is required for --kind edit");
        return 2;
    }

    let scenario = match workload {
        ScenarioWorkloadArg::RustHeavyWorkspace => {
            laminaria_run::scenario::rust_heavy_workspace_scenario(
                cache_state_label,
                &manifest_path
                    .unwrap_or_else(|| PathBuf::from("fixtures/rust-heavy-workspace/Cargo.toml")),
                &target_dir
                    .unwrap_or_else(|| PathBuf::from("fixtures/rust-heavy-workspace/target")),
                edited_source_path.as_deref(),
            )
        }
        ScenarioWorkloadArg::NimHeavyWorkspace => {
            laminaria_run::scenario::nim_heavy_workspace_scenario(
                cache_state_label,
                &manifest_path.unwrap_or_else(|| {
                    PathBuf::from("fixtures/nim-heavy-workspace/src/fixture.nim")
                }),
                &nimcache_dir
                    .unwrap_or_else(|| PathBuf::from("fixtures/nim-heavy-workspace/nimcache")),
                &out_path
                    .unwrap_or_else(|| PathBuf::from("fixtures/nim-heavy-workspace/fixture_out")),
                edited_source_path.as_deref(),
            )
        }
    };

    let report = match laminaria_run::scenario::run_scenario_repeated(
        &scenario, repeat, &runs_root, &lock, &repo_root,
    ) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("laminaria scenario-run: failed: {err}");
            return 2;
        }
    };

    print_scenario_report(&report, json)
}

fn scenario_regenerate_command(
    runs_root: PathBuf,
    scenario_id: String,
    run_ids: Vec<String>,
) -> i32 {
    match laminaria_run::scenario::regenerate_report_from_disk(&runs_root, &scenario_id, &run_ids) {
        Ok(report) => print_scenario_report(&report, true),
        Err(err) => {
            eprintln!("laminaria scenario-regenerate: failed: {err}");
            2
        }
    }
}

fn scenario_compare_command(baseline_path: PathBuf, candidate_path: PathBuf) -> i32 {
    let baseline = match read_scenario_report(&baseline_path) {
        Ok(report) => report,
        Err(err) => {
            eprintln!(
                "laminaria scenario-compare: failed to read {}: {err}",
                baseline_path.display()
            );
            return 2;
        }
    };
    let candidate = match read_scenario_report(&candidate_path) {
        Ok(report) => report,
        Err(err) => {
            eprintln!(
                "laminaria scenario-compare: failed to read {}: {err}",
                candidate_path.display()
            );
            return 2;
        }
    };

    let comparison = match laminaria_run::scenario::compare_reports(&baseline, &candidate) {
        Ok(comparison) => comparison,
        Err(reason) => {
            eprintln!("laminaria scenario-compare: {reason}");
            return 2;
        }
    };
    match serde_json::to_string_pretty(&comparison) {
        Ok(text) => println!("{text}"),
        Err(err) => {
            eprintln!("laminaria scenario-compare: failed to serialize comparison: {err}");
            return 2;
        }
    }

    if !comparison.confounding_notes.is_empty() {
        1
    } else {
        0
    }
}

fn read_scenario_report(
    path: &PathBuf,
) -> std::io::Result<laminaria_run::scenario::ScenarioReport> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn print_scenario_report(report: &laminaria_run::scenario::ScenarioReport, json: bool) -> i32 {
    if json {
        match serde_json::to_string_pretty(report) {
            Ok(text) => println!("{text}"),
            Err(err) => {
                eprintln!("laminaria scenario: failed to serialize report: {err}");
                return 2;
            }
        }
    } else {
        println!("scenario_id: {}", report.scenario_id);
        println!("workload_id: {}", report.workload_id);
        println!("success: {:?}", report.success);
        let failed_count = report.success.iter().filter(|s| !**s).count();
        if failed_count > 0 {
            println!(
                "  ! {failed_count}/{} repetition(s) failed -- excluded from wall_seconds below, \
                 not counted as fast samples",
                report.success.len()
            );
        }
        println!(
            "sample_count: {} (successful repetitions only)",
            report.wall_seconds.sample_count
        );
        println!(
            "wall_seconds: min={:.3} p50={:.3} p90={:.3} mean={:.3} stddev={:.3}",
            report.wall_seconds.min,
            report.wall_seconds.p50,
            report.wall_seconds.p90,
            report.wall_seconds.mean,
            report.wall_seconds.stddev,
        );
        println!("process_counts: {:?}", report.process_counts);
        println!(
            "artifacts: created={:?} modified={:?} deleted={:?} unchanged={:?}",
            report.artifact_created,
            report.artifact_modified,
            report.artifact_deleted,
            report.artifact_unchanged
        );
    }
    0
}

#[allow(clippy::too_many_arguments)]
fn run_command(
    workload_id: String,
    scenario_id: String,
    requested_artifact: Option<String>,
    runs_root: PathBuf,
    lock: PathBuf,
    repo_root: PathBuf,
    json: bool,
    probe_level: ProbeLevelArg,
    observe: Vec<PathBuf>,
    command: Vec<String>,
) -> i32 {
    let program = command[0].clone();
    let args = command[1..].to_vec();
    let root = laminaria_run::types::RootCommand {
        program,
        args,
        cwd: None,
        env_overrides: std::collections::BTreeMap::new(),
    };

    let (run, dir) = match laminaria_run::run_and_record(
        &runs_root,
        &workload_id,
        &scenario_id,
        requested_artifact,
        &lock,
        &repo_root,
        root,
        probe_level.into(),
        &observe,
    ) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("laminaria run: failed to trace command: {err}");
            return 2;
        }
    };

    let success = run.result.as_ref().is_some_and(|r| r.success);

    if json {
        match serde_json::to_string_pretty(&run) {
            Ok(text) => println!("{text}"),
            Err(err) => {
                eprintln!("laminaria run: failed to serialize Run: {err}");
                return 2;
            }
        }
    } else {
        println!("run_id: {}", run.run_id);
        println!("written to: {}", dir.display());
        println!("success: {success}");
        if let Some(record) = run.process_trace.processes.first() {
            let wall_ns = record
                .end_elapsed_ns
                .map(|end| end - record.start_elapsed_ns)
                .unwrap_or(0);
            println!("wall: {:.3}s", wall_ns as f64 / 1_000_000_000.0);
            if let Some(cpu) = record.resource_usage.user_cpu_seconds {
                println!("user cpu: {cpu:.3}s");
            }
            if let Some(cpu) = record.resource_usage.system_cpu_seconds {
                println!("system cpu: {cpu:.3}s");
            }
            if let Some(rss) = record.resource_usage.peak_rss_bytes {
                println!("peak rss: {rss} bytes");
            }
        }
        if !run.process_trace.known_gaps.is_empty() {
            println!("known gaps:");
            for gap in &run.process_trace.known_gaps {
                println!("  - {gap}");
            }
        }
    }

    if success {
        0
    } else {
        1
    }
}

fn regenerate_summary_command(runs_root: PathBuf, run_id: String) -> i32 {
    match laminaria_run::store::regenerate_summary_from_disk(&runs_root, &run_id) {
        Ok(summary) => {
            match serde_json::to_string_pretty(&summary) {
                Ok(text) => println!("{text}"),
                Err(err) => {
                    eprintln!("laminaria regenerate-summary: failed to serialize Summary: {err}");
                    return 2;
                }
            }
            0
        }
        Err(err) => {
            eprintln!("laminaria regenerate-summary: failed: {err}");
            2
        }
    }
}

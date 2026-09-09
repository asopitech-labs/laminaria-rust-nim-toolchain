use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
            command,
        } => run_command(
            workload_id,
            scenario_id,
            requested_artifact,
            runs_root,
            lock,
            repo_root,
            json,
            command,
        ),
        Commands::RegenerateSummary { runs_root, run_id } => {
            regenerate_summary_command(runs_root, run_id)
        }
    };
    std::process::exit(code);
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

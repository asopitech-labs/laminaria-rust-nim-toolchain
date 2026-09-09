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
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Commands::Doctor {
            json,
            lock,
            repo_root,
        } => laminaria_fingerprint::doctor::run(&lock, &repo_root, json),
    };
    std::process::exit(code);
}

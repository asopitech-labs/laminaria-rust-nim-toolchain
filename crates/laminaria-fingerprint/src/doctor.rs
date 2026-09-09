//! Orchestrates `EnvironmentFingerprint` + per-toolchain fingerprint
//! resolution into one `ToolchainReport`, and renders it either as
//! machine-readable JSON or a human-readable summary. This is the `doctor`
//! responsibility described in `docs/measurement-foundation.md` section 14.

use std::path::{Path, PathBuf};

use crate::env;
use crate::external;
use crate::lock::{self, LockLoadError};
use crate::nim_toolchain;
use crate::rust_requirements;
use crate::rust_toolchain;
use crate::types::ToolchainReport;

pub const HARNESS_NAME: &str = "laminaria-doctor";
pub const HARNESS_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct DoctorRun {
    pub report: ToolchainReport,
    pub lock_path: PathBuf,
    pub lock_load_error: Option<LockLoadError>,
}

pub fn build(lock_path: &Path, repo_root: &Path) -> DoctorRun {
    let (lock_file, lock_load_error) = match lock::load(lock_path) {
        Ok(lf) => (Some(lf), None),
        Err(e) => (None, Some(e)),
    };

    let mut environment = env::detect(repo_root, HARNESS_NAME, HARNESS_VERSION);

    let rust_toolchains: Vec<_> = lock_file
        .as_ref()
        .map(|lf| {
            lf.rust
                .toolchains
                .iter()
                .map(|(name, sel)| rust_toolchain::resolve(name, sel))
                .collect()
        })
        .unwrap_or_default();

    if let Some(triple) = rust_toolchains
        .iter()
        .find_map(|t| t.host_triple.as_deref())
    {
        environment.architecture_notice =
            env::architecture_notice_for(&environment.architecture, triple);
    }

    let nim_toolchains: Vec<_> = lock_file
        .as_ref()
        .map(|lf| {
            lf.nim
                .toolchains
                .iter()
                .map(|(name, sel)| nim_toolchain::resolve(name, sel))
                .collect()
        })
        .unwrap_or_default();

    let external_tools: Vec<_> = lock_file
        .as_ref()
        .map(|lf| {
            lf.tools
                .iter()
                .map(|(name, sel)| external::resolve(name, sel))
                .collect()
        })
        .unwrap_or_default();

    let requirements = rust_requirements::discover_workspace_requirements(repo_root);
    let rust_requirement_evaluations: Vec<_> = requirements
        .iter()
        .flat_map(|req| {
            rust_toolchains
                .iter()
                .map(move |toolchain| rust_requirements::evaluate(req, toolchain))
        })
        .collect();

    DoctorRun {
        report: ToolchainReport {
            environment,
            rust_toolchains,
            nim_toolchains,
            external_tools,
            rust_requirement_evaluations,
        },
        lock_path: lock_path.to_path_buf(),
        lock_load_error,
    }
}

/// Runs doctor end-to-end and prints the result. Returns a process exit
/// code: 0 when the lock file loaded (regardless of individual tools being
/// absent — that is diagnostic content, not a hard failure), 2 when the
/// repository-owned lock file itself could not be read/parsed.
pub fn run(lock_path: &Path, repo_root: &Path, json: bool) -> i32 {
    let doctor_run = build(lock_path, repo_root);

    if json {
        print_json(&doctor_run);
    } else {
        print_human(&doctor_run);
    }

    if doctor_run.lock_load_error.is_some() {
        2
    } else {
        0
    }
}

fn print_json(doctor_run: &DoctorRun) {
    #[derive(serde::Serialize)]
    struct Envelope<'a> {
        lock_path: &'a Path,
        lock_load_error: Option<String>,
        #[serde(flatten)]
        report: &'a ToolchainReport,
    }
    let envelope = Envelope {
        lock_path: &doctor_run.lock_path,
        lock_load_error: doctor_run.lock_load_error.as_ref().map(|e| e.to_string()),
        report: &doctor_run.report,
    };
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
}

fn print_human(doctor_run: &DoctorRun) {
    let report = &doctor_run.report;
    let env = &report.environment;

    println!("LAMINARIA doctor — {}", env.measurement_harness);
    println!("lock file: {}", doctor_run.lock_path.display());
    if let Some(err) = &doctor_run.lock_load_error {
        println!("  ! {err}");
    }
    println!();

    println!("Environment");
    println!(
        "  os:              {} {}",
        env.os,
        env.os_version.as_deref().unwrap_or("(unknown)")
    );
    println!(
        "  kernel:          {}",
        env.kernel.as_deref().unwrap_or("(unknown)")
    );
    println!("  architecture:    {}", env.architecture);
    println!(
        "  cpu:             {} ({} physical / {} logical cores)",
        env.cpu_model.as_deref().unwrap_or("(unknown)"),
        env.cpu_physical_cores
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into()),
        env.cpu_logical_cores
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into()),
    );
    println!(
        "  memory:          {}",
        env.memory_bytes
            .map(|b| format!("{:.1} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0)))
            .unwrap_or_else(|| "(unknown)".to_string())
    );
    println!(
        "  filesystem:      {}",
        env.filesystem_type.as_deref().unwrap_or("(unknown)")
    );
    println!("  environment class: {}", env.environment_class);
    println!(
        "  repository:      commit={} dirty={}",
        env.repository.commit.as_deref().unwrap_or("(unknown)"),
        env.repository
            .dirty
            .map(|d| d.to_string())
            .unwrap_or_else(|| "(unknown)".into())
    );
    if let Some(sdk) = &env.sdk_path {
        println!("  sdk path:        {}", sdk.display());
    }
    if let Some(notice) = &env.self_process_translation_notice {
        println!("  ! {notice}");
    }
    if let Some(notice) = &env.path_toolchain_shadow {
        println!("  ! {notice}");
    }
    if let Some(notice) = &env.architecture_notice {
        println!("  ! {}", notice.explanation);
    }
    if !env.unobserved_fields.is_empty() {
        println!("  unobserved fields: {}", env.unobserved_fields.join(", "));
    }
    println!();

    println!("Rust toolchains");
    if report.rust_toolchains.is_empty() {
        println!("  (none declared in lock file)");
    }
    for t in &report.rust_toolchains {
        println!(
            "  [{}] selector={} -> {} ({}, LLVM {})",
            t.logical_name,
            t.requested_selector.as_deref().unwrap_or("?"),
            t.resolved_version.as_deref().unwrap_or("UNRESOLVED"),
            t.host_triple.as_deref().unwrap_or("?"),
            t.llvm_version.as_deref().unwrap_or("unknown"),
        );
        if let Some(p) = &t.rustc.path {
            println!(
                "      rustc: {} (sha256 {})",
                p.display(),
                t.rustc.digest_sha256.as_deref().unwrap_or("?")
            );
        }
        for note in &t.resolution_notes {
            println!("      ! {note}");
        }
    }
    println!();

    println!("Nim toolchains");
    if report.nim_toolchains.is_empty() {
        println!("  (none declared in lock file)");
    }
    for t in &report.nim_toolchains {
        println!(
            "  [{}] selector={} -> {} ({} / {})",
            t.logical_name,
            t.requested_selector.as_deref().unwrap_or("?"),
            t.resolved_version.as_deref().unwrap_or("UNRESOLVED"),
            t.target_os.as_deref().unwrap_or("?"),
            t.target_cpu.as_deref().unwrap_or("?"),
        );
        for note in &t.resolution_notes {
            println!("      ! {note}");
        }
    }
    println!();

    println!("External tools");
    if report.external_tools.is_empty() {
        println!("  (none declared in lock file)");
    }
    for t in &report.external_tools {
        let status = if t.present { "present" } else { "MISSING" };
        println!(
            "  [{}] selector={} -> {} ({})",
            t.logical_name,
            t.requested_selector.as_deref().unwrap_or("?"),
            status,
            t.resolved_version.as_deref().unwrap_or("?"),
        );
        for note in &t.notes {
            println!("      ! {note}");
        }
    }
    println!();

    println!("Cargo rust-version / edition requirements");
    if report.rust_requirement_evaluations.is_empty() {
        println!(
            "  (no workspace packages with a Cargo.toml declaring rust-version/edition found)"
        );
    }
    for e in &report.rust_requirement_evaluations {
        let msrv_status = match e.rust_version_satisfied {
            Some(true) => "OK",
            Some(false) => "VIOLATED",
            None => "n/a",
        };
        println!(
            "  {} vs [{}]: rust-version={} (resolved {}) edition={} -> {}",
            e.package_name,
            e.toolchain_logical_name,
            e.rust_version.as_deref().unwrap_or("(none)"),
            e.resolved_compiler_version.as_deref().unwrap_or("?"),
            e.edition.as_deref().unwrap_or("(none)"),
            msrv_status,
        );
        for note in &e.notes {
            println!("      ! {note}");
        }
    }
}

//! Issue #28 D1-b1 review round 1: a preflight environment-compatibility
//! check for the D1-b1 reference-case runner. Found by direct
//! observation, not theory: this dev machine has two independent Rust
//! toolchains on `PATH` (a Homebrew install targeting `x86_64-apple-darwin`
//! and a rustup install targeting `aarch64-apple-darwin`, the real
//! hardware), and whichever `rustc` resolves first is what every fixture
//! `cargo build`/`cargo run` below actually uses -- while `nim c` always
//! targets the real hardware architecture. When these disagree, linking
//! a `nim c`-produced static library into a Rust binary fails with a
//! `ld: ... found architecture 'X', required architecture 'Y'` error
//! that looks like a genuine case defect but is actually neither: the
//! fixtures and their D0-pinned expected values are correct, only this
//! *environment* is internally inconsistent. This check runs once,
//! before any case, and reports that distinction explicitly rather than
//! letting it surface as a confusing per-case `FAIL`.

use std::process::Command;

#[derive(Debug, Clone)]
pub struct PreflightReport {
    pub rustc_host_triple: String,
    pub rustc_arch: String,
    pub hardware_arch: String,
    pub compatible: bool,
}

/// Collapses the handful of spellings for the same architecture that
/// `rustc -vV`'s `host:` triple and `uname -m` actually use in practice
/// (`aarch64` vs `arm64`, `amd64` vs `x86_64`) so the comparison isn't
/// defeated by naming alone.
fn normalize_arch(arch: &str) -> String {
    match arch {
        "aarch64" | "arm64" => "arm64".to_string(),
        "x86_64" | "amd64" => "x86_64".to_string(),
        other => other.to_string(),
    }
}

fn rustc_host_triple() -> Result<String, String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("failed to run `rustc -vV`: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "`rustc -vV` exited {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("`rustc -vV` output had no `host:` line:\n{stdout}"))
}

/// The real hardware's own architecture, independent of which `rustc`
/// (or which architecture *this very runner binary* was itself compiled
/// for) happens to be resolved on `PATH` right now -- `uname -m` reports
/// the kernel's own view, which is what `cc`/`nim c`'s default codegen
/// target actually follows.
#[cfg(unix)]
fn hardware_arch() -> Result<String, String> {
    let output = Command::new("uname")
        .arg("-m")
        .output()
        .map_err(|e| format!("failed to run `uname -m`: {e}"))?;
    if !output.status.success() {
        return Err(format!("`uname -m` exited {:?}", output.status.code()));
    }
    Ok(normalize_arch(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

#[cfg(not(unix))]
fn hardware_arch() -> Result<String, String> {
    // No `uname` on Windows; `std::env::consts::ARCH` (the architecture
    // this runner binary was itself compiled for) is the best available
    // signal there, and this project's Windows CI runner has never shown
    // the dual-toolchain split this check exists to catch.
    Ok(normalize_arch(std::env::consts::ARCH))
}

pub fn run() -> Result<PreflightReport, String> {
    let rustc_host_triple = rustc_host_triple()?;
    let rustc_arch = normalize_arch(rustc_host_triple.split('-').next().unwrap_or(""));
    let hardware_arch = hardware_arch()?;
    let compatible = rustc_arch == hardware_arch;
    Ok(PreflightReport {
        rustc_host_triple,
        rustc_arch,
        hardware_arch,
        compatible,
    })
}

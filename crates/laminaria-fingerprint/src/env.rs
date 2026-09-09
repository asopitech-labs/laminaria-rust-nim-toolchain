//! `EnvironmentFingerprint` detection: OS, kernel, CPU, memory, filesystem,
//! repository state, and an allow-listed environment-variable snapshot.
//!
//! Every field is best-effort. When a signal is not observable on the
//! current platform, the field is left `None`/empty and its name is added
//! to `unobserved_fields` instead of guessing, so a downstream comparison
//! can tell "known equal" apart from "not measured".

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::exec::{run, which};
use crate::types::{ArchitectureNotice, EnvironmentFingerprint, RepositoryState};

/// Environment variables safe to record in a fingerprint. Anything not on
/// this list is never captured, regardless of what is set in the process
/// environment (measurement-foundation.md §4: "must use an allow-list").
pub const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "TERM",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "NIMBLE_DIR",
    "NIM_LIB_DIR",
    "CI",
    "GITHUB_ACTIONS",
    "GITHUB_RUN_ID",
    "WSL_DISTRO_NAME",
    "WSL_INTEROP",
];

pub fn detect(
    repo_root: &Path,
    harness_name: &str,
    harness_version: &str,
) -> EnvironmentFingerprint {
    let mut unobserved = Vec::new();

    let os = std::env::consts::OS.to_string();
    let (architecture, self_process_translation_notice) = host_architecture();
    let path_toolchain_shadow = detect_path_toolchain_shadow();
    let kernel = uname_field("-r");
    let os_version = detect_os_version(&mut unobserved);
    let (cpu_model, cpu_physical_cores) = detect_cpu(&mut unobserved);
    let cpu_logical_cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .ok();
    let memory_bytes = detect_memory_bytes(&mut unobserved);
    let filesystem_type = detect_filesystem_type(repo_root, &mut unobserved);
    let environment_class = detect_environment_class();
    let repository = detect_repository_state(repo_root);
    let sdk_path = detect_sdk_path(&mut unobserved);
    let allowed_environment_variables = capture_allowed_env();

    let architecture_notice = None; // filled in once a resolved rustc host triple is known.

    EnvironmentFingerprint {
        schema_version: crate::types::SCHEMA_VERSION.to_string(),
        captured_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        os,
        os_version,
        kernel,
        architecture,
        cpu_model,
        cpu_physical_cores,
        cpu_logical_cores,
        memory_bytes,
        filesystem_type,
        environment_class,
        repository,
        sdk_path,
        measurement_harness: format!("{harness_name} {harness_version}"),
        architecture_notice,
        self_process_translation_notice,
        path_toolchain_shadow,
        allowed_environment_variables,
        unobserved_fields: unobserved,
    }
}

/// Normalizes physical-hardware architecture and a Rust target-triple arch
/// segment onto the same vocabulary (e.g. "arm64" == "aarch64"), then
/// records an explicit notice when they still disagree — usually Rosetta 2
/// or qemu-user translation running a foreign-arch toolchain binary.
pub fn architecture_notice_for(
    host_arch: &str,
    toolchain_host_triple: &str,
) -> Option<ArchitectureNotice> {
    let normalize = |s: &str| match s {
        "arm64" => "aarch64".to_string(),
        other => other.to_string(),
    };
    let triple_arch = toolchain_host_triple.split('-').next().unwrap_or("");
    if normalize(host_arch) == normalize(triple_arch) {
        return None;
    }
    Some(ArchitectureNotice {
        host_architecture: host_arch.to_string(),
        toolchain_host_triple: toolchain_host_triple.to_string(),
        explanation: format!(
            "resolved toolchain host triple '{toolchain_host_triple}' does not match the physical \
             host architecture '{host_arch}'; this toolchain is likely running under binary \
             translation (e.g. Rosetta 2 or qemu-user) rather than natively"
        ),
    })
}

/// `uname -m` (and even `sysctl -n hw.machine`) report the *calling
/// process's* execution personality on macOS, not the physical hardware: a
/// translated (Rosetta 2) process sees "x86_64" even on Apple Silicon.
/// `sysctl.proc_translated` is the reliable signal for "am I translated
/// right now", and since Rosetta 2 only ever translates x86_64-on-arm64,
/// detecting translation is sufficient to recover the true hardware arch.
fn host_architecture() -> (String, Option<String>) {
    #[cfg(target_os = "macos")]
    {
        if run("sysctl", &["-n", "sysctl.proc_translated"]).as_deref() == Some("1") {
            let notice = "this doctor process is itself running as a translated x86_64 process \
                           under Rosetta 2 on Apple Silicon hardware (sysctl.proc_translated=1); \
                           'architecture' below reflects the real arm64 hardware, but every other \
                           subprocess this tool spawns to detect facts also runs translated"
                .to_string();
            return ("arm64".to_string(), Some(notice));
        }
    }
    (
        uname_field("-m").unwrap_or_else(|| std::env::consts::ARCH.to_string()),
        None,
    )
}

fn uname_field(flag: &str) -> Option<String> {
    run("uname", &[flag])
}

/// Detects whether a non-`rustup` `cargo` earlier on `PATH` (e.g. a
/// Homebrew Rust install) shadows the toolchain `rustup`/
/// `rust-toolchain.toml` would otherwise select, which silently changes
/// which compiler builds LAMINARIA's own code.
///
/// `rustup which cargo` always reports the real per-toolchain binary path
/// (`~/.rustup/toolchains/<name>/bin/cargo`), never the small rustup proxy
/// at `~/.cargo/bin/cargo` that normally sits first on `PATH` and forwards
/// to it — those two paths differing is the *expected*, unshadowed case.
/// Shadowing means `PATH` resolves `cargo` to neither of those locations.
fn detect_path_toolchain_shadow() -> Option<String> {
    which("rustup")?;
    let path_cargo = which("cargo")?;
    let home = home_dir();

    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".cargo")));
    if cargo_home
        .as_ref()
        .is_some_and(|c| path_cargo == c.join("bin").join("cargo"))
    {
        return None;
    }

    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| home.map(|h| h.join(".rustup")));
    if rustup_home.is_some_and(|r| path_cargo.starts_with(r.join("toolchains"))) {
        return None;
    }

    Some(format!(
        "PATH resolves 'cargo' to {}, which is neither the rustup proxy nor a rustup-managed \
         toolchain binary; a non-rustup cargo/rustc earlier on PATH is shadowing the project's \
         rust-toolchain.toml selector for ordinary `cargo build` invocations",
        path_cargo.display()
    ))
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}

#[cfg(target_os = "macos")]
fn detect_os_version(_unobserved: &mut Vec<String>) -> Option<String> {
    run("sw_vers", &["-productVersion"])
}

#[cfg(target_os = "linux")]
fn detect_os_version(unobserved: &mut Vec<String>) -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok();
    match text {
        Some(text) => text
            .lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim_matches('"').to_string()),
        None => {
            unobserved.push("os_version".to_string());
            None
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_os_version(unobserved: &mut Vec<String>) -> Option<String> {
    unobserved.push("os_version".to_string());
    None
}

#[cfg(target_os = "macos")]
fn detect_cpu(_unobserved: &mut Vec<String>) -> (Option<String>, Option<u32>) {
    let model = run("sysctl", &["-n", "machdep.cpu.brand_string"]);
    let physical = run("sysctl", &["-n", "hw.physicalcpu"]).and_then(|s| s.parse().ok());
    (model, physical)
}

#[cfg(target_os = "linux")]
fn detect_cpu(unobserved: &mut Vec<String>) -> (Option<String>, Option<u32>) {
    let text = match std::fs::read_to_string("/proc/cpuinfo") {
        Ok(t) => t,
        Err(_) => {
            unobserved.push("cpu_model".to_string());
            unobserved.push("cpu_physical_cores".to_string());
            return (None, None);
        }
    };
    let model = text
        .lines()
        .find_map(|l| l.strip_prefix("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string());
    let physical_ids: std::collections::BTreeSet<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("physical id"))
        .filter_map(|l| l.split(':').nth(1))
        .map(|s| s.trim())
        .collect();
    let physical = if physical_ids.is_empty() {
        None
    } else {
        Some(physical_ids.len() as u32)
    };
    (model, physical)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_cpu(unobserved: &mut Vec<String>) -> (Option<String>, Option<u32>) {
    unobserved.push("cpu_model".to_string());
    unobserved.push("cpu_physical_cores".to_string());
    (None, None)
}

#[cfg(target_os = "macos")]
fn detect_memory_bytes(_unobserved: &mut Vec<String>) -> Option<u64> {
    run("sysctl", &["-n", "hw.memsize"]).and_then(|s| s.parse().ok())
}

#[cfg(target_os = "linux")]
fn detect_memory_bytes(unobserved: &mut Vec<String>) -> Option<u64> {
    let text = match std::fs::read_to_string("/proc/meminfo") {
        Ok(t) => t,
        Err(_) => {
            unobserved.push("memory_bytes".to_string());
            return None;
        }
    };
    text.lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|kb| kb.parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_memory_bytes(unobserved: &mut Vec<String>) -> Option<u64> {
    unobserved.push("memory_bytes".to_string());
    None
}

#[cfg(target_os = "macos")]
fn detect_filesystem_type(path: &Path, _unobserved: &mut Vec<String>) -> Option<String> {
    run("stat", &["-f", "%T", &path.to_string_lossy()])
}

#[cfg(target_os = "linux")]
fn detect_filesystem_type(path: &Path, _unobserved: &mut Vec<String>) -> Option<String> {
    run("stat", &["-f", "-c", "%T", &path.to_string_lossy()])
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_filesystem_type(_path: &Path, unobserved: &mut Vec<String>) -> Option<String> {
    unobserved.push("filesystem_type".to_string());
    None
}

fn detect_environment_class() -> String {
    if std::env::var_os("CI").is_some() || std::env::var_os("GITHUB_ACTIONS").is_some() {
        return "ci".to_string();
    }
    if std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some() {
        return "wsl".to_string();
    }
    if Path::new("/.dockerenv").exists() {
        return "container".to_string();
    }
    if let Ok(text) = std::fs::read_to_string("/proc/1/cgroup") {
        if text.contains("docker") || text.contains("kubepods") || text.contains("containerd") {
            return "container".to_string();
        }
    }
    "native".to_string()
}

fn detect_repository_state(repo_root: &Path) -> RepositoryState {
    let commit = run(
        "git",
        &["-C", &repo_root.to_string_lossy(), "rev-parse", "HEAD"],
    );
    let dirty = run(
        "git",
        &["-C", &repo_root.to_string_lossy(), "status", "--porcelain"],
    )
    .map(|s| !s.is_empty());
    RepositoryState { commit, dirty }
}

#[cfg(target_os = "macos")]
fn detect_sdk_path(_unobserved: &mut Vec<String>) -> Option<std::path::PathBuf> {
    which("xcrun").and(run("xcrun", &["--show-sdk-path"]).map(std::path::PathBuf::from))
}

#[cfg(not(target_os = "macos"))]
fn detect_sdk_path(unobserved: &mut Vec<String>) -> Option<std::path::PathBuf> {
    unobserved.push("sdk_path".to_string());
    None
}

fn capture_allowed_env() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for key in ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(key) {
            map.insert((*key).to_string(), value);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_notice_when_arm64_matches_aarch64_triple() {
        assert!(architecture_notice_for("arm64", "aarch64-apple-darwin").is_none());
    }

    #[test]
    fn no_notice_when_architectures_match_exactly() {
        assert!(architecture_notice_for("x86_64", "x86_64-unknown-linux-gnu").is_none());
    }

    #[test]
    fn notice_when_host_and_toolchain_architecture_disagree() {
        let notice = architecture_notice_for("arm64", "x86_64-apple-darwin")
            .expect("expected a translation notice");
        assert_eq!(notice.host_architecture, "arm64");
        assert_eq!(notice.toolchain_host_triple, "x86_64-apple-darwin");
    }

    #[test]
    fn env_allowlist_never_captures_arbitrary_secret_like_keys() {
        // Loading a secret-shaped var should not sneak into the fingerprint
        // just because it happens to be set in this test process.
        assert!(!ENV_ALLOWLIST.contains(&"AWS_SECRET_ACCESS_KEY"));
        assert!(!ENV_ALLOWLIST.contains(&"GITHUB_TOKEN"));
        assert!(!ENV_ALLOWLIST.contains(&"NPM_TOKEN"));
    }
}

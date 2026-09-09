//! Resolves each named Rust toolchain in `toolchains.lock.toml` to an exact
//! `RustToolchainFingerprint`, per `docs/multi-version-toolchains.md`
//! sections 2-3: the requested selector (e.g. "stable") and the resolved
//! compiler identity are kept as separate, both-recorded fields.

use std::path::PathBuf;

use crate::exec::{run, sha256_file, which};
use crate::lock::RustToolchainSelector;
use crate::types::{ExecutableIdentity, RustToolchainFingerprint};

pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Resolves one named toolchain. When `rustup` is available the selector is
/// resolved through it (`rustup run <selector> ...`), which is what makes
/// multiple exact Rust toolchains coexist and be individually selected
/// rather than only ever reporting whatever `rustc` happens to be on PATH.
/// Without `rustup`, falls back to the active `rustc`/`cargo` on PATH and
/// records that the selector could not be independently resolved.
pub fn resolve(logical_name: &str, selector: &RustToolchainSelector) -> RustToolchainFingerprint {
    let mut notes = Vec::new();
    let rustup_present = which("rustup").is_some();

    let (rustc_verbose, cargo_version_line, rustc_path, cargo_path, components) = if rustup_present
    {
        let rustc_verbose = run("rustup", &["run", &selector.selector, "rustc", "-vV"]);
        let cargo_version_line = run("rustup", &["run", &selector.selector, "cargo", "--version"]);
        let rustc_path = run(
            "rustup",
            &["which", "--toolchain", &selector.selector, "rustc"],
        )
        .map(PathBuf::from);
        let cargo_path = run(
            "rustup",
            &["which", "--toolchain", &selector.selector, "cargo"],
        )
        .map(PathBuf::from);
        let components = run(
            "rustup",
            &[
                "component",
                "list",
                "--installed",
                "--toolchain",
                &selector.selector,
            ],
        )
        .map(|s| s.lines().map(|l| l.trim().to_string()).collect())
        .unwrap_or_default();
        if rustc_verbose.is_none() {
            notes.push(format!(
                "rustup could not resolve toolchain selector '{}'; is it installed? (`rustup toolchain install {}`)",
                selector.selector, selector.selector
            ));
        }
        (
            rustc_verbose,
            cargo_version_line,
            rustc_path,
            cargo_path,
            components,
        )
    } else {
        notes.push(
            "rustup not found on PATH; resolved the active system rustc/cargo instead of the \
             requested selector independently"
                .to_string(),
        );
        let rustc_verbose = run("rustc", &["-vV"]);
        let cargo_version_line = run("cargo", &["--version"]);
        (
            rustc_verbose,
            cargo_version_line,
            which("rustc"),
            which("cargo"),
            Vec::new(),
        )
    };

    let fields = rustc_verbose
        .as_deref()
        .map(parse_rustc_vv)
        .unwrap_or_default();
    let sysroot = rustc_path.as_ref().and_then(|_| {
        if rustup_present {
            run(
                "rustup",
                &["run", &selector.selector, "rustc", "--print", "sysroot"],
            )
        } else {
            run("rustc", &["--print", "sysroot"])
        }
        .map(PathBuf::from)
    });

    let channel = fields.get("release").map(|v| channel_of(v));

    RustToolchainFingerprint {
        logical_name: logical_name.to_string(),
        requested_selector: Some(selector.selector.clone()),
        compiler_family: "rust".to_string(),
        resolved_version: fields.get("release").cloned(),
        resolved_commit_hash: fields.get("commit-hash").cloned(),
        resolved_commit_date: fields.get("commit-date").cloned(),
        host_triple: fields.get("host").cloned(),
        channel,
        llvm_version: fields.get("llvm version").cloned(),
        rustc: ExecutableIdentity {
            digest_sha256: rustc_path.as_deref().and_then(sha256_file),
            path: rustc_path,
        },
        cargo_version: cargo_version_line,
        cargo: ExecutableIdentity {
            digest_sha256: cargo_path.as_deref().and_then(sha256_file),
            path: cargo_path,
        },
        sysroot,
        components,
        adapter_version: ADAPTER_VERSION.to_string(),
        resolution_notes: notes,
    }
}

/// Parses `rustc -vV` output (`key: value` lines) into a lowercase-keyed map.
fn parse_rustc_vv(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect()
}

/// Derives "stable" / "beta" / "nightly" from a resolved `release` version
/// string (e.g. `1.99.0-nightly`, `1.98.0-beta.3`, `1.97.1`), independent of
/// whatever selector text was requested.
fn channel_of(release: &str) -> String {
    if release.contains("nightly") {
        "nightly".to_string()
    } else if release.contains("beta") {
        "beta".to_string()
    } else {
        "stable".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rustc_vv_fields() {
        let sample = "rustc 1.97.1 (8bab26f4f 2026-07-14) (Homebrew)\n\
                       binary: rustc\n\
                       commit-hash: 8bab26f4f68e0e26f0bb7960be334d5b520ea452\n\
                       commit-date: 2026-07-14\n\
                       host: aarch64-apple-darwin\n\
                       release: 1.97.1\n\
                       LLVM version: 22.1.8\n";
        let fields = parse_rustc_vv(sample);
        assert_eq!(fields.get("release").map(String::as_str), Some("1.97.1"));
        assert_eq!(
            fields.get("host").map(String::as_str),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(
            fields.get("llvm version").map(String::as_str),
            Some("22.1.8")
        );
        assert_eq!(
            fields.get("commit-hash").map(String::as_str),
            Some("8bab26f4f68e0e26f0bb7960be334d5b520ea452")
        );
    }

    #[test]
    fn parse_rustc_vv_ignores_lines_without_a_colon() {
        let fields =
            parse_rustc_vv("rustc 1.97.1 (8bab26f4f 2026-07-14) (Homebrew)\nrelease: 1.97.1\n");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields.get("release").map(String::as_str), Some("1.97.1"));
    }

    #[test]
    fn channel_of_detects_stable_beta_nightly() {
        assert_eq!(channel_of("1.97.1"), "stable");
        assert_eq!(channel_of("1.98.0-beta.3"), "beta");
        assert_eq!(channel_of("1.99.0-nightly"), "nightly");
    }
}

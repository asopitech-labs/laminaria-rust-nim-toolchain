//! Resolves the backend/target tools listed under `[tools.*]` in
//! `toolchains.lock.toml` (LLVM/Clang/LLD, `wasm-ld`, Binaryen `wasm-opt`,
//! `wasm-tools`, ...). Absence is recorded explicitly (`present: false`)
//! rather than the entry being omitted, so a doctor report shows exactly
//! which required tools are missing instead of silently having fewer rows.
//!
//! A `via = "rustup-llvm-tools"` entry is resolved from a Rust toolchain's
//! own sysroot (populated by `rustup component add llvm-tools`) instead of
//! `PATH`. That LLVM build is exactly the one bundled with the resolved
//! rustc — a more precise fingerprint than an independently-versioned
//! system LLVM install, and one this project already manages without a
//! system package manager.

use std::path::{Path, PathBuf};

use crate::exec::{extract_version_like, first_line, run, sha256_file, which};
use crate::lock::ToolSelector;
use crate::types::{ExecutableIdentity, ExternalToolFingerprint, RustToolchainFingerprint};

/// (logical name, executable name, version args)
const KNOWN_TOOLS: &[(&str, &str, &[&str])] = &[
    ("llvm", "llvm-config", &["--version"]),
    ("clang", "clang", &["--version"]),
    ("lld", "ld.lld", &["--version"]),
    ("wasm_ld", "wasm-ld", &["--version"]),
    ("wasm_opt", "wasm-opt", &["--version"]),
    ("wasm_tools", "wasm-tools", &["--version"]),
];

pub fn resolve(
    logical_name: &str,
    selector: &ToolSelector,
    rust_toolchains: &[RustToolchainFingerprint],
) -> ExternalToolFingerprint {
    let (_, executable, version_args) = KNOWN_TOOLS
        .iter()
        .find(|(name, _, _)| *name == logical_name)
        .copied()
        .unwrap_or((logical_name, logical_name, &["--version"]));

    let mut notes = Vec::new();
    let path = match selector.via.as_deref() {
        Some("rustup-llvm-tools") => {
            let found = find_in_rustup_llvm_tools(executable, rust_toolchains);
            if found.is_none() {
                notes.push(format!(
                    "'{executable}' not found under any resolved Rust toolchain's llvm-tools \
                     sysroot; run `rustup component add llvm-tools --toolchain <selector>`"
                ));
            }
            found
        }
        Some(other) => {
            let found = which(executable);
            if found.is_none() {
                notes.push(format!(
                    "'{executable}' not found on PATH for lock entry '{logical_name}' \
                     (via: '{other}', selector: '{}')",
                    selector.selector
                ));
            }
            found
        }
        None => {
            let found = which(executable);
            if found.is_none() {
                notes.push(format!(
                    "'{executable}' not found on PATH for lock entry '{logical_name}' \
                     (selector: '{}')",
                    selector.selector
                ));
            }
            found
        }
    };

    // Run the exact resolved binary rather than re-searching PATH, so the
    // reported version always matches the fingerprinted executable/digest.
    let version_output = path
        .as_deref()
        .and_then(|p| run(&p.to_string_lossy(), version_args));
    let resolved_version = version_output
        .as_deref()
        .map(first_line)
        .and_then(|l| extract_version_like(&l).or(Some(l)));

    ExternalToolFingerprint {
        logical_name: logical_name.to_string(),
        requested_selector: Some(selector.selector.clone()),
        present: path.is_some(),
        resolved_version,
        executable: ExecutableIdentity {
            digest_sha256: path.as_deref().and_then(sha256_file),
            path,
        },
        notes,
    }
}

/// Searches every resolved Rust toolchain's `lib/rustlib/<host>/bin` (and
/// its `gcc-ld` flavor-dispatch subdirectory, where `rustup component add
/// llvm-tools` places `ld.lld`/`wasm-ld`/`ld64.lld`/`lld-link`) for
/// `executable`.
fn find_in_rustup_llvm_tools(
    executable: &str,
    rust_toolchains: &[RustToolchainFingerprint],
) -> Option<PathBuf> {
    rust_toolchains.iter().find_map(|toolchain| {
        let sysroot = toolchain.sysroot.as_ref()?;
        let host = toolchain.host_triple.as_deref()?;
        find_in_sysroot_bin(sysroot, host, executable)
    })
}

fn find_in_sysroot_bin(sysroot: &Path, host_triple: &str, executable: &str) -> Option<PathBuf> {
    let bin = sysroot
        .join("lib")
        .join("rustlib")
        .join(host_triple)
        .join("bin");
    [bin.join("gcc-ld").join(executable), bin.join(executable)]
        .into_iter()
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("laminaria-external-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch_executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
    }

    #[test]
    fn finds_executable_in_gcc_ld_subdir() {
        let sysroot = scratch_dir("gcc-ld-hit");
        let host = "aarch64-apple-darwin";
        let bin = sysroot.join("lib").join("rustlib").join(host).join("bin");
        touch_executable(&bin.join("gcc-ld").join("wasm-ld"));

        let found = find_in_sysroot_bin(&sysroot, host, "wasm-ld");
        assert_eq!(found, Some(bin.join("gcc-ld").join("wasm-ld")));

        let _ = std::fs::remove_dir_all(&sysroot);
    }

    #[test]
    fn finds_executable_directly_in_bin() {
        let sysroot = scratch_dir("direct-bin-hit");
        let host = "aarch64-apple-darwin";
        let bin = sysroot.join("lib").join("rustlib").join(host).join("bin");
        touch_executable(&bin.join("opt"));

        let found = find_in_sysroot_bin(&sysroot, host, "opt");
        assert_eq!(found, Some(bin.join("opt")));

        let _ = std::fs::remove_dir_all(&sysroot);
    }

    #[test]
    fn returns_none_when_executable_is_absent() {
        let sysroot = scratch_dir("miss");
        let found = find_in_sysroot_bin(&sysroot, "aarch64-apple-darwin", "llvm-config");
        assert_eq!(found, None);
        let _ = std::fs::remove_dir_all(&sysroot);
    }

    #[test]
    fn find_in_rustup_llvm_tools_skips_toolchains_missing_sysroot_or_host() {
        let incomplete = RustToolchainFingerprint {
            logical_name: "no_sysroot".to_string(),
            requested_selector: None,
            compiler_family: "rust".to_string(),
            resolved_version: None,
            resolved_commit_hash: None,
            resolved_commit_date: None,
            host_triple: None,
            channel: None,
            llvm_version: None,
            rustc: ExecutableIdentity::default(),
            cargo_version: None,
            cargo: ExecutableIdentity::default(),
            sysroot: None,
            components: Vec::new(),
            adapter_version: "0.0.0".to_string(),
            resolution_notes: Vec::new(),
        };
        assert_eq!(
            find_in_rustup_llvm_tools("wasm-ld", std::slice::from_ref(&incomplete)),
            None
        );
    }
}

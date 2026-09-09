//! Resolves the backend/target tools listed under `[tools.*]` in
//! `toolchains.lock.toml` (LLVM/Clang/LLD, `wasm-ld`, Binaryen `wasm-opt`,
//! `wasm-tools`, ...). Absence is recorded explicitly (`present: false`)
//! rather than the entry being omitted, so a doctor report shows exactly
//! which required tools are missing instead of silently having fewer rows.

use crate::exec::{extract_version_like, first_line, run, sha256_file, which};
use crate::lock::ToolSelector;
use crate::types::{ExecutableIdentity, ExternalToolFingerprint};

/// (logical name, executable name, version args)
const KNOWN_TOOLS: &[(&str, &str, &[&str])] = &[
    ("llvm", "llvm-config", &["--version"]),
    ("clang", "clang", &["--version"]),
    ("lld", "ld.lld", &["--version"]),
    ("wasm_ld", "wasm-ld", &["--version"]),
    ("wasm_opt", "wasm-opt", &["--version"]),
    ("wasm_tools", "wasm-tools", &["--version"]),
];

pub fn resolve(logical_name: &str, selector: &ToolSelector) -> ExternalToolFingerprint {
    let (_, executable, version_args) = KNOWN_TOOLS
        .iter()
        .find(|(name, _, _)| *name == logical_name)
        .copied()
        .unwrap_or((logical_name, logical_name, &["--version"]));

    let path = which(executable);
    let mut notes = Vec::new();
    if path.is_none() {
        notes.push(format!(
            "'{executable}' not found on PATH for lock entry '{logical_name}' (selector: '{}')",
            selector.selector
        ));
    }

    let version_output = run(executable, version_args);
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

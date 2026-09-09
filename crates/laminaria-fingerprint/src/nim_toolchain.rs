//! Resolves each named Nim toolchain in `toolchains.lock.toml` to an exact
//! `NimToolchainFingerprint`. Nim 2 and Nimony/Nim 3 are meant to share this
//! same abstraction (`docs/multi-version-toolchains.md` section 2), but this
//! first pass only resolves whatever `nim`/`nimble` are active on PATH —
//! there is no `rustup`-equivalent multi-Nim switcher wired in yet, so a
//! mismatch between the requested selector and the resolved version is
//! surfaced as a note rather than silently ignored.

use crate::exec::{extract_version_like, first_line, run, sha256_file, which};
use crate::lock::NimToolchainSelector;
use crate::types::{ExecutableIdentity, NimToolchainFingerprint};

pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn resolve(logical_name: &str, selector: &NimToolchainSelector) -> NimToolchainFingerprint {
    let mut notes = Vec::new();

    let nim_path = which("nim");
    let nimble_path = which("nimble");
    let version_output = run("nim", &["--version"]);
    let nimble_version_output = run("nimble", &["--version"]);

    if nim_path.is_none() {
        notes.push("nim not found on PATH".to_string());
    }

    let (resolved_version, target_os, target_cpu) = version_output
        .as_deref()
        .map(parse_nim_version_line)
        .unwrap_or((None, None, None));

    if let Some(resolved) = &resolved_version {
        if !selector.selector.is_empty()
            && !resolved.starts_with(selector_prefix(&selector.selector))
        {
            notes.push(format!(
                "requested selector '{}' does not match resolved active Nim version '{}'; \
                 this crate does not yet manage multiple side-by-side Nim installs",
                selector.selector, resolved
            ));
        }
    }

    let compiled_at = version_output.as_deref().and_then(|text| {
        text.lines()
            .find(|l| l.starts_with("Compiled at"))
            .map(|l| l.trim_start_matches("Compiled at").trim().to_string())
    });

    NimToolchainFingerprint {
        logical_name: logical_name.to_string(),
        requested_selector: Some(selector.selector.clone()),
        compiler_family: "nim",
        resolved_version,
        target_os,
        target_cpu,
        compiled_at,
        nim: ExecutableIdentity {
            digest_sha256: nim_path.as_deref().and_then(sha256_file),
            path: nim_path,
        },
        nimble_version: nimble_version_output.as_deref().map(first_line),
        nimble: ExecutableIdentity {
            digest_sha256: nimble_path.as_deref().and_then(sha256_file),
            path: nimble_path,
        },
        adapter_version: ADAPTER_VERSION,
        resolution_notes: notes,
    }
}

/// `stable`/`latest`-style selectors have no numeric prefix to compare
/// against; treat them as always matching rather than emitting a false
/// mismatch note.
fn selector_prefix(selector: &str) -> &str {
    if selector.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        selector
    } else {
        ""
    }
}

/// Parses `Nim Compiler Version 2.2.10 [MacOSX: amd64]`.
fn parse_nim_version_line(text: &str) -> (Option<String>, Option<String>, Option<String>) {
    let line = first_line(text);
    let version = extract_version_like(&line);
    let bracket = line.split('[').nth(1).and_then(|s| s.strip_suffix(']'));
    let (os, cpu) = match bracket.and_then(|b| b.split_once(':')) {
        Some((os, cpu)) => (Some(os.trim().to_string()), Some(cpu.trim().to_string())),
        None => (None, None),
    };
    (version, os, cpu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nim_version_line() {
        let (version, os, cpu) = parse_nim_version_line(
            "Nim Compiler Version 2.2.10 [MacOSX: amd64]\nCompiled at 2026-04-24\n",
        );
        assert_eq!(version, Some("2.2.10".to_string()));
        assert_eq!(os, Some("MacOSX".to_string()));
        assert_eq!(cpu, Some("amd64".to_string()));
    }

    #[test]
    fn numeric_selector_matches_its_prefix() {
        assert_eq!(selector_prefix("2.2"), "2.2");
    }

    #[test]
    fn non_numeric_selector_never_triggers_a_mismatch_note() {
        assert_eq!(selector_prefix("stable"), "");
        assert_eq!(selector_prefix("latest"), "");
    }
}

//! Cargo `rust-version` (MSRV) / edition discovery and evaluation against
//! resolved Rust toolchains.
//!
//! `docs/multi-version-toolchains.md` section 3 requires these to be kept
//! as separate constraints rather than collapsed into one generic
//! `version` field: a package's declared MSRV, its edition, and the
//! compiler that actually resolved/ran are three different things. This
//! module discovers the first two from workspace manifests and checks them
//! against the third (a `RustToolchainFingerprint`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::RustToolchainFingerprint;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustPackageRequirement {
    pub package_name: String,
    pub manifest_path: PathBuf,
    /// Cargo `rust-version` (MSRV declaration), if the package declares one.
    pub rust_version: Option<String>,
    /// Rust edition (source/language mode) — independent of `rust_version`.
    pub edition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustRequirementEvaluation {
    pub package_name: String,
    pub toolchain_logical_name: String,
    pub rust_version: Option<String>,
    pub edition: Option<String>,
    pub resolved_compiler_version: Option<String>,
    /// `None` when there is nothing to check (no declared `rust-version`,
    /// or the resolved version could not be parsed) rather than defaulting
    /// to a possibly-wrong `true`/`false`.
    pub rust_version_satisfied: Option<bool>,
    pub notes: Vec<String>,
}

/// Discovers every `[workspace] members` package's `rust-version`/`edition`
/// declared in `<repo_root>/Cargo.toml`, resolving `field.workspace = true`
/// inheritance against `[workspace.package]`. Returns an empty list rather
/// than erroring when there is no root manifest or no workspace section —
/// not every repository doctor runs against is a Cargo workspace.
pub fn discover_workspace_requirements(repo_root: &Path) -> Vec<RustPackageRequirement> {
    let root_manifest = repo_root.join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(&root_manifest) else {
        return Vec::new();
    };
    let Ok(root_value) = text.parse::<toml::Value>() else {
        return Vec::new();
    };

    let workspace_package = root_value.get("workspace").and_then(|w| w.get("package"));
    let workspace_rust_version = workspace_package
        .and_then(|p| p.get("rust-version"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let workspace_edition = workspace_package
        .and_then(|p| p.get("edition"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let members: Vec<String> = root_value
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    members
        .into_iter()
        .filter_map(|member| {
            read_package_requirement(
                repo_root,
                &member,
                workspace_rust_version.as_deref(),
                workspace_edition.as_deref(),
            )
        })
        .collect()
}

fn read_package_requirement(
    repo_root: &Path,
    member: &str,
    workspace_rust_version: Option<&str>,
    workspace_edition: Option<&str>,
) -> Option<RustPackageRequirement> {
    let manifest_path = repo_root.join(member).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest_path).ok()?;
    let value = text.parse::<toml::Value>().ok()?;
    let package = value.get("package")?;

    let package_name = package
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(member)
        .to_string();

    let rust_version = inherited_string_field(package, "rust-version", workspace_rust_version);
    let edition = inherited_string_field(package, "edition", workspace_edition);

    Some(RustPackageRequirement {
        package_name,
        manifest_path,
        rust_version,
        edition,
    })
}

/// Cargo lets a package field be either a literal value or `field.workspace
/// = true`, which inherits from `[workspace.package]`.
fn inherited_string_field(
    package: &toml::Value,
    field: &str,
    workspace_value: Option<&str>,
) -> Option<String> {
    match package.get(field) {
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(toml::Value::Table(t))
            if t.get("workspace").and_then(|v| v.as_bool()) == Some(true) =>
        {
            workspace_value.map(str::to_string)
        }
        _ => None,
    }
}

/// Checks one package's `rust-version`/`edition` requirement against one
/// resolved toolchain. Does not attempt to model edition-availability by
/// compiler version (out of scope for #18) — only the MSRV/compiler-version
/// relationship is a solvable "does the resolved compiler qualify" check.
pub fn evaluate(
    requirement: &RustPackageRequirement,
    toolchain: &RustToolchainFingerprint,
) -> RustRequirementEvaluation {
    let mut notes = Vec::new();

    let rust_version_satisfied = match (&requirement.rust_version, &toolchain.resolved_version) {
        (Some(min), Some(resolved)) => match (parse_version(min), parse_version(resolved)) {
            (Some(min_v), Some(resolved_v)) => Some(resolved_v >= min_v),
            _ => {
                notes.push(format!(
                    "could not compare rust-version '{min}' against resolved compiler version '{resolved}'"
                ));
                None
            }
        },
        (Some(_), None) => {
            notes.push(
                "toolchain did not resolve to a compiler version; cannot check rust-version"
                    .to_string(),
            );
            None
        }
        (None, _) => None,
    };

    RustRequirementEvaluation {
        package_name: requirement.package_name.clone(),
        toolchain_logical_name: toolchain.logical_name.clone(),
        rust_version: requirement.rust_version.clone(),
        edition: requirement.edition.clone(),
        resolved_compiler_version: toolchain.resolved_version.clone(),
        rust_version_satisfied,
        notes,
    }
}

fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let mut parts = s.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toolchain_with_version(version: &str) -> RustToolchainFingerprint {
        RustToolchainFingerprint {
            logical_name: "system_stable".to_string(),
            requested_selector: Some("stable".to_string()),
            compiler_family: "rust".to_string(),
            resolved_version: Some(version.to_string()),
            resolved_commit_hash: None,
            resolved_commit_date: None,
            host_triple: None,
            channel: None,
            llvm_version: None,
            rustc: crate::types::ExecutableIdentity::default(),
            cargo_version: None,
            cargo: crate::types::ExecutableIdentity::default(),
            sysroot: None,
            components: Vec::new(),
            adapter_version: "0.0.0".to_string(),
            resolution_notes: Vec::new(),
        }
    }

    #[test]
    fn parses_two_and_three_component_versions() {
        assert_eq!(parse_version("1.74"), Some((1, 74, 0)));
        assert_eq!(parse_version("1.97.1"), Some((1, 97, 1)));
    }

    #[test]
    fn msrv_satisfied_when_resolved_is_newer() {
        let req = RustPackageRequirement {
            package_name: "demo".to_string(),
            manifest_path: PathBuf::new(),
            rust_version: Some("1.74".to_string()),
            edition: Some("2021".to_string()),
        };
        let eval = evaluate(&req, &toolchain_with_version("1.97.1"));
        assert_eq!(eval.rust_version_satisfied, Some(true));
        assert!(eval.notes.is_empty());
    }

    #[test]
    fn msrv_violated_when_resolved_is_older() {
        let req = RustPackageRequirement {
            package_name: "demo".to_string(),
            manifest_path: PathBuf::new(),
            rust_version: Some("1.90".to_string()),
            edition: None,
        };
        let eval = evaluate(&req, &toolchain_with_version("1.74.0"));
        assert_eq!(eval.rust_version_satisfied, Some(false));
    }

    #[test]
    fn no_declared_rust_version_means_no_constraint() {
        let req = RustPackageRequirement {
            package_name: "demo".to_string(),
            manifest_path: PathBuf::new(),
            rust_version: None,
            edition: None,
        };
        let eval = evaluate(&req, &toolchain_with_version("1.74.0"));
        assert_eq!(eval.rust_version_satisfied, None);
        assert!(eval.notes.is_empty());
    }

    #[test]
    fn inherited_workspace_field_resolves_when_marked_true() {
        let package: toml::Value = toml::from_str("rust-version.workspace = true").unwrap();
        assert_eq!(
            inherited_string_field(&package, "rust-version", Some("1.74")),
            Some("1.74".to_string())
        );
    }

    #[test]
    fn literal_field_is_used_as_is_even_with_a_workspace_value_present() {
        let package: toml::Value = toml::from_str("rust-version = \"1.60\"").unwrap();
        assert_eq!(
            inherited_string_field(&package, "rust-version", Some("1.74")),
            Some("1.60".to_string())
        );
    }
}

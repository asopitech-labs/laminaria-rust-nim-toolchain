//! Issue #48 (G1) Checkpoint D: a pure-text parser for a real
//! `nimble.lock` file -- `nimble lock`'s own JSON output. Read-only,
//! never invoked as a subprocess (that boundary lives entirely in
//! `laminaria-run`'s file-reading orchestration, not here); this
//! module only ever turns real lock-file text into typed facts, the
//! same discipline [`crate::nimble_manifest_discover`] uses for the
//! `.nimble` manifest itself.

use std::collections::BTreeMap;

/// One real dependency/toolchain identity a `nimble.lock` pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimbleLockPinnedPackage {
    pub version: String,
    pub vcs_revision: Option<String>,
}

/// The real facts a `nimble.lock` file declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimbleLockFacts {
    /// The lock file's own schema revision (its top-level `"version"`
    /// field) -- distinct from any pinned package's own version.
    pub schema_version: u64,
    pub pinned_packages: BTreeMap<String, NimbleLockPinnedPackage>,
}

/// A real structural defect in lock-file text -- invalid JSON, or
/// JSON that isn't shaped like a real `nimble.lock` -- reported
/// directly rather than guessed past.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedNimbleLock {
    pub detail: String,
}

impl std::fmt::Display for MalformedNimbleLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "malformed nimble.lock: {}", self.detail)
    }
}

impl std::error::Error for MalformedNimbleLock {}

/// Parses real `nimble.lock` text. Never invents a schema version or a
/// pinned package's own version -- a missing or wrongly-typed field is
/// a [`MalformedNimbleLock`], not a default value.
pub fn parse_nimble_lock(text: &str) -> Result<NimbleLockFacts, MalformedNimbleLock> {
    let json: serde_json::Value = serde_json::from_str(text).map_err(|e| MalformedNimbleLock {
        detail: format!("not valid JSON: {e}"),
    })?;
    let schema_version = json
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| MalformedNimbleLock {
            detail: "no numeric top-level 'version' (lock schema revision)".to_string(),
        })?;
    let packages_obj = json
        .get("packages")
        .and_then(|v| v.as_object())
        .ok_or_else(|| MalformedNimbleLock {
            detail: "no 'packages' object".to_string(),
        })?;
    let mut pinned_packages = BTreeMap::new();
    for (name, entry) in packages_obj {
        let version = entry
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MalformedNimbleLock {
                detail: format!("pinned package '{name}' has no string 'version'"),
            })?
            .to_string();
        let vcs_revision = entry
            .get("vcsRevision")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        pinned_packages.insert(
            name.clone(),
            NimbleLockPinnedPackage {
                version,
                vcs_revision,
            },
        );
    }
    Ok(NimbleLockFacts {
        schema_version,
        pinned_packages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_lock_file_is_parsed_fully() {
        let text = r#"{
  "version": 2,
  "packages": {
    "nim": {
      "version": "2.2.10",
      "vcsRevision": "9fe2137fa2f3f66cf5a44f357d461829ac9e20c4",
      "url": "https://github.com/nim-lang/Nim.git",
      "downloadMethod": "git",
      "dependencies": [],
      "checksums": { "sha1": "75a7771f179e45551c60347d4e5e883028c3deff" }
    }
  },
  "tasks": {}
}"#;
        let facts = parse_nimble_lock(text).expect("must parse");
        assert_eq!(facts.schema_version, 2);
        let nim = facts
            .pinned_packages
            .get("nim")
            .expect("nim must be pinned");
        assert_eq!(nim.version, "2.2.10");
        assert_eq!(
            nim.vcs_revision.as_deref(),
            Some("9fe2137fa2f3f66cf5a44f357d461829ac9e20c4")
        );
    }

    #[test]
    fn invalid_json_is_a_malformed_lock() {
        let result = parse_nimble_lock("{ not json");
        assert!(result.is_err());
    }

    #[test]
    fn missing_packages_object_is_a_malformed_lock() {
        let result = parse_nimble_lock(r#"{"version": 2}"#);
        assert!(result.is_err());
    }

    #[test]
    fn a_pinned_package_missing_its_version_is_a_malformed_lock() {
        let result = parse_nimble_lock(r#"{"version": 2, "packages": {"nim": {}}}"#);
        assert!(result.is_err());
    }
}

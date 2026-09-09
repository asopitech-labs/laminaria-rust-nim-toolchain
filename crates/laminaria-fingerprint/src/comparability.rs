//! Environment comparability policy.
//!
//! `docs/measurement-foundation.md` section 2 and the "Environment rule" in
//! `docs/issue-plan.md`: a reproducible environment and a representative
//! performance environment are different concerns, WSL/native Linux/macOS/
//! containers are distinct environment classes, and Runs with different
//! `EnvironmentFingerprint` values are not directly comparable for
//! performance by default. This is the structural guard #18's acceptance
//! criteria require ("different environment/toolchain fingerprints are not
//! silently compared as one performance baseline"); later Run-comparison
//! work (#19/#21) is expected to call this rather than reinvent it.

use crate::types::EnvironmentFingerprint;

/// Checks whether two `EnvironmentFingerprint` captures may be treated as
/// the same performance baseline. `Ok(())` means every comparability-load-
/// bearing field matched (or was unobserved on both sides, which is a gap
/// to note elsewhere, not by itself a mismatch); `Err(reasons)` lists every
/// field that differs, in a stable field order, so a caller can display the
/// full picture rather than only the first mismatch found.
pub fn environments_comparable(
    a: &EnvironmentFingerprint,
    b: &EnvironmentFingerprint,
) -> Result<(), Vec<String>> {
    let mut reasons = Vec::new();

    push_if_differs(&mut reasons, "os", Some(&a.os), Some(&b.os));
    push_if_differs(
        &mut reasons,
        "architecture",
        Some(&a.architecture),
        Some(&b.architecture),
    );
    push_if_differs(
        &mut reasons,
        "environment_class",
        Some(&a.environment_class),
        Some(&b.environment_class),
    );
    push_if_differs(
        &mut reasons,
        "os_version",
        a.os_version.as_ref(),
        b.os_version.as_ref(),
    );
    push_if_differs(
        &mut reasons,
        "cpu_model",
        a.cpu_model.as_ref(),
        b.cpu_model.as_ref(),
    );
    push_if_differs(
        &mut reasons,
        "filesystem_type",
        a.filesystem_type.as_ref(),
        b.filesystem_type.as_ref(),
    );

    if reasons.is_empty() {
        Ok(())
    } else {
        Err(reasons)
    }
}

fn push_if_differs<T: PartialEq + std::fmt::Display>(
    reasons: &mut Vec<String>,
    label: &str,
    a: Option<T>,
    b: Option<T>,
) {
    if let (Some(a), Some(b)) = (a, b) {
        if a != b {
            reasons.push(format!("{label} differs: '{a}' vs '{b}'"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn base_environment() -> EnvironmentFingerprint {
        EnvironmentFingerprint {
            schema_version: crate::types::SCHEMA_VERSION.to_string(),
            captured_at_unix: 0,
            os: "macos".to_string(),
            os_version: Some("26.6.2".to_string()),
            kernel: None,
            architecture: "arm64".to_string(),
            cpu_model: Some("Apple M3".to_string()),
            cpu_physical_cores: None,
            cpu_logical_cores: None,
            memory_bytes: None,
            filesystem_type: Some("apfs".to_string()),
            environment_class: "native".to_string(),
            repository: crate::types::RepositoryState {
                commit: None,
                dirty: None,
            },
            sdk_path: None,
            measurement_harness: "test".to_string(),
            architecture_notice: None,
            self_process_translation_notice: None,
            path_toolchain_shadow: None,
            allowed_environment_variables: BTreeMap::new(),
            unobserved_fields: Vec::new(),
        }
    }

    #[test]
    fn identical_environments_are_comparable() {
        let a = base_environment();
        let b = base_environment();
        assert_eq!(environments_comparable(&a, &b), Ok(()));
    }

    #[test]
    fn different_environment_class_is_not_comparable() {
        let a = base_environment();
        let mut b = base_environment();
        b.environment_class = "wsl".to_string();
        let err = environments_comparable(&a, &b).unwrap_err();
        assert!(err.iter().any(|r| r.contains("environment_class")));
    }

    #[test]
    fn different_architecture_is_not_comparable() {
        let a = base_environment();
        let mut b = base_environment();
        b.architecture = "x86_64".to_string();
        let err = environments_comparable(&a, &b).unwrap_err();
        assert!(err.iter().any(|r| r.contains("architecture")));
    }

    #[test]
    fn reports_every_mismatch_not_just_the_first() {
        let a = base_environment();
        let mut b = base_environment();
        b.environment_class = "container".to_string();
        b.cpu_model = Some("Different CPU".to_string());
        let err = environments_comparable(&a, &b).unwrap_err();
        assert_eq!(err.len(), 2);
    }

    #[test]
    fn unobserved_field_on_either_side_is_not_treated_as_a_mismatch() {
        let a = base_environment();
        let mut b = base_environment();
        b.cpu_model = None;
        assert_eq!(environments_comparable(&a, &b), Ok(()));
    }
}

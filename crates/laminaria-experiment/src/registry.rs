//! Issue #28 D1-a: reads and validates
//! `docs/design/issue-35-d0-cases.yaml` -- case ID uniqueness, `refs`
//! resolution, required-field presence, and cross-checks against the
//! D0-confirmed values (`issue35-d0-accepted-c812d70-v1`) that the
//! instructor and this session independently computed and agreed on.
//!
//! **A case's registration is never recorded as an execution success.**
//! `Case::d1_disposition` classifies every case into exactly one of
//! `Implemented` (D1 runs real code and produces a real result),
//! `ReferenceHeld` (an existing fixture, case metadata only), or
//! `ConfigurationOnly` (`reached_stage: configuration-definition` --
//! M9/M10's product code is not implemented in D1 at all, per the
//! spec's own R5/round-2 correction). Only `Implemented` cases can ever
//! report a `pass_criteria.d1` result; the other two are reported as
//! exactly what they are.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CasesFile {
    pub schema_version: String,
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
pub struct EditSpec {
    pub kind: String,
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExpectedSpec {
    pub kind: String,
    pub value_or_diagnostic: String,
    pub derivation: String,
}

#[derive(Debug, Deserialize)]
pub struct SubsetScope {
    #[serde(default)]
    pub d1_verifies: serde_yaml::Value,
    #[serde(default)]
    pub future_work: serde_yaml::Value,
}

#[derive(Debug, Deserialize)]
pub struct PassCriteria {
    pub d1: String,
    pub d4: String,
}

#[derive(Debug, Deserialize)]
pub struct Case {
    pub id: String,
    pub pattern: String,
    pub execution_role: String,
    pub reached_stage: String,
    pub origin: String,
    #[serde(default)]
    pub refs: Vec<String>,
    pub source_layout: serde_yaml::Value,
    #[serde(default)]
    pub dependency_edges: Option<Vec<String>>,
    #[serde(default)]
    pub non_dependency: Option<String>,
    pub requested_artifacts: serde_yaml::Value,
    pub demand_mode: String,
    pub edit: EditSpec,
    #[serde(default)]
    pub forbidden_work: Vec<String>,
    #[serde(default)]
    pub required_work: Vec<String>,
    pub expected: ExpectedSpec,
    pub subset_scope: SubsetScope,
    pub scale: serde_yaml::Value,
    pub measurement: serde_yaml::Value,
    pub pass_criteria: PassCriteria,
}

pub const VALID_EXECUTION_ROLES: [&str; 3] = ["owned", "reference", "bootstrap"];
pub const VALID_REACHED_STAGES: [&str; 3] = [
    "configuration-definition",
    "source-ir-evaluation",
    "target-generation-execution",
];
pub const VALID_ORIGINS: [&str; 4] = ["self", "fixture-existing", "fixture-new", "self-planned"];
pub const VALID_PATTERNS: [&str; 10] =
    ["M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9", "M10"];

/// D0-confirmed values (`issue35-d0-accepted-c812d70-v1`): every
/// substring that must appear verbatim in the named case's
/// `expected.value_or_diagnostic`. A case not listed here has no
/// numeric-golden cross-check (its `expected` is a diagnostic/structural
/// claim, checked only for presence).
pub fn d0_confirmed_value_substrings() -> HashMap<&'static str, Vec<&'static str>> {
    let mut m = HashMap::new();
    m.insert(
        "M7-long-chain-wide-branches-small",
        vec!["152668892010644049"],
    );
    m.insert(
        "M7-long-chain-wide-branches-medium",
        vec!["975184859065030187", "15936356680776682716"],
    );
    m.insert(
        "M7-long-chain-wide-branches-large",
        vec!["233828575729373081"],
    );
    m.insert(
        "M9-fingerprint-compat-chain",
        vec![
            "9b541417be6e7a89f2cff354eb48574f387c9006ff94e8e182d111653f255b13",
            "2e2d1b0f00b38cc4cb54e37d22d1ee8e96bd5ea4773cc1b25b68a9ac141748d9",
        ],
    );
    m
}

/// One of three mutually exclusive D1 dispositions -- never conflated
/// with "executed successfully".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum D1Disposition {
    /// D1 implements real code and/or executes an existing owned path,
    /// producing a real, checkable result.
    Implemented,
    /// An existing fixture is reused as-is; D1 adds case metadata only,
    /// never modifying the fixture's own source/assertions.
    ReferenceHeld,
    /// `reached_stage: configuration-definition` -- D1 finalizes the
    /// case *definition* only. No code is implemented or executed for
    /// this case in D1; it must never be reported as a pass/fail
    /// execution result.
    ConfigurationOnly,
}

impl Case {
    pub fn d1_disposition(&self) -> D1Disposition {
        if self.reached_stage == "configuration-definition" {
            D1Disposition::ConfigurationOnly
        } else if self.origin == "fixture-existing" {
            D1Disposition::ReferenceHeld
        } else {
            D1Disposition::Implemented
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    DuplicateId(String),
    DanglingRef {
        case_id: String,
        ref_id: String,
    },
    InvalidExecutionRole {
        case_id: String,
        value: String,
    },
    InvalidReachedStage {
        case_id: String,
        value: String,
    },
    InvalidOrigin {
        case_id: String,
        value: String,
    },
    InvalidPattern {
        case_id: String,
        value: String,
    },
    MissingConfirmedValueSubstring {
        case_id: String,
        expected_substring: String,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::DuplicateId(id) => write!(f, "duplicate case id: {id}"),
            ValidationError::DanglingRef { case_id, ref_id } => {
                write!(f, "{case_id}: refs unknown case id {ref_id}")
            }
            ValidationError::InvalidExecutionRole { case_id, value } => write!(
                f,
                "{case_id}: execution_role {value:?} is not one of {VALID_EXECUTION_ROLES:?}"
            ),
            ValidationError::InvalidReachedStage { case_id, value } => write!(
                f,
                "{case_id}: reached_stage {value:?} is not one of {VALID_REACHED_STAGES:?}"
            ),
            ValidationError::InvalidOrigin { case_id, value } => write!(
                f,
                "{case_id}: origin {value:?} is not one of {VALID_ORIGINS:?}"
            ),
            ValidationError::InvalidPattern { case_id, value } => write!(
                f,
                "{case_id}: pattern {value:?} is not one of {VALID_PATTERNS:?}"
            ),
            ValidationError::MissingConfirmedValueSubstring {
                case_id,
                expected_substring,
            } => write!(
                f,
                "{case_id}: expected.value_or_diagnostic does not contain the D0-confirmed \
                 value {expected_substring:?}"
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

pub struct Registry {
    pub cases: Vec<Case>,
}

impl Registry {
    pub fn load_from_str(yaml: &str) -> Result<Self, serde_yaml::Error> {
        let file: CasesFile = serde_yaml::from_str(yaml)?;
        Ok(Registry { cases: file.cases })
    }

    pub fn load_from_path(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::load_from_str(&text).map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }

    pub fn get(&self, id: &str) -> Option<&Case> {
        self.cases.iter().find(|c| c.id == id)
    }

    /// Checks ID uniqueness, `refs` resolution, enum-valued field
    /// membership, and D0-confirmed-value cross-checks. Returns every
    /// violation found (not just the first), sorted by nothing in
    /// particular -- callers report them all.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        let mut seen_ids: HashMap<&str, usize> = HashMap::new();
        for case in &self.cases {
            *seen_ids.entry(case.id.as_str()).or_insert(0) += 1;
        }
        for (id, count) in &seen_ids {
            if *count > 1 {
                errors.push(ValidationError::DuplicateId(id.to_string()));
            }
        }

        let known_ids: std::collections::HashSet<&str> =
            self.cases.iter().map(|c| c.id.as_str()).collect();

        for case in &self.cases {
            for r in &case.refs {
                if !known_ids.contains(r.as_str()) {
                    errors.push(ValidationError::DanglingRef {
                        case_id: case.id.clone(),
                        ref_id: r.clone(),
                    });
                }
            }
            if !VALID_EXECUTION_ROLES.contains(&case.execution_role.as_str()) {
                errors.push(ValidationError::InvalidExecutionRole {
                    case_id: case.id.clone(),
                    value: case.execution_role.clone(),
                });
            }
            if !VALID_REACHED_STAGES.contains(&case.reached_stage.as_str()) {
                errors.push(ValidationError::InvalidReachedStage {
                    case_id: case.id.clone(),
                    value: case.reached_stage.clone(),
                });
            }
            if !VALID_ORIGINS.contains(&case.origin.as_str()) {
                errors.push(ValidationError::InvalidOrigin {
                    case_id: case.id.clone(),
                    value: case.origin.clone(),
                });
            }
            if !VALID_PATTERNS.contains(&case.pattern.as_str()) {
                errors.push(ValidationError::InvalidPattern {
                    case_id: case.id.clone(),
                    value: case.pattern.clone(),
                });
            }
        }

        // Cross-checks a case's `expected` against the D0-confirmed value
        // only if that case is actually present in *this* registry --
        // this table names cases from the real design file, but the
        // validator must stay usable against smaller synthetic
        // registries (tests) without reporting "case not present" as a
        // structural defect. `full_registry_is_missing_a_d0_confirmed_case`
        // (below) is the dedicated check for the real file specifically.
        for (case_id, substrings) in d0_confirmed_value_substrings() {
            if let Some(case) = self.get(case_id) {
                for substring in substrings {
                    if !case.expected.value_or_diagnostic.contains(substring) {
                        errors.push(ValidationError::MissingConfirmedValueSubstring {
                            case_id: case_id.to_string(),
                            expected_substring: substring.to_string(),
                        });
                    }
                }
            }
        }

        errors
    }
}

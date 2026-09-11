//! Issue #28 D1-a regression coverage for the case registry: the real
//! `docs/design/issue-35-d0-cases.yaml` must load cleanly with exactly
//! 20 cases and zero validation errors, and the validator must actually
//! catch each class of defect a prior review round found by hand
//! (duplicate id, dangling ref, invalid enum value, a golden value that
//! doesn't match D0's confirmed value) -- not just pass on the happy
//! path.

use laminaria_experiment::registry::{Registry, ValidationError};

fn real_registry() -> Registry {
    let path = laminaria_experiment::default_cases_yaml_path();
    Registry::load_from_path(&path).unwrap_or_else(|e| panic!("failed to load {path:?}: {e}"))
}

#[test]
fn the_real_registry_has_exactly_twenty_cases() {
    let registry = real_registry();
    assert_eq!(registry.cases.len(), 20);
}

#[test]
fn the_real_registry_validates_with_zero_errors() {
    let registry = real_registry();
    let errors = registry.validate();
    assert!(
        errors.is_empty(),
        "expected zero validation errors, got: {errors:?}"
    );
}

#[test]
fn every_case_has_exactly_one_d1_disposition_and_configuration_only_cases_are_never_implemented() {
    use laminaria_experiment::registry::D1Disposition;

    let registry = real_registry();
    for case in &registry.cases {
        if case.reached_stage == "configuration-definition" {
            assert_eq!(
                case.d1_disposition(),
                D1Disposition::ConfigurationOnly,
                "{}: reached_stage=configuration-definition must never be classified as \
                 Implemented or ReferenceHeld",
                case.id
            );
        }
    }
}

const MINIMAL_VALID_YAML: &str = r#"
schema_version: "0.2.0-draft"
cases:
  - id: A
    pattern: M1
    execution_role: reference
    reached_stage: target-generation-execution
    origin: self
    refs: []
    source_layout: "test"
    dependency_edges: []
    requested_artifacts: ["x"]
    demand_mode: full
    edit: { kind: none, target: null }
    forbidden_work: []
    required_work: []
    expected:
      kind: value
      value_or_diagnostic: "42"
      derivation: "test"
    subset_scope:
      d1_verifies: []
      future_work: []
    scale: { small: null, medium: null, large: null, seed: null }
    measurement:
      comparison_modes: []
      cpu_budget: []
      memory_state: [normal]
      cache_state: [cold]
      warmup_runs: 0
      repetitions: 1
      noise_floor: "not-applicable"
    pass_criteria:
      d1: "test"
      d4: not-applicable-yet
"#;

fn parse(yaml: &str) -> Registry {
    Registry::load_from_str(yaml).expect("fixture YAML must parse")
}

#[test]
fn the_minimal_fixture_itself_validates_cleanly() {
    let registry = parse(MINIMAL_VALID_YAML);
    assert_eq!(registry.cases.len(), 1);
    assert!(registry.validate().is_empty());
}

#[test]
fn duplicate_ids_are_caught() {
    let yaml = format!(
        "{}\n  - id: A\n    pattern: M2\n    execution_role: reference\n    reached_stage: target-generation-execution\n    origin: self\n    refs: []\n    source_layout: \"test2\"\n    dependency_edges: []\n    requested_artifacts: [\"y\"]\n    demand_mode: full\n    edit: {{ kind: none, target: null }}\n    forbidden_work: []\n    required_work: []\n    expected:\n      kind: value\n      value_or_diagnostic: \"7\"\n      derivation: \"test\"\n    subset_scope:\n      d1_verifies: []\n      future_work: []\n    scale: {{ small: null, medium: null, large: null, seed: null }}\n    measurement:\n      comparison_modes: []\n      cpu_budget: []\n      memory_state: [normal]\n      cache_state: [cold]\n      warmup_runs: 0\n      repetitions: 1\n      noise_floor: \"not-applicable\"\n    pass_criteria:\n      d1: \"test\"\n      d4: not-applicable-yet\n",
        MINIMAL_VALID_YAML
    );
    let registry = parse(&yaml);
    assert_eq!(registry.cases.len(), 2);
    let errors = registry.validate();
    assert!(
        errors.contains(&ValidationError::DuplicateId("A".to_string())),
        "expected a DuplicateId error, got: {errors:?}"
    );
}

#[test]
fn a_dangling_ref_is_caught() {
    let yaml = MINIMAL_VALID_YAML.replace("refs: []", "refs: [\"does-not-exist\"]");
    let registry = parse(&yaml);
    let errors = registry.validate();
    assert!(
        errors.contains(&ValidationError::DanglingRef {
            case_id: "A".to_string(),
            ref_id: "does-not-exist".to_string(),
        }),
        "expected a DanglingRef error, got: {errors:?}"
    );
}

#[test]
fn an_undeclared_execution_role_is_caught() {
    let yaml =
        MINIMAL_VALID_YAML.replace("execution_role: reference", "execution_role: self-planned");
    let registry = parse(&yaml);
    let errors = registry.validate();
    assert!(
        errors.contains(&ValidationError::InvalidExecutionRole {
            case_id: "A".to_string(),
            value: "self-planned".to_string(),
        }),
        "expected an InvalidExecutionRole error (self-planned is an origin value, not an \
         execution_role value), got: {errors:?}"
    );
}

#[test]
fn a_golden_value_that_drifts_from_the_d0_confirmed_value_is_caught() {
    // Simulates exactly the class of defect a prior review round found:
    // an M7 case whose `expected` no longer contains the independently
    // confirmed aggregate value.
    let yaml = MINIMAL_VALID_YAML
        .replace("id: A", "id: M7-long-chain-wide-branches-small")
        .replace(
            "value_or_diagnostic: \"42\"",
            "value_or_diagnostic: \"999999999999999999\"",
        );
    let registry = parse(&yaml);
    let errors = registry.validate();
    assert!(
        errors.contains(&ValidationError::MissingConfirmedValueSubstring {
            case_id: "M7-long-chain-wide-branches-small".to_string(),
            expected_substring: "152668892010644049".to_string(),
        }),
        "expected a MissingConfirmedValueSubstring error, got: {errors:?}"
    );
}

#[test]
fn the_real_registry_actually_contains_every_d0_confirmed_case() {
    // The cross-check above only fires for cases present in the
    // registry being validated (so smaller synthetic registries in
    // this file don't spuriously fail) -- this test is what actually
    // guards against the real design file quietly dropping one of the
    // named D0-confirmed cases.
    use laminaria_experiment::registry::d0_confirmed_value_substrings;
    let registry = real_registry();
    for case_id in d0_confirmed_value_substrings().keys() {
        assert!(
            registry.get(case_id).is_some(),
            "the real registry is missing D0-confirmed case {case_id:?}"
        );
    }
}

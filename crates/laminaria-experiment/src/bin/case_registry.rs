//! `cargo run -p laminaria-experiment --bin laminaria-case-registry`
//!
//! Reads and validates `docs/design/issue-35-d0-cases.yaml`, reporting
//! every case's D1 disposition (Implemented / ReferenceHeld /
//! ConfigurationOnly -- never conflated with "executed successfully")
//! and any structural violation. Exits non-zero on any validation
//! error.

use laminaria_experiment::registry::{D1Disposition, Registry};

fn main() {
    let path = laminaria_experiment::default_cases_yaml_path();
    let registry = match Registry::load_from_path(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("failed to load {}: {e}", path.display());
            std::process::exit(2);
        }
    };

    println!(
        "loaded {} cases from {}",
        registry.cases.len(),
        path.display()
    );

    let mut implemented = 0;
    let mut reference_held = 0;
    let mut configuration_only = 0;
    for case in &registry.cases {
        match case.d1_disposition() {
            D1Disposition::Implemented => implemented += 1,
            D1Disposition::ReferenceHeld => reference_held += 1,
            D1Disposition::ConfigurationOnly => configuration_only += 1,
        }
    }
    println!(
        "disposition: {implemented} Implemented, {reference_held} ReferenceHeld, \
         {configuration_only} ConfigurationOnly (none of these three counts is an execution result)"
    );

    let errors = registry.validate();
    if errors.is_empty() {
        println!("validation: OK, no errors");
        std::process::exit(0);
    }

    eprintln!("validation: {} error(s)", errors.len());
    for e in &errors {
        eprintln!("  - {e}");
    }
    std::process::exit(1);
}

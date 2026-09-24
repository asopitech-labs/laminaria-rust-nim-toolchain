//! Issue #50's existing locked Rust workload, through the owned source-fact
//! collector and pure feedback planner. Cargo is used only after that check as
//! the reference executable oracle; it is never a producer on this path.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use laminaria_ir::rust_dependency_discover::discover_external_crate_references;
use laminaria_ir::rust_generic_demand::{
    discover_generic_functions, discover_generic_instances,
    RustGenericInstance as DiscoveredGenericInstance,
};
use laminaria_plan::rust_cross_layer::{plan_rust_cross_layer, RustCrossLayerInput, RustWorkStage};
use laminaria_plan::rust_cross_layer::{
    plan_rust_generic_work, RustGenericInstance as PlannedGenericInstance,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn locked_packages(lock: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(lock)
        .unwrap()
        .lines()
        .filter_map(|line| line.trim().strip_prefix("name = \"")?.strip_suffix('\"'))
        .map(str::to_string)
        .collect()
}

fn declared_dependencies(manifest: &Path) -> BTreeSet<String> {
    let content = std::fs::read_to_string(manifest).unwrap();
    let mut in_dependencies = false;
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('[') {
                in_dependencies = line == "[dependencies]";
                return None;
            }
            in_dependencies
                .then(|| {
                    line.split_once('=')
                        .map(|(name, _)| name.trim().to_string())
                })
                .flatten()
        })
        .collect()
}

#[test]
fn feedback_from_real_rust_source_prunes_existing_locked_workspace_members_before_compilation() {
    let workspace = repo_root().join("fixtures/many-unrequested-targets/small");
    let source = std::fs::read_to_string(workspace.join("crates/fixture-bin/src/main.rs")).unwrap();
    let semantic_references = discover_external_crate_references(&source).unwrap();
    let plan = plan_rust_cross_layer(&RustCrossLayerInput {
        requested_artifact: "fixture-bin".to_string(),
        entry_package: "fixture-bin".to_string(),
        package_candidates: locked_packages(&workspace.join("Cargo.lock")),
        declared_dependency_packages: declared_dependencies(
            &workspace.join("crates/fixture-bin/Cargo.toml"),
        ),
        semantic_references,
    })
    .expect("the lock must provide each source-referenced crate");

    assert_eq!(
        plan.selected_packages,
        ["fixture-bin", "used-core", "used-util"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert_eq!(plan.pruned_packages.len(), 4);
    assert!(plan
        .pruned_packages
        .iter()
        .all(|package| package.starts_with("unused-pkg-")));
    assert!(plan.feedback_work.iter().all(|work| {
        !work.package.starts_with("unused-pkg-")
            && matches!(
                work.stage,
                RustWorkStage::Parse
                    | RustWorkStage::Typecheck
                    | RustWorkStage::Lower
                    | RustWorkStage::Codegen
            )
    }));
    assert_eq!(plan.eager_work.len() - plan.feedback_work.len(), 16);
}

#[test]
fn generic_feedback_keeps_requested_fixture_instance_and_prunes_test_only_instance() {
    let workspace = repo_root().join("fixtures/rust-heavy-workspace");
    let core = std::fs::read_to_string(workspace.join("crates/fixture-core/src/lib.rs")).unwrap();
    let binary = std::fs::read_to_string(workspace.join("crates/fixture-bin/src/main.rs")).unwrap();
    let functions = discover_generic_functions("fixture-core", &core).unwrap();
    let requested = discover_generic_instances("fixture-bin", &binary, &functions, false).unwrap();
    let test_demand = discover_generic_instances("fixture-core", &core, &functions, true).unwrap();
    let eager: BTreeSet<DiscoveredGenericInstance> =
        requested.union(&test_demand).cloned().collect();

    let to_plan_identity = |instance: &DiscoveredGenericInstance| PlannedGenericInstance {
        package: instance.package.replace('-', "_"),
        function: instance.function.clone(),
        type_arguments: instance.type_arguments.clone(),
    };
    let eager = eager.iter().map(to_plan_identity).collect();
    let requested = requested.iter().map(to_plan_identity).collect();
    let plan = plan_rust_generic_work(&eager, &requested)
        .expect("requested generic work must be included in the conservative eager inventory");

    assert_eq!(plan.eager_work.len(), 4);
    assert_eq!(plan.feedback_work.len(), 2);
    assert_eq!(plan.pruned_work.len(), 2);
    assert!(plan.feedback_work.iter().all(|work| {
        work.instance.package == "fixture_core" && work.instance.type_arguments == ["i64"]
    }));
    assert!(plan.pruned_work.iter().all(|work| {
        work.instance.package == "fixture_core" && work.instance.type_arguments == ["i32"]
    }));

    let core_tests = test_demand.iter().map(to_plan_identity).collect();
    let test_plan = plan_rust_generic_work(&eager, &core_tests)
        .expect("the core-test request must retain its own generic demand");
    assert!(test_plan.feedback_work.iter().all(|work| {
        work.instance.package == "fixture_core" && work.instance.type_arguments == ["i32"]
    }));
    assert!(test_plan.pruned_work.iter().all(|work| {
        work.instance.package == "fixture_core" && work.instance.type_arguments == ["i64"]
    }));
}

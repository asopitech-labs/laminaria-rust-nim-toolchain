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
use laminaria_plan::rust_cross_layer::{
    plan_rust_artifact_feedback, plan_rust_generic_work, RustArtifactFeedbackInput,
    RustCrossLayerInput, RustGenericInstance as PlannedGenericInstance,
};
use laminaria_plan::rust_cross_layer::{plan_rust_cross_layer, RustWorkStage};

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
        package: instance.package.clone(),
        function: instance.function.clone(),
        type_arguments: instance.type_arguments.clone(),
    };
    let eager = eager.iter().map(to_plan_identity).collect();
    let requested = requested.iter().map(to_plan_identity).collect();
    let plan = plan_rust_generic_work(&eager, &requested)
        .expect("requested generic work must be included in the conservative eager inventory");

    assert_eq!(plan.eager_work.len(), 10);
    assert_eq!(plan.feedback_work.len(), 5);
    assert_eq!(plan.pruned_work.len(), 5);
    assert!(plan.feedback_work.iter().all(|work| {
        work.instance.package == "fixture-core" && work.instance.type_arguments == ["i64"]
    }));
    assert!(plan.pruned_work.iter().all(|work| {
        work.instance.package == "fixture-core" && work.instance.type_arguments == ["i32"]
    }));

    let core_tests = test_demand.iter().map(to_plan_identity).collect();
    let test_plan = plan_rust_generic_work(&eager, &core_tests)
        .expect("the core-test request must retain its own generic demand");
    assert!(test_plan.feedback_work.iter().all(|work| {
        work.instance.package == "fixture-core" && work.instance.type_arguments == ["i32"]
    }));
    assert!(test_plan.pruned_work.iter().all(|work| {
        work.instance.package == "fixture-core" && work.instance.type_arguments == ["i64"]
    }));
}

#[test]
fn artifact_feedback_joins_cargo_workspace_packages_to_fixture_generic_demand() {
    let workspace = repo_root().join("fixtures/rust-heavy-workspace");
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(workspace.join("Cargo.toml"))
        .exec()
        .expect("Cargo metadata must describe the locked workspace");
    let workspace_packages: BTreeSet<_> = metadata.workspace_members.iter().cloned().collect();
    let package_candidates: BTreeSet<String> = metadata
        .packages
        .iter()
        .filter(|package| workspace_packages.contains(&package.id))
        .map(|package| package.name.clone())
        .collect();
    let binary_package = metadata
        .packages
        .iter()
        .find(|package| package.name == "fixture-bin")
        .expect("the requested executable package is in Cargo metadata");
    let declared_dependency_packages = binary_package
        .dependencies
        .iter()
        .filter(|dependency| {
            !matches!(
                dependency.kind,
                cargo_metadata::DependencyKind::Development | cargo_metadata::DependencyKind::Build
            )
        })
        .map(|dependency| dependency.name.clone())
        .collect();
    let core = std::fs::read_to_string(workspace.join("crates/fixture-core/src/lib.rs")).unwrap();
    let binary = std::fs::read_to_string(workspace.join("crates/fixture-bin/src/main.rs")).unwrap();
    let known = discover_generic_functions("fixture-core", &core).unwrap();
    let requested = discover_generic_instances("fixture-bin", &binary, &known, false).unwrap();
    let test_demand = discover_generic_instances("fixture-core", &core, &known, true).unwrap();
    let eager_discovered: BTreeSet<DiscoveredGenericInstance> =
        requested.union(&test_demand).cloned().collect();
    let to_planned = |instance: &DiscoveredGenericInstance| PlannedGenericInstance {
        package: instance.package.clone(),
        function: instance.function.clone(),
        type_arguments: instance.type_arguments.clone(),
    };
    let eager_instances: BTreeSet<PlannedGenericInstance> =
        eager_discovered.iter().map(to_planned).collect();
    let requested_instances: BTreeSet<PlannedGenericInstance> =
        requested.iter().map(to_planned).collect();
    let binary_references = discover_external_crate_references(&binary).unwrap();
    let plan = plan_rust_artifact_feedback(&RustArtifactFeedbackInput {
        package_input: RustCrossLayerInput {
            requested_artifact: "fixture-bin".to_string(),
            entry_package: "fixture-bin".to_string(),
            package_candidates,
            declared_dependency_packages,
            semantic_references: binary_references,
        },
        eager_instances: eager_instances.clone(),
        requested_instances,
    })
    .expect("the executable's packages must include its generic provider");

    assert_eq!(
        plan.package_plan.selected_packages,
        ["fixture-bin", "fixture-core", "fixture-mid"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert_eq!(plan.generic_work_plan.feedback_work.len(), 5);
    assert_eq!(plan.generic_work_plan.pruned_work.len(), 5);
    assert!(plan.generic_work_plan.feedback_work.iter().all(|work| {
        work.instance.function == "sum_generic" && work.instance.type_arguments == ["i64"]
    }));

    let core_package = metadata
        .packages
        .iter()
        .find(|package| package.name == "fixture-core")
        .unwrap();
    let core_tests = test_demand.iter().map(to_planned).collect();
    let test_plan = plan_rust_artifact_feedback(&RustArtifactFeedbackInput {
        package_input: RustCrossLayerInput {
            requested_artifact: "fixture-core-tests".to_string(),
            entry_package: core_package.name.clone(),
            package_candidates: metadata
                .packages
                .iter()
                .filter(|package| workspace_packages.contains(&package.id))
                .map(|package| package.name.clone())
                .collect(),
            declared_dependency_packages: BTreeSet::new(),
            semantic_references: BTreeSet::new(),
        },
        eager_instances,
        requested_instances: core_tests,
    })
    .expect("the core-test request must include its generic provider");
    assert_eq!(
        test_plan.package_plan.selected_packages,
        BTreeSet::from(["fixture-core".to_string()])
    );
    assert!(test_plan
        .generic_work_plan
        .feedback_work
        .iter()
        .all(|work| { work.instance.type_arguments == ["i32"] }));
}

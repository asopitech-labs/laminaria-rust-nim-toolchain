use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use laminaria_plan::rust_cross_layer::{
    plan_rust_artifact_feedback, RustArtifactFeedbackInput, RustCrossLayerInput, RustExecutionWork,
    RustWork, RustWorkStage,
};
use laminaria_plan::{ExecutionPlan, PLAN_SCHEMA_VERSION, PRODUCED_BY};
use laminaria_run::compiler_work_executor::{
    build_execution_receipt, independent_chain_actions, run_compiler_work_plan_with_dispatch_trace,
    validate_execution_receipt, ArtifactStore, ExecutionReceiptError,
};

fn combined_plan(
    first: &[laminaria_plan::Action],
    second: &[laminaria_plan::Action],
) -> ExecutionPlan {
    let actions = first
        .iter()
        .chain(second)
        .cloned()
        .map(|action| (action.id.clone(), action))
        .collect::<BTreeMap<_, _>>();
    let ordered_actions = actions.keys().cloned().collect();
    ExecutionPlan {
        schema_version: PLAN_SCHEMA_VERSION.to_string(),
        produced_by: PRODUCED_BY.to_string(),
        producer_version: PLAN_SCHEMA_VERSION.to_string(),
        plan_id: "independent-consumer-positive".to_string(),
        ordered_actions,
        actions,
        physical_work: BTreeMap::new(),
    }
}

#[test]
fn independent_consumer_accepts_feedback_receipt_and_rejects_tampering() {
    let root = std::env::temp_dir().join(format!(
        "laminaria-independent-consumer-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create isolated source directory");
    let first_path = root.join("first.rs");
    let second_path = root.join("second.rs");
    let first_source = "fn f(x: i32) -> i32 { x }\n";
    let second_source = "fn f(x: i32) -> i32 { x } \n";
    std::fs::write(&first_path, first_source).expect("write first source");
    std::fs::write(&second_path, second_source).expect("write second source");
    let (first, first_artifact) = independent_chain_actions(
        "rust",
        &first_path.to_string_lossy(),
        first_source,
        "f",
        &[vec![1]],
    );
    let (second, _) = independent_chain_actions(
        "rust",
        &second_path.to_string_lossy(),
        second_source,
        "f",
        &[vec![1]],
    );
    let plan = combined_plan(&first, &second);
    let feedback = plan_rust_artifact_feedback(&RustArtifactFeedbackInput {
        package_input: RustCrossLayerInput {
            requested_artifact: "app".to_string(),
            entry_package: "app".to_string(),
            package_candidates: BTreeSet::from(["app".to_string(), "unused".to_string()]),
            declared_dependency_packages: BTreeSet::new(),
            semantic_references: BTreeSet::new(),
        },
        eager_instances: BTreeSet::new(),
        requested_instances: BTreeSet::new(),
    })
    .expect("entry package must be selected");
    let action_work = first
        .iter()
        .zip([
            RustWorkStage::Parse,
            RustWorkStage::Typecheck,
            RustWorkStage::Lower,
        ])
        .map(|(action, stage)| {
            (
                action.id.clone(),
                RustExecutionWork::Package(RustWork {
                    package: "app".to_string(),
                    stage,
                }),
            )
        })
        .chain(
            second
                .iter()
                .zip([
                    RustWorkStage::Parse,
                    RustWorkStage::Typecheck,
                    RustWorkStage::Lower,
                ])
                .map(|(action, stage)| {
                    (
                        action.id.clone(),
                        RustExecutionWork::Package(RustWork {
                            package: "unused".to_string(),
                            stage,
                        }),
                    )
                }),
        )
        .collect::<BTreeMap<_, _>>();
    let selected = feedback.select_action_ids(&action_work, true);
    let eager_selected = plan.actions.keys().cloned().collect::<BTreeSet<_>>();

    let mut eager_store = ArtifactStore::new();
    let (eager_result, eager_successful) = run_compiler_work_plan_with_dispatch_trace(
        &plan,
        &mut eager_store,
        NonZeroUsize::new(1).unwrap(),
    );
    eager_result.expect("eager producer must complete");
    let eager_receipt =
        build_execution_receipt(&plan, eager_selected, eager_successful, &eager_store);
    validate_execution_receipt(&plan, &eager_receipt, &first_artifact)
        .expect("independent consumer accepts eager positive artifact");

    let feedback_plan =
        laminaria_run::compiler_work_executor::restrict_execution_plan(&plan, &selected)
            .expect("feedback selection must preserve producer closure");
    let mut feedback_store = ArtifactStore::new();
    let (feedback_result, feedback_successful) = run_compiler_work_plan_with_dispatch_trace(
        &feedback_plan,
        &mut feedback_store,
        NonZeroUsize::new(1).unwrap(),
    );
    feedback_result.expect("feedback producer must complete");
    let feedback_receipt = build_execution_receipt(
        &feedback_plan,
        selected,
        feedback_successful,
        &feedback_store,
    );
    validate_execution_receipt(&plan, &feedback_receipt, &first_artifact)
        .expect("independent consumer accepts feedback positive artifact");

    let mut tampered = feedback_receipt.clone();
    tampered
        .successful_actions
        .insert("not-in-plan".to_string());
    assert!(matches!(
        validate_execution_receipt(&plan, &tampered, &first_artifact),
        Err(ExecutionReceiptError::UnknownAction(action)) if action == "not-in-plan"
    ));
}

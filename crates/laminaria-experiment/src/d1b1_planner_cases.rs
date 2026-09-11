//! Issue #28 D1-b1: `M2-nim-planner-shared-module` and `M3-topology`.
//! Both cases are about the *real* Nim planner (`nim-planner/src/
//! planning_kernel.nim`) and its two real consumers -- this module never
//! re-implements `plan()`, it only re-runs what already exists and
//! records the result.

use std::path::PathBuf;
use std::process::Command;

use laminaria_plan::{Action, ActionKind, ArtifactRef, PlanOutcome, PlanningInput};

use crate::d1b1_reference_cases::CaseEvidence;
use crate::planner_binary::{repo_root, resolve_or_build_planner_binary};

fn runs_dir() -> PathBuf {
    repo_root().join("runs").join("d1b1")
}

fn write_log(case_id: &str, content: &str) -> Result<PathBuf, String> {
    let dir = runs_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{case_id}.log"));
    std::fs::write(&path, content)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok(path)
}

/// The same structural shape `nim-planner/tests/test_planning_kernel.nim`'s
/// own "a linear chain plans in dependency order" test (lines 23-30) and
/// `crates/laminaria-plan/src/nim_planner_client.rs`'s own (private)
/// `sample_input()` both use: two independent build actions feeding one
/// `Integrate` action, demand naming only the integrate output. Built
/// fresh here (rather than importing the private test helper) so this
/// case exercises `laminaria_plan::call_planner`'s real JSON-over-stdio
/// path against the production binary, cross-checked below against
/// `test_planning_kernel`'s own in-process assertion of the identical
/// shape.
fn linear_chain_input() -> PlanningInput {
    PlanningInput::new(
        vec!["done".to_string()],
        vec![
            Action {
                id: "integrate".to_string(),
                kind: ActionKind::Integrate,
                command_identity: "integrate".to_string(),
                inputs: vec![
                    ArtifactRef::declared("planner-bin"),
                    ArtifactRef::declared("host-bin"),
                ],
                outputs: vec![ArtifactRef::declared("done")],
                compiler_work: None,
            },
            Action {
                id: "compile-rust-host".to_string(),
                kind: ActionKind::CargoBuild,
                command_identity: "cargo build".to_string(),
                inputs: vec![],
                outputs: vec![ArtifactRef::declared("host-bin")],
                compiler_work: None,
            },
            Action {
                id: "compile-nim-planner".to_string(),
                kind: ActionKind::NimBuild,
                command_identity: "nimble build".to_string(),
                inputs: vec![],
                outputs: vec![ArtifactRef::declared("planner-bin")],
                compiler_work: None,
            },
        ],
    )
}

const EXPECTED_ORDER: [&str; 3] = ["compile-nim-planner", "compile-rust-host", "integrate"];

/// M2-nim-planner-shared-module. `execution_role: reference` (D0-fixed).
/// Two pieces of evidence, per the case's own `required_work`/`expected`:
/// (1) source-level confirmation both binaries `import` the same
/// `planning_kernel` module (not a copy) -- grepped directly from the
/// real source files; (2) the same underlying `plan()` produces the same
/// result whether called in-process (`test_planning_kernel`'s own
/// "same input plans identically twice" test, `nim-planner/tests/
/// test_planning_kernel.nim:34-44`) or via the production binary's real
/// JSON-over-stdio path (`laminaria_plan::call_planner`) -- run 3 times
/// independently (`measurement.repetitions: 3`).
pub fn run_m2_nim_planner_shared_module() -> Result<CaseEvidence, String> {
    let case_id = "M2-nim-planner-shared-module";
    let root = repo_root();
    let mut log = String::new();

    // (1) Source-level import check -- both files' own text, not an
    // assumption.
    let planner_src = std::fs::read_to_string(root.join("nim-planner/src/laminaria_planner.nim"))
        .map_err(|e| format!("failed to read laminaria_planner.nim: {e}"))?;
    let test_src = std::fs::read_to_string(root.join("nim-planner/tests/test_planning_kernel.nim"))
        .map_err(|e| format!("failed to read test_planning_kernel.nim: {e}"))?;
    let planner_imports = planner_src.contains("planning_kernel");
    let test_imports = test_src.contains("import ../src/planning_kernel");
    log.push_str(&format!(
        "laminaria_planner.nim references planning_kernel: {planner_imports}\n\
         test_planning_kernel.nim imports ../src/planning_kernel: {test_imports}\n"
    ));

    // (2a) In-process determinism via the unittest binary, 3 independent
    // process invocations.
    let mut unittest_pass = true;
    for i in 0..3 {
        let output = Command::new("nim")
            .args([
                "c",
                "-r",
                "--nimcache:nimcache",
                "tests/test_planning_kernel.nim",
            ])
            .current_dir(root.join("nim-planner"))
            .output()
            .map_err(|e| format!("failed to spawn nim c -r: {e}"))?;
        log.push_str(&format!(
            "--- test_planning_kernel run {i} ---\nexit={:?}\nstdout:\n{}\nstderr:\n{}\n",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
        if !output.status.success() {
            unittest_pass = false;
        }
    }

    // (2b) The same linear-chain shape through the real production
    // binary, 3 independent invocations, cross-checked against the exact
    // order test_planning_kernel.nim's own "a linear chain plans in
    // dependency order" test asserts.
    let planner_bin = resolve_or_build_planner_binary()?;
    let input = linear_chain_input();
    let mut binary_pass = true;
    let mut orders = Vec::new();
    for i in 0..3 {
        let outcome = laminaria_plan::call_planner(&planner_bin, &input)
            .map_err(|e| format!("call_planner failed on repetition {i}: {e}"))?;
        match outcome {
            PlanOutcome::Planned(plan) => {
                let matches_expected = plan.ordered_actions == EXPECTED_ORDER;
                log.push_str(&format!(
                    "--- production binary call {i} ---\nordered_actions={:?}\nmatches_expected={matches_expected}\n",
                    plan.ordered_actions
                ));
                orders.push(plan.ordered_actions.clone());
                if !matches_expected {
                    binary_pass = false;
                }
            }
            PlanOutcome::Rejected(r) => {
                binary_pass = false;
                log.push_str(&format!(
                    "--- production binary call {i} ---\nREJECTED: {r:?}\n"
                ));
            }
        }
    }
    let all_orders_identical = orders.windows(2).all(|w| w[0] == w[1]);

    let pass =
        planner_imports && test_imports && unittest_pass && binary_pass && all_orders_identical;
    let log_path = write_log(case_id, &log)?;

    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "reference".to_string(),
        reproduction_commands: vec![
            "cd nim-planner && nim c -r --nimcache:nimcache tests/test_planning_kernel.nim  # x3"
                .to_string(),
            "laminaria_plan::call_planner(<production laminaria-planner binary>, \
             <linear-chain PlanningInput, same shape as test_planning_kernel.nim's own \
             'a linear chain plans in dependency order' test>)  # x3"
                .to_string(),
        ],
        raw_log_path: log_path,
        pass,
        summary: format!(
            "planner_imports={planner_imports} test_imports={test_imports} \
             unittest_pass={unittest_pass} binary_pass={binary_pass} \
             all_3_binary_orders_identical={all_orders_identical} \
             (expected_order={EXPECTED_ORDER:?})"
        ),
    })
}

/// M3-topology. `execution_role: bootstrap` (D0-fixed -- this case
/// observes the self-build's own non-connected CargoBuild/NimBuild
/// assembly, not owned-scheduler evidence). D0's own `derivation`
/// names the exact existing test this case's `expected` value comes
/// from; D1's own `subset_scope.d1_verifies` is "既存テストの再実行"
/// (rerun the existing test) -- this reruns it rather than
/// reimplementing its assertions separately. The second half of the
/// case's `pass_criteria.d1` ("CargoBuild/NimBuild never reach
/// compiler_work_executor") is confirmed by rerunning
/// `a_legacy_delegated_build_action_kind_is_rejected_not_silently_run`,
/// plus a direct source citation of the allowlist `compiler_work_executor.rs`
/// checks *before* ever dispatching an action
/// (`compiler_work_executor.rs:820-831`).
pub fn run_m3_topology() -> Result<CaseEvidence, String> {
    let case_id = "M3-topology";
    let root = repo_root();

    let plan_test = Command::new("cargo")
        .args([
            "test",
            "-p",
            "laminaria-plan",
            "--lib",
            "nim_planner_client::tests::call_planner_against_the_real_binary_produces_a_deterministic_plan",
            "--",
            "--exact",
            "--nocapture",
        ])
        .current_dir(&root)
        .output()
        .map_err(|e| format!("failed to run laminaria-plan test: {e}"))?;

    let executor_test = Command::new("cargo")
        .args([
            "test",
            "-p",
            "laminaria-run",
            "--lib",
            "compiler_work_executor::tests::a_legacy_delegated_build_action_kind_is_rejected_not_silently_run",
            "--",
            "--exact",
            "--nocapture",
        ])
        .current_dir(&root)
        .output()
        .map_err(|e| format!("failed to run laminaria-run test: {e}"))?;

    let plan_test_pass = plan_test.status.success();
    let executor_test_pass = executor_test.status.success();

    let executor_src =
        std::fs::read_to_string(root.join("crates/laminaria-run/src/compiler_work_executor.rs"))
            .map_err(|e| format!("failed to read compiler_work_executor.rs: {e}"))?;
    // Structural evidence: dispatch_action's own allowlist match, kept as
    // a source-text citation alongside the behavioral test above (not a
    // substitute for it).
    let allowlist_present = executor_src.contains(
        "ActionKind::LowerSource\n            | ActionKind::ValidateIr\n            | ActionKind::TransformFunction\n            | ActionKind::EvaluateEvidence",
    );

    let log = format!(
        "--- cargo test -p laminaria-plan call_planner_against_the_real_binary_produces_a_deterministic_plan ---\nexit={:?}\nstdout:\n{}\nstderr:\n{}\n\n--- cargo test -p laminaria-run a_legacy_delegated_build_action_kind_is_rejected_not_silently_run ---\nexit={:?}\nstdout:\n{}\nstderr:\n{}\n\nallowlist_present_in_source={allowlist_present} (compiler_work_executor.rs:820-831)\n",
        plan_test.status.code(),
        String::from_utf8_lossy(&plan_test.stdout),
        String::from_utf8_lossy(&plan_test.stderr),
        executor_test.status.code(),
        String::from_utf8_lossy(&executor_test.stdout),
        String::from_utf8_lossy(&executor_test.stderr),
    );
    let log_path = write_log(case_id, &log)?;

    let pass = plan_test_pass && executor_test_pass && allowlist_present;

    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "bootstrap".to_string(),
        reproduction_commands: vec![
            "cargo test -p laminaria-plan --lib \
             nim_planner_client::tests::call_planner_against_the_real_binary_produces_a_deterministic_plan"
                .to_string(),
            "cargo test -p laminaria-run --lib \
             compiler_work_executor::tests::a_legacy_delegated_build_action_kind_is_rejected_not_silently_run"
                .to_string(),
        ],
        raw_log_path: log_path,
        pass,
        summary: format!(
            "plan_test_pass={plan_test_pass} (ordered_actions == [compile-nim-planner, \
             compile-rust-host, integrate]) executor_rejects_delegated_kinds_pass={executor_test_pass} \
             allowlist_present={allowlist_present}"
        ),
    })
}

pub fn run_all() -> Result<Vec<CaseEvidence>, String> {
    Ok(vec![
        run_m2_nim_planner_shared_module()?,
        run_m3_topology()?,
    ])
}

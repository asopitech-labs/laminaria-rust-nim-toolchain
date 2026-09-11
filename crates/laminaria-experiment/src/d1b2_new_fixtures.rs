//! Issue #28 D1-b2: wires the two *new* fixtures
//! `docs/design/issue-35-d0-cases.yaml` names (`fixtures/
//! long-chain-wide-branches` for M7, `fixtures/many-unrequested-targets`
//! for M8-many-unrequested-cargo) into re-runnable evidence, matching the
//! `d1b1_reference_cases`/`d1b1_planner_cases` pattern: reproduce, don't
//! re-select source layout, scale, edit, or expected value -- all
//! D0-fixed.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use crate::d1b1_reference_cases::CaseEvidence;
use crate::planner_binary::repo_root;

fn runs_dir() -> PathBuf {
    repo_root().join("runs").join("d1b2")
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

fn run_and_capture(cwd: &std::path::Path, args: &[&str]) -> Result<(bool, String), String> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("failed to spawn cargo {args:?}: {e}"))?;
    let transcript = format!(
        "cwd={}\ncargo {}\n--- stdout ---\n{}\n--- stderr ---\n{}\nexit_code={:?}\n",
        cwd.display(),
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        output.status.code()
    );
    Ok((output.status.success(), transcript))
}

fn compiling_crate_names(log: &str) -> Vec<String> {
    let mut names: Vec<String> = log
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("Compiling "))
        .map(|rest| rest.split_whitespace().next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

fn extract_result_value(log: &str) -> Option<u64> {
    log.lines()
        .find_map(|l| l.split("result=").nth(1))
        .and_then(|s| s.trim().parse::<u64>().ok())
}

/// M7-long-chain-wide-branches-small. `execution_role: reference`.
pub fn run_m7_small() -> Result<CaseEvidence, String> {
    let case_id = "M7-long-chain-wide-branches-small";
    let dir = repo_root().join("fixtures/long-chain-wide-branches");
    let _ = std::fs::remove_dir_all(dir.join("target"));

    let (build_ok, build_log) = run_and_capture(&dir, &["build", "-p", "aggregator-small", "-v"])?;
    let stage_01_count = build_log.matches("Compiling stage-01 ").count();
    let stage_02_count = build_log.matches("Compiling stage-02 ").count();

    let (run_ok, run_log) = run_and_capture(&dir, &["run", "-p", "aggregator-small"])?;
    let result = extract_result_value(&run_log);

    let pass = build_ok
        && run_ok
        && stage_01_count == 1
        && stage_02_count == 1
        && result == Some(152_668_892_010_644_049);

    let log_path = write_log(
        case_id,
        &format!(
            "{build_log}\n====\n{run_log}\nstage_01_compiled_count={stage_01_count} \
             stage_02_compiled_count={stage_02_count} result={result:?}\n"
        ),
    )?;

    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "reference".to_string(),
        reproduction_commands: vec![
            "cd fixtures/long-chain-wide-branches && rm -rf target && cargo build -p aggregator-small -v".to_string(),
            "cargo run -p aggregator-small".to_string(),
        ],
        raw_log_path: log_path,
        pass,
        summary: format!(
            "result={result:?} (expected 152668892010644049) stage-01 compiled \
             {stage_01_count}x, stage-02 compiled {stage_02_count}x (both must be exactly 1, \
             despite stage-02 being referenced by leaf-a-small/leaf-b-small/stage-03)"
        ),
    })
}

/// M7-long-chain-wide-branches-medium. `execution_role: reference`. The
/// edit scenario mutates `crates/stage-04/src/lib.rs` in place and
/// always reverts it before returning, even on an early error -- the
/// fixture's source must never be left mutated.
pub fn run_m7_medium() -> Result<CaseEvidence, String> {
    let case_id = "M7-long-chain-wide-branches-medium";
    let dir = repo_root().join("fixtures/long-chain-wide-branches");
    let stage04_path = dir.join("crates/stage-04/src/lib.rs");
    let original = std::fs::read_to_string(&stage04_path)
        .map_err(|e| format!("failed to read stage-04/src/lib.rs: {e}"))?;
    const BEFORE_CONST: &str = "0xff51afd7ed558ccd";
    const AFTER_CONST: &str = "0xc4ceb9fe1a85ec53";
    if !original.contains(BEFORE_CONST) {
        return Err(format!(
            "stage-04/src/lib.rs does not contain the expected pre-edit constant {BEFORE_CONST} \
             -- refusing to edit a file that isn't in the expected starting state"
        ));
    }

    type EditCycleResult = Result<(bool, String, Option<u64>, Option<u64>, Vec<String>), String>;
    let result: EditCycleResult = (|| {
        let _ = std::fs::remove_dir_all(dir.join("target"));
        let (before_ok, before_log) = run_and_capture(&dir, &["run", "-p", "aggregator-medium"])?;
        let before_value = extract_result_value(&before_log);

        let edited = original.replace(BEFORE_CONST, AFTER_CONST);
        std::fs::write(&stage04_path, &edited)
            .map_err(|e| format!("failed to apply the edit: {e}"))?;

        let (edit_build_ok, edit_build_log) =
            run_and_capture(&dir, &["build", "-p", "aggregator-medium", "-v"])?;
        let rebuilt = compiling_crate_names(&edit_build_log);
        let (after_ok, after_log) = run_and_capture(&dir, &["run", "-p", "aggregator-medium"])?;
        let after_value = extract_result_value(&after_log);

        let ok = before_ok && edit_build_ok && after_ok;
        let combined = format!("{before_log}\n====\n{edit_build_log}\n====\n{after_log}");
        Ok((ok, combined, before_value, after_value, rebuilt))
    })();

    // Always revert, even if the closure above errored partway through.
    std::fs::write(&stage04_path, &original).map_err(|e| {
        format!("CRITICAL: failed to revert stage-04/src/lib.rs after editing: {e}")
    })?;

    let (ran_ok, combined_log, before_value, after_value, rebuilt) = result?;

    let expected_rebuild: std::collections::BTreeSet<&str> = [
        "aggregator-medium",
        "leaf-a-medium",
        "leaf-b-medium",
        "leaf-c-medium",
        "leaf-matrix-sum-medium",
        "stage-04",
        "stage-05",
        "stage-06",
        "stage-07",
        "stage-08",
    ]
    .into_iter()
    .collect();
    let actual_rebuild: std::collections::BTreeSet<&str> =
        rebuilt.iter().map(|s| s.as_str()).collect();
    let rebuild_set_matches = actual_rebuild == expected_rebuild;
    let no_upstream_rebuild = !actual_rebuild.contains("stage-01")
        && !actual_rebuild.contains("stage-02")
        && !actual_rebuild.contains("stage-03");

    let pass = ran_ok
        && before_value == Some(975_184_859_065_030_187)
        && after_value == Some(15_936_356_680_776_682_716)
        && rebuild_set_matches
        && no_upstream_rebuild;

    let log_path = write_log(
        case_id,
        &format!(
            "{combined_log}\n====\nbefore_value={before_value:?} after_value={after_value:?} \
             rebuilt={rebuilt:?} rebuild_set_matches={rebuild_set_matches} \
             no_upstream_rebuild={no_upstream_rebuild}\n"
        ),
    )?;

    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "reference".to_string(),
        reproduction_commands: vec![
            "cd fixtures/long-chain-wide-branches && cargo run -p aggregator-medium  # before edit, expect 975184859065030187".to_string(),
            "sed -i 's/0xff51afd7ed558ccd/0xc4ceb9fe1a85ec53/' crates/stage-04/src/lib.rs".to_string(),
            "cargo build -p aggregator-medium -v  # expect exactly {stage-04..08, 4 leaf-*-medium, aggregator-medium}, never stage-01/02/03".to_string(),
            "cargo run -p aggregator-medium  # after edit, expect 15936356680776682716".to_string(),
            "git checkout -- crates/stage-04/src/lib.rs".to_string(),
        ],
        raw_log_path: log_path,
        pass,
        summary: format!(
            "before={before_value:?} (expected 975184859065030187) \
             after={after_value:?} (expected 15936356680776682716) \
             rebuild_set_matches={rebuild_set_matches} no_upstream_rebuild={no_upstream_rebuild} \
             rebuilt={rebuilt:?}"
        ),
    })
}

/// M7-long-chain-wide-branches-large. `execution_role: reference`.
/// `measurement.warmup_runs: 1, repetitions: 3` -- axis 2 (payload
/// volume) is this binary's own release-mode wall-clock run time
/// (dominated by the two `n=80` `leaf-matrix-sum-core` calls); axis 1
/// (source volume) is `leaf-matrix-sum-core`'s already-fixed 4-function
/// split, confirmed structurally by reading its own source, never timed
/// -- see `aggregator-large`'s own doc comment for why these stay
/// separate.
pub fn run_m7_large() -> Result<CaseEvidence, String> {
    let case_id = "M7-long-chain-wide-branches-large";
    let dir = repo_root().join("fixtures/long-chain-wide-branches");
    let _ = std::fs::remove_dir_all(dir.join("target"));

    // Axis 1: a structural fact about the source, not a timing.
    let matrix_sum_core_src =
        std::fs::read_to_string(dir.join("crates/leaf-matrix-sum-core/src/lib.rs"))
            .map_err(|e| format!("failed to read leaf-matrix-sum-core/src/lib.rs: {e}"))?;
    let axis1_functions = ["generate_row", "compute_cell", "row_sum", "total_sum"];
    let axis1_all_present = axis1_functions
        .iter()
        .all(|f| matrix_sum_core_src.contains(&format!("fn {f}(")));

    // Warmup (discarded, matches measurement.warmup_runs: 1).
    let (warmup_ok, warmup_log) =
        run_and_capture(&dir, &["run", "--release", "-p", "aggregator-large"])?;
    if !warmup_ok {
        let log_path = write_log(case_id, &warmup_log)?;
        return Ok(CaseEvidence {
            case_id: case_id.to_string(),
            execution_role: "reference".to_string(),
            reproduction_commands: vec![
                "cd fixtures/long-chain-wide-branches && cargo run --release -p aggregator-large"
                    .to_string(),
            ],
            raw_log_path: log_path,
            pass: false,
            summary: "warmup run failed before any measured repetition".to_string(),
        });
    }

    // 3 measured repetitions (axis 2: wall-clock timing).
    let mut combined_log = warmup_log;
    let mut wall_seconds = Vec::new();
    let mut all_pass = true;
    let mut last_value = None;
    for _ in 0..3 {
        let start = Instant::now();
        let (ok, log) = run_and_capture(&dir, &["run", "--release", "-p", "aggregator-large"])?;
        wall_seconds.push(start.elapsed().as_secs_f64());
        combined_log.push_str("\n====\n");
        combined_log.push_str(&log);
        last_value = extract_result_value(&log);
        if !ok || last_value != Some(233_828_575_729_373_081) {
            all_pass = false;
        }
    }

    let pass = all_pass && axis1_all_present;
    let mean = wall_seconds.iter().sum::<f64>() / wall_seconds.len().max(1) as f64;

    let log_path = write_log(
        case_id,
        &format!(
            "{combined_log}\n====\naxis1_functions_present={axis1_all_present} \
             axis2_wall_seconds={wall_seconds:?} last_value={last_value:?}\n"
        ),
    )?;

    Ok(CaseEvidence {
        case_id: case_id.to_string(),
        execution_role: "reference".to_string(),
        reproduction_commands: vec![
            "cd fixtures/long-chain-wide-branches && cargo run --release -p aggregator-large  # 1 warmup + 3 measured repetitions".to_string(),
        ],
        raw_log_path: log_path,
        pass,
        summary: format!(
            "result={last_value:?} (expected 233828575729373081, 3/3 repetitions) \
             axis1(source-volume, structural)={axis1_all_present} \
             axis2(payload-volume, wall_seconds)={wall_seconds:?} mean={mean:.4}"
        ),
    })
}

/// M8-many-unrequested-cargo (one scale). `execution_role: reference`.
fn run_m8_scale(scale: &str, expected_unused: usize) -> Result<CaseEvidence, String> {
    let case_id = format!("M8-many-unrequested-cargo-{scale}");
    let dir = repo_root()
        .join("fixtures/many-unrequested-targets")
        .join(scale);
    let _ = std::fs::remove_dir_all(dir.join("target"));

    let (build_ok, build_log) = run_and_capture(&dir, &["build", "-v", "--bin", "fixture-bin"])?;
    let unused_compiled = build_log.matches("Compiling unused-pkg-").count();
    let unrequested_bin_built = build_log.contains("fixture-bin-unrequested");
    let used_core_built = build_log.contains("Compiling used-core ");
    let used_util_built = build_log.contains("Compiling used-util ");
    let fixture_bin_built = build_log.contains("Compiling fixture-bin ");

    let pass = build_ok
        && unused_compiled == 0
        && !unrequested_bin_built
        && used_core_built
        && used_util_built
        && fixture_bin_built;

    let log_path = write_log(
        &case_id,
        &format!(
            "{build_log}\nunused_pkg_compiled_count={unused_compiled} \
             unrequested_bin_appears={unrequested_bin_built} \
             used_core_built={used_core_built} used_util_built={used_util_built} \
             fixture_bin_built={fixture_bin_built} (this scale declares {expected_unused} \
             unused-pkg-* crates, none of which may appear above)\n"
        ),
    )?;

    Ok(CaseEvidence {
        case_id,
        execution_role: "reference".to_string(),
        reproduction_commands: vec![format!(
            "cd fixtures/many-unrequested-targets/{scale} && rm -rf target && cargo build -v --bin fixture-bin"
        )],
        raw_log_path: log_path,
        pass,
        summary: format!(
            "unused_pkg_compiled_count={unused_compiled} (must be 0 of {expected_unused} \
             declared) unrequested_bin_appears={unrequested_bin_built} (must be false) \
             used-core/used-util/fixture-bin built={used_core_built}/{used_util_built}/{fixture_bin_built}"
        ),
    })
}

pub fn run_m8_small() -> Result<CaseEvidence, String> {
    run_m8_scale("small", 4)
}
pub fn run_m8_medium() -> Result<CaseEvidence, String> {
    run_m8_scale("medium", 10)
}
pub fn run_m8_large() -> Result<CaseEvidence, String> {
    run_m8_scale("large", 30)
}

pub fn run_all() -> Result<Vec<CaseEvidence>, String> {
    Ok(vec![
        run_m7_small()?,
        run_m7_medium()?,
        run_m7_large()?,
        run_m8_small()?,
        run_m8_medium()?,
        run_m8_large()?,
    ])
}

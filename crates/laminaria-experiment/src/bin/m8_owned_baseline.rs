//! `cargo run -p laminaria-experiment --bin laminaria-m8-owned-baseline`
//!
//! Issue #28 D1-a: `M8-many-unrequested-nim-planner` owned baseline
//! (unused_actions in {4, 10, 30}, warmup 1 + 3 measured repetitions
//! each). Every repetition is persisted as its own `Run` (including its
//! judgment data, in `Run.compiler_telemetry`) under `runs/<run_id>/`;
//! the round-trip `ScenarioReport` and the separate Nim-kernel-only
//! `kernel_nanos_stats` are both a *pointer* into those files --
//! `laminaria_experiment::m8_baseline::regenerate` is the authority for
//! what the evidence actually says.

use laminaria_experiment::m8_baseline;

fn main() {
    let report = match m8_baseline::run(&[4, 10, 30], 1, 3) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("M8 owned baseline failed: {e}");
            std::process::exit(1);
        }
    };

    for scale in &report.scales {
        let failed = scale.repetitions.iter().filter(|r| !r.success).count();
        let round_trip = match &scale.scenario_report {
            Some(sr) => format!(
                "mean={:.4} stddev={:.4}",
                sr.wall_seconds.mean, sr.wall_seconds.stddev
            ),
            None => format!("error={:?}", scale.scenario_report_error),
        };
        let kernel = match &scale.kernel_nanos_stats {
            Some(s) => format!("mean_ns={:.0} stddev_ns={:.0}", s.mean, s.stddev),
            None => "none".to_string(),
        };
        println!(
            "unused_actions={} repetitions={} failed={} round_trip_wall_seconds({round_trip}) \
             kernel_nanos({kernel}) needed_set_matches_in_every_successful_rep={} \
             kernel_nanos_present_in_every_successful_rep={}",
            scale.unused_actions,
            scale.repetitions.len(),
            failed,
            scale.needed_set_matches_in_every_successful_rep(),
            scale.kernel_nanos_present_in_every_successful_rep(),
        );
    }

    let out_dir = laminaria_experiment::planner_binary::repo_root().join("runs/d1a");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("failed to create {}: {e}", out_dir.display());
        std::process::exit(1);
    }
    let out_path = out_dir.join("m8-many-unrequested-nim-planner.json");
    let json = serde_json::to_string_pretty(&report).expect("report always serializes");
    if let Err(e) = std::fs::write(&out_path, json) {
        eprintln!("failed to write {}: {e}", out_path.display());
        std::process::exit(1);
    }
    println!("wrote {}", out_path.display());
    println!(
        "per-repetition Run evidence (including judgment data, in compiler_telemetry) under {}",
        report.runs_root.display()
    );

    // Prove regeneration from disk actually reproduces the same
    // judgment, not merely the wall-time summary.
    let regenerated = match m8_baseline::regenerate(report) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("FAIL: regenerate() could not rebuild the report from disk: {e}");
            std::process::exit(1);
        }
    };

    let all_ok = regenerated.scales.iter().all(|s| s.is_a_valid_baseline());
    let comparable_ok = regenerated
        .owned_baseline_comparable_across_scales()
        .is_ok();

    if !all_ok || !comparable_ok {
        eprintln!(
            "FAIL: at least one scale was not a valid baseline, or scales were not comparable \
             (all_ok={all_ok}, comparable_ok={comparable_ok})"
        );
        if let Err(e) = regenerated.owned_baseline_comparable_across_scales() {
            eprintln!("  comparability error: {e}");
        }
        std::process::exit(1);
    }
    println!("pass_criteria.d1: OK (verified from regenerate()'d, disk-derived evidence)");
}

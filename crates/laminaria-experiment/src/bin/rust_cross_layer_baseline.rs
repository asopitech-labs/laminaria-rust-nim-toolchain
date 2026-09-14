//! `cargo run -p laminaria-experiment --bin laminaria-issue50-rust-cross-layer-baseline`
//!
//! Issue #50: real, `wait4`-measured resource evidence for the P0
//! cross-layer pruning hypothesis (warmup 1 + 3 measured repetitions of
//! both the eager and feedback-pruned `cargo build` scenarios). See
//! `laminaria_experiment::rust_cross_layer_baseline`'s own module doc for
//! what this does and does not prove.

use laminaria_experiment::rust_cross_layer_baseline;

fn main() {
    let report = match rust_cross_layer_baseline::run(1, 3) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("issue #50 rust-cross-layer baseline failed: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "plan: selected={:?} pruned={:?}",
        report.plan_selected_packages, report.plan_pruned_packages
    );
    for outcome in [&report.eager, &report.feedback] {
        let cpu = outcome
            .cpu_seconds_stats
            .as_ref()
            .map(|s| format!("mean={:.4}s stddev={:.4}s", s.mean, s.stddev))
            .unwrap_or_else(|| "none".to_string());
        let rss = outcome
            .peak_rss_bytes_stats
            .as_ref()
            .map(|s| format!("mean={:.0}B stddev={:.0}B", s.mean, s.stddev))
            .unwrap_or_else(|| "none".to_string());
        println!(
            "{}: wall_seconds(mean={:.4}s stddev={:.4}s) cpu_seconds({cpu}) peak_rss({rss})",
            outcome.scenario_id,
            outcome.scenario_report.wall_seconds.mean,
            outcome.scenario_report.wall_seconds.stddev
        );
    }
    match &report.wall_time_comparison {
        Ok(comparison) => println!(
            "wall_time_comparison: relative_diff={:.4} verdict={:?} confounding_notes={:?}",
            comparison.wall_seconds_relative_diff,
            comparison.wall_time_verdict,
            comparison.confounding_notes
        ),
        Err(e) => println!("wall_time_comparison: not comparable ({e})"),
    }
    println!(
        "eager_build_compiled_a_pruned_package={} feedback_build_never_compiled_a_pruned_package={}",
        report.eager_build_compiled_a_pruned_package, report.feedback_build_never_compiled_a_pruned_package
    );

    let out_dir = laminaria_experiment::planner_binary::repo_root().join("runs/issue50");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("failed to create {}: {e}", out_dir.display());
        std::process::exit(1);
    }
    let out_path = out_dir.join("rust-cross-layer-baseline.json");
    let json = serde_json::to_string_pretty(&report).expect("report always serializes");
    if let Err(e) = std::fs::write(&out_path, json) {
        eprintln!("failed to write {}: {e}", out_path.display());
        std::process::exit(1);
    }
    println!("wrote {}", out_path.display());
    println!(
        "per-repetition Run evidence (including Cargo's own compiler_telemetry) under {}",
        report.runs_root.display()
    );

    // Prove regeneration from disk actually reproduces the same
    // judgment, not merely the in-memory summary.
    let regenerated = match rust_cross_layer_baseline::regenerate(report) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("FAIL: regenerate() could not rebuild the report from disk: {e}");
            std::process::exit(1);
        }
    };

    if !regenerated.feedback_pruning_is_supported_by_raw_evidence() {
        eprintln!(
            "FAIL: the P0 pass criterion does not hold from regenerate()'d, disk-derived evidence \
             (eager_build_compiled_a_pruned_package={}, feedback_build_never_compiled_a_pruned_package={}, \
             plan_pruned_packages={:?})",
            regenerated.eager_build_compiled_a_pruned_package,
            regenerated.feedback_build_never_compiled_a_pruned_package,
            regenerated.plan_pruned_packages
        );
        std::process::exit(1);
    }
    println!("pass_criteria.issue50_p0: OK (verified from regenerate()'d, disk-derived evidence)");
}

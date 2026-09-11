//! `cargo run -p laminaria-experiment --bin laminaria-m3-owned-baseline`
//!
//! Issue #28 D1-a: `M3-owned-independent-chains` owned baseline
//! (CPU budget 1 and 2, warmup 1 + 5 measured repetitions each). Every
//! repetition is persisted as its own `Run` under `runs/<run_id>/`; the
//! printed report and the JSON this writes to
//! `runs/d1a/m3-owned-independent-chains.json` are both derived from
//! those files, never from in-memory-only samples.

use laminaria_experiment::m3_baseline;

fn main() {
    let report = match m3_baseline::run(1, 5) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("M3 owned baseline failed: {e}");
            std::process::exit(1);
        }
    };

    for budget in &report.budgets {
        let failed = budget.repetitions.iter().filter(|r| !r.success).count();
        match &budget.scenario_report {
            Some(sr) => println!(
                "budget={} repetitions={} failed={} wall_seconds(mean={:.4} stddev={:.4}) \
                 peak_concurrency={:?} evidence_consistent_within_budget={}",
                budget.cpu_budget,
                budget.repetitions.len(),
                failed,
                sr.wall_seconds.mean,
                sr.wall_seconds.stddev,
                budget.successful_peak_concurrency_values(),
                budget.evidence_digest_if_consistent().is_some(),
            ),
            None => println!(
                "budget={} repetitions={} failed={} scenario_report_error={:?}",
                budget.cpu_budget,
                budget.repetitions.len(),
                failed,
                budget.scenario_report_error
            ),
        }
    }
    println!(
        "evidence_matches_across_budgets={}",
        report.evidence_matches_across_budgets()
    );

    let out_dir = laminaria_experiment::planner_binary::repo_root().join("runs/d1a");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("failed to create {}: {e}", out_dir.display());
        std::process::exit(1);
    }
    let out_path = out_dir.join("m3-owned-independent-chains.json");
    let json = serde_json::to_string_pretty(&report).expect("report always serializes");
    if let Err(e) = std::fs::write(&out_path, json) {
        eprintln!("failed to write {}: {e}", out_path.display());
        std::process::exit(1);
    }
    println!("wrote {}", out_path.display());
    println!(
        "per-repetition Run evidence under {}",
        report.runs_root.display()
    );

    let budget1_ok = report
        .budgets
        .iter()
        .find(|b| b.cpu_budget == 1)
        .is_some_and(|b| {
            !b.repetitions.is_empty()
                && b.repetitions.iter().all(|r| r.success)
                && b.successful_peak_concurrency_values()
                    .iter()
                    .all(|&p| p == 1)
        });
    let budget2_ok = report
        .budgets
        .iter()
        .find(|b| b.cpu_budget == 2)
        .is_some_and(|b| {
            !b.repetitions.is_empty()
                && b.repetitions.iter().all(|r| r.success)
                && b.successful_peak_concurrency_values()
                    .iter()
                    .all(|&p| p == 2)
        });
    let evidence_ok = report.evidence_matches_across_budgets();

    if !budget1_ok || !budget2_ok || !evidence_ok {
        eprintln!(
            "FAIL: budget1_peak_always_1={budget1_ok} budget2_peak_always_2={budget2_ok} \
             evidence_matches_across_budgets={evidence_ok}"
        );
        std::process::exit(1);
    }
    println!("pass_criteria.d1: OK");
}

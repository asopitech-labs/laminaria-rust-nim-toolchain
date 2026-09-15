//! `cargo run -p laminaria-experiment --bin laminaria-issue47-g3-e0-vs-e1`
//!
//! Issue #47 (G3, Lane B): real E0-vs-E1 comparison at increasing
//! injected-unreachable-candidate scale, against the real G1/G2
//! `cadd`/`app` fixture and its real Checkpoint 1 action. See
//! `laminaria_experiment::g3_e0_vs_e1`'s own module doc for what this
//! does and does not prove.

use laminaria_experiment::g3_e0_vs_e1;

fn main() {
    let scales = [0usize, 10, 100, 1_000, 10_000];
    let report = match g3_e0_vs_e1::run(&scales) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("issue #47 G3 E0-vs-E1 comparison failed: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "artifact_equivalence: identical={} e0_sha256={} e1_sha256={}",
        report.artifact_equivalence.identical,
        report.artifact_equivalence.e0_archive_sha256,
        report.artifact_equivalence.e1_archive_sha256
    );
    println!();
    println!(
        "{:>10} | {:>14} | {:>14} | {:>10} | {:>10}",
        "scale", "e0_wall_secs", "e1_wall_secs", "e0_oblig", "e1_oblig"
    );
    for (e0, e1) in report.eager.iter().zip(report.demand_driven.iter()) {
        println!(
            "{:>10} | {:>14.6} | {:>14.6} | {:>10} | {:>10} (considered={:?})",
            e0.unreachable_candidates_injected,
            e0.wall_seconds,
            e1.wall_seconds,
            e0.obligations_in_closure,
            e1.obligations_in_closure,
            e1.package_candidates_considered
        );
    }
    println!();
    println!(
        "e1_matches_e0_behavior_and_actually_prunes: {}",
        report.e1_matches_e0_behavior_and_actually_prunes()
    );

    let report_json = serde_json::to_string_pretty(&report).expect("report must serialize");
    let out_path = std::env::temp_dir().join("laminaria-issue47-g3-e0-vs-e1-report.json");
    std::fs::write(&out_path, report_json).expect("must write report json");
    println!("full report: {}", out_path.display());

    if !report.e1_matches_e0_behavior_and_actually_prunes() {
        std::process::exit(1);
    }
}

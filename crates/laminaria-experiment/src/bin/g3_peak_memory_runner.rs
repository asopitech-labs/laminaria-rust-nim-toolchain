//! `cargo run -p laminaria-experiment --bin laminaria-issue65-g3-peak-memory`
//!
//! Issue #65 (G3 follow-up, carried-forward item 2): real out-of-process
//! peak-memory comparison between E0 (`resolve`) and E1
//! (`resolve_demand_driven`) at increasing injected-unreachable-candidate
//! scale, against the real G1/G2 `cadd`/`app` fixture. See
//! `laminaria_experiment::g3_peak_memory`'s own module doc for what this
//! does and does not prove.

use laminaria_experiment::g3_peak_memory;

fn main() {
    let scales = [0usize, 10, 100, 1_000, 10_000];
    let report = match g3_peak_memory::run(&scales) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("issue #65 G3 peak-memory comparison failed: {e}");
            std::process::exit(1);
        }
    };

    println!("peak_rss_supported: {}", report.peak_rss_supported);
    println!();
    println!(
        "{:>10} | {:>16} | {:>16} | {:>10} | {:>10}",
        "scale", "e0_peak_rss", "e1_peak_rss", "e0_oblig", "e1_oblig"
    );
    for (e0, e1) in report.eager.iter().zip(report.demand_driven.iter()) {
        println!(
            "{:>10} | {:>16} | {:>16} | {:>10} | {:>10} (considered={:?})",
            e0.unreachable_candidates_injected,
            e0.peak_rss_bytes
                .map(|b| b.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            e1.peak_rss_bytes
                .map(|b| b.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            e0.obligations_in_closure,
            e1.obligations_in_closure,
            e1.package_candidates_considered
        );
    }
    println!();
    println!(
        "e1_considered_candidates_are_scale_invariant: {}",
        report.e1_considered_candidates_are_scale_invariant()
    );
    println!(
        "NOTE: peak_rss_bytes is recorded above but this runner draws no adopt/reject \
         conclusion from it -- see g3_peak_memory's own module doc comment: a native-host \
         run and a run inside docker/bootstrap.Dockerfile produced opposite E0-vs-E1 \
         peak-RSS orderings for the identical harness, so ru_maxrss is not treated as a \
         reliable signal for this comparison at this fixture's scale."
    );

    let report_json = serde_json::to_string_pretty(&report).expect("report must serialize");
    let out_path = std::env::temp_dir().join("laminaria-issue65-g3-peak-memory-report.json");
    std::fs::write(&out_path, report_json).expect("must write report json");
    println!("full report: {}", out_path.display());

    if !report.e1_considered_candidates_are_scale_invariant() {
        std::process::exit(1);
    }
}

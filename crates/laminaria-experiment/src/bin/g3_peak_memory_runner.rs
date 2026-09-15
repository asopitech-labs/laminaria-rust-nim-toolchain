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
        "e1_peak_memory_at_or_below_e0_at_largest_scale: {:?}",
        report.e1_peak_memory_at_or_below_e0_at_largest_scale()
    );
    println!(
        "e0_peak_rss_trends_up_with_scale_while_e1_stays_flat: {:?}",
        report.e0_peak_rss_trends_up_with_scale_while_e1_stays_flat()
    );

    let report_json = serde_json::to_string_pretty(&report).expect("report must serialize");
    let out_path = std::env::temp_dir().join("laminaria-issue65-g3-peak-memory-report.json");
    std::fs::write(&out_path, report_json).expect("must write report json");
    println!("full report: {}", out_path.display());

    // The trend claim (E1 stays flat, E0 grows with injected scale) is
    // this measurement's actual direct-acceptance claim -- see
    // g3_peak_memory's own doc comment on why the absolute-value
    // comparison alone is noise-dominated at small scales.
    if report.e0_peak_rss_trends_up_with_scale_while_e1_stays_flat() == Some(false) {
        std::process::exit(1);
    }
}

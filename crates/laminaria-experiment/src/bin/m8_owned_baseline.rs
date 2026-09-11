//! `cargo run -p laminaria-experiment --bin laminaria-m8-owned-baseline`
//!
//! Issue #28 D1-a: `M8-many-unrequested-nim-planner` owned baseline
//! (unused_actions in {4, 10, 30}, warmup 1 + 3 measured repetitions
//! each). Saves raw samples + a regenerable report to
//! `runs/d1a/m8-many-unrequested-nim-planner.json`.

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
        println!(
            "unused_actions={} samples={} wall_seconds(mean={:.4} stddev={:.4}) \
             needed_set_matches_in_every_rep={}",
            scale.unused_actions,
            scale.samples.len(),
            scale.wall_seconds.mean,
            scale.wall_seconds.stddev,
            scale.needed_set_matches_in_every_rep
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

    let all_ok = report
        .scales
        .iter()
        .all(|s| s.needed_set_matches_in_every_rep);
    if !all_ok {
        eprintln!("FAIL: at least one scale's ordered_actions included an unused-pkg-* action");
        std::process::exit(1);
    }
    println!("pass_criteria.d1: OK");
}

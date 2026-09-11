//! `cargo run -p laminaria-experiment --bin laminaria-d1b1-reference-cases`
//!
//! Issue #28 D1-b1: re-runs the existing M4/M5 fixtures named by
//! `docs/design/issue-35-d0-cases.yaml` and reports pass/fail against
//! each case's own D0-pinned `expected` value. Raw per-case logs (the
//! exact command, stdout, stderr, exit status) are written under
//! `runs/d1b1/<case-id>.log` -- this binary's own stdout is a summary
//! only, never the sole evidence.

fn main() {
    let results = match laminaria_experiment::d1b1_reference_cases::run_all() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("D1-b1 reference-case runner failed before completing: {e}");
            std::process::exit(1);
        }
    };

    let mut any_failed = false;
    for case in &results {
        println!(
            "{} [{}] {}: {}",
            case.case_id,
            case.execution_role,
            if case.pass { "PASS" } else { "FAIL" },
            case.summary
        );
        for cmd in &case.reproduction_commands {
            println!("  repro: {cmd}");
        }
        println!("  raw_log: {}", case.raw_log_path.display());
        if !case.pass {
            any_failed = true;
        }
    }

    if any_failed {
        eprintln!("FAIL: at least one D1-b1 reference case did not reproduce its D0-pinned expected value");
        std::process::exit(1);
    }
    println!(
        "pass_criteria.d1: OK for all reference cases above (verified from raw re-execution logs)"
    );
}

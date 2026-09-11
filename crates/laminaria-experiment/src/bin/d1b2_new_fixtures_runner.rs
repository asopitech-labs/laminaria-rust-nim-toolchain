//! `cargo run -p laminaria-experiment --bin laminaria-d1b2-new-fixtures`
//!
//! Issue #28 D1-b2: re-runs the two new fixtures named by
//! `docs/design/issue-35-d0-cases.yaml` (M7's `fixtures/
//! long-chain-wide-branches`, M8-many-unrequested-cargo's `fixtures/
//! many-unrequested-targets`) and reports pass/fail against each case's
//! own D0-pinned `expected` value. Raw per-case logs are written under
//! `runs/d1b2/<case-id>.log`. Reuses `d1b1_preflight` first, for the
//! same reason `laminaria-d1b1-reference-cases` does (see that module's
//! doc comment) -- these fixtures link/build the same way M4/M5's do.

fn main() {
    let preflight = match laminaria_experiment::d1b1_preflight::run() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("D1-b2 preflight check itself failed to run: {e}");
            std::process::exit(2);
        }
    };
    println!(
        "preflight: rustc_host={} rustc_arch={} hardware_arch={} compatible={}",
        preflight.rustc_host_triple,
        preflight.rustc_arch,
        preflight.hardware_arch,
        preflight.compatible
    );
    if !preflight.compatible {
        eprintln!(
            "ENVIRONMENT-INCOMPATIBLE: the PATH-resolved rustc targets {} but the real hardware \
             is {} -- refusing to run any D1-b2 case (this is not a case failure)",
            preflight.rustc_arch, preflight.hardware_arch
        );
        std::process::exit(2);
    }

    let results = match laminaria_experiment::d1b2_new_fixtures::run_all() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("D1-b2 case runner failed before completing: {e}");
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
        eprintln!("FAIL: at least one D1-b2 case did not reproduce its D0-pinned expected value");
        std::process::exit(1);
    }
    println!(
        "pass_criteria.d1: OK for all D1-b2 cases above (verified from raw re-execution logs)"
    );
}

//! `cargo run -p laminaria-experiment --bin laminaria-d1b1-reference-cases`
//!
//! Issue #28 D1-b1: re-runs the existing M4/M5 fixtures named by
//! `docs/design/issue-35-d0-cases.yaml` and reports pass/fail against
//! each case's own D0-pinned `expected` value. Raw per-case logs (the
//! exact command, stdout, stderr, exit status) are written under
//! `runs/d1b1/<case-id>.log` -- this binary's own stdout is a summary
//! only, never the sole evidence.
//!
//! Runs `d1b1_preflight` first and refuses to attempt any case at all
//! (exit code 2, distinct from a genuine case failure's exit code 1) if
//! the environment itself is internally inconsistent (a `PATH`-resolved
//! `rustc` targeting a different architecture than the real hardware) --
//! see that module's own doc comment for why this exists and what it
//! would otherwise look like (a confusing per-case link failure that
//! isn't actually about any case's correctness).

fn main() {
    let preflight = match laminaria_experiment::d1b1_preflight::run() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("D1-b1 preflight check itself failed to run: {e}");
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
             is {} -- refusing to run any D1-b1 case (this is not a case failure; fix PATH so \
             the resolved rustc matches the real hardware, e.g. by putting a native-arch \
             toolchain first, then rerun)",
            preflight.rustc_arch, preflight.hardware_arch
        );
        std::process::exit(2);
    }

    let mut results = match laminaria_experiment::d1b1_reference_cases::run_all() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("D1-b1 reference-case runner failed before completing: {e}");
            std::process::exit(1);
        }
    };
    match laminaria_experiment::d1b1_planner_cases::run_all() {
        Ok(more) => results.extend(more),
        Err(e) => {
            eprintln!("D1-b1 planner-case runner failed before completing: {e}");
            std::process::exit(1);
        }
    }

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

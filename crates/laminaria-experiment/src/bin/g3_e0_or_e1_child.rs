//! Issue #65 (G3 follow-up, carried-forward item 2): the out-of-process
//! child half of the peak-memory comparison. This binary does exactly one
//! thing -- ingest the real `cadd`/`app` fixture, inject `--unreachable`
//! synthetic candidates, run exactly one of `resolve` (E0) or
//! `resolve_demand_driven` (E1) once, and print the result as one line of
//! JSON on stdout -- then exits.
//!
//! **Why a separate process at all**: issue #47's own `g3_e0_vs_e1` module
//! calls `resolve`/`resolve_demand_driven` in-process, which
//! `crate::tracer::trace_root_command`'s `wait4`-based peak-RSS accounting
//! cannot isolate (it measures a spawned child process's own `rusage`, not
//! an in-process function call's share of the parent's heap -- see issue
//! #65's carried-forward item 2). Spawning this binary as its own child
//! process for exactly one resolve call gives `wait4` a real process
//! boundary to measure: `ru_maxrss` on this process reports *this
//! process's own* peak RSS, not the parent harness's.
//!
//! **What this measures and does not measure**: this binary's own baseline
//! process overhead (Rust runtime init, the fixture ingest itself) is
//! included in the reported peak RSS alongside the resolve call -- it is
//! not a *pure* resolve-only allocation delta. `g3_peak_memory`'s own
//! report compares E0 and E1 under the identical harness (same binary,
//! same ingest, same injection code, differing only in which resolve path
//! runs), so this shared overhead cancels out in the E0-vs-E1 comparison
//! even though it inflates both absolute numbers equally.

use std::process::ExitCode;

use laminaria_plan::dependency_graph::{resolve, resolve_demand_driven};
use laminaria_run::command_runner::RecordingCommandRunner;
use laminaria_run::cross_ecosystem_ingest::{ingest_fixture_input, FixtureLayout};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ChildResult {
    resolver: String,
    unreachable_candidates_injected: usize,
    package_candidates_total: usize,
    package_candidates_considered: Option<usize>,
    obligations_in_closure: usize,
}

fn usage() -> ! {
    eprintln!("usage: laminaria-g3-e0-or-e1-child <e0|e1> <unreachable_candidates_injected:usize>");
    std::process::exit(2);
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        usage();
    }
    let resolver = args[1].as_str();
    let scale: usize = match args[2].parse() {
        Ok(v) => v,
        Err(_) => usage(),
    };
    if resolver != "e0" && resolver != "e1" {
        usage();
    }

    let layout = FixtureLayout::discover();
    let runner = RecordingCommandRunner::new();
    let base_input = match ingest_fixture_input(&runner, &layout, &["1.0.0"]) {
        Ok(input) => input,
        Err(e) => {
            eprintln!("failed to ingest the real fixture input: {e:?}");
            return ExitCode::FAILURE;
        }
    };
    let input = laminaria_experiment::g3_e0_vs_e1::inject_unreachable_candidates(base_input, scale);

    let result = if resolver == "e0" {
        match resolve(&input) {
            Ok(closure) => ChildResult {
                resolver: "e0".to_string(),
                unreachable_candidates_injected: scale,
                package_candidates_total: input.package_candidates.len(),
                package_candidates_considered: None,
                obligations_in_closure: closure.obligations.len(),
            },
            Err(e) => {
                eprintln!("E0 resolve() rejected: {e:?}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        match resolve_demand_driven(&input) {
            Ok((closure, expansion)) => ChildResult {
                resolver: "e1".to_string(),
                unreachable_candidates_injected: scale,
                package_candidates_total: expansion.package_candidates_total,
                package_candidates_considered: Some(expansion.package_candidates_considered),
                obligations_in_closure: closure.obligations.len(),
            },
            Err(e) => {
                eprintln!("E1 resolve_demand_driven() rejected: {e:?}");
                return ExitCode::FAILURE;
            }
        }
    };

    match serde_json::to_string(&result) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("failed to serialize child result: {e}");
            ExitCode::FAILURE
        }
    }
}

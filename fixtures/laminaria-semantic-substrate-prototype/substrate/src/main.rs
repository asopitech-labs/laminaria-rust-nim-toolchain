//! Orchestrates the three things ../CONTRACT.md's substrate prototype
//! demonstrates: `eval` (reference evaluator output, for cross-checking
//! against the real Rust/Nim binaries), `emit-ir` (the LLVM-IR backend
//! projection, for `llc`/`cc` to compile and run), and `demo-inline` (the
//! allowed-vs-rejected inlining experiment). See ../trace.sh for how these
//! are actually run and cross-checked end to end.

mod eval;
mod inline;
mod llvm_ir;
mod repr;

use repr::{double_with_side_effect_fact, workload_program};

/// Single source of truth for the test inputs every cross-checked program
/// (real Rust/Nim binaries, reference evaluator, LLVM-IR-projected
/// binary) is run against -- must stay in sync with ../rust-src/
/// add_or_double.rs and ../nim-src/add_or_double.nim's own `TEST_INPUTS`
/// by hand (a named limitation: nothing mechanically enforces these three
/// lists agree, see NOTES.md).
const TEST_INPUTS: &[(i32, i32, i32)] = &[(3, 4, 0), (3, 4, 1), (i32::MAX, 1, 0), (-5, 10, 1)];

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    match mode.as_str() {
        "eval" => run_eval(),
        "emit-ir" => run_emit_ir(),
        "demo-inline" => run_demo_inline(),
        _ => {
            eprintln!("usage: substrate <eval|emit-ir|demo-inline>");
            std::process::exit(2);
        }
    }
}

fn run_eval() {
    let program = workload_program();
    for &(a, b, use_double) in TEST_INPUTS {
        let result = eval::eval_function(&program, "add_or_double", &[a, b, use_double]);
        println!("{a},{b},{use_double},{result}");
    }
}

fn run_emit_ir() {
    let program = workload_program();
    print!("{}", llvm_ir::emit_module(&program, TEST_INPUTS));
}

fn run_demo_inline() {
    let mut all_passed = true;

    // Allowed case: double.has_side_effects == false (the real fact).
    let program = workload_program();
    let original_body = program.functions["add_or_double"].body.clone();
    match inline::inline_calls_to_stmt(&original_body, "double", &program) {
        Ok(inlined_body) => {
            let mut mismatches = Vec::new();
            for &(a, b, ud) in TEST_INPUTS {
                let args = [a, b, ud];
                let original_result = eval::eval_stmt(&original_body, &args, &program);
                let inlined_result = eval::eval_stmt(&inlined_body, &args, &program);
                if original_result != inlined_result {
                    mismatches.push((args, original_result, inlined_result));
                }
            }
            if mismatches.is_empty() {
                println!(
                    "ALLOWED case: double.has_side_effects=false -> inlining permitted, and \
                     verified behavior-preserving across {} test input(s): PASS",
                    TEST_INPUTS.len()
                );
            } else {
                all_passed = false;
                println!("ALLOWED case: FAIL -- inlining changed behavior: {mismatches:?}");
            }
        }
        Err(reason) => {
            all_passed = false;
            println!("ALLOWED case: FAIL -- expected inlining to be permitted, got: {reason}");
        }
    }

    // Rejected case: a hypothetical double with has_side_effects == true.
    let mut program_with_effect = workload_program();
    program_with_effect.insert(double_with_side_effect_fact());
    let original_body = program_with_effect.functions["add_or_double"].body.clone();
    match inline::inline_calls_to_stmt(&original_body, "double", &program_with_effect) {
        Ok(_) => {
            all_passed = false;
            println!("REJECTED case: FAIL -- inlining was permitted despite has_side_effects=true");
        }
        Err(reason) => {
            println!("REJECTED case: has_side_effects=true -> inlining refused: PASS ({reason})");
        }
    }

    if !all_passed {
        std::process::exit(1);
    }
}

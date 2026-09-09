//! Direct interpreter for the `repr` representation -- issue #25's
//! "reference evaluator" requirement. Used both to cross-check the
//! representation against the real Rust/Nim compiled binaries (`main.rs`)
//! and to verify `inline.rs`'s transformation is actually
//! behavior-preserving (not just assumed so by construction).

use crate::repr::{Expr, SemanticProgram, Stmt};

pub fn eval_expr(expr: &Expr, args: &[i32], program: &SemanticProgram) -> i32 {
    match expr {
        Expr::Param(n) => args[*n],
        Expr::WrappingAdd(lhs, rhs) => {
            eval_expr(lhs, args, program).wrapping_add(eval_expr(rhs, args, program))
        }
        Expr::NotEqZero(inner) => {
            if eval_expr(inner, args, program) != 0 {
                1
            } else {
                0
            }
        }
        Expr::Call(name, call_args) => {
            let evaluated: Vec<i32> = call_args
                .iter()
                .map(|a| eval_expr(a, args, program))
                .collect();
            let callee = program
                .functions
                .get(name)
                .unwrap_or_else(|| panic!("eval_expr: unknown function `{name}`"));
            eval_stmt(&callee.body, &evaluated, program)
        }
    }
}

pub fn eval_stmt(stmt: &Stmt, args: &[i32], program: &SemanticProgram) -> i32 {
    match stmt {
        Stmt::If { cond, then, els } => {
            if eval_expr(cond, args, program) != 0 {
                eval_stmt(then, args, program)
            } else {
                eval_stmt(els, args, program)
            }
        }
        Stmt::Return(expr) => eval_expr(expr, args, program),
    }
}

/// Evaluates `function_name` in `program` with `args`.
pub fn eval_function(program: &SemanticProgram, function_name: &str, args: &[i32]) -> i32 {
    let fact = &program.functions[function_name];
    eval_stmt(&fact.body, args, program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repr::workload_program;

    #[test]
    fn matches_the_real_rust_and_nim_binaries_test_inputs() {
        // Same TEST_INPUTS as ../../rust-src/add_or_double.rs and
        // ../../nim-src/add_or_double.nim, and the same expected results
        // those binaries were verified (by hand, `cargo run`/`nim c`) to
        // produce -- see ../../NOTES.md's cross-check table.
        let program = workload_program();
        let cases = [
            ([3, 4, 0], 7),
            ([3, 4, 1], 6),
            ([i32::MAX, 1, 0], i32::MIN),
            ([-5, 10, 1], -10),
        ];
        for (args, expected) in cases {
            assert_eq!(
                eval_function(&program, "add_or_double", &args),
                expected,
                "args={args:?}"
            );
        }
    }
}

//! The one transformation this fixture demonstrates both ways (issue #25's
//! ask for concrete "allowed with semantic info" / "rejected without it"
//! examples): inlining a call to `double` at its call site inside
//! `add_or_double`, by substituting `double`'s own body (with its
//! parameter replaced by the actual call argument) for the `Call`
//! expression.
//!
//! The transformation is gated on exactly one tracked fact --
//! `FnFact::has_side_effects` -- matching the minimum information a real
//! inliner needs to decide safety (LLVM's own real inliner gates
//! `rust_add`'s inlining on a different but structurally identical kind of
//! fact -- attribute compatibility -- in
//! `fixtures/llvm-rediscovery-semantic-workload/NOTES.md`; this module is
//! the same *shape* of decision, in this project's own representation
//! rather than observed inside LLVM's).

use crate::repr::{Expr, SemanticProgram, Stmt};

fn substitute_params(expr: &Expr, actual_args: &[Expr]) -> Expr {
    match expr {
        Expr::Param(n) => actual_args[*n].clone(),
        Expr::WrappingAdd(lhs, rhs) => Expr::WrappingAdd(
            Box::new(substitute_params(lhs, actual_args)),
            Box::new(substitute_params(rhs, actual_args)),
        ),
        Expr::NotEqZero(inner) => Expr::NotEqZero(Box::new(substitute_params(inner, actual_args))),
        Expr::Call(name, args) => Expr::Call(
            name.clone(),
            args.iter()
                .map(|a| substitute_params(a, actual_args))
                .collect(),
        ),
    }
}

/// Inlines one `Call(callee_name, call_args)` expression, given `callee`'s
/// own fact/body from `program`. Refuses (`Err`) when `has_side_effects`
/// is set -- the load-bearing check this whole module exists to
/// demonstrate -- or when the callee's body isn't the single-`Return`
/// shape this small experiment's inliner knows how to substitute (a named
/// limitation, not a silent wrong answer for a shape it can't handle).
pub fn inline_call(
    program: &SemanticProgram,
    callee_name: &str,
    call_args: &[Expr],
) -> Result<Expr, String> {
    let callee = program
        .functions
        .get(callee_name)
        .unwrap_or_else(|| panic!("inline_call: unknown function `{callee_name}`"));
    if callee.has_side_effects {
        return Err(format!(
            "refusing to inline `{callee_name}`: has_side_effects=true -- the tracked semantic \
             fact says this call cannot be safely duplicated/reordered/elided, independent of \
             any cost heuristic"
        ));
    }
    match &callee.body {
        Stmt::Return(body_expr) => Ok(substitute_params(body_expr, call_args)),
        _ => Err(format!(
            "refusing to inline `{callee_name}`: body is not a single Return -- this \
             experiment's inliner only handles that shape, not a real limitation of the \
             has_side_effects check itself"
        )),
    }
}

/// Recursively inlines every call to `callee_name` found anywhere inside
/// `expr`.
pub fn inline_calls_to(
    expr: &Expr,
    callee_name: &str,
    program: &SemanticProgram,
) -> Result<Expr, String> {
    match expr {
        Expr::Call(name, args) => {
            let inlined_args = args
                .iter()
                .map(|a| inline_calls_to(a, callee_name, program))
                .collect::<Result<Vec<_>, _>>()?;
            if name == callee_name {
                inline_call(program, callee_name, &inlined_args)
            } else {
                Ok(Expr::Call(name.clone(), inlined_args))
            }
        }
        Expr::WrappingAdd(lhs, rhs) => Ok(Expr::WrappingAdd(
            Box::new(inline_calls_to(lhs, callee_name, program)?),
            Box::new(inline_calls_to(rhs, callee_name, program)?),
        )),
        Expr::NotEqZero(inner) => Ok(Expr::NotEqZero(Box::new(inline_calls_to(
            inner,
            callee_name,
            program,
        )?))),
        Expr::Param(_) => Ok(expr.clone()),
    }
}

/// `inline_calls_to`, applied to a `Stmt` (recursing into both branches of
/// an `If`, and the `Return` expression).
pub fn inline_calls_to_stmt(
    stmt: &Stmt,
    callee_name: &str,
    program: &SemanticProgram,
) -> Result<Stmt, String> {
    match stmt {
        Stmt::Return(expr) => Ok(Stmt::Return(inline_calls_to(expr, callee_name, program)?)),
        Stmt::If { cond, then, els } => Ok(Stmt::If {
            cond: inline_calls_to(cond, callee_name, program)?,
            then: Box::new(inline_calls_to_stmt(then, callee_name, program)?),
            els: Box::new(inline_calls_to_stmt(els, callee_name, program)?),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::eval_stmt;
    use crate::repr::{double_with_side_effect_fact, workload_program};

    #[test]
    fn allowed_case_inlining_is_behavior_preserving_across_test_inputs() {
        let program = workload_program();
        let original_body = program.functions["add_or_double"].body.clone();
        let inlined_body = inline_calls_to_stmt(&original_body, "double", &program)
            .expect("double.has_side_effects=false must permit inlining");

        // The inlined body must contain no more Call("double", ...) --
        // otherwise this "verified" claim below would be vacuous.
        assert!(!format!("{inlined_body:?}").contains("Call(\"double\""));

        for args in [
            [3, 4, 0],
            [3, 4, 1],
            [i32::MAX, 1, 0],
            [-5, 10, 1],
            [7, -7, 1],
        ] {
            let original_result = eval_stmt(&original_body, &args, &program);
            let inlined_result = eval_stmt(&inlined_body, &args, &program);
            assert_eq!(
                original_result, inlined_result,
                "inlining changed add_or_double's result for args={args:?}"
            );
        }
    }

    #[test]
    fn rejected_case_inlining_is_refused_when_the_side_effect_fact_is_set() {
        let mut program = workload_program();
        program.insert(double_with_side_effect_fact());
        let original_body = program.functions["add_or_double"].body.clone();

        let result = inline_calls_to_stmt(&original_body, "double", &program);

        assert!(
            result.is_err(),
            "expected inlining to be refused once has_side_effects=true, got Ok"
        );
        assert!(result.unwrap_err().contains("has_side_effects=true"));
    }
}

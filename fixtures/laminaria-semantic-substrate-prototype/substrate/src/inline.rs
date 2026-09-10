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

/// How many times `expr` references parameter `param_index` -- used to
/// find parameters `substitute_params` would copy the actual argument
/// expression into more than once.
fn count_param_occurrences(expr: &Expr, param_index: usize) -> usize {
    match expr {
        Expr::Param(n) => usize::from(*n == param_index),
        Expr::WrappingAdd(lhs, rhs) => {
            count_param_occurrences(lhs, param_index) + count_param_occurrences(rhs, param_index)
        }
        Expr::NotEqZero(inner) => count_param_occurrences(inner, param_index),
        Expr::Call(_, args) => args
            .iter()
            .map(|a| count_param_occurrences(a, param_index))
            .sum(),
    }
}

/// The evaluation-order sequence of parameter references inside `expr`,
/// in the exact order `eval_expr` (`eval.rs`) actually evaluates them:
/// left operand before right operand for `WrappingAdd`, argument order
/// for `Call`, and straight through for `NotEqZero`'s single inner
/// expression. Used to detect whether substituting call arguments into
/// the callee's body would reorder their evaluation relative to the
/// caller's own left-to-right argument evaluation -- checked directly
/// against `eval_expr`'s real traversal order, not assumed to match it.
fn param_evaluation_order(expr: &Expr, out: &mut Vec<usize>) {
    match expr {
        Expr::Param(n) => out.push(*n),
        Expr::WrappingAdd(lhs, rhs) => {
            param_evaluation_order(lhs, out);
            param_evaluation_order(rhs, out);
        }
        Expr::NotEqZero(inner) => param_evaluation_order(inner, out),
        Expr::Call(_, args) => {
            for a in args {
                param_evaluation_order(a, out);
            }
        }
    }
}

/// Whether `expr` contains a function call anywhere inside it. A `Call`
/// represents "evaluate this once" in a call-by-value language -- an
/// argument expression containing one is not safe to copy into more than
/// one substitution site, regardless of whether the *callee being called
/// within that argument* happens to be marked `has_side_effects` or not:
/// this experiment's representation has no purity fact for arbitrary
/// expressions, only for whole functions, so a `Call` anywhere inside an
/// argument is treated as "cannot prove this is safe to duplicate" --
/// fail closed, not "assume calls are pure unless proven otherwise."
fn expr_contains_call(expr: &Expr) -> bool {
    match expr {
        Expr::Param(_) => false,
        Expr::WrappingAdd(lhs, rhs) => expr_contains_call(lhs) || expr_contains_call(rhs),
        Expr::NotEqZero(inner) => expr_contains_call(inner),
        Expr::Call(..) => true,
    }
}

/// Inlines one `Call(callee_name, call_args)` expression, given `callee`'s
/// own fact/body from `program`. Refuses (`Err`) when:
/// - `has_side_effects` is set on the callee itself -- the load-bearing
///   check this whole module exists to demonstrate;
/// - the callee's body isn't the single-`Return` shape this small
///   experiment's inliner knows how to substitute (a named limitation,
///   not a silent wrong answer for a shape it can't handle);
/// - **a parameter's occurrence count in the callee's body is anything
///   other than exactly one, and the corresponding actual argument
///   expression contains a function call** -- two distinct real bugs an
///   external review caught, both from the same root cause
///   (`substitute_params` copies the argument expression verbatim into
///   *every* occurrence site, including zero or many):
///   - **more than one occurrence** duplicates the call: `double(effect
///     (x))` (where `double`'s own body is `x +% x`, referencing its one
///     parameter twice) would silently duplicate the call to `effect`,
///     invoking it twice -- changing observable behavior even though only
///     `double`'s own `has_side_effects` fact (correctly `false`) was ever
///     checked, never the argument's own shape. Reproduced directly before
///     this fix: inlining `double(effect(x))` was permitted despite
///     `effect` being registered as having side effects.
///   - **zero occurrences** (an unused parameter) silently *drops* the
///     call entirely: for `pick(x, y) = x`, inlining `pick(x, effect(x))`
///     substitutes only the `x` actually referenced, so the call to
///     `effect` disappears from the result -- never evaluated at all,
///     which is just as much an observable-behavior change as duplicating
///     it. Reproduced directly before this fix.
///
/// - **two or more parameters each receive a call-argument expression,
///   and the callee's body evaluates them in a different relative order
///   than the caller's own left-to-right argument list** -- a third
///   external review caught this exact gap, previously documented here
///   as open and unfixed: an occurrence-count of exactly one only proves
///   "evaluated once," not "evaluated at the same point in the
///   sequence." Reproduced directly before this fix: for `reverse(x, y)
///   = y +% x` (each parameter referenced exactly once, so the
///   occurrence-count check above raised no objection), inlining
///   `reverse(effect1(), effect2())` returned `Ok` and produced `effect2
///   () +% effect1()` -- silently reordering two side-effecting calls
///   relative to the caller's own `effect1()`-then-`effect2()`
///   evaluation order. Checked via `param_evaluation_order`, which walks
///   the callee's body in the exact order `eval_expr` (`eval.rs`)
///   actually evaluates it: for every pair of parameters whose actual
///   argument could have an effect (`expr_contains_call`), the earlier-
///   indexed argument's parameter must occur at an earlier evaluation
///   position than the later-indexed one, or inlining is refused.
///
///   This still does not cover every conceivable case a real
///   single-evaluation, order-preserving binding form (a `let`-like
///   construct) would handle more precisely -- that representation
///   remains unimplemented in this small experiment -- but the rule
///   above is conservative in the correct direction: it never permits an
///   order it cannot show matches the caller's own, rather than
///   assuming reordering is safe absent proof otherwise.
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
        Stmt::Return(body_expr) => {
            for (param_index, arg) in call_args.iter().enumerate() {
                let occurrences = count_param_occurrences(body_expr, param_index);
                if occurrences != 1 && expr_contains_call(arg) {
                    let hazard = if occurrences == 0 {
                        "is never referenced in its body, so substituting it naively would drop \
                         the call (and any effect it has) entirely, never evaluating it"
                    } else {
                        "is referenced more than once in its body, so substituting it naively \
                         would duplicate that call (and any effect it has)"
                    };
                    return Err(format!(
                        "refusing to inline `{callee_name}`: parameter {param_index} {hazard} -- \
                         which this experiment's inliner cannot prove safe without a single-\
                         evaluation binding it doesn't implement (occurrences={occurrences})"
                    ));
                }
            }

            // Evaluation-order hazard: every parameter reaching this
            // point that receives an effectful argument is guaranteed
            // (by the loop above) to occur exactly once in body_expr,
            // but "exactly once" says nothing about *where* -- checked
            // by pairwise-adjacent comparison over the effectful
            // arguments in their original (ascending) call-argument
            // order, which is sufficient to verify the whole sequence is
            // monotonically increasing (if position(a) < position(b) and
            // position(b) < position(c), then position(a) < position(c)
            // transitively).
            let mut evaluation_order = Vec::new();
            param_evaluation_order(body_expr, &mut evaluation_order);
            let effectful_param_indices: Vec<usize> = call_args
                .iter()
                .enumerate()
                .filter(|(_, arg)| expr_contains_call(arg))
                .map(|(i, _)| i)
                .collect();
            for pair in effectful_param_indices.windows(2) {
                let (earlier, later) = (pair[0], pair[1]);
                let earlier_pos = evaluation_order
                    .iter()
                    .position(|&p| p == earlier)
                    .expect("an effectful argument's parameter occurs exactly once, checked above");
                let later_pos = evaluation_order
                    .iter()
                    .position(|&p| p == later)
                    .expect("an effectful argument's parameter occurs exactly once, checked above");
                if earlier_pos > later_pos {
                    return Err(format!(
                        "refusing to inline `{callee_name}`: parameters {earlier} and {later} \
                         both receive call-argument expressions, but `{callee_name}`'s body \
                         evaluates parameter {later} before parameter {earlier} -- substitution \
                         would reorder their side effects relative to the caller's own \
                         left-to-right argument evaluation, which this experiment's inliner \
                         cannot prove safe without a single-evaluation, order-preserving binding \
                         it doesn't implement"
                    ));
                }
            }

            Ok(substitute_params(body_expr, call_args))
        }
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
    use crate::repr::{double_with_side_effect_fact, workload_program, FnFact};

    /// A program identical to `workload_program()`'s, plus a hypothetical
    /// `effect` function (`has_side_effects=true`, identity body) --
    /// used only to demonstrate the "duplicated side-effecting argument"
    /// bug: `double`'s own `has_side_effects` stays `false` throughout,
    /// so only the argument-duplication check (not the callee check) can
    /// catch this case.
    fn program_with_effect_function() -> crate::repr::SemanticProgram {
        let mut program = workload_program();
        program.insert(FnFact {
            name: "effect".to_string(),
            param_widths: vec![32],
            return_width: 32,
            has_side_effects: true,
            body: Stmt::Return(Expr::Param(0)),
        });
        program
    }

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

    /// The exact bug an external review caught: `double`'s own body (`x
    /// +% x`) references its one parameter twice, so inlining `double
    /// (effect(x))` would naively duplicate the call to `effect` --
    /// invoking a side-effecting function twice -- even though `double`
    /// itself is correctly marked `has_side_effects=false`. Only checking
    /// the callee's own fact (as the "allowed"/"rejected" cases above do)
    /// cannot catch this; the argument's own shape must be checked too.
    #[test]
    fn inlining_is_refused_when_a_multiply_referenced_parameter_receives_a_call_argument() {
        let program = program_with_effect_function();
        let call_args = vec![Expr::Call("effect".to_string(), vec![Expr::Param(0)])];

        let result = inline_call(&program, "double", &call_args);

        assert!(
            result.is_err(),
            "inlining double(effect(x)) must be refused -- double's body references its \
             parameter twice, so substitution would duplicate the call to effect, got Ok"
        );
        let reason = result.unwrap_err();
        assert!(reason.contains("referenced"), "reason: {reason}");
    }

    /// The exact bug an external review caught, second half: for `pick(x,
    /// y) = x`, `y` is never referenced in the body at all, so inlining
    /// `pick(x, effect(x))` previously substituted only the referenced
    /// parameter and silently dropped the call to `effect` -- the call
    /// disappears from the result entirely, never evaluated, which is
    /// just as much an observable-behavior change as duplicating it.
    #[test]
    fn inlining_is_refused_when_an_unused_parameter_receives_a_call_argument() {
        let mut program = program_with_effect_function();
        program.insert(FnFact {
            name: "pick".to_string(),
            param_widths: vec![32, 32],
            return_width: 32,
            has_side_effects: false,
            body: Stmt::Return(Expr::Param(0)),
        });
        let call_args = vec![
            Expr::Param(0),
            Expr::Call("effect".to_string(), vec![Expr::Param(0)]),
        ];

        let result = inline_call(&program, "pick", &call_args);

        assert!(
            result.is_err(),
            "inlining pick(x, effect(x)) must be refused -- pick's body never references its \
             second parameter, so substitution would silently drop the call to effect, got Ok"
        );
        let reason = result.unwrap_err();
        assert!(reason.contains("occurrences=0"), "reason: {reason}");
    }

    /// The same shape, with a callee whose parameter is referenced only
    /// once -- inlining must still be permitted, since there is nothing
    /// to duplicate. Confirms the fix is scoped to the actual hazard
    /// (occurrence counts other than exactly one), not a blanket "never
    /// inline a call argument" rule.
    #[test]
    fn inlining_a_call_argument_into_a_singly_referenced_parameter_is_still_allowed() {
        let mut program = program_with_effect_function();
        program.insert(FnFact {
            name: "identity".to_string(),
            param_widths: vec![32],
            return_width: 32,
            has_side_effects: false,
            body: Stmt::Return(Expr::Param(0)),
        });
        let call_args = vec![Expr::Call("effect".to_string(), vec![Expr::Param(0)])];

        let result = inline_call(&program, "identity", &call_args);

        assert!(
            result.is_ok(),
            "identity's parameter is referenced only once, so substituting a call argument \
             there duplicates nothing and must be permitted, got {result:?}"
        );
    }

    /// The exact bug a third external review caught: `reverse`'s body
    /// (`y +% x`) references each of its two parameters exactly once --
    /// the occurrence-count check alone raises no objection -- but
    /// evaluates them in the *opposite* order from the caller's own
    /// left-to-right `reverse(effect1(), effect2())` argument list.
    /// Naive substitution would produce `effect2() +% effect1()`,
    /// silently reordering two side-effecting calls.
    #[test]
    fn inlining_is_refused_when_two_effectful_arguments_would_be_evaluated_out_of_order() {
        let mut program = program_with_effect_function();
        program.insert(FnFact {
            name: "reverse".to_string(),
            param_widths: vec![32, 32],
            return_width: 32,
            has_side_effects: false,
            body: Stmt::Return(Expr::WrappingAdd(
                Box::new(Expr::Param(1)),
                Box::new(Expr::Param(0)),
            )),
        });
        let call_args = vec![
            Expr::Call("effect".to_string(), vec![Expr::Param(0)]),
            Expr::Call("effect".to_string(), vec![Expr::Param(1)]),
        ];

        let result = inline_call(&program, "reverse", &call_args);

        assert!(
            result.is_err(),
            "inlining reverse(effect1(), effect2()) must be refused -- reverse's body evaluates \
             its second argument before its first, reordering their side effects, got Ok"
        );
        let reason = result.unwrap_err();
        assert!(
            reason.contains("evaluates parameter") && reason.contains("before parameter"),
            "reason: {reason}"
        );
    }

    /// The companion case: a callee whose body evaluates its parameters
    /// in the *same* order as the caller's own argument list (`x +% y`,
    /// not `y +% x`) must still be permitted even with two effectful
    /// arguments -- confirms the new check is scoped to actual
    /// reordering, not a blanket "never inline more than one effectful
    /// argument" rule.
    #[test]
    fn inlining_two_effectful_arguments_in_the_same_order_is_still_allowed() {
        let mut program = program_with_effect_function();
        program.insert(FnFact {
            name: "combine".to_string(),
            param_widths: vec![32, 32],
            return_width: 32,
            has_side_effects: false,
            body: Stmt::Return(Expr::WrappingAdd(
                Box::new(Expr::Param(0)),
                Box::new(Expr::Param(1)),
            )),
        });
        let call_args = vec![
            Expr::Call("effect".to_string(), vec![Expr::Param(0)]),
            Expr::Call("effect".to_string(), vec![Expr::Param(1)]),
        ];

        let result = inline_call(&program, "combine", &call_args);

        assert!(
            result.is_ok(),
            "combine's body evaluates its parameters in the same order the caller's argument \
             list did, so nothing is reordered and inlining must be permitted, got {result:?}"
        );
    }
}

//! Candidate A: A-normal-form-style binding insertion. Instead of checking
//! whether verbatim substitution is safe (candidate B,
//! [`super::checked_inline`]), this hoists *every* call argument into an
//! explicit [`crate::types::Expr::Let`] binding, in the caller's own
//! left-to-right evaluation order, before substituting the callee body.
//! Each argument is then evaluated exactly once, at the position the
//! original call occupied, regardless of how many times (zero, one, or
//! many) the callee body references the corresponding parameter, and
//! regardless of what order it references them in -- duplication,
//! dropping, and reordering all become structurally unrepresentable, not
//! merely detected and refused. See `transform`'s own module doc comment
//! for the comparison this exists to support.

use crate::types::{Expr, LocalId, Program, Provenance};

use super::{
    as_simple_return_expr, prepare_callee_body_for_grafting, rewrite_calls_in_stmt,
    substitute_params_with_locals, TransformError,
};

/// Inlines every call to `callee_name` inside `caller_name`'s body via
/// ANF-style let-insertion. Unlike [`super::checked_inline::checked_inline`],
/// this never rejects a call site for duplication/dropping/reordering --
/// only for an unsupported callee shape (see
/// [`TransformError::UnsupportedCalleeShape`]) or an arity mismatch.
pub fn anf_insert(
    program: &Program,
    caller_name: &str,
    callee_name: &str,
) -> Result<Program, TransformError> {
    let callee_fact = program
        .functions
        .get(callee_name)
        .ok_or_else(|| TransformError::UnknownCallee(callee_name.to_string()))?;
    let raw_callee_body = as_simple_return_expr(callee_fact)
        .ok_or_else(|| TransformError::UnsupportedCalleeShape {
            callee: callee_name.to_string(),
        })?
        .clone();
    let param_count = callee_fact.params.len();

    let caller_fact = program
        .functions
        .get(caller_name)
        .ok_or_else(|| TransformError::UnknownCaller(caller_name.to_string()))?;

    // Alpha-renames the callee body's own embedded `Let`s (if it has any,
    // from an earlier inlining pass) and returns a starting counter for
    // this call's *own* fresh per-argument bindings that continues past
    // both the caller's existing ids and the (now-renamed) callee body's
    // own -- see `prepare_callee_body_for_grafting`'s doc comment for the
    // composition bug this closes, and the doc comment on the earlier,
    // narrower fix this replaces (considering only the caller's own
    // pre-existing locals was not enough once a callee's body can itself
    // already contain embedded `Let`s from a prior transformation).
    let (callee_body, mut next_local) =
        prepare_callee_body_for_grafting(&caller_fact.body, &raw_callee_body);

    let new_body = rewrite_calls_in_stmt(
        &caller_fact.body,
        callee_name,
        &mut |args, prov: &Provenance| {
            if args.len() != param_count {
                return Err(TransformError::UnsupportedCalleeShape {
                    callee: callee_name.to_string(),
                });
            }

            let locals: Vec<LocalId> = (0..param_count)
                .map(|_| {
                    let id = LocalId(next_local);
                    next_local += 1;
                    id
                })
                .collect();

            let substituted = substitute_params_with_locals(&callee_body, &locals, prov);

            // Wrap from the last argument inward, so the resulting nested
            // `Let`s evaluate in the original left-to-right argument order:
            // `Let(l0, args[0], Let(l1, args[1], ..., substituted))`.
            let mut result = substituted;
            for (i, local) in locals.into_iter().enumerate().rev() {
                result = Expr::Let {
                    local,
                    value: Box::new(args[i].clone()),
                    body: Box::new(result),
                    provenance: prov.clone(),
                };
            }
            Ok(result)
        },
    )?;

    super::apply_inlined_caller(program, caller_name, new_body)
}

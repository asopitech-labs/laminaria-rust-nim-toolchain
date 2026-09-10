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
    as_simple_return_expr, rewrite_calls_in_stmt, substitute_params_with_locals, TransformError,
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
    let callee_body = as_simple_return_expr(callee_fact)
        .ok_or_else(|| TransformError::UnsupportedCalleeShape {
            callee: callee_name.to_string(),
        })?
        .clone();
    let param_count = callee_fact.params.len();

    let caller_fact = program
        .functions
        .get(caller_name)
        .ok_or_else(|| TransformError::UnknownCaller(caller_name.to_string()))?;

    // Starts *above* every `LocalId` already used anywhere in the
    // caller's body, not at 0 -- a review caught a real bug here: an id
    // starting at 0 collides with a pre-existing local (e.g. a real
    // source-level `let`), and because the interpreter's `Let`-scoping
    // restores "whatever was bound before" by numeric id, a colliding
    // fresh binding can silently capture -- and then, once its own scope
    // ends, leave overwritten -- a same-numbered outer variable a later
    // hoisted argument still needed to read. Reproduced directly: with
    // `caller() = combine(b, a)` inlined where `a`/`b` are pre-existing
    // locals whose ids happen to equal the two fresh ids this transform
    // would otherwise pick, the second hoisted argument's own value
    // expression (`Local` referencing the outer `a`) read back the
    // *already-rebound* slot from the first hoisted argument instead of
    // `a`'s real value, computing the wrong result silently. Starting
    // above the caller's own maximum in-scope id makes every fresh
    // binding this transform introduces provably distinct from anything
    // it could otherwise shadow.
    let mut next_local = crate::types::max_local_id_in_stmt(&caller_fact.body)
        .map(|m| m + 1)
        .unwrap_or(0);

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

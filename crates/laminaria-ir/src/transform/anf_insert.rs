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
    alpha_rename_for_one_graft, as_simple_return_expr, initial_next_local, rewrite_calls_in_stmt,
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

    // Starting counter for this whole operation's fresh ids, above every id
    // already meaningful in the caller's body.
    let mut next_local = initial_next_local(&caller_fact.body);

    let new_body = rewrite_calls_in_stmt(
        &caller_fact.body,
        callee_name,
        &mut |args, prov: &Provenance| {
            if args.len() != param_count {
                return Err(TransformError::UnsupportedCalleeShape {
                    callee: callee_name.to_string(),
                });
            }

            // A *fresh* alpha-rename of the callee's raw body, per call
            // site -- see `alpha_rename_for_one_graft`'s own doc comment
            // for why reusing one renamed copy (and its ids) across more
            // than one call site of the same callee inside this caller
            // would be wrong: every such site ends up grafted into the
            // same caller scope, coexisting with each other. `next_local`
            // continues advancing past whatever this rename allocates, so
            // this call site's own fresh per-argument bindings (below)
            // are also guaranteed disjoint from it.
            let callee_body = alpha_rename_for_one_graft(&raw_callee_body, &mut next_local);

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

//! Candidate B: verbatim substitution, gated by a post-hoc safety check --
//! ports `fixtures/laminaria-semantic-substrate-prototype/substrate/src/
//! inline.rs`'s actual approach (occurrence counting plus
//! `param_evaluation_order` comparison) onto this crate's richer IR. See
//! `transform`'s own module doc comment for how this compares against
//! [`super::anf_insert::anf_insert`].

use crate::types::{expr_contains_call, Program};

use super::{
    as_simple_return_expr, count_param_occurrences, param_evaluation_order, rewrite_calls_in_stmt,
    substitute_params_verbatim, TransformError,
};

/// Inlines every call to `callee_name` inside `caller_name`'s body,
/// substituting verbatim -- but first checks, per call site, that doing so
/// cannot duplicate, drop, or reorder a call-containing (therefore
/// possibly effectful) argument relative to the caller's own left-to-right
/// evaluation. Rejects the whole transform (no partial substitution,
/// propagated via the first rejection found) if any call site fails.
pub fn checked_inline(
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

    let new_body = rewrite_calls_in_stmt(&caller_fact.body, callee_name, &mut |args, _prov| {
        if args.len() != param_count {
            return Err(TransformError::UnsupportedCalleeShape {
                callee: callee_name.to_string(),
            });
        }

        let mut evaluation_order = Vec::new();
        param_evaluation_order(&callee_body, &mut evaluation_order);
        let effectful_indices: Vec<usize> = (0..param_count)
            .filter(|&i| expr_contains_call(&args[i]))
            .collect();

        for &i in &effectful_indices {
            let occurrences = count_param_occurrences(&callee_body, i);
            if occurrences > 1 {
                return Err(TransformError::WouldDuplicateEffect {
                    callee: callee_name.to_string(),
                    param_index: i,
                });
            }
            if occurrences == 0 {
                return Err(TransformError::WouldDropEffect {
                    callee: callee_name.to_string(),
                    param_index: i,
                });
            }
        }
        for pair in effectful_indices.windows(2) {
            let (earlier, later) = (pair[0], pair[1]);
            let pos_earlier = evaluation_order.iter().position(|&p| p == earlier);
            let pos_later = evaluation_order.iter().position(|&p| p == later);
            if let (Some(pe), Some(pl)) = (pos_earlier, pos_later) {
                if pe > pl {
                    return Err(TransformError::WouldReorderEffects {
                        callee: callee_name.to_string(),
                        earlier_arg: earlier,
                        later_arg: later,
                    });
                }
            }
        }

        Ok(substitute_params_verbatim(&callee_body, args))
    })?;

    super::apply_inlined_caller(program, caller_name, new_body)
}

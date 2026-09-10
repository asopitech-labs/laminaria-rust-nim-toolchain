//! Two compared candidate representations for "an argument is evaluated
//! exactly once, in the caller's original order" -- issue #25's own third
//! comment flags this as the concrete open gap in
//! `fixtures/laminaria-semantic-substrate-prototype/substrate/src/inline.rs`
//! ("a real single-evaluation order-preserving `let`-like binding form
//! remains unimplemented"). Both candidates inline every call to a named
//! callee inside a named caller's body, run through the identical safety
//! battery in this crate's tests, and are compared honestly rather than
//! one being assumed correct upfront (`docs/llvm-rediscovery-research.md`'s
//! own "do not adopt concepts because they already exist" stance, applied
//! here to ANF/SSA-style binding disciplines, not just LLVM):
//!
//! - [`checked_inline`] ports the existing fixture's approach: substitute
//!   verbatim, but first *check* whether that substitution would
//!   duplicate, drop, or reorder a call-containing (therefore possibly
//!   effectful) argument, and refuse if so.
//! - [`anf_insert`] instead hoists every argument into an explicit
//!   [`crate::types::Expr::Let`] binding, in the caller's own left-to-right
//!   evaluation order, before substituting -- duplication, dropping, and
//!   reordering become structurally impossible (each argument is evaluated
//!   exactly once, at the original call site's position, regardless of how
//!   many times or in what order the callee body's parameters are
//!   referenced), so it never needs to refuse on those grounds at all.
//!
//! See `NOTES.md` for the honest comparison write-up.

pub mod anf_insert;
pub mod checked_inline;
#[cfg(test)]
mod composition_fuzz;

use crate::types::{Expr, FnFact, FnId, LocalId, Program, Provenance, Stmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformError {
    UnknownCaller(String),
    UnknownCallee(String),
    /// This task's first slice only substitutes a callee whose body is a
    /// single `Return(expr)` with no internal `Let`/`If` of its own --
    /// matching the existing fixture's own `double`-shaped example
    /// exactly. Extending either candidate to a richer callee body is a
    /// direct follow-up, not a design gap either candidate's own
    /// substitution logic would need to change to support.
    UnsupportedCalleeShape {
        callee: String,
    },
    /// `checked_inline` only: a call-containing argument would be
    /// duplicated (the callee references that parameter more than once).
    WouldDuplicateEffect {
        callee: String,
        param_index: usize,
    },
    /// `checked_inline` only: a call-containing argument would never be
    /// evaluated at all (the callee never references that parameter).
    WouldDropEffect {
        callee: String,
        param_index: usize,
    },
    /// `checked_inline` only: two call-containing arguments would be
    /// evaluated in a different relative order than the caller's own
    /// left-to-right argument evaluation.
    WouldReorderEffects {
        callee: String,
        earlier_arg: usize,
        later_arg: usize,
    },
    /// `checked_inline` only: a call-containing argument would end up
    /// evaluated *after* a call the callee body makes internally (a `Call`
    /// present directly in the callee's own body, not derived from any
    /// parameter substitution). Real call semantics evaluate every argument
    /// before the callee body starts running at all, so this would run the
    /// callee's own internal call ahead of an argument's effect instead of
    /// after it. See issue #27's reference to rustc's AST-attribute/
    /// evaluation-order handling -- this is the same "the callee's own body
    /// is not just a hole for parameters" class of gap, applied to effect
    /// ordering rather than attributes.
    WouldReorderRelativeToCalleeCall {
        callee: String,
        param_index: usize,
    },
}

/// The callee shape both candidates currently support: a body that is
/// exactly one `Return(expr)`. See [`TransformError::UnsupportedCalleeShape`].
fn as_simple_return_expr(fact: &FnFact) -> Option<&Expr> {
    match &fact.body {
        Stmt::Return(expr, _) => Some(expr),
        _ => None,
    }
}

/// Alpha-renames every `LocalId` bound by a `Let` inside `expr` (and every
/// `Local` reference to it) to a fresh id starting from `*next_local`,
/// which is advanced past whatever it allocates. Applied to a callee's
/// body *before* either candidate substitutes/wraps it into a caller,
/// closing a real composition bug a review reproduced: once a callee's own
/// body already contains embedded `Let`s (because it was itself produced
/// by an earlier inlining pass), grafting that body verbatim into a
/// *different* function can numerically collide with `LocalId`s already
/// meaningful there -- either the caller's own pre-existing locals, or
/// (for `checked_inline`, which introduces no fresh ids of its own) a
/// `Local` reference embedded in one of the *arguments* being substituted
/// in. When a nested scope reusing the same id sits between where such a
/// reference is evaluated and where it's actually read, it silently
/// resolves to the wrong binding instead. Reproduced directly with
/// `add(x,y)=x+y`, `g(x)=add(1,x)`, `f()=g(10)`: inlining `add` into `g`
/// first (giving `g` two internal `Let`s starting at id 0), then inlining
/// `g` into `f` without this rename, computed `2` instead of the correct
/// `11`. Starting the caller-side fresh-id counter *for this whole
/// operation* above both the caller's own maximum id and the (now-renamed)
/// callee body's maximum id makes every id introduced or carried in by
/// this one splice provably distinct from anything already in scope at
/// the graft site, regardless of how many prior transformations either
/// side has already been through.
fn alpha_rename_callee_body(
    expr: &Expr,
    remap: &mut std::collections::BTreeMap<LocalId, LocalId>,
    next_local: &mut u32,
) -> Expr {
    match expr {
        Expr::IntLit(..) | Expr::Param(..) => expr.clone(),
        Expr::Local(id, p) => {
            let renamed = remap.get(id).copied().unwrap_or(*id);
            Expr::Local(renamed, p.clone())
        }
        Expr::WrappingAdd(a, b, p) => Expr::WrappingAdd(
            Box::new(alpha_rename_callee_body(a, remap, next_local)),
            Box::new(alpha_rename_callee_body(b, remap, next_local)),
            p.clone(),
        ),
        Expr::WrappingSub(a, b, p) => Expr::WrappingSub(
            Box::new(alpha_rename_callee_body(a, remap, next_local)),
            Box::new(alpha_rename_callee_body(b, remap, next_local)),
            p.clone(),
        ),
        Expr::WrappingMul(a, b, p) => Expr::WrappingMul(
            Box::new(alpha_rename_callee_body(a, remap, next_local)),
            Box::new(alpha_rename_callee_body(b, remap, next_local)),
            p.clone(),
        ),
        Expr::NotEqZero(inner, p) => Expr::NotEqZero(
            Box::new(alpha_rename_callee_body(inner, remap, next_local)),
            p.clone(),
        ),
        Expr::Call(name, args, p) => Expr::Call(
            name.clone(),
            args.iter()
                .map(|a| alpha_rename_callee_body(a, remap, next_local))
                .collect(),
            p.clone(),
        ),
        Expr::Let {
            local,
            value,
            body,
            provenance,
        } => {
            // `value` is renamed under the remap as it stood *before* this
            // Let's own binder is added -- a well-formed `let` initializer
            // never references its own binder.
            let renamed_value = alpha_rename_callee_body(value, remap, next_local);
            let new_id = LocalId(*next_local);
            *next_local += 1;
            remap.insert(*local, new_id);
            let renamed_body = alpha_rename_callee_body(body, remap, next_local);
            Expr::Let {
                local: new_id,
                value: Box::new(renamed_value),
                body: Box::new(renamed_body),
                provenance: provenance.clone(),
            }
        }
    }
}

/// Starting fresh-`LocalId` counter for a whole inlining operation: strictly
/// above every id already meaningful in `caller_body`, so the *first* graft
/// this operation performs (see [`alpha_rename_for_one_graft`]) cannot
/// collide with anything the caller already binds or references.
fn initial_next_local(caller_body: &Stmt) -> u32 {
    crate::types::max_local_id_in_stmt(caller_body)
        .map(|m| m + 1)
        .unwrap_or(0)
}

/// Alpha-renames a *fresh copy* of the callee's raw (un-renamed) body for
/// exactly one graft site, using a brand-new, empty substitution map every
/// time this is called. `next_local` is threaded across every call site an
/// inlining operation performs (the shared `rewrite_calls_in_stmt` closure
/// calls this once per matched call site), so each successive call
/// continues allocating ids strictly above whatever the previous one used.
///
/// This must NOT reuse one renamed copy (and its ids) across more than one
/// call site: a review reproduced the real consequence directly when
/// `callee_name` is called more than once inside the same caller body --
/// every one of those call sites ends up grafted into the *same* caller
/// scope, coexisting with each other, so reusing identical ids for the
/// callee's own embedded `Let`s at each site is exactly the same hazard the
/// caller-vs-callee alpha-rename above closes, one level up: two grafted
/// copies of the same callee body would bind the *same* `LocalId` in the
/// same enclosing scope, so a reference meant for one copy's binding can
/// silently resolve to the other copy's instead. Nim's own
/// `openScope`/`rawCloseScope` (issue #27's reference table) keep exactly
/// this invariant for a single lexical scope -- a scope's own bindings
/// never leak into a sibling scope opened afterward; a fresh, empty `remap`
/// per graft site is this transform's equivalent: each insertion point is
/// its own scope, closed (its `remap` dropped) before the next one opens.
fn alpha_rename_for_one_graft(raw_callee_body: &Expr, next_local: &mut u32) -> Expr {
    let mut remap = std::collections::BTreeMap::new();
    alpha_rename_callee_body(raw_callee_body, &mut remap, next_local)
}

fn count_param_occurrences(expr: &Expr, target: usize) -> usize {
    match expr {
        Expr::Param(i, _) => usize::from(*i == target),
        Expr::IntLit(..) | Expr::Local(..) => 0,
        Expr::WrappingAdd(a, b, _) | Expr::WrappingSub(a, b, _) | Expr::WrappingMul(a, b, _) => {
            count_param_occurrences(a, target) + count_param_occurrences(b, target)
        }
        Expr::NotEqZero(inner, _) => count_param_occurrences(inner, target),
        Expr::Call(_, args, _) => args
            .iter()
            .map(|a| count_param_occurrences(a, target))
            .sum(),
        Expr::Let { value, body, .. } => {
            count_param_occurrences(value, target) + count_param_occurrences(body, target)
        }
    }
}

/// The evaluation-order sequence of parameter references inside `expr`, in
/// the exact order `interpreter::eval_expr` actually evaluates them --
/// generalizes `fixtures/.../inline.rs`'s own `param_evaluation_order`
/// (left operand before right for wrapping ops, argument order for calls,
/// straight through for `NotEqZero`) to this crate's richer `Expr`.
fn param_evaluation_order(expr: &Expr, out: &mut Vec<usize>) {
    match expr {
        Expr::Param(i, _) => out.push(*i),
        Expr::IntLit(..) | Expr::Local(..) => {}
        Expr::WrappingAdd(a, b, _) | Expr::WrappingSub(a, b, _) | Expr::WrappingMul(a, b, _) => {
            param_evaluation_order(a, out);
            param_evaluation_order(b, out);
        }
        Expr::NotEqZero(inner, _) => param_evaluation_order(inner, out),
        Expr::Call(_, args, _) => {
            for a in args {
                param_evaluation_order(a, out);
            }
        }
        Expr::Let { value, body, .. } => {
            param_evaluation_order(value, out);
            param_evaluation_order(body, out);
        }
    }
}

/// One position in a callee body's own left-to-right evaluation order,
/// unifying two kinds of observable event `checked_inline` must reason
/// about together: a parameter reference (which becomes an argument's own
/// effect once substituted) and a `Call` the callee body makes *directly*,
/// independent of any parameter (a genuine internal effect of the callee
/// itself, e.g. `mark(2) +% x`'s `mark(2)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservedEvent {
    Param(usize),
    CalleeCall,
}

/// The evaluation-order sequence of both parameter references *and* the
/// callee body's own internal calls, in the exact order
/// `interpreter::eval_expr` actually evaluates them (per its own `Call`
/// arm: a call's arguments evaluate first, then the call itself is
/// recorded as an effect, matching the `CalleeCall` marker's position
/// here). `checked_inline` uses this to check a fact `param_evaluation_order`
/// alone cannot see: whether an effectful argument would end up evaluated
/// *after* a call the callee body makes on its own -- real call semantics
/// evaluate every argument before the callee body runs at all, so an
/// effectful `Param` occurring after a `CalleeCall` here means verbatim
/// substitution would reorder them (issue #27's own worked example: a
/// callee body of `mark(2) +% x` called as `f(mark(1))` must run `mark(1)`
/// before `mark(2)`, but substituting `x -> mark(1)` verbatim produces
/// `mark(2) +% mark(1)`, evaluated in the opposite order).
fn observed_event_order(expr: &Expr, out: &mut Vec<ObservedEvent>) {
    match expr {
        Expr::Param(i, _) => out.push(ObservedEvent::Param(*i)),
        Expr::IntLit(..) | Expr::Local(..) => {}
        Expr::WrappingAdd(a, b, _) | Expr::WrappingSub(a, b, _) | Expr::WrappingMul(a, b, _) => {
            observed_event_order(a, out);
            observed_event_order(b, out);
        }
        Expr::NotEqZero(inner, _) => observed_event_order(inner, out),
        Expr::Call(_, args, _) => {
            for a in args {
                observed_event_order(a, out);
            }
            out.push(ObservedEvent::CalleeCall);
        }
        Expr::Let { value, body, .. } => {
            observed_event_order(value, out);
            observed_event_order(body, out);
        }
    }
}

/// Verbatim substitution: every `Param(i)` in `callee_body` becomes a
/// *clone* of `args[i]` -- used by `checked_inline` only after it has
/// already verified this cannot duplicate/drop/reorder an effect.
fn substitute_params_verbatim(callee_body: &Expr, args: &[Expr]) -> Expr {
    match callee_body {
        Expr::Param(i, _) => args[*i].clone(),
        Expr::IntLit(..) | Expr::Local(..) => callee_body.clone(),
        Expr::WrappingAdd(a, b, p) => Expr::WrappingAdd(
            Box::new(substitute_params_verbatim(a, args)),
            Box::new(substitute_params_verbatim(b, args)),
            p.clone(),
        ),
        Expr::WrappingSub(a, b, p) => Expr::WrappingSub(
            Box::new(substitute_params_verbatim(a, args)),
            Box::new(substitute_params_verbatim(b, args)),
            p.clone(),
        ),
        Expr::WrappingMul(a, b, p) => Expr::WrappingMul(
            Box::new(substitute_params_verbatim(a, args)),
            Box::new(substitute_params_verbatim(b, args)),
            p.clone(),
        ),
        Expr::NotEqZero(inner, p) => {
            Expr::NotEqZero(Box::new(substitute_params_verbatim(inner, args)), p.clone())
        }
        Expr::Call(name, cargs, p) => Expr::Call(
            name.clone(),
            cargs
                .iter()
                .map(|a| substitute_params_verbatim(a, args))
                .collect(),
            p.clone(),
        ),
        Expr::Let {
            local,
            value,
            body,
            provenance,
        } => Expr::Let {
            local: *local,
            value: Box::new(substitute_params_verbatim(value, args)),
            body: Box::new(substitute_params_verbatim(body, args)),
            provenance: provenance.clone(),
        },
    }
}

/// Structural substitution: every `Param(i)` in `callee_body` becomes a
/// reference to `locals[i]` -- used by `anf_insert`, which has already
/// bound each `locals[i]` to `args[i]`'s value via a `Let` wrapped around
/// the result, so the parameter's *value* is always `args[i]` regardless
/// of how many times (zero, one, or many) this substitution references it.
fn substitute_params_with_locals(
    callee_body: &Expr,
    locals: &[LocalId],
    provenance: &Provenance,
) -> Expr {
    match callee_body {
        Expr::Param(i, _) => Expr::Local(locals[*i], provenance.clone()),
        Expr::IntLit(..) | Expr::Local(..) => callee_body.clone(),
        Expr::WrappingAdd(a, b, p) => Expr::WrappingAdd(
            Box::new(substitute_params_with_locals(a, locals, provenance)),
            Box::new(substitute_params_with_locals(b, locals, provenance)),
            p.clone(),
        ),
        Expr::WrappingSub(a, b, p) => Expr::WrappingSub(
            Box::new(substitute_params_with_locals(a, locals, provenance)),
            Box::new(substitute_params_with_locals(b, locals, provenance)),
            p.clone(),
        ),
        Expr::WrappingMul(a, b, p) => Expr::WrappingMul(
            Box::new(substitute_params_with_locals(a, locals, provenance)),
            Box::new(substitute_params_with_locals(b, locals, provenance)),
            p.clone(),
        ),
        Expr::NotEqZero(inner, p) => Expr::NotEqZero(
            Box::new(substitute_params_with_locals(inner, locals, provenance)),
            p.clone(),
        ),
        Expr::Call(name, cargs, p) => Expr::Call(
            name.clone(),
            cargs
                .iter()
                .map(|a| substitute_params_with_locals(a, locals, provenance))
                .collect(),
            p.clone(),
        ),
        Expr::Let {
            local,
            value,
            body,
            provenance: bp,
        } => Expr::Let {
            local: *local,
            value: Box::new(substitute_params_with_locals(value, locals, provenance)),
            body: Box::new(substitute_params_with_locals(body, locals, provenance)),
            provenance: bp.clone(),
        },
    }
}

/// Rewrites every `Call(callee_name, args, _)` node anywhere in `stmt`
/// (recursing into `args` first, so a call nested inside another call's
/// arguments is rewritten too) via `replace`, which receives the already-
/// rewritten argument expressions and produces the replacement expression.
/// Shared by both candidates -- they differ only in what `replace` does.
fn rewrite_calls_in_stmt(
    stmt: &Stmt,
    callee_name: &str,
    replace: &mut impl FnMut(&[Expr], &Provenance) -> Result<Expr, TransformError>,
) -> Result<Stmt, TransformError> {
    Ok(match stmt {
        Stmt::Let {
            local,
            value,
            body,
            provenance,
        } => Stmt::Let {
            local: *local,
            value: rewrite_calls_in_expr(value, callee_name, replace)?,
            body: Box::new(rewrite_calls_in_stmt(body, callee_name, replace)?),
            provenance: provenance.clone(),
        },
        Stmt::If {
            cond,
            then,
            els,
            provenance,
        } => Stmt::If {
            cond: rewrite_calls_in_expr(cond, callee_name, replace)?,
            then: Box::new(rewrite_calls_in_stmt(then, callee_name, replace)?),
            els: Box::new(rewrite_calls_in_stmt(els, callee_name, replace)?),
            provenance: provenance.clone(),
        },
        Stmt::Return(expr, provenance) => Stmt::Return(
            rewrite_calls_in_expr(expr, callee_name, replace)?,
            provenance.clone(),
        ),
    })
}

fn rewrite_calls_in_expr(
    expr: &Expr,
    callee_name: &str,
    replace: &mut impl FnMut(&[Expr], &Provenance) -> Result<Expr, TransformError>,
) -> Result<Expr, TransformError> {
    Ok(match expr {
        Expr::IntLit(..) | Expr::Param(..) | Expr::Local(..) => expr.clone(),
        Expr::WrappingAdd(a, b, p) => Expr::WrappingAdd(
            Box::new(rewrite_calls_in_expr(a, callee_name, replace)?),
            Box::new(rewrite_calls_in_expr(b, callee_name, replace)?),
            p.clone(),
        ),
        Expr::WrappingSub(a, b, p) => Expr::WrappingSub(
            Box::new(rewrite_calls_in_expr(a, callee_name, replace)?),
            Box::new(rewrite_calls_in_expr(b, callee_name, replace)?),
            p.clone(),
        ),
        Expr::WrappingMul(a, b, p) => Expr::WrappingMul(
            Box::new(rewrite_calls_in_expr(a, callee_name, replace)?),
            Box::new(rewrite_calls_in_expr(b, callee_name, replace)?),
            p.clone(),
        ),
        Expr::NotEqZero(inner, p) => Expr::NotEqZero(
            Box::new(rewrite_calls_in_expr(inner, callee_name, replace)?),
            p.clone(),
        ),
        Expr::Call(FnId(name), args, p) => {
            let rewritten_args: Vec<Expr> = args
                .iter()
                .map(|a| rewrite_calls_in_expr(a, callee_name, replace))
                .collect::<Result<_, _>>()?;
            if name == callee_name {
                replace(&rewritten_args, p)?
            } else {
                Expr::Call(FnId(name.clone()), rewritten_args, p.clone())
            }
        }
        Expr::Let {
            local,
            value,
            body,
            provenance,
        } => Expr::Let {
            local: *local,
            value: Box::new(rewrite_calls_in_expr(value, callee_name, replace)?),
            body: Box::new(rewrite_calls_in_expr(body, callee_name, replace)?),
            provenance: provenance.clone(),
        },
    })
}

/// Runs `inline_fn` (either candidate) and, on success, replaces `caller`'s
/// `FnFact` in a clone of `program` -- shared plumbing so both candidates'
/// public functions are a few lines each.
fn apply_inlined_caller(
    program: &Program,
    caller_name: &str,
    new_body: Stmt,
) -> Result<Program, TransformError> {
    let mut caller_fact = program
        .functions
        .get(caller_name)
        .ok_or_else(|| TransformError::UnknownCaller(caller_name.to_string()))?
        .clone();
    caller_fact.body = new_body;
    let mut result = program.clone();
    result.insert(caller_fact);
    Ok(result)
}

/// The comparison battery: the same four historical hazards
/// `fixtures/.../inline.rs` was built to catch (duplication, dropping,
/// reordering, plus a positive control), run against **both** candidates,
/// asserting what each one actually does -- not assuming an answer.
/// `mark(v) = v` stands in for "some effectful computation": it's a real
/// `Call`, so `expr_contains_call` sees it and the interpreter's effect
/// trace records it, without needing any I/O primitive this IR doesn't have.
#[cfg(test)]
mod tests {
    use super::anf_insert::anf_insert;
    use super::checked_inline::checked_inline;
    use super::*;
    use crate::interpreter::eval_function;
    use crate::types::{IntWidth, SourceLanguage, SourcePosition, SourceSpan};
    use std::path::PathBuf;

    fn prov() -> Provenance {
        Provenance {
            source_file: PathBuf::from("test"),
            span: SourceSpan {
                start: SourcePosition { line: 1, column: 1 },
                end: SourcePosition { line: 1, column: 1 },
            },
            language: SourceLanguage::Rust,
        }
    }

    fn fact(name: &str, params: usize, body: Stmt) -> FnFact {
        FnFact {
            name: name.to_string(),
            params: (0..params)
                .map(|i| (format!("p{i}"), IntWidth::I32))
                .collect(),
            return_width: IntWidth::I32,
            body,
            provenance: prov(),
        }
    }

    fn mark_fact() -> FnFact {
        // mark(v) = v -- a real Call, standing in for "some effectful
        // computation," with no I/O primitive needed.
        fact("mark", 1, Stmt::Return(Expr::Param(0, prov()), prov()))
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call(FnId(name.to_string()), args, prov())
    }

    fn marked(inner: Expr) -> Expr {
        call("mark", vec![inner])
    }

    fn lit(v: i64) -> Expr {
        Expr::IntLit(v, IntWidth::I32, prov())
    }

    fn effects_of(program: &Program, fn_name: &str) -> Vec<i64> {
        eval_function(program, fn_name, &[])
            .unwrap()
            .effects
            .into_iter()
            .filter(|e| e.fn_name == "mark")
            .map(|e| e.args[0])
            .collect()
    }

    /// `double(x) = x +% x` (occurs twice) -- inlining `double(mark(5))`
    /// verbatim would duplicate `mark(5)`'s effect.
    #[test]
    fn duplication_hazard_checked_inline_rejects_anf_insert_accepts() {
        let mut program = Program::default();
        program.insert(mark_fact());
        program.insert(fact(
            "double",
            1,
            Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::Param(0, prov())),
                    Box::new(Expr::Param(0, prov())),
                    prov(),
                ),
                prov(),
            ),
        ));
        program.insert(fact(
            "caller",
            0,
            Stmt::Return(call("double", vec![marked(lit(5))]), prov()),
        ));

        let baseline = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(baseline.value, 10);
        assert_eq!(effects_of(&program, "caller"), vec![5]);

        match checked_inline(&program, "caller", "double") {
            Err(TransformError::WouldDuplicateEffect { param_index: 0, .. }) => {}
            other => panic!("expected WouldDuplicateEffect, got {other:?}"),
        }

        let transformed = anf_insert(&program, "caller", "double").unwrap();
        let after = eval_function(&transformed, "caller", &[]).unwrap();
        assert_eq!(
            after.value, baseline.value,
            "return value must be preserved"
        );
        assert_eq!(
            effects_of(&transformed, "caller"),
            vec![5],
            "mark(5) must be evaluated exactly once, not duplicated"
        );
    }

    /// `pick(x, y) = x` (never references `y`) -- inlining `pick(a,
    /// mark(a))` verbatim would drop `mark(a)`'s effect entirely.
    #[test]
    fn dropped_argument_hazard_checked_inline_rejects_anf_insert_accepts() {
        let mut program = Program::default();
        program.insert(mark_fact());
        program.insert(fact(
            "pick",
            2,
            Stmt::Return(Expr::Param(0, prov()), prov()),
        ));
        program.insert(fact(
            "caller",
            0,
            Stmt::Return(call("pick", vec![lit(7), marked(lit(9))]), prov()),
        ));

        let baseline = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(baseline.value, 7);
        assert_eq!(
            effects_of(&program, "caller"),
            vec![9],
            "a real call always evaluates all its arguments, used or not"
        );

        match checked_inline(&program, "caller", "pick") {
            Err(TransformError::WouldDropEffect { param_index: 1, .. }) => {}
            other => panic!("expected WouldDropEffect, got {other:?}"),
        }

        let transformed = anf_insert(&program, "caller", "pick").unwrap();
        let after = eval_function(&transformed, "caller", &[]).unwrap();
        assert_eq!(after.value, baseline.value);
        assert_eq!(
            effects_of(&transformed, "caller"),
            vec![9],
            "ANF must still evaluate the unused argument exactly once, matching real call \
             semantics -- not silently drop it the way verbatim substitution would"
        );
    }

    /// `reverse(x, y) = y +% x` (body references param 1 before param 0)
    /// -- inlining `reverse(mark(1), mark(2))` verbatim would evaluate
    /// `mark(2)` before `mark(1)`, reversing the caller's own left-to-right
    /// argument order.
    #[test]
    fn reordering_hazard_checked_inline_rejects_anf_insert_accepts() {
        let mut program = Program::default();
        program.insert(mark_fact());
        program.insert(fact(
            "reverse",
            2,
            Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::Param(1, prov())),
                    Box::new(Expr::Param(0, prov())),
                    prov(),
                ),
                prov(),
            ),
        ));
        program.insert(fact(
            "caller",
            0,
            Stmt::Return(
                call("reverse", vec![marked(lit(1)), marked(lit(2))]),
                prov(),
            ),
        ));

        let baseline = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(baseline.value, 3);
        assert_eq!(effects_of(&program, "caller"), vec![1, 2]);

        match checked_inline(&program, "caller", "reverse") {
            Err(TransformError::WouldReorderEffects {
                earlier_arg: 0,
                later_arg: 1,
                ..
            }) => {}
            other => panic!("expected WouldReorderEffects, got {other:?}"),
        }

        let transformed = anf_insert(&program, "caller", "reverse").unwrap();
        let after = eval_function(&transformed, "caller", &[]).unwrap();
        assert_eq!(after.value, baseline.value);
        assert_eq!(
            effects_of(&transformed, "caller"),
            vec![1, 2],
            "ANF must preserve the caller's original left-to-right effect order regardless of \
             the callee body's own reference order"
        );
    }

    /// `callee_with_own_effect(x) = mark(2) +% x` -- the callee makes a
    /// call *of its own*, directly, independent of any parameter. Calling
    /// it as `caller() = callee_with_own_effect(mark(1))` must run
    /// `mark(1)` (the argument) before `mark(2)` (the callee's own
    /// internal call), matching real call semantics (every argument
    /// evaluates before the callee body starts running at all). Verbatim
    /// substitution instead produces `mark(2) +% mark(1)`, which the
    /// interpreter evaluates left-to-right as `[2, 1]` -- reordered.
    /// Issue #27's own worked example for this exact hazard: checking only
    /// argument-to-argument order (as the pre-existing
    /// `WouldReorderEffects` check does) misses it entirely, since there is
    /// only one effectful argument here, never compared against anything.
    /// A generative fuzz battery
    /// (`composition_fuzz::ir_level_composition_fuzz_preserves_value_and_observed_effects`)
    /// independently rediscovered this same class of bug at a randomly
    /// generated seed; confirmed to actually matter by reverting
    /// `WouldReorderRelativeToCalleeCall`'s own check and re-running: both
    /// this test and that fuzz seed failed with the exact predicted wrong
    /// order, not a hypothetical concern.
    #[test]
    fn reordering_relative_to_callee_internal_call_hazard_checked_inline_rejects_anf_insert_accepts(
    ) {
        let mut program = Program::default();
        program.insert(mark_fact());
        program.insert(fact(
            "callee_with_own_effect",
            1,
            Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(call("mark", vec![lit(2)])),
                    Box::new(Expr::Param(0, prov())),
                    prov(),
                ),
                prov(),
            ),
        ));
        program.insert(fact(
            "caller",
            0,
            Stmt::Return(call("callee_with_own_effect", vec![marked(lit(1))]), prov()),
        ));

        let baseline = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(baseline.value, 3);
        assert_eq!(
            effects_of(&program, "caller"),
            vec![1, 2],
            "the argument's own mark(1) must run before the callee's internal mark(2)"
        );

        match checked_inline(&program, "caller", "callee_with_own_effect") {
            Err(TransformError::WouldReorderRelativeToCalleeCall { param_index: 0, .. }) => {}
            other => panic!("expected WouldReorderRelativeToCalleeCall, got {other:?}"),
        }

        let transformed = anf_insert(&program, "caller", "callee_with_own_effect").unwrap();
        let after = eval_function(&transformed, "caller", &[]).unwrap();
        assert_eq!(after.value, baseline.value);
        assert_eq!(
            effects_of(&transformed, "caller"),
            vec![1, 2],
            "ANF must still hoist the argument ahead of the callee's own internal call"
        );
    }

    /// `combine(x, y) = x +% y` (same order, no duplication/dropping) --
    /// the positive control: both candidates must accept this.
    #[test]
    fn same_order_no_hazard_both_candidates_accept() {
        let mut program = Program::default();
        program.insert(mark_fact());
        program.insert(fact(
            "combine",
            2,
            Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::Param(0, prov())),
                    Box::new(Expr::Param(1, prov())),
                    prov(),
                ),
                prov(),
            ),
        ));
        program.insert(fact(
            "caller",
            0,
            Stmt::Return(
                call("combine", vec![marked(lit(1)), marked(lit(2))]),
                prov(),
            ),
        ));
        let baseline = eval_function(&program, "caller", &[]).unwrap();

        let checked = checked_inline(&program, "caller", "combine").unwrap();
        let checked_after = eval_function(&checked, "caller", &[]).unwrap();
        assert_eq!(checked_after.value, baseline.value);
        assert_eq!(effects_of(&checked, "caller"), vec![1, 2]);

        let anf = anf_insert(&program, "caller", "combine").unwrap();
        let anf_after = eval_function(&anf, "caller", &[]).unwrap();
        assert_eq!(anf_after.value, baseline.value);
        assert_eq!(effects_of(&anf, "caller"), vec![1, 2]);
    }

    /// Regression test for the exact bug a review reproduced:
    /// `anf_insert`'s fresh `LocalId` counter used to start at 0
    /// unconditionally, colliding with pre-existing locals already bound
    /// in the caller's own body. With `caller() { let a = 1; let b = 2;
    /// combine(b, a) }` (`a`=`LocalId(0)`, `b`=`LocalId(1)` from the real
    /// frontend's own numbering) and `combine`'s two fresh locals for its
    /// call also starting at 0/1, the second hoisted argument's own value
    /// expression (a reference to the outer `a`) read back the
    /// already-rebound slot from the first hoisted argument instead of
    /// `a`'s real value -- silently computing the wrong result. Built
    /// through the real `nim_frontend` (not hand-constructed IR), so the
    /// `LocalId`s are exactly what a real caller produces.
    #[test]
    fn anf_insert_does_not_capture_a_pre_existing_local_with_a_colliding_fresh_id() {
        let source = "proc combine(x, y: int32): int32 =\n  x +% y\n\nproc caller(): int32 =\n  let a = 1'i32\n  let b = 2'i32\n  combine(b, a)\n";
        let program = crate::nim_frontend::lower_nim_source(
            &PathBuf::from("test.nim"),
            source,
            &["combine", "caller"],
        )
        .unwrap();

        let baseline = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(baseline.value, 3, "combine(b, a) = b +% a = 2 +% 1 = 3");

        let transformed = anf_insert(&program, "caller", "combine").unwrap();
        let after = eval_function(&transformed, "caller", &[]).unwrap();
        assert_eq!(
            after.value, baseline.value,
            "a colliding fresh LocalId must not change the result"
        );
    }

    /// Regression test for repeated/iterated transformation: inlining a
    /// *second* call on the result of an earlier `anf_insert` call must
    /// not collide with the `LocalId`s the *first* call already
    /// introduced (not just the original source-level locals) -- proving
    /// `max_local_id_in_stmt` is re-scanned from the current body on every
    /// call, not computed once and reused.
    #[test]
    fn anf_insert_applied_twice_in_sequence_does_not_collide_with_its_own_earlier_output() {
        let source = "proc double(x: int32): int32 =\n  x +% x\n\nproc pick(x, y: int32): int32 =\n  x\n\nproc caller(): int32 =\n  let a = 3'i32\n  let b = 5'i32\n  double(a) +% pick(b, a)\n";
        let program = crate::nim_frontend::lower_nim_source(
            &PathBuf::from("test.nim"),
            source,
            &["double", "pick", "caller"],
        )
        .unwrap();

        let baseline = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(baseline.value, 11, "double(3) + pick(5, 3) = 6 + 5 = 11");

        let after_first = anf_insert(&program, "caller", "double").unwrap();
        assert_eq!(
            eval_function(&after_first, "caller", &[]).unwrap().value,
            11
        );

        let after_second = anf_insert(&after_first, "caller", "pick").unwrap();
        assert_eq!(
            eval_function(&after_second, "caller", &[]).unwrap().value,
            11,
            "a second, sequential anf_insert call must not collide with the first call's own \
             introduced locals"
        );
    }

    /// Regression test for the exact composition bug a review reproduced:
    /// `add(x,y)=x+y`, `g(x)=add(1,x)`, `f()=g(10)`. Inlining `add` into
    /// `g` first gives `g`'s body two embedded `Let`s (ids 0 and 1, its
    /// own fresh numbering). Inlining `g` into `f` *without* alpha-
    /// renaming those embedded ids collides with the fresh id this second
    /// call introduces for hoisting `f`'s own argument (both started
    /// counting from 0, since `f` itself has no pre-existing locals) --
    /// computing `2` instead of the correct `11`. `checked_inline` gets
    /// the mirror case: it introduces no fresh ids, so the collision
    /// instead happens between `g`'s embedded ids and nothing in this
    /// particular repro, but the same rename is exercised at the second
    /// inlining step regardless of which candidate performs it.
    #[test]
    fn composing_an_already_transformed_callee_does_not_collide_with_its_embedded_locals() {
        let source = "proc add(x, y: int32): int32 =\n  x +% y\n\nproc g(x: int32): int32 =\n  add(1'i32, x)\n\nproc f(): int32 =\n  g(10'i32)\n";
        let program = crate::nim_frontend::lower_nim_source(
            &PathBuf::from("test.nim"),
            source,
            &["add", "g", "f"],
        )
        .unwrap();

        let baseline = eval_function(&program, "f", &[]).unwrap();
        assert_eq!(baseline.value, 11, "f() = g(10) = add(1, 10) = 11");

        // anf_insert into g, then anf_insert the now-transformed g into f.
        let after_g = anf_insert(&program, "g", "add").unwrap();
        let g_body_has_embedded_lets = matches!(&after_g.functions["g"].body, crate::types::Stmt::Return(e, _) if matches!(e, Expr::Let { .. }));
        assert!(
            g_body_has_embedded_lets,
            "expected g's body to now contain an embedded Let"
        );

        let after_f = anf_insert(&after_g, "f", "g").unwrap();
        assert_eq!(
            eval_function(&after_f, "f", &[]).unwrap().value,
            11,
            "composing an already-ANF-transformed callee must not silently corrupt the result"
        );

        // Mirror through checked_inline for the second step: g's body is
        // still a simple Return(expr) shape (now containing embedded
        // Lets), so checked_inline can still attempt it.
        let after_f_checked = checked_inline(&after_g, "f", "g").unwrap();
        assert_eq!(
            eval_function(&after_f_checked, "f", &[]).unwrap().value,
            11,
            "checked_inline composing an already-ANF-transformed callee must also not corrupt \
             the result"
        );
    }
}

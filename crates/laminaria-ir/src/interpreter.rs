//! A direct recursive reference evaluator, generalizing
//! `fixtures/laminaria-semantic-substrate-prototype/substrate/src/eval.rs`.
//! Beyond computing a function's return value, this also produces an
//! **effect trace**: the real, ordered sequence of function calls actually
//! executed. A call is this subset's only source of an effect an outside
//! observer could see (arithmetic/conditionals/bindings are pure by
//! construction), so recording the real executed call sequence -- which
//! function, with which evaluated arguments, in which order -- *is* the
//! effect model, replacing the old bare `has_side_effects: bool` fact with
//! something a transformation's before/after states can actually be
//! compared against (`transform`'s own tests do exactly this).

use std::collections::BTreeMap;

use crate::types::{Expr, FnId, LocalId, Program, Stmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEvent {
    pub fn_name: String,
    pub args: Vec<i64>,
    pub order_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalOutcome {
    pub value: i64,
    pub effects: Vec<CallEvent>,
}

/// The shared, explicit instrumentation contract this crate's regression
/// tests and generative fuzz batteries (`transform::composition_fuzz`)
/// compare a transform's before/after behavior against -- not an ad-hoc
/// per-test filter invented independently at each call site (issue #27
/// A3's own distinction: "このfixtureで用いるmark等の観測対象集合は
/// テストごとの場当たりなfilterではなく、共通の明示されたinstrumentation
/// 契約とする"). An "observed event" is a call to the function named
/// `observed_fn_name` (by this crate's own convention, `"mark"` -- a real
/// `Call`, so both the interpreter's own effect trace and
/// `types::expr_contains_call` see it, without this IR needing a
/// dedicated I/O primitive), compared by its own evaluated single
/// argument value, in occurrence order.
///
/// `order_index` is deliberately excluded from this comparison: it exists
/// so a callee's own effects splice correctly into a caller's trace at
/// evaluation time (see `eval_expr`'s own `Call` arm's doc comment), not
/// as part of what "the same observed behavior" means for a
/// before/after-transform comparison -- comparing the returned `Vec`'s
/// own order already captures relative sequencing, which is what
/// actually matters here.
///
/// This is *not* a claim to model real Rust/Nim I/O, exceptions, or a
/// general ownership/side-effect system -- A3's own explicit boundary:
/// "真のI/O/例外/所有権effect体系の完成はこのゲート外."
pub fn observed_calls(effects: &[CallEvent], observed_fn_name: &str) -> Vec<i64> {
    effects
        .iter()
        .filter(|e| e.fn_name == observed_fn_name)
        .map(|e| e.args[0])
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    UnknownFunction(String),
    ArityMismatch {
        function: String,
        expected: usize,
        got: usize,
    },
    UnboundLocal(LocalId),
}

struct EvalState<'a> {
    program: &'a Program,
    params: &'a [i64],
    locals: BTreeMap<LocalId, i64>,
    effects: Vec<CallEvent>,
}

/// Evaluates `function_name(args)` in `program`, wrapping wrapping-arithmetic
/// in real 32-bit two's-complement semantics (`i32::wrapping_*`) --
/// matching both `rustc`'s `wrapping_add`/etc. and Nim's `+%`/`-%`/`*%`
/// exactly, per `fixtures/llvm-rediscovery-semantic-workload`'s own
/// established cross-language equivalence.
pub fn eval_function(
    program: &Program,
    function_name: &str,
    args: &[i64],
) -> Result<EvalOutcome, EvalError> {
    let fact = program
        .functions
        .get(function_name)
        .ok_or_else(|| EvalError::UnknownFunction(function_name.to_string()))?;
    if fact.params.len() != args.len() {
        return Err(EvalError::ArityMismatch {
            function: function_name.to_string(),
            expected: fact.params.len(),
            got: args.len(),
        });
    }

    let mut state = EvalState {
        program,
        params: args,
        locals: BTreeMap::new(),
        effects: Vec::new(),
    };
    let value = eval_stmt(&fact.body, &mut state)?;
    Ok(EvalOutcome {
        value,
        effects: state.effects,
    })
}

fn eval_stmt(stmt: &Stmt, state: &mut EvalState) -> Result<i64, EvalError> {
    match stmt {
        Stmt::Let {
            local, value, body, ..
        } => {
            let v = eval_expr(value, state)?;
            let previous = state.locals.insert(*local, v);
            let result = eval_stmt(body, state);
            match previous {
                Some(p) => {
                    state.locals.insert(*local, p);
                }
                None => {
                    state.locals.remove(local);
                }
            }
            result
        }
        Stmt::If {
            cond, then, els, ..
        } => {
            if eval_expr(cond, state)? != 0 {
                eval_stmt(then, state)
            } else {
                eval_stmt(els, state)
            }
        }
        Stmt::Return(expr, _) => eval_expr(expr, state),
    }
}

fn eval_expr(expr: &Expr, state: &mut EvalState) -> Result<i64, EvalError> {
    match expr {
        Expr::IntLit(v, _, _) => Ok(*v),
        Expr::Param(n, _) => Ok(state.params[*n]),
        Expr::Local(id, _) => state
            .locals
            .get(id)
            .copied()
            .ok_or(EvalError::UnboundLocal(*id)),
        Expr::WrappingAdd(a, b, _) => {
            let a = eval_expr(a, state)? as i32;
            let b = eval_expr(b, state)? as i32;
            Ok(a.wrapping_add(b) as i64)
        }
        Expr::WrappingSub(a, b, _) => {
            let a = eval_expr(a, state)? as i32;
            let b = eval_expr(b, state)? as i32;
            Ok(a.wrapping_sub(b) as i64)
        }
        Expr::WrappingMul(a, b, _) => {
            let a = eval_expr(a, state)? as i32;
            let b = eval_expr(b, state)? as i32;
            Ok(a.wrapping_mul(b) as i64)
        }
        Expr::NotEqZero(inner, _) => Ok((eval_expr(inner, state)? != 0) as i64),
        Expr::Call(FnId(name), args, _) => {
            let mut evaluated = Vec::with_capacity(args.len());
            for a in args {
                evaluated.push(eval_expr(a, state)?);
            }
            let order_index = state.effects.len();
            state.effects.push(CallEvent {
                fn_name: name.clone(),
                args: evaluated.clone(),
                order_index,
            });
            let outcome = eval_function(state.program, name, &evaluated)?;
            // `outcome.effects` was built by `eval_function`'s own fresh
            // `EvalState`, whose `order_index` values count from 0 within
            // *that* call alone -- not globally across the whole
            // evaluation. A review caught the real consequence directly:
            // splicing them in as-is produces duplicate/out-of-order
            // indices (e.g. `[0, 0, 2, 0]`) whenever the callee's own body
            // makes more than one call and the caller already has earlier
            // effects recorded. Renumbered here to continue from this
            // call's own position in the *caller's* trace, so the final
            // `order_index` sequence is globally monotonic regardless of
            // call nesting depth.
            let base = state.effects.len();
            state
                .effects
                .extend(outcome.effects.into_iter().enumerate().map(|(i, mut e)| {
                    e.order_index = base + i;
                    e
                }));
            Ok(outcome.value)
        }
        Expr::Let {
            local, value, body, ..
        } => {
            let v = eval_expr(value, state)?;
            let previous = state.locals.insert(*local, v);
            let result = eval_expr(body, state);
            match previous {
                Some(p) => {
                    state.locals.insert(*local, p);
                }
                None => {
                    state.locals.remove(local);
                }
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FnFact, Program, Provenance, SourceLanguage, SourcePosition, SourceSpan};
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

    fn simple_program() -> Program {
        let mut program = Program::default();
        // double(x) = x +% x
        program.insert(FnFact {
            name: "double".to_string(),
            params: vec![("x".to_string(), crate::types::IntWidth::I32)],
            return_width: crate::types::IntWidth::I32,
            provenance: prov(),
            body: Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::Param(0, prov())),
                    Box::new(Expr::Param(0, prov())),
                    prov(),
                ),
                prov(),
            ),
        });
        program
    }

    #[test]
    fn a_call_produces_exactly_one_effect_event_with_its_evaluated_arguments() {
        let mut program = simple_program();
        // caller(a) = double(a)
        program.insert(FnFact {
            name: "caller".to_string(),
            params: vec![("a".to_string(), crate::types::IntWidth::I32)],
            return_width: crate::types::IntWidth::I32,
            provenance: prov(),
            body: Stmt::Return(
                Expr::Call(
                    FnId("double".to_string()),
                    vec![Expr::Param(0, prov())],
                    prov(),
                ),
                prov(),
            ),
        });
        let outcome = eval_function(&program, "caller", &[21]).unwrap();
        assert_eq!(outcome.value, 42);
        assert_eq!(
            outcome.effects,
            vec![CallEvent {
                fn_name: "double".to_string(),
                args: vec![21],
                order_index: 0,
            }]
        );
    }

    #[test]
    fn nested_effects_are_recorded_in_real_execution_order() {
        let mut program = simple_program();
        // caller() = double(double(1))  -- the inner double() must be
        // recorded before the outer one, since it's evaluated first as
        // the outer call's own argument.
        program.insert(FnFact {
            name: "caller".to_string(),
            params: vec![],
            return_width: crate::types::IntWidth::I32,
            provenance: prov(),
            body: Stmt::Return(
                Expr::Call(
                    FnId("double".to_string()),
                    vec![Expr::Call(
                        FnId("double".to_string()),
                        vec![Expr::IntLit(1, crate::types::IntWidth::I32, prov())],
                        prov(),
                    )],
                    prov(),
                ),
                prov(),
            ),
        });
        let outcome = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(outcome.value, 4); // double(double(1)) = double(2) = 4
        assert_eq!(outcome.effects.len(), 2);
        assert_eq!(outcome.effects[0].args, vec![1]); // inner double(1) first
        assert_eq!(outcome.effects[1].args, vec![2]); // outer double(2) second
        assert_eq!(outcome.effects[0].order_index, 0);
        assert_eq!(outcome.effects[1].order_index, 1);
    }

    #[test]
    fn a_let_bound_value_is_evaluated_exactly_once_even_when_referenced_twice() {
        let mut program = simple_program();
        // caller() = let x = double(3); x +% x
        // If `double(3)` were re-evaluated per reference to `x`, this
        // would record two effect events instead of one.
        program.insert(FnFact {
            name: "caller".to_string(),
            params: vec![],
            return_width: crate::types::IntWidth::I32,
            provenance: prov(),
            body: Stmt::Let {
                local: LocalId(0),
                value: Expr::Call(
                    FnId("double".to_string()),
                    vec![Expr::IntLit(3, crate::types::IntWidth::I32, prov())],
                    prov(),
                ),
                body: Box::new(Stmt::Return(
                    Expr::WrappingAdd(
                        Box::new(Expr::Local(LocalId(0), prov())),
                        Box::new(Expr::Local(LocalId(0), prov())),
                        prov(),
                    ),
                    prov(),
                )),
                provenance: prov(),
            },
        });
        let outcome = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(outcome.value, 12); // double(3)=6, 6+6=12
        assert_eq!(
            outcome.effects.len(),
            1,
            "expected exactly one effect event, got {:?}",
            outcome.effects
        );
    }

    /// Regression test for the exact bug a review reproduced: a callee's
    /// own internal effects are recorded via a *fresh* `EvalState`
    /// (`eval_function` creates one per call), whose `order_index`
    /// numbering starts at 0 for that call alone. Splicing those events
    /// into the caller's own trace without renumbering produced
    /// duplicate/out-of-order indices whenever the caller already had
    /// earlier effects recorded and the callee's own body made more than
    /// one call. `caller() = mark(0) +% inner(5)`, where `inner(x) =
    /// mark(x) +% mark(x +% 1)` makes two calls of its own inside its
    /// body (reached through `eval_function`'s fresh state, not as
    /// `inner`'s own arguments) -- exactly the shape the simpler
    /// `nested_effects_are_recorded_in_real_execution_order` test above
    /// does not exercise (there, the nested call is an *argument*,
    /// evaluated through the *same* shared state, never through a fresh
    /// one).
    #[test]
    fn effect_order_index_is_globally_monotonic_across_nested_function_bodies() {
        let mut program = Program::default();
        program.insert(mark_fact());
        // inner(x) = mark(x) +% mark(x +% 1)
        program.insert(FnFact {
            name: "inner".to_string(),
            params: vec![("x".to_string(), crate::types::IntWidth::I32)],
            return_width: crate::types::IntWidth::I32,
            provenance: prov(),
            body: Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::Call(
                        FnId("mark".to_string()),
                        vec![Expr::Param(0, prov())],
                        prov(),
                    )),
                    Box::new(Expr::Call(
                        FnId("mark".to_string()),
                        vec![Expr::WrappingAdd(
                            Box::new(Expr::Param(0, prov())),
                            Box::new(Expr::IntLit(1, crate::types::IntWidth::I32, prov())),
                            prov(),
                        )],
                        prov(),
                    )),
                    prov(),
                ),
                prov(),
            ),
        });
        // caller() = mark(0) +% inner(5)
        program.insert(FnFact {
            name: "caller".to_string(),
            params: vec![],
            return_width: crate::types::IntWidth::I32,
            provenance: prov(),
            body: Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::Call(
                        FnId("mark".to_string()),
                        vec![Expr::IntLit(0, crate::types::IntWidth::I32, prov())],
                        prov(),
                    )),
                    Box::new(Expr::Call(
                        FnId("inner".to_string()),
                        vec![Expr::IntLit(5, crate::types::IntWidth::I32, prov())],
                        prov(),
                    )),
                    prov(),
                ),
                prov(),
            ),
        });

        fn mark_fact() -> FnFact {
            FnFact {
                name: "mark".to_string(),
                params: vec![("v".to_string(), crate::types::IntWidth::I32)],
                return_width: crate::types::IntWidth::I32,
                provenance: prov(),
                body: Stmt::Return(Expr::Param(0, prov()), prov()),
            }
        }

        let outcome = eval_function(&program, "caller", &[]).unwrap();
        let indices: Vec<usize> = outcome.effects.iter().map(|e| e.order_index).collect();
        let expected: Vec<usize> = (0..outcome.effects.len()).collect();
        assert_eq!(
            indices, expected,
            "order_index must be a globally monotonic 0..N sequence, got {indices:?} for \
             effects {:?}",
            outcome.effects
        );
    }

    /// The shared observation contract itself: filters by function name,
    /// ignores `order_index` (a different, non-"mark" call sitting
    /// between two `mark` calls must not appear, and must not shift which
    /// argument value is reported for either `mark`).
    #[test]
    fn observed_calls_filters_by_name_and_ignores_order_index() {
        let effects = vec![
            CallEvent {
                fn_name: "mark".to_string(),
                args: vec![1],
                order_index: 0,
            },
            CallEvent {
                fn_name: "helper".to_string(),
                args: vec![99],
                order_index: 1,
            },
            CallEvent {
                fn_name: "mark".to_string(),
                args: vec![2],
                order_index: 2,
            },
        ];
        assert_eq!(observed_calls(&effects, "mark"), vec![1, 2]);
    }
}

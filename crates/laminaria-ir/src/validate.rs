//! Issue #27's A2: the API boundary between an unvalidated `Program`
//! candidate (whatever a frontend or transform produced) and one whose
//! bindings, calls, argument counts, and literal ranges have actually
//! been checked. `ValidatedProgram` has no public constructor other than
//! [`validate_program`] -- there is no way to obtain one without actually
//! running the checks, matching A2's own text directly: "検証前の
//! Programと検証済み入力をAPI上区別する... 未検証IRをexecutorが黙って実行
//! してはならない."
//!
//! Called from both points A2 names: `rust_frontend::lower_rust_source`/
//! `nim_frontend::lower_nim_source` run this as a genuine postcondition
//! before ever returning `Ok` ("lowering後"), and
//! `transform::{anf_insert, checked_inline}` run it on their own output
//! before returning `Ok` ("変換後") -- a live invariant check on the
//! transforms themselves, not merely a type a caller could choose to run
//! or skip.
//!
//! What this does *not* do: it does not introduce a new IR representation
//! or SSA form (A2's own text: "新IR体系やSSAの導入は条件にしない"), and it
//! does not change [`crate::interpreter::eval_function`]'s own signature
//! -- that function is this crate's *test* harness for semantic-
//! preservation, used across hundreds of existing call sites for a
//! different concern (does a transform preserve behavior?) than the one
//! this module closes (must an executor be able to silently run
//! unvalidated IR? -- no such executor exists in this crate yet; issue
//! #27's stage C is where this type will actually gate execution).

use std::collections::BTreeSet;

use crate::types::{Expr, FnFact, FnId, IntWidth, LocalId, Program, Stmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramValidationError {
    /// An `Expr::Local` reference to an id not bound at that position --
    /// either never introduced, or referenced outside the `Let` that
    /// introduced it (after its scope closed). Source-level shadowing is
    /// unaffected: each `let` gets its own distinct `LocalId`, so a
    /// shadowing binding is never confused with the one it shadows.
    UnboundLocal { function: String, local: LocalId },
    /// An `Expr::Param` index beyond the function's own declared
    /// parameter count.
    ParamOutOfRange {
        function: String,
        index: usize,
        param_count: usize,
    },
    /// A `Call` names a function this `Program` does not declare.
    UnknownCallee { function: String, callee: String },
    /// A `Call`'s argument count does not match the callee's own declared
    /// parameter count.
    ArityMismatch {
        function: String,
        callee: String,
        expected: usize,
        got: usize,
    },
    /// An `IntLit` value does not fit its own declared `IntWidth`.
    ValueOutOfRange {
        function: String,
        value: i64,
        width: IntWidth,
    },
    /// A `NotEqZero` (this subset's only condition form) appears
    /// somewhere other than directly as an `Stmt::If`'s own `cond` --
    /// e.g. nested inside an arithmetic expression or a `Return`. This
    /// subset has no general boolean value, so a condition used as a
    /// value is exactly the "condition/value" distinction A2 names.
    ConditionUsedAsValue { function: String },
    /// The reverse direction of the same distinction, caught by a review:
    /// an `Stmt::If`'s own `cond` is *not* a `NotEqZero` at all (a bare
    /// value used directly as a condition, e.g. `if x { .. }` with no
    /// comparison). The interpreter's own `eval_stmt` would still treat
    /// any value as an implicit `!= 0` test, so this was previously
    /// accepted silently -- but this subset's own declared grammar has
    /// exactly one condition form, and a real `if` position must actually
    /// contain it, not merely be permitted to.
    ValueUsedAsCondition { function: String },
}

impl std::fmt::Display for ProgramValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgramValidationError::UnboundLocal { function, local } => write!(
                f,
                "function {function:?} references {local:?} outside any binding that \
                 introduces it"
            ),
            ProgramValidationError::ParamOutOfRange {
                function,
                index,
                param_count,
            } => write!(
                f,
                "function {function:?} references parameter {index}, but only declares \
                 {param_count} parameter(s)"
            ),
            ProgramValidationError::UnknownCallee { function, callee } => write!(
                f,
                "function {function:?} calls {callee:?}, which this Program does not declare"
            ),
            ProgramValidationError::ArityMismatch {
                function,
                callee,
                expected,
                got,
            } => write!(
                f,
                "function {function:?} calls {callee:?} with {got} argument(s), but {callee:?} \
                 declares {expected}"
            ),
            ProgramValidationError::ValueOutOfRange {
                function,
                value,
                width,
            } => write!(
                f,
                "function {function:?} has a literal {value} that does not fit {width:?}"
            ),
            ProgramValidationError::ConditionUsedAsValue { function } => write!(
                f,
                "function {function:?} uses a condition (NotEqZero) somewhere other than an \
                 if's own condition position -- this subset has no general boolean value"
            ),
            ProgramValidationError::ValueUsedAsCondition { function } => write!(
                f,
                "function {function:?} uses a bare value directly as an if's own condition, not \
                 a NotEqZero comparison -- this subset's only condition form must actually \
                 appear there"
            ),
        }
    }
}

impl std::error::Error for ProgramValidationError {}

/// A `Program` whose bindings, calls, argument counts, and literal ranges
/// have all been checked by [`validate_program`]. The only way to obtain
/// one is that function -- there is no public constructor, so a caller
/// cannot manufacture a `ValidatedProgram` without the checks actually
/// having run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedProgram(Program);

impl ValidatedProgram {
    pub fn program(&self) -> &Program {
        &self.0
    }

    pub fn into_program(self) -> Program {
        self.0
    }
}

/// Checks every function `program` declares: every `Local` reference
/// resolves to a binding in scope at that position (A2: "すべてのLocal
///参照はその位置で有効な束縛へ解決する"), every `Call` names a declared
/// function with a matching argument count (A2: "関数存在・引数個数...を
/// ...検証する"), every literal fits its declared width (A2: "i32値域...
/// を検証する"), and every condition form appears only where this subset
/// actually has a boolean-shaped position (A2: "条件/値の区別").
pub fn validate_program(program: &Program) -> Result<ValidatedProgram, ProgramValidationError> {
    for (name, fact) in &program.functions {
        validate_fn(name, fact, program)?;
    }
    Ok(ValidatedProgram(program.clone()))
}

fn validate_fn(name: &str, fact: &FnFact, program: &Program) -> Result<(), ProgramValidationError> {
    let mut bound: BTreeSet<LocalId> = BTreeSet::new();
    validate_stmt(name, &fact.body, fact.params.len(), &mut bound, program)
}

/// Binds/unbinds `local` around `f`, mirroring
/// `interpreter::eval_stmt`/`eval_expr`'s own save-then-restore Let
/// scoping exactly -- a `Local` is only ever "in scope" for the dynamic
/// extent this same discipline gives it at evaluation time, so validating
/// against a *different* scoping rule would validate a fact the
/// interpreter doesn't actually rely on.
fn with_bound<T>(
    bound: &mut BTreeSet<LocalId>,
    local: LocalId,
    f: impl FnOnce(&mut BTreeSet<LocalId>) -> Result<T, ProgramValidationError>,
) -> Result<T, ProgramValidationError> {
    let newly_inserted = bound.insert(local);
    let result = f(bound);
    if newly_inserted {
        bound.remove(&local);
    }
    result
}

fn validate_stmt(
    fn_name: &str,
    stmt: &Stmt,
    param_count: usize,
    bound: &mut BTreeSet<LocalId>,
    program: &Program,
) -> Result<(), ProgramValidationError> {
    match stmt {
        Stmt::Let {
            local, value, body, ..
        } => {
            validate_expr(fn_name, value, param_count, bound, program, false)?;
            with_bound(bound, *local, |bound| {
                validate_stmt(fn_name, body, param_count, bound, program)
            })
        }
        Stmt::If {
            cond, then, els, ..
        } => {
            // A review caught this check was one-directional: it rejected
            // a `NotEqZero` used as a value, but never required an `If`'s
            // own `cond` to actually *be* one -- the interpreter's own
            // `eval_stmt` implicitly treats any value as a `!= 0` test, so
            // a bare value directly in condition position (never produced
            // by either real frontend, but constructible at the IR level)
            // previously validated successfully.
            if !matches!(cond, Expr::NotEqZero(..)) {
                return Err(ProgramValidationError::ValueUsedAsCondition {
                    function: fn_name.to_string(),
                });
            }
            validate_expr(fn_name, cond, param_count, bound, program, true)?;
            validate_stmt(fn_name, then, param_count, bound, program)?;
            validate_stmt(fn_name, els, param_count, bound, program)
        }
        Stmt::Return(expr, _) => validate_expr(fn_name, expr, param_count, bound, program, false),
    }
}

/// `is_condition_position` is true only for an `Stmt::If`'s own `cond` --
/// never propagated into any sub-expression, so a `NotEqZero` nested
/// *inside* a condition (`(x != 0) != 0`, say) is still correctly
/// rejected as a condition used as a value.
fn validate_expr(
    fn_name: &str,
    expr: &Expr,
    param_count: usize,
    bound: &mut BTreeSet<LocalId>,
    program: &Program,
    is_condition_position: bool,
) -> Result<(), ProgramValidationError> {
    match expr {
        Expr::IntLit(value, width, _) => {
            let (lo, hi) = match width {
                IntWidth::I32 => (i32::MIN as i64, i32::MAX as i64),
            };
            if *value < lo || *value > hi {
                return Err(ProgramValidationError::ValueOutOfRange {
                    function: fn_name.to_string(),
                    value: *value,
                    width: *width,
                });
            }
            Ok(())
        }
        Expr::Param(index, _) => {
            if *index >= param_count {
                return Err(ProgramValidationError::ParamOutOfRange {
                    function: fn_name.to_string(),
                    index: *index,
                    param_count,
                });
            }
            Ok(())
        }
        Expr::Local(id, _) => {
            if !bound.contains(id) {
                return Err(ProgramValidationError::UnboundLocal {
                    function: fn_name.to_string(),
                    local: *id,
                });
            }
            Ok(())
        }
        Expr::WrappingAdd(a, b, _) | Expr::WrappingSub(a, b, _) | Expr::WrappingMul(a, b, _) => {
            validate_expr(fn_name, a, param_count, bound, program, false)?;
            validate_expr(fn_name, b, param_count, bound, program, false)
        }
        Expr::NotEqZero(inner, _) => {
            if !is_condition_position {
                return Err(ProgramValidationError::ConditionUsedAsValue {
                    function: fn_name.to_string(),
                });
            }
            validate_expr(fn_name, inner, param_count, bound, program, false)
        }
        Expr::Call(FnId(callee_name), args, _) => {
            let callee = program.functions.get(callee_name).ok_or_else(|| {
                ProgramValidationError::UnknownCallee {
                    function: fn_name.to_string(),
                    callee: callee_name.clone(),
                }
            })?;
            if callee.params.len() != args.len() {
                return Err(ProgramValidationError::ArityMismatch {
                    function: fn_name.to_string(),
                    callee: callee_name.clone(),
                    expected: callee.params.len(),
                    got: args.len(),
                });
            }
            for arg in args {
                validate_expr(fn_name, arg, param_count, bound, program, false)?;
            }
            Ok(())
        }
        Expr::Let {
            local, value, body, ..
        } => {
            validate_expr(fn_name, value, param_count, bound, program, false)?;
            with_bound(bound, *local, |bound| {
                validate_expr(fn_name, body, param_count, bound, program, false)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Provenance, SourceLanguage, SourcePosition, SourceSpan};
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

    #[test]
    fn a_well_formed_program_validates() {
        let mut program = Program::default();
        program.insert(fact("id", 1, Stmt::Return(Expr::Param(0, prov()), prov())));
        assert!(validate_program(&program).is_ok());
    }

    #[test]
    fn a_program_produced_by_the_real_rust_frontend_validates() {
        let source = "fn add(x: i32, y: i32) -> i32 { x.wrapping_add(y) }\nfn f(a: i32) -> i32 { let b = add(a, 1); b }\n";
        let program = crate::rust_frontend::lower_rust_source(
            &PathBuf::from("test.rs"),
            source,
            &["add", "f"],
        )
        .unwrap();
        assert!(validate_program(&program).is_ok());
    }

    #[test]
    fn a_program_produced_by_the_real_nim_frontend_validates() {
        let source = "proc add(x, y: int32): int32 =\n  x +% y\n\nproc f(a: int32): int32 =\n  let b = add(a, 1'i32)\n  b\n";
        let program = crate::nim_frontend::lower_nim_source(
            &PathBuf::from("test.nim"),
            source,
            &["add", "f"],
        )
        .unwrap();
        assert!(validate_program(&program).is_ok());
    }

    #[test]
    fn a_program_produced_by_anf_insert_validates() {
        let source = "proc add(x, y: int32): int32 =\n  x +% y\n\nproc f(a: int32): int32 =\n  add(a, 1'i32)\n";
        let program = crate::nim_frontend::lower_nim_source(
            &PathBuf::from("test.nim"),
            source,
            &["add", "f"],
        )
        .unwrap();
        let transformed = crate::transform::anf_insert::anf_insert(&program, "f", "add").unwrap();
        assert!(validate_program(&transformed).is_ok());
    }

    #[test]
    fn a_program_produced_by_checked_inline_validates() {
        let source = "proc combine(x, y: int32): int32 =\n  x +% y\n\nproc f(a: int32): int32 =\n  combine(a, 1'i32)\n";
        let program = crate::nim_frontend::lower_nim_source(
            &PathBuf::from("test.nim"),
            source,
            &["combine", "f"],
        )
        .unwrap();
        let transformed =
            crate::transform::checked_inline::checked_inline(&program, "f", "combine").unwrap();
        assert!(validate_program(&transformed).is_ok());
    }

    #[test]
    fn a_local_referenced_outside_its_binding_scope_is_rejected() {
        let mut program = Program::default();
        // `f() = { let x = 1; 0 } +% x` -- `x` is bound only within the
        // Let's own body, not visible to the sibling operand outside it.
        program.insert(fact(
            "f",
            0,
            Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::Let {
                        local: LocalId(0),
                        value: Box::new(Expr::IntLit(1, IntWidth::I32, prov())),
                        body: Box::new(Expr::IntLit(0, IntWidth::I32, prov())),
                        provenance: prov(),
                    }),
                    Box::new(Expr::Local(LocalId(0), prov())),
                    prov(),
                ),
                prov(),
            ),
        ));
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::UnboundLocal {
                function: "f".to_string(),
                local: LocalId(0),
            })
        );
    }

    #[test]
    fn a_call_to_an_undeclared_function_is_rejected() {
        let mut program = Program::default();
        program.insert(fact(
            "f",
            0,
            Stmt::Return(
                Expr::Call(FnId("does_not_exist".to_string()), vec![], prov()),
                prov(),
            ),
        ));
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::UnknownCallee {
                function: "f".to_string(),
                callee: "does_not_exist".to_string(),
            })
        );
    }

    #[test]
    fn a_call_with_the_wrong_argument_count_is_rejected() {
        let mut program = Program::default();
        program.insert(fact("g", 2, Stmt::Return(Expr::Param(0, prov()), prov())));
        program.insert(fact(
            "f",
            0,
            Stmt::Return(
                Expr::Call(
                    FnId("g".to_string()),
                    vec![Expr::IntLit(1, IntWidth::I32, prov())],
                    prov(),
                ),
                prov(),
            ),
        ));
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::ArityMismatch {
                function: "f".to_string(),
                callee: "g".to_string(),
                expected: 2,
                got: 1,
            })
        );
    }

    #[test]
    fn a_param_index_beyond_the_functions_own_count_is_rejected() {
        let mut program = Program::default();
        program.insert(fact("f", 1, Stmt::Return(Expr::Param(5, prov()), prov())));
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::ParamOutOfRange {
                function: "f".to_string(),
                index: 5,
                param_count: 1,
            })
        );
    }

    #[test]
    fn a_literal_outside_i32_range_is_rejected() {
        let mut program = Program::default();
        program.insert(fact(
            "f",
            0,
            Stmt::Return(
                Expr::IntLit(i32::MAX as i64 + 1, IntWidth::I32, prov()),
                prov(),
            ),
        ));
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::ValueOutOfRange {
                function: "f".to_string(),
                value: i32::MAX as i64 + 1,
                width: IntWidth::I32,
            })
        );
    }

    /// A condition used as a general value -- this subset's own
    /// condition/value distinction, not producible by either real
    /// frontend but constructible directly at the IR level (exactly what
    /// this validator exists to catch for IR built outside a frontend).
    #[test]
    fn a_condition_used_as_a_value_is_rejected() {
        let mut program = Program::default();
        program.insert(fact(
            "f",
            1,
            Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::NotEqZero(Box::new(Expr::Param(0, prov())), prov())),
                    Box::new(Expr::IntLit(1, IntWidth::I32, prov())),
                    prov(),
                ),
                prov(),
            ),
        ));
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::ConditionUsedAsValue {
                function: "f".to_string(),
            })
        );
    }

    /// A `NotEqZero` correctly used as an `if`'s own condition validates.
    #[test]
    fn a_condition_used_in_an_ifs_own_cond_validates() {
        let mut program = Program::default();
        program.insert(fact(
            "f",
            1,
            Stmt::If {
                cond: Expr::NotEqZero(Box::new(Expr::Param(0, prov())), prov()),
                then: Box::new(Stmt::Return(Expr::IntLit(1, IntWidth::I32, prov()), prov())),
                els: Box::new(Stmt::Return(Expr::IntLit(0, IntWidth::I32, prov()), prov())),
                provenance: prov(),
            },
        ));
        assert!(validate_program(&program).is_ok());
    }

    /// A review caught the reverse direction of the condition/value
    /// distinction: a bare value (not a `NotEqZero`) used directly as an
    /// `If`'s own `cond` -- the interpreter's own `eval_stmt` would still
    /// treat it as an implicit `!= 0` test, so this previously validated
    /// successfully despite this subset's declared grammar having exactly
    /// one condition form.
    #[test]
    fn a_bare_value_used_directly_as_an_ifs_condition_is_rejected() {
        let mut program = Program::default();
        program.insert(fact(
            "f",
            1,
            Stmt::If {
                cond: Expr::Param(0, prov()),
                then: Box::new(Stmt::Return(Expr::IntLit(1, IntWidth::I32, prov()), prov())),
                els: Box::new(Stmt::Return(Expr::IntLit(0, IntWidth::I32, prov()), prov())),
                provenance: prov(),
            },
        ));
        assert_eq!(
            validate_program(&program),
            Err(ProgramValidationError::ValueUsedAsCondition {
                function: "f".to_string(),
            })
        );
    }

    /// Source-level shadowing: two distinct `Let`s (distinct `LocalId`s,
    /// as any real frontend assigns) referencing the same surface name
    /// must both validate -- the check is scoped by numeric id, never by
    /// name.
    #[test]
    fn shadowing_via_distinct_local_ids_validates() {
        let source = "proc f(): int32 =\n  let a = 1'i32\n  let a = 2'i32\n  a\n";
        let program =
            crate::nim_frontend::lower_nim_source(&PathBuf::from("test.nim"), source, &["f"])
                .unwrap();
        assert!(validate_program(&program).is_ok());
    }
}

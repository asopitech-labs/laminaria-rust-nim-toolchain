//! The candidate LAMINARIA semantic representation, scoped to exactly the
//! `add_or_double`/`double` workload (see ../../CONTRACT.md). Not a
//! general-purpose IR -- deliberately small enough to hand-verify every
//! transformation this fixture performs on it.
//!
//! Grammar: an `Expr` computes an `i32`; a `Stmt` is a function body,
//! always ending in a `Return` on every path (so an `If`'s branches never
//! need a phi-node merge when projected to LLVM IR -- each branch just
//! returns directly, see `llvm_ir.rs`).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Reference to the enclosing function's `n`-th parameter.
    Param(usize),
    /// Wrapping (two's-complement, no overflow trap) addition -- the same
    /// operation `add.rs`'s `wrapping_add`/Nim's `+%` use, established in
    /// `fixtures/llvm-rediscovery-semantic-workload`.
    WrappingAdd(Box<Expr>, Box<Expr>),
    /// Whether `Expr` is nonzero -- the only condition this grammar's `If`
    /// needs for `use_double != 0`.
    NotEqZero(Box<Expr>),
    /// Call another function (by name, looked up in the same
    /// `SemanticProgram::functions` map) with the given argument
    /// expressions.
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    If {
        cond: Expr,
        then: Box<Stmt>,
        els: Box<Stmt>,
    },
    Return(Expr),
}

/// The semantic facts issue #25 asks a candidate substrate to carry
/// per-function, scoped to what this workload actually needs: bit widths
/// (all `i32` here, but tracked explicitly rather than hard-coded, so a
/// transformation can check them rather than assume them) and whether the
/// function is known to have any effect beyond its return value.
/// `has_side_effects` is the one fact `inline.rs`'s transformation
/// actually gates on -- deliberately not modeled as "what the effect is,"
/// only "whether one exists," since that's the minimum a transformation
/// needs to decide safety, matching how rustc's own `probe-stack` fact
/// (`fixtures/llvm-rediscovery-semantic-workload/NOTES.md`) is a policy
/// flag, not a full effect description either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnFact {
    pub name: String,
    pub param_widths: Vec<u32>,
    pub return_width: u32,
    pub has_side_effects: bool,
    pub body: Stmt,
}

#[derive(Debug, Clone, Default)]
pub struct SemanticProgram {
    pub functions: BTreeMap<String, FnFact>,
}

impl SemanticProgram {
    pub fn insert(&mut self, fact: FnFact) {
        self.functions.insert(fact.name.clone(), fact);
    }
}

/// The real workload's representation: `double(x) = x +% x`,
/// `add_or_double(a, b, use_double) = if use_double != 0 { double(a) }
/// else { a +% b }` -- transcribed by hand from `../rust-src/
/// add_or_double.rs`/`../nim-src/add_or_double.nim`, not derived
/// mechanically from either (this fixture's own honest limitation: the
/// representation is hand-authored to match the source's *documented*
/// semantic contract, not automatically extracted -- see NOTES.md).
pub fn workload_program() -> SemanticProgram {
    let mut program = SemanticProgram::default();
    program.insert(FnFact {
        name: "double".to_string(),
        param_widths: vec![32],
        return_width: 32,
        has_side_effects: false,
        body: Stmt::Return(Expr::WrappingAdd(
            Box::new(Expr::Param(0)),
            Box::new(Expr::Param(0)),
        )),
    });
    program.insert(FnFact {
        name: "add_or_double".to_string(),
        param_widths: vec![32, 32, 32],
        return_width: 32,
        has_side_effects: false,
        body: Stmt::If {
            cond: Expr::NotEqZero(Box::new(Expr::Param(2))),
            then: Box::new(Stmt::Return(Expr::Call(
                "double".to_string(),
                vec![Expr::Param(0)],
            ))),
            els: Box::new(Stmt::Return(Expr::WrappingAdd(
                Box::new(Expr::Param(0)),
                Box::new(Expr::Param(1)),
            ))),
        },
    });
    program
}

/// A hypothetical variant of `double` that has an observable effect the
/// fact set records -- used only by `inline.rs`'s "should be rejected"
/// case. The effect itself is not modeled (this representation has no
/// concept of mutable state) -- only the *fact that one exists* is, which
/// is exactly the information a transformation needs to refuse inlining,
/// per issue #25's ask for concrete allowed-vs-rejected examples.
pub fn double_with_side_effect_fact() -> FnFact {
    FnFact {
        name: "double".to_string(),
        has_side_effects: true,
        ..workload_program().functions["double"].clone()
    }
}

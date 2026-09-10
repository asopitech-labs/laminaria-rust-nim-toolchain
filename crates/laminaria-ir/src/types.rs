//! LAMINARIA's own owned IR for a declared Rust/Nim source subset (issues
//! #25/#3), generalizing `fixtures/laminaria-semantic-substrate-prototype/
//! substrate/src/repr.rs`'s hand-transcribed `Expr`/`Stmt` grammar to close
//! two concrete gaps that fixture's own doc comments name honestly:
//!
//! - Every node here carries real source [`Provenance`] (file + span),
//!   populated by `rust_frontend`/`nim_frontend` from the actual parsed
//!   source, not omitted the way the hand-transcribed fixture omits it
//!   entirely.
//! - [`Expr::Let`] is a new, explicit single-evaluation binding form. The
//!   fixture's `inline.rs` names its absence directly: "a real
//!   single-evaluation order-preserving `let`-like binding form remains
//!   unimplemented." `transform::anf_insert` is built on exactly this node.
//!
//! This is deliberately still a small grammar, scoped to this task's
//! declared subset (fixed-width integers, explicit wrapping arithmetic,
//! function calls, conditionals, local bindings) -- not a general-purpose
//! IR, and not a claim that this shape is the final answer for LAMINARIA's
//! eventual compiler (see `docs/llvm-rediscovery-research.md`: arriving at a
//! different architecture later is equally acceptable).

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Which source language a [`Provenance`] (and therefore an IR node) was
/// derived from -- carried explicitly so a later consumer (e.g. a
/// diagnostic renderer, or issue #3's cross-language comparison work) never
/// has to guess it back out of a file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLanguage {
    Rust,
    Nim,
}

/// A `(line, column)` position, 1-indexed to match how editors and
/// compilers conventionally report positions to humans -- both frontends
/// convert their own native span representation (`proc_macro2::LineColumn`
/// for Rust, the hand-written Nim lexer's own line/column counters) into
/// this one shared type, so every diagnostic/IR node is comparable
/// regardless of source language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourcePosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

/// Where an IR node actually came from: which file, which language, and
/// which exact span in that file's text -- the concrete answer to "does
/// this IR carry source positions," which the hand-transcribed fixture
/// this generalizes never had to answer because it was never derived from
/// a real parse in the first place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub source_file: PathBuf,
    pub span: SourceSpan,
    pub language: SourceLanguage,
}

/// Fixed integer bit width. One variant for this task's declared subset
/// (both `rust-src/add_or_double.rs` and `nim-src/add_or_double.nim` are
/// `i32`/`int32` only) -- not hard-coded into [`Expr`]/[`Stmt`] themselves,
/// so a later task can add `I64`/`U32`/etc. without changing the grammar
/// shape, only this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntWidth {
    I32,
}

/// Identifies one `let`-bound local within a single function body. Scoped
/// per-function (not globally unique), assigned by whichever frontend
/// lowers that function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalId(pub u32);

/// Identifies a callable function by name within a [`Program`]. A thin
/// wrapper over `String` (not an interned/validated handle yet) --
/// resolving a `Call`'s `FnId` against `Program::functions` is the
/// consuming code's job (`interpreter.rs` does this at evaluation time).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FnId(pub String);

/// The owned expression grammar. Every variant carries the [`Provenance`]
/// of the exact source construct it was lowered from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    IntLit(i64, IntWidth, Provenance),
    /// Reference to the enclosing function's `n`-th parameter.
    Param(usize, Provenance),
    /// Reference to a `let`-bound local, by the [`LocalId`] its introducing
    /// [`Expr::Let`] assigned.
    Local(LocalId, Provenance),
    /// Wrapping (two's-complement, no overflow trap) addition -- `rustc`'s
    /// `i32::wrapping_add`/Nim's `+%`.
    WrappingAdd(Box<Expr>, Box<Expr>, Provenance),
    WrappingSub(Box<Expr>, Box<Expr>, Provenance),
    WrappingMul(Box<Expr>, Box<Expr>, Provenance),
    /// Whether `Expr` is nonzero -- this subset's only condition form
    /// (`!= 0`/`== 0` against a literal zero).
    NotEqZero(Box<Expr>, Provenance),
    /// Call another function (by name, looked up in the same
    /// [`Program::functions`] map) with the given argument expressions, in
    /// left-to-right source order.
    Call(FnId, Vec<Expr>, Provenance),
    /// `let LocalId = value; body` -- `value` is evaluated exactly once,
    /// before `body`, and bound to `LocalId` for every [`Expr::Local`]
    /// reference inside `body`. The single-evaluation, in-order binding
    /// primitive `transform::anf_insert` relies on structurally.
    Let {
        local: LocalId,
        value: Box<Expr>,
        body: Box<Expr>,
        provenance: Provenance,
    },
}

impl Expr {
    pub fn provenance(&self) -> &Provenance {
        match self {
            Expr::IntLit(_, _, p)
            | Expr::Param(_, p)
            | Expr::Local(_, p)
            | Expr::WrappingAdd(_, _, p)
            | Expr::WrappingSub(_, _, p)
            | Expr::WrappingMul(_, _, p)
            | Expr::NotEqZero(_, p)
            | Expr::Call(_, _, p)
            | Expr::Let { provenance: p, .. } => p,
        }
    }
}

/// A function body is a `Stmt`. `Let` sequences a source-level `let NAME =
/// EXPR;` statement (as it actually appears in Rust/Nim surface syntax --
/// always a statement there, never an expression position) ahead of the
/// rest of the body; `body` is everything that follows in the same block.
/// This is distinct from [`Expr::Let`], which binds a value at an
/// *expression* position -- `transform::anf_insert` needs that form to
/// hoist a call-containing sub-expression in the middle of an arithmetic
/// tree, a position `Stmt::Let` cannot reach (it can only wrap a whole
/// statement, not a sub-expression). Both forms bind their `LocalId`
/// exactly once, evaluated before anything that can reference it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let {
        local: LocalId,
        value: Expr,
        body: Box<Stmt>,
        provenance: Provenance,
    },
    If {
        cond: Expr,
        then: Box<Stmt>,
        els: Box<Stmt>,
        provenance: Provenance,
    },
    Return(Expr, Provenance),
}

impl Stmt {
    pub fn provenance(&self) -> &Provenance {
        match self {
            Stmt::Let { provenance: p, .. } => p,
            Stmt::If { provenance: p, .. } => p,
            Stmt::Return(_, p) => p,
        }
    }
}

/// One function's semantic facts: its parameters (name kept for
/// diagnostics/debugging, width for the actual semantics), return width,
/// body, and where it was declared. Unlike the fixture's `FnFact`, there is
/// no `has_side_effects: bool` here -- whether a function can have an
/// observable effect is derived structurally (does its body transitively
/// contain a `Call`?), and the *actual* effect model lives in
/// `interpreter::EvalOutcome`'s effect trace, not a declared fact. See this
/// crate's top-level doc comment for why a bare boolean was replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnFact {
    pub name: String,
    pub params: Vec<(String, IntWidth)>,
    pub return_width: IntWidth,
    pub body: Stmt,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Program {
    pub functions: BTreeMap<String, FnFact>,
}

impl Program {
    pub fn insert(&mut self, fact: FnFact) {
        self.functions.insert(fact.name.clone(), fact);
    }
}

/// Whether `expr`'s subtree can contain a [`Expr::Call`] at all -- the
/// structural stand-in for the old `has_side_effects: bool` fact: a
/// function with no `Call` anywhere in its body is definitely pure in this
/// subset (arithmetic/conditionals/bindings have no other effect source),
/// so purity is *derived*, not separately declared and therefore possible
/// to get out of sync with the real body.
pub fn expr_contains_call(expr: &Expr) -> bool {
    match expr {
        Expr::IntLit(..) | Expr::Param(..) | Expr::Local(..) => false,
        Expr::WrappingAdd(a, b, _) | Expr::WrappingSub(a, b, _) | Expr::WrappingMul(a, b, _) => {
            expr_contains_call(a) || expr_contains_call(b)
        }
        Expr::NotEqZero(inner, _) => expr_contains_call(inner),
        Expr::Call(..) => true,
        Expr::Let { value, body, .. } => expr_contains_call(value) || expr_contains_call(body),
    }
}

pub fn stmt_contains_call(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { value, body, .. } => expr_contains_call(value) || stmt_contains_call(body),
        Stmt::If {
            cond, then, els, ..
        } => expr_contains_call(cond) || stmt_contains_call(then) || stmt_contains_call(els),
        Stmt::Return(expr, _) => expr_contains_call(expr),
    }
}

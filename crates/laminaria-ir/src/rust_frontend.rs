//! Lowers a declared subset of real Rust source into [`crate::types::Program`].
//!
//! Uses the `syn` crate for the *syntax* layer only (tokenizing and parsing
//! into a `syn::File`/`syn::Item`/`syn::Expr` tree) -- `syn` performs zero
//! semantic analysis, zero type checking, and zero macro expansion. Every
//! semantic fact (what `+` means, what `i32` means, whether a construct is
//! in the supported subset) is derived by this module's own code walking
//! that tree. This matches `docs/compiler-ownership-contract.md`'s
//! lexical/syntactic-parsing-reuse boundary directly: a parsing library
//! producing a syntax tree is not itself "meaning."
//!
//! The supported subset is exactly: `i32` parameters/locals/return type,
//! integer literals, `.wrapping_add/sub/mul(..)`, `!= 0`/`0 != ..` as an
//! `if` condition, `if { .. } else { .. }` at a block's tail position,
//! `let NAME = EXPR;` prefixing a block, calls to other functions lowered
//! in the same request, and a trailing tail expression or `return EXPR;`.
//! Anything else is a [`LoweringError::UnsupportedConstruct`] naming the
//! real span -- never a panic, never a partial `Program`, never a fallback
//! to invoking `rustc`.

use std::collections::BTreeMap;
use std::path::Path;

use proc_macro2::{LineColumn, Span};
use syn::spanned::Spanned;
use syn::{
    BinOp, Block, Expr as SynExpr, ExprIf, FnArg, Item, ItemFn, Lit, Local, Pat, ReturnType,
    Stmt as SynStmt, Type,
};

use crate::diagnostics::{Diagnostic, LoweringError};
use crate::types::{
    Expr, FnFact, FnId, IntWidth, LocalId, Program, Provenance, SourceLanguage, SourcePosition,
    SourceSpan, Stmt,
};

fn to_source_position(lc: LineColumn) -> SourcePosition {
    // `LineColumn::column` is 0-indexed (per proc_macro2's own docs); this
    // crate reports 1-indexed positions to humans everywhere, matching
    // `line` (already 1-indexed).
    SourcePosition {
        line: lc.line as u32,
        column: lc.column as u32 + 1,
    }
}

fn to_provenance(source_file: &Path, span: Span) -> Provenance {
    Provenance {
        source_file: source_file.to_path_buf(),
        span: SourceSpan {
            start: to_source_position(span.start()),
            end: to_source_position(span.end()),
        },
        language: SourceLanguage::Rust,
    }
}

struct Ctx<'a> {
    source_file: &'a Path,
    declared_functions: &'a std::collections::BTreeSet<String>,
    params: BTreeMap<String, usize>,
    locals: BTreeMap<String, LocalId>,
    next_local: u32,
}

impl<'a> Ctx<'a> {
    fn prov(&self, span: Span) -> Provenance {
        to_provenance(self.source_file, span)
    }

    fn fresh_local(&mut self) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        id
    }
}

fn unsupported(construct: impl Into<String>, span: Span) -> LoweringError {
    LoweringError::UnsupportedConstruct {
        construct: construct.into(),
        span: SourceSpan {
            start: to_source_position(span.start()),
            end: to_source_position(span.end()),
        },
    }
}

fn unsupported_shape(detail: impl Into<String>, span: Span) -> LoweringError {
    LoweringError::UnsupportedShape {
        detail: detail.into(),
        span: SourceSpan {
            start: to_source_position(span.start()),
            end: to_source_position(span.end()),
        },
    }
}

/// Pushes an [`unsupported`] error for every attribute in `attrs` that isn't
/// `#[doc = ...]` (what a `///` comment desugars to, allowed through since
/// it has no semantic effect). A review first added this check only for a
/// function's own attributes (`item_fn.attrs`), but `cfg` and other
/// attributes attach to far more positions than just item declarations --
/// confirmed directly against rustc's own `StripUnconfigured::configure`,
/// which `rustc_expand::expand`'s `flat_map_stmt`/`flat_map_param` invoke on
/// individual statements and function parameters, not only whole items
/// (issue #27's reference table). This subset does not implement `cfg` at
/// all, so an unsupported attribute at *any* position it can appear at must
/// be diagnosed the same way -- silently accepting one anywhere this
/// helper isn't called would let it act on (or be ignored on) the tagged
/// construct without ever being noticed.
fn reject_unsupported_attrs(attrs: &[syn::Attribute], errors: &mut Vec<LoweringError>) {
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            errors.push(unsupported("attribute", attr.span()));
        }
    }
}

/// Lowers exactly the named top-level functions from `source_text` (a whole
/// Rust file) into a [`Program`]. Only these functions -- and any function
/// they call, which must *also* be in `requested_functions` -- are ever
/// lowered; other items in the file (a `main` with a `for` loop and
/// `println!`, a `const`, ...) are never inspected, so a workload file that
/// mixes a supported library subset with an unrelated unsupported driver
/// (exactly `rust-src/add_or_double.rs`'s own shape) does not need every
/// item in the file to be in-subset, only the ones actually requested.
pub fn lower_rust_source(
    source_file: &Path,
    source_text: &str,
    requested_functions: &[&str],
) -> Result<Program, Vec<Diagnostic>> {
    let file = syn::parse_file(source_text).map_err(|e| {
        vec![Diagnostic::from_lowering_error(
            LoweringError::ParseError {
                detail: e.to_string(),
                span: SourceSpan {
                    start: to_source_position(e.span().start()),
                    end: to_source_position(e.span().end()),
                },
            },
            SourceLanguage::Rust,
        )]
    })?;

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // A review caught a real gap: this module only ever inspects the
    // *items* it's asked about, but a file-level inner attribute
    // (`#![cfg(..)]`, `#![feature(..)]`, ...) can change what the file
    // even means as a whole -- rustc's own `StripUnconfigured::configure`
    // (issue #27's reference table) applies `cfg` at the crate-root level
    // too, not only per-item. This subset implements no `cfg` at all, so
    // a file-level attribute must be diagnosed the same way a function's/
    // parameter's/let's/if's own attribute already is, not silently
    // ignored because only `file.items` was ever walked.
    let mut file_attr_errors = Vec::new();
    reject_unsupported_attrs(&file.attrs, &mut file_attr_errors);
    diagnostics.extend(
        file_attr_errors
            .into_iter()
            .map(|e| Diagnostic::from_lowering_error(e, SourceLanguage::Rust)),
    );

    // Grouped by name (not overwritten) specifically so a duplicate
    // declaration of a *requested* function can be diagnosed below,
    // instead of `BTreeMap::insert` silently keeping whichever
    // declaration happens to appear last in the file -- a review caught
    // that this previously picked one definition with no diagnostic at
    // all, even though real Rust rejects a duplicate item definition
    // outright as a hard compile error.
    let mut by_name: BTreeMap<String, Vec<&ItemFn>> = BTreeMap::new();
    for item in &file.items {
        if let Item::Fn(f) = item {
            by_name.entry(f.sig.ident.to_string()).or_default().push(f);
        }
    }
    let declared_functions: std::collections::BTreeSet<String> =
        requested_functions.iter().map(|s| s.to_string()).collect();

    let mut program = Program::default();
    for name in requested_functions {
        let Some(candidates) = by_name.get(*name) else {
            diagnostics.push(Diagnostic::from_lowering_error(
                LoweringError::UnsupportedShape {
                    detail: format!("requested function '{name}' is not declared in this file"),
                    span: SourceSpan {
                        start: SourcePosition { line: 1, column: 1 },
                        end: SourcePosition { line: 1, column: 1 },
                    },
                },
                SourceLanguage::Rust,
            ));
            continue;
        };
        if candidates.len() > 1 {
            diagnostics.push(Diagnostic::from_lowering_error(
                LoweringError::UnsupportedShape {
                    detail: format!(
                        "'{name}' is declared {} times in this file -- real Rust rejects a \
                         duplicate item definition outright, so this lowering does not pick one \
                         arbitrarily either",
                        candidates.len()
                    ),
                    span: SourceSpan {
                        start: to_source_position(candidates[1].span().start()),
                        end: to_source_position(candidates[1].span().end()),
                    },
                },
                SourceLanguage::Rust,
            ));
            continue;
        }
        let item_fn = candidates[0];
        match lower_item_fn(source_file, item_fn, &declared_functions) {
            Ok(fact) => program.insert(fact),
            Err(errors) => diagnostics.extend(
                errors
                    .into_iter()
                    .map(|e| Diagnostic::from_lowering_error(e, SourceLanguage::Rust)),
            ),
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    // A live postcondition (issue #27 A2: "検証はlowering後...で行う"),
    // not merely a comment claiming this lowering's output is
    // well-formed -- see `validate::validate_program`'s own doc comment.
    if let Err(e) = crate::validate::validate_program(&program) {
        return Err(vec![Diagnostic::from_lowering_error(
            LoweringError::PostconditionViolated {
                detail: e.to_string(),
                span: SourceSpan {
                    start: SourcePosition { line: 1, column: 1 },
                    end: SourcePosition { line: 1, column: 1 },
                },
            },
            SourceLanguage::Rust,
        )]);
    }

    Ok(program)
}

fn type_is_i32(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.is_ident("i32"))
}

/// Parses an integer literal into a value guaranteed to fit `i32`,
/// fixing two bugs a review reproduced:
///
/// - **The unary-minus path skipped the suffix check entirely** --
///   `syn::LitInt::base10_parse::<i32>()` only looks at the digits, never
///   the suffix, so `-1u64` (a `u64`-typed literal, a completely
///   different, unsigned 64-bit type) silently parsed as `IntLit(-1,
///   I32)` through the unary-negation arm, while the *non-negated*
///   literal path already correctly rejected a non-`i32` suffix. Both
///   paths now go through this one function.
/// - **`i32::MIN` (`-2147483648`) was rejected outright.** Rust's own
///   surface syntax has no single "negative literal" token -- `-N` is
///   always unary negation of a separately-tokenized *positive* digit
///   sequence, and `2147483648` alone does not fit in `i32`
///   (`i32::MAX` is `2147483647`), so parsing the pre-negation digits
///   directly as `i32` failed before negation could ever bring the value
///   back into range. Fixed by parsing the digits as `i64` first (wide
///   enough for any value this literal could name), negating in `i64`,
///   *then* range-checking the final value against `i32::MIN..=i32::MAX`
///   -- correctly accepting `-2147483648` while still rejecting a
///   genuinely out-of-range value like `-2147483649`.
fn parse_i32_literal(
    lit: &syn::LitInt,
    negate: bool,
    span: Span,
) -> Result<i64, Vec<LoweringError>> {
    if !lit.suffix().is_empty() && lit.suffix() != "i32" {
        return Err(vec![unsupported(
            "integer literal suffix other than i32",
            span,
        )]);
    }
    let digits: i64 = lit
        .base10_parse::<i64>()
        .map_err(|e| vec![unsupported_shape(format!("integer literal: {e}"), span)])?;
    let value = if negate { -digits } else { digits };
    if value < i32::MIN as i64 || value > i32::MAX as i64 {
        return Err(vec![unsupported_shape(
            format!("integer literal {value} does not fit in i32"),
            span,
        )]);
    }
    Ok(value)
}

fn lower_item_fn(
    source_file: &Path,
    item_fn: &ItemFn,
    declared_functions: &std::collections::BTreeSet<String>,
) -> Result<FnFact, Vec<LoweringError>> {
    let mut errors = Vec::new();

    // A review caught that these were silently ignored rather than
    // rejected: an `async fn`, a generic `fn f<T>(...)`, or a
    // `#[cfg(...)]`-style attribute on a requested function all lowered
    // as if they were ordinary, unconditional `i32`-only functions --
    // e.g. `fn identity<T>(x: T) -> T { x }` slipped past the *existing*
    // generic-rejection test only by accident (its parameter type `T`
    // happens to fail the unrelated `i32`-only type check first), so a
    // generic function whose parameters/return type *do* happen to be
    // `i32` was never actually rejected for being generic at all.
    // Checked explicitly here instead of relying on an unrelated check to
    // coincidentally catch it. Doc-comment attributes (`#[doc = ...]`,
    // what a `///` comment desugars to) are allowed through, since they
    // carry no semantic effect on the function itself.
    if item_fn.sig.asyncness.is_some() {
        errors.push(unsupported("async fn", item_fn.sig.span()));
    }
    if item_fn.sig.unsafety.is_some() {
        errors.push(unsupported("unsafe fn", item_fn.sig.span()));
    }
    if item_fn.sig.abi.is_some() {
        errors.push(unsupported("extern fn", item_fn.sig.span()));
    }
    if !item_fn.sig.generics.params.is_empty() {
        errors.push(unsupported("generic function", item_fn.sig.generics.span()));
    }
    if let Some(where_clause) = &item_fn.sig.generics.where_clause {
        errors.push(unsupported("where clause", where_clause.span()));
    }
    reject_unsupported_attrs(&item_fn.attrs, &mut errors);

    let mut params = Vec::new();
    let mut param_index = BTreeMap::new();

    for (index, arg) in item_fn.sig.inputs.iter().enumerate() {
        match arg {
            FnArg::Typed(pat_type) => {
                // A parameter can carry its own attribute independent of
                // the function's own (`fn f(#[cfg(test)] x: i32) -> i32`)
                // -- rustc's `flat_map_param` (issue #27's reference table)
                // strips `cfg` here specifically, one of the positions the
                // function-level-only check above did not cover.
                reject_unsupported_attrs(&pat_type.attrs, &mut errors);
                let name = match pat_type.pat.as_ref() {
                    Pat::Ident(pi) if pi.by_ref.is_none() && pi.subpat.is_none() => {
                        pi.ident.to_string()
                    }
                    other => {
                        errors.push(unsupported(
                            "non-identifier parameter pattern",
                            other.span(),
                        ));
                        continue;
                    }
                };
                if !type_is_i32(&pat_type.ty) {
                    errors.push(unsupported(
                        "parameter type other than i32",
                        pat_type.ty.span(),
                    ));
                    continue;
                }
                param_index.insert(name.clone(), index);
                params.push((name, IntWidth::I32));
            }
            FnArg::Receiver(r) => errors.push(unsupported("self parameter", r.span())),
        }
    }

    let return_width = match &item_fn.sig.output {
        ReturnType::Type(_, ty) if type_is_i32(ty) => IntWidth::I32,
        ReturnType::Type(_, ty) => {
            errors.push(unsupported("return type other than i32", ty.span()));
            IntWidth::I32
        }
        ReturnType::Default => {
            errors.push(unsupported(
                "function with no return type",
                item_fn.sig.span(),
            ));
            IntWidth::I32
        }
    };

    if !errors.is_empty() {
        return Err(errors);
    }

    let mut ctx = Ctx {
        source_file,
        declared_functions,
        params: param_index,
        locals: BTreeMap::new(),
        next_local: 0,
    };

    let body = lower_block(&item_fn.block, &mut ctx)?;

    Ok(FnFact {
        name: item_fn.sig.ident.to_string(),
        params,
        return_width,
        provenance: ctx.prov(item_fn.span()),
        body,
    })
}

/// Lowers a block whose shape is: zero or more `let NAME = EXPR;`
/// statements, followed by exactly one tail form (a trailing expression
/// with no semicolon, an `if` used at the tail, or a `return EXPR;`).
/// Anything else (extra statements after the tail, a `let` with no
/// initializer, item/macro statements, ...) is rejected.
fn lower_block(block: &Block, ctx: &mut Ctx) -> Result<Stmt, Vec<LoweringError>> {
    let mut lets: Vec<(&Local, Span)> = Vec::new();
    let mut tail: Option<&SynStmt> = None;
    for stmt in &block.stmts {
        if tail.is_some() {
            return Err(vec![unsupported_shape(
                "a statement follows this block's tail form; only a let-prefix followed by \
                 exactly one tail expression/return is supported",
                stmt.span(),
            )]);
        }
        match stmt {
            SynStmt::Local(local) => lets.push((local, local.span())),
            SynStmt::Expr(_, _) => tail = Some(stmt),
            other => {
                return Err(vec![unsupported("item/macro statement", other.span())]);
            }
        }
    }
    let Some(tail_stmt) = tail else {
        return Err(vec![unsupported_shape(
            "block has no tail expression/return",
            block.span(),
        )]);
    };

    // Lower each let's *value* expression and bind it in forward (source)
    // order first -- a later let's initializer, and the tail itself, must
    // be able to see every earlier let's binding. The nested `Stmt::Let`
    // tree is then built in reverse afterward, since the first let must
    // end up as the *outermost* node (its `body` is everything that
    // follows it, matching `types::Stmt::Let`'s own doc comment) -- name
    // resolution order and tree-construction order are different passes
    // over the same list, not the same loop.
    struct PendingLet {
        name: String,
        id: LocalId,
        value: Expr,
        span: Span,
        shadowed: Option<LocalId>,
    }
    let mut pending = Vec::with_capacity(lets.len());
    for (local, span) in &lets {
        // `cfg` and other attributes can attach to a `let` statement itself
        // (`#[cfg(test)] let x = 1;`), not only to a function/parameter --
        // rustc's `flat_map_stmt` (issue #27's reference table) is exactly
        // where a real compiler applies `cfg` at this position. This
        // subset implements no `cfg`, so any non-doc attribute here is
        // diagnosed the same way a function-level one already is, instead
        // of being silently accepted because only `ItemFn.attrs` was ever
        // checked.
        let mut attr_errors = Vec::new();
        reject_unsupported_attrs(&local.attrs, &mut attr_errors);
        if !attr_errors.is_empty() {
            return Err(attr_errors);
        }
        let name = match &local.pat {
            Pat::Ident(pi) if pi.by_ref.is_none() && pi.subpat.is_none() => pi.ident.to_string(),
            other => {
                return Err(vec![unsupported(
                    "non-identifier let pattern",
                    other.span(),
                )])
            }
        };
        let Some(init) = &local.init else {
            return Err(vec![unsupported_shape(
                "let binding with no initializer",
                *span,
            )]);
        };
        if init.diverge.is_some() {
            return Err(vec![unsupported("let-else", *span)]);
        }
        let value = lower_expr(&init.expr, ctx)?;
        let id = ctx.fresh_local();
        let shadowed = ctx.locals.insert(name.clone(), id);
        pending.push(PendingLet {
            name,
            id,
            value,
            span: *span,
            shadowed,
        });
    }

    let mut inner = lower_tail_stmt(tail_stmt, ctx)?;

    for p in pending.into_iter().rev() {
        match p.shadowed {
            Some(prev) => {
                ctx.locals.insert(p.name, prev);
            }
            None => {
                ctx.locals.remove(&p.name);
            }
        }
        inner = Stmt::Let {
            local: p.id,
            value: p.value,
            body: Box::new(inner),
            provenance: to_provenance(ctx.source_file, p.span),
        };
    }
    Ok(inner)
}

fn lower_tail_stmt(stmt: &SynStmt, ctx: &mut Ctx) -> Result<Stmt, Vec<LoweringError>> {
    let SynStmt::Expr(expr, semi) = stmt else {
        unreachable!("lower_block only ever stores an Expr statement as `tail`")
    };
    match expr {
        SynExpr::If(expr_if) if semi.is_none() => lower_if(expr_if, ctx),
        SynExpr::Return(ret) if semi.is_some() => {
            let Some(inner) = &ret.expr else {
                return Err(vec![unsupported_shape(
                    "bare return with no value",
                    ret.span(),
                )]);
            };
            let e = lower_expr(inner, ctx)?;
            Ok(Stmt::Return(e, to_provenance(ctx.source_file, ret.span())))
        }
        _ if semi.is_none() => {
            let e = lower_expr(expr, ctx)?;
            let prov = e.provenance().clone();
            Ok(Stmt::Return(e, prov))
        }
        _ => Err(vec![unsupported_shape(
            "a semicolon-terminated non-return tail statement (implicit () return) is not \
             supported",
            expr.span(),
        )]),
    }
}

fn lower_if(expr_if: &ExprIf, ctx: &mut Ctx) -> Result<Stmt, Vec<LoweringError>> {
    let mut attr_errors = Vec::new();
    reject_unsupported_attrs(&expr_if.attrs, &mut attr_errors);
    if !attr_errors.is_empty() {
        return Err(attr_errors);
    }
    let (cond, swap) = lower_condition(&expr_if.cond, ctx)?;
    let then = lower_block(&expr_if.then_branch, ctx)?;
    let Some((_, else_expr)) = &expr_if.else_branch else {
        return Err(vec![unsupported_shape(
            "if with no else (every path must return)",
            expr_if.span(),
        )]);
    };
    let els = match else_expr.as_ref() {
        SynExpr::Block(b) => lower_block(&b.block, ctx)?,
        SynExpr::If(inner) => lower_if(inner, ctx)?,
        other => return Err(vec![unsupported("else-branch shape", other.span())]),
    };
    let provenance = to_provenance(ctx.source_file, expr_if.span());
    Ok(if swap {
        Stmt::If {
            cond,
            then: Box::new(els),
            els: Box::new(then),
            provenance,
        }
    } else {
        Stmt::If {
            cond,
            then: Box::new(then),
            els: Box::new(els),
            provenance,
        }
    })
}

/// Lowers an `if` condition, which this subset restricts to `EXPR != 0`,
/// `0 != EXPR`, `EXPR == 0`, or `0 == EXPR`. Returns the lowered
/// `NotEqZero` expression plus whether the caller must swap its `then`/
/// `else` branches (true for the `== 0` forms, since this IR has no
/// standalone boolean negation).
fn lower_condition(expr: &SynExpr, ctx: &mut Ctx) -> Result<(Expr, bool), Vec<LoweringError>> {
    let SynExpr::Binary(bin) = expr else {
        return Err(vec![unsupported_shape(
            "if condition must be `EXPR != 0` or `EXPR == 0`",
            expr.span(),
        )]);
    };
    let swap = match bin.op {
        BinOp::Ne(_) => false,
        BinOp::Eq(_) => true,
        _ => {
            return Err(vec![unsupported_shape(
                "if condition must use != or ==",
                bin.span(),
            )])
        }
    };
    let is_zero_lit = |e: &SynExpr| -> bool {
        matches!(e, SynExpr::Lit(l) if matches!(&l.lit, Lit::Int(i) if i.base10_digits() == "0"))
    };
    let inner = if is_zero_lit(&bin.right) {
        &bin.left
    } else if is_zero_lit(&bin.left) {
        &bin.right
    } else {
        return Err(vec![unsupported_shape(
            "if condition must compare against a literal 0",
            bin.span(),
        )]);
    };
    let lowered_inner = lower_expr(inner, ctx)?;
    let provenance = to_provenance(ctx.source_file, bin.span());
    Ok((Expr::NotEqZero(Box::new(lowered_inner), provenance), swap))
}

fn lower_expr(expr: &SynExpr, ctx: &mut Ctx) -> Result<Expr, Vec<LoweringError>> {
    match expr {
        SynExpr::Paren(p) => lower_expr(&p.expr, ctx),
        SynExpr::Group(g) => lower_expr(&g.expr, ctx),
        SynExpr::Lit(l) => match &l.lit {
            Lit::Int(i) => {
                let value = parse_i32_literal(i, false, l.span())?;
                Ok(Expr::IntLit(
                    value,
                    IntWidth::I32,
                    to_provenance(ctx.source_file, l.span()),
                ))
            }
            _ => Err(vec![unsupported("non-integer literal", l.span())]),
        },
        SynExpr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
            // `-N` for a literal is folded directly; `-x` for a general
            // expression is out of scope for this subset (no standalone
            // negation operator in the IR).
            if let SynExpr::Lit(l) = u.expr.as_ref() {
                if let Lit::Int(i) = &l.lit {
                    let value = parse_i32_literal(i, true, u.span())?;
                    return Ok(Expr::IntLit(
                        value,
                        IntWidth::I32,
                        to_provenance(ctx.source_file, u.span()),
                    ));
                }
            }
            Err(vec![unsupported(
                "unary negation of a non-literal",
                u.span(),
            )])
        }
        SynExpr::Path(p) if p.path.get_ident().is_some() => {
            let name = p.path.get_ident().unwrap().to_string();
            let provenance = to_provenance(ctx.source_file, p.span());
            if let Some(local) = ctx.locals.get(&name) {
                Ok(Expr::Local(*local, provenance))
            } else if let Some(index) = ctx.params.get(&name) {
                Ok(Expr::Param(*index, provenance))
            } else {
                Err(vec![unsupported_shape(
                    format!("reference to undeclared identifier '{name}'"),
                    p.span(),
                )])
            }
        }
        SynExpr::MethodCall(m) => {
            let method = m.method.to_string();
            let kind = match method.as_str() {
                "wrapping_add" => 0,
                "wrapping_sub" => 1,
                "wrapping_mul" => 2,
                _ => {
                    return Err(vec![unsupported(
                        format!("method call '{method}'"),
                        m.span(),
                    )])
                }
            };
            if m.args.len() != 1 {
                return Err(vec![unsupported_shape(
                    format!("'{method}' with other than one argument"),
                    m.span(),
                )]);
            }
            let receiver = lower_expr(&m.receiver, ctx)?;
            let arg = lower_expr(&m.args[0], ctx)?;
            let provenance = to_provenance(ctx.source_file, m.span());
            Ok(match kind {
                0 => Expr::WrappingAdd(Box::new(receiver), Box::new(arg), provenance),
                1 => Expr::WrappingSub(Box::new(receiver), Box::new(arg), provenance),
                _ => Expr::WrappingMul(Box::new(receiver), Box::new(arg), provenance),
            })
        }
        // A review caught a real type-consistency bug: an earlier arm
        // here accepted `!=`/`==` as a *general*, `i32`-valued expression
        // (via `lower_condition`), so `fn f(x: i32) -> i32 { x != 0 }`
        // lowered successfully even though real Rust rejects it outright
        // -- `x != 0` has type `bool`, not `i32`, and cannot be returned
        // from a function declared to return `i32`. `NotEqZero` is only
        // ever constructed by `lower_condition`, called directly from
        // `lower_if`'s condition position (the one place this subset
        // actually has a boolean-shaped value to consume) -- never
        // through this general, `i32`-typed-value path; that arm has been
        // removed, so a bare `!=`/`==` reaching this match now falls
        // through to the generic `unsupported` case below, matching real
        // Rust's own type error.
        SynExpr::Call(c) => {
            let SynExpr::Path(p) = c.func.as_ref() else {
                return Err(vec![unsupported(
                    "call to a non-identifier callee",
                    c.span(),
                )]);
            };
            let Some(ident) = p.path.get_ident() else {
                return Err(vec![unsupported("call to a qualified path", c.span())]);
            };
            let name = ident.to_string();
            // A review caught a real name-resolution gap: this only ever
            // checked `declared_functions`, never whether `name` is *also*
            // bound as a local/parameter in scope at the call site. Real
            // Rust would reject calling a plain `i32` local even when a
            // function of the same name exists elsewhere (`let double =
            // 5; double(x)` is a type error: `i32` isn't callable) --
            // this subset has no callable local values at all (no
            // closures/fn pointers), so a local/parameter shadowing a
            // function name in call position is never valid here either,
            // and must not silently fall through to calling the
            // differently-scoped function of the same name instead.
            if ctx.locals.contains_key(&name) || ctx.params.contains_key(&name) {
                return Err(vec![unsupported_shape(
                    format!(
                        "'{name}' is a local/parameter here, not a callable value -- this subset \
                         has no function-valued locals"
                    ),
                    c.span(),
                )]);
            }
            if !ctx.declared_functions.contains(&name) {
                return Err(vec![unsupported_shape(
                    format!("call to '{name}', which is not in this lowering request"),
                    c.span(),
                )]);
            }
            let mut args = Vec::with_capacity(c.args.len());
            for a in &c.args {
                args.push(lower_expr(a, ctx)?);
            }
            Ok(Expr::Call(
                FnId(name),
                args,
                to_provenance(ctx.source_file, c.span()),
            ))
        }
        SynExpr::If(_) => Err(vec![unsupported(
            "if used as a value expression (only supported at a block's tail position)",
            expr.span(),
        )]),
        other => Err(vec![unsupported(describe_expr_kind(other), other.span())]),
    }
}

fn describe_expr_kind(expr: &SynExpr) -> &'static str {
    match expr {
        SynExpr::Array(_) => "array expression",
        SynExpr::Assign(_) => "assignment",
        SynExpr::Async(_) => "async block",
        SynExpr::Await(_) => "await",
        SynExpr::Block(_) => "block expression",
        SynExpr::Break(_) => "break",
        SynExpr::Cast(_) => "cast",
        SynExpr::Closure(_) => "closure",
        SynExpr::Continue(_) => "continue",
        SynExpr::Field(_) => "field access",
        SynExpr::ForLoop(_) => "for loop",
        SynExpr::Index(_) => "indexing",
        SynExpr::Loop(_) => "loop",
        SynExpr::Macro(_) => "macro invocation",
        SynExpr::Match(_) => "match",
        SynExpr::MethodCall(_) => "unsupported method call",
        SynExpr::Range(_) => "range",
        SynExpr::Reference(_) => "reference expression",
        SynExpr::Repeat(_) => "array repeat expression",
        SynExpr::Struct(_) => "struct literal",
        SynExpr::Try(_) => "try (?) operator",
        SynExpr::TryBlock(_) => "try block",
        SynExpr::Tuple(_) => "tuple expression",
        SynExpr::Unary(_) => "unary operator",
        SynExpr::Unsafe(_) => "unsafe block",
        SynExpr::While(_) => "while loop",
        _ => "unsupported expression",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::eval_function;
    use std::path::PathBuf;

    fn path() -> PathBuf {
        PathBuf::from("test.rs")
    }

    #[test]
    fn lowers_the_existing_workload_functions_and_matches_real_semantics() {
        let source = r#"
            fn double(x: i32) -> i32 {
                x.wrapping_add(x)
            }
            fn add_or_double(a: i32, b: i32, use_double: i32) -> i32 {
                if use_double != 0 {
                    double(a)
                } else {
                    a.wrapping_add(b)
                }
            }
        "#;
        let program = lower_rust_source(&path(), source, &["double", "add_or_double"]).unwrap();
        assert_eq!(program.functions.len(), 2);

        let cases: &[((i32, i32, i32), i32)] = &[
            ((3, 4, 0), 7),
            ((3, 4, 1), 6),
            ((i32::MAX, 1, 0), i32::MIN),
            ((-5, 10, 1), -10),
        ];
        for &((a, b, u), expected) in cases {
            let outcome =
                eval_function(&program, "add_or_double", &[a as i64, b as i64, u as i64]).unwrap();
            assert_eq!(outcome.value as i32, expected, "inputs ({a},{b},{u})");
        }
    }

    #[test]
    fn lowers_let_bindings_and_multiple_wrapping_ops() {
        let source = r#"
            fn combo(a: i32, b: i32, c: i32) -> i32 {
                let x = a.wrapping_add(b);
                let y = x.wrapping_sub(c);
                y.wrapping_mul(2)
            }
        "#;
        let program = lower_rust_source(&path(), source, &["combo"]).unwrap();
        let outcome = eval_function(&program, "combo", &[10, 3, 2]).unwrap();
        // (10+3)=13, 13-2=11, 11*2=22
        assert_eq!(outcome.value, 22);
    }

    #[test]
    fn explicit_return_and_return_only_body_are_supported() {
        let source = r#"
            fn f(a: i32) -> i32 {
                return a.wrapping_add(1);
            }
        "#;
        let program = lower_rust_source(&path(), source, &["f"]).unwrap();
        let outcome = eval_function(&program, "f", &[41]).unwrap();
        assert_eq!(outcome.value, 42);
    }

    #[test]
    fn rejects_a_match_expression_as_unsupported() {
        let source = r#"
            fn f(a: i32) -> i32 {
                match a {
                    _ => a,
                }
            }
        "#;
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_a_while_loop_as_unsupported() {
        let source = r#"
            fn f(a: i32) -> i32 {
                while a > 0 {}
                a
            }
        "#;
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_a_struct_definition_referenced_from_the_requested_function() {
        let source = r#"
            struct Point { x: i32 }
            fn f(a: i32) -> i32 {
                let p = Point { x: a };
                p.x
            }
        "#;
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_a_generic_function() {
        let source = r#"
            fn identity<T>(x: T) -> T { x }
            fn f(a: i32) -> i32 {
                a
            }
        "#;
        // `identity` itself is never requested, so this only proves the
        // *requested* function's own generic-parameter-free shape is what
        // matters -- a separate, direct generic-parameter rejection is
        // exercised by requesting `identity` itself:
        let result = lower_rust_source(&path(), source, &["identity"]);
        assert!(result.is_err());
        // and `f` alone still lowers fine, unaffected by an unrelated
        // unsupported item elsewhere in the file (matches this module's
        // "only inspect requested functions" contract).
        assert!(lower_rust_source(&path(), source, &["f"]).is_ok());
    }

    /// Regression test for the exact bug a review reproduced: a generic
    /// function whose parameters/return type *happen* to be `i32` used to
    /// slip through, because the previous test's own `identity<T>(x: T)`
    /// only got caught by the unrelated "parameter type other than i32"
    /// check (`T != i32`), not a genuine generic-parameter check.
    #[test]
    fn rejects_a_generic_function_even_when_its_types_are_all_i32() {
        let source = "fn identity<T>(x: i32) -> i32 { x }";
        let result = lower_rust_source(&path(), source, &["identity"]);
        assert!(
            result.is_err(),
            "a generic function must be rejected regardless of its types"
        );
    }

    #[test]
    fn rejects_an_async_function() {
        let source = "async fn f(x: i32) -> i32 { x }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_an_unsafe_function() {
        let source = "unsafe fn f(x: i32) -> i32 { x }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err());
    }

    /// A `#[cfg(...)]`-style attribute could silently select between
    /// different bodies depending on a build configuration this frontend
    /// never inspects -- must be rejected, not ignored.
    #[test]
    fn rejects_a_cfg_attribute() {
        let source = "#[cfg(target_os = \"linux\")]\nfn f(x: i32) -> i32 { x }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err());
    }

    /// A review caught a real gap: only `file.items` (and their own
    /// attributes) were ever inspected -- a file-level *inner* attribute
    /// (`#![cfg(..)]`), which can change what the whole file means
    /// (rustc's own `StripUnconfigured::configure` applies `cfg` at the
    /// crate-root level too, per issue #27's reference table), was never
    /// checked at all.
    #[test]
    fn rejects_a_file_level_cfg_attribute() {
        let source = "#![cfg(target_os = \"linux\")]\nfn f(x: i32) -> i32 { x }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err(), "{result:?}");
    }

    /// A plain file-level doc comment (`//!`) carries no semantic effect
    /// and must still be accepted, mirroring `accepts_a_plain_doc_comment`
    /// at the item level.
    #[test]
    fn accepts_a_file_level_doc_comment() {
        let source = "//! A file doc comment.\nfn f(x: i32) -> i32 { x }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_ok(), "{result:?}");
    }

    /// A review caught a real gap: two functions in the same file sharing
    /// a *requested* function's name were silently collapsed to whichever
    /// one `BTreeMap::insert` saw last, with no diagnostic at all -- real
    /// Rust rejects a duplicate item definition outright as a hard
    /// compile error, so this lowering must not pick one arbitrarily
    /// either.
    #[test]
    fn rejects_a_duplicate_declaration_of_a_requested_function() {
        let source = "fn f(x: i32) -> i32 { x }\nfn f(x: i32) -> i32 { x.wrapping_add(1) }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(
            result.is_err(),
            "a function declared twice must be rejected, not silently resolved to one definition"
        );
    }

    /// The same duplicate must be caught even when it's a function another
    /// requested function calls, not only when requested directly.
    #[test]
    fn rejects_a_duplicate_declaration_of_a_function_called_by_another_requested_function() {
        let source =
            "fn f(x: i32) -> i32 { x }\nfn f(x: i32) -> i32 { x }\nfn g(x: i32) -> i32 { f(x) }";
        let result = lower_rust_source(&path(), source, &["f", "g"]);
        assert!(
            result.is_err(),
            "'f', requested (transitively, via 'g') for lowering, is still declared twice and \
             must be rejected"
        );
    }

    /// A review caught that only `ItemFn.attrs` was ever checked -- `cfg`
    /// and other attributes attach to a `let` statement too (verified
    /// against rustc's own `flat_map_stmt`, issue #27's reference table),
    /// and were silently accepted there before this fix.
    #[test]
    fn rejects_a_cfg_attribute_on_a_let_statement() {
        let source = "fn f(x: i32) -> i32 {\n  #[cfg(test)]\n  let y = x;\n  y\n}";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err(), "{result:?}");
    }

    /// The same gap, on a function parameter (`flat_map_param` in rustc's
    /// own `rustc_expand::expand`, issue #27's reference table) -- silently
    /// accepted before this fix since only the function's own attributes
    /// were checked, never a parameter's.
    #[test]
    fn rejects_a_cfg_attribute_on_a_function_parameter() {
        let source = "fn f(#[cfg(test)] x: i32) -> i32 { x }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err(), "{result:?}");
    }

    /// The same gap, on an `if` used at a block's tail position.
    #[test]
    fn rejects_a_cfg_attribute_on_an_if_tail() {
        let source = "fn f(x: i32) -> i32 {\n  #[cfg(test)]\n  if x != 0 { x } else { 0 }\n}";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_err(), "{result:?}");
    }

    /// A plain doc comment carries no semantic effect and must still be
    /// accepted (it lowers to a harmless `#[doc = ...]` attribute).
    #[test]
    fn accepts_a_plain_doc_comment() {
        let source = "/// A doc comment.\nfn f(x: i32) -> i32 { x }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(result.is_ok(), "{result:?}");
    }

    /// Regression test for the exact type-consistency bug a review
    /// reproduced: `x != 0` has type `bool` in real Rust, not `i32`, and
    /// cannot be returned from a function declared to return `i32` --
    /// real `rustc` rejects this outright. `NotEqZero` must only be
    /// reachable through an `if` condition, never as a general value.
    #[test]
    fn rejects_a_bool_expression_used_as_an_i32_value() {
        let source = "fn f(x: i32) -> i32 { x != 0 }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(
            result.is_err(),
            "a bool-typed expression must not be accepted where an i32 value is expected"
        );
    }

    /// Regression test for the exact name-resolution bug a review
    /// reproduced: a local shadowing a declared function's name must not
    /// silently resolve a call to the *other-scope* function instead --
    /// real Rust would reject calling a plain `i32` local outright.
    #[test]
    fn rejects_calling_a_local_that_shadows_a_declared_functions_name() {
        let source = r#"
            fn double(x: i32) -> i32 { x.wrapping_add(x) }
            fn f(a: i32) -> i32 {
                let double = a;
                double(a)
            }
        "#;
        let result = lower_rust_source(&path(), source, &["double", "f"]);
        assert!(
            result.is_err(),
            "calling a name shadowed by a local must be rejected, not silently resolved to the \
             differently-scoped function"
        );
    }

    /// Regression test for the exact bug a review reproduced: the
    /// unary-negation path parsed only the literal's *digits*, never
    /// checking its suffix -- so `-1u64` (an unsigned 64-bit literal, a
    /// completely different type) silently became `IntLit(-1, I32)`.
    #[test]
    fn rejects_a_negative_literal_with_a_non_i32_suffix() {
        let source = "fn f() -> i32 { -1i64 }";
        let result = lower_rust_source(&path(), source, &["f"]);
        assert!(
            result.is_err(),
            "a non-i32-suffixed negative literal must be rejected"
        );
    }

    /// Regression test for the exact bug a review reproduced: Rust has no
    /// single "negative literal" token -- `-2147483648` is unary negation
    /// of the separately-tokenized positive digits `2147483648`, which do
    /// not themselves fit in `i32` (`i32::MAX` is `2147483647`), so
    /// parsing the pre-negation digits directly as `i32` rejected the
    /// literal even though the final, negated value is exactly `i32::MIN`
    /// and perfectly valid.
    #[test]
    fn accepts_i32_min_as_a_negative_literal() {
        let source = "fn f() -> i32 { -2147483648 }";
        let program = lower_rust_source(&path(), source, &["f"]).unwrap();
        let outcome = eval_function(&program, "f", &[]).unwrap();
        assert_eq!(outcome.value as i32, i32::MIN);
    }

    #[test]
    fn every_node_in_a_lowered_program_carries_real_source_provenance() {
        let source = r#"
            fn f(a: i32) -> i32 {
                a.wrapping_add(1)
            }
        "#;
        let program = lower_rust_source(&path(), source, &["f"]).unwrap();
        let fact = &program.functions["f"];
        assert_eq!(fact.provenance.language, SourceLanguage::Rust);
        assert!(fact.provenance.span.start.line >= 1);
    }

    // The following close named gaps in SUBSET.md's acceptance table
    // (issue #27 A1) -- each pairs a table row that previously had no
    // dedicated test with one.

    #[test]
    fn rejects_an_extern_fn() {
        let result = lower_rust_source(&path(), r#"extern "C" fn f(x: i32) -> i32 { x }"#, &["f"]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_a_where_clause() {
        let source = "fn f(x: i32) -> i32 where i32: Sized { x }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_self_parameter() {
        let source = "fn f(self, x: i32) -> i32 { x }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_non_identifier_parameter_pattern() {
        let source = "fn f((a, b): (i32, i32)) -> i32 { a }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_return_type_other_than_i32() {
        let source = "fn f(x: i32) -> bool { true }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_function_with_no_return_type() {
        let source = "fn f(x: i32) { }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_non_identifier_let_pattern() {
        let source = "fn f() -> i32 { let (a, b) = (1, 2); a }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_let_with_no_initializer() {
        let source = "fn f() -> i32 { let x: i32; x }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_let_else() {
        let source = "fn f(x: i32) -> i32 { let y = x else { return 0; }; y }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_an_item_statement_inside_a_block() {
        let source = "fn f(x: i32) -> i32 { fn g() {} x }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_bare_return_with_no_value() {
        let source = "fn f(x: i32) -> i32 { return; }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_semicolon_terminated_non_return_tail() {
        let source = "fn f(x: i32) -> i32 { x.wrapping_add(1); }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_non_block_else_branch() {
        let source = "fn f(x: i32) -> i32 { if x != 0 { x } else if x == 0 { 0 } else { 1 } }";
        // A genuinely non-block, non-if else (e.g. a call) is what this
        // targets; an `else if` chain is itself supported (SUBSET.md's own
        // table), so this uses a shape neither block nor `if`.
        let bad = "fn f(x: i32) -> i32 { if x != 0 { x } else x.wrapping_add(1) }";
        assert!(lower_rust_source(&path(), bad, &["f"]).is_err());
        // The `else if` chain above must still lower fine, confirming the
        // rejection above is about the *shape*, not `else` in general.
        assert!(lower_rust_source(&path(), source, &["f"]).is_ok());
    }

    #[test]
    fn accepts_an_else_if_chain() {
        let source = "fn f(x: i32) -> i32 {\n  if x != 0 { 1 } else if x == 0 { 2 } else { 3 }\n}";
        let program = lower_rust_source(&path(), source, &["f"]).unwrap();
        assert_eq!(eval_function(&program, "f", &[0]).unwrap().value, 2);
    }

    #[test]
    fn rejects_an_if_used_as_a_value_expression() {
        let source = "fn f(x: i32) -> i32 { (if x != 0 { 1 } else { 2 }).wrapping_add(0) }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_block_with_no_tail_expression() {
        let source = "fn f(x: i32) -> i32 { let y = x; }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_reference_to_an_undeclared_identifier() {
        let source = "fn f(x: i32) -> i32 { y }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_call_to_a_function_not_in_the_lowering_request() {
        let source = "fn helper(x: i32) -> i32 { x }\nfn f(x: i32) -> i32 { helper(x) }";
        // Only "f" is requested -- "helper" is declared in the file but
        // not part of this lowering request, so calling it must fail
        // rather than reach into the file for a function never asked for.
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_call_through_a_qualified_path() {
        let source = "fn f(x: i32) -> i32 { std::convert::identity(x) }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_non_integer_literal() {
        let source = r#"fn f() -> i32 { "not an int" }"#;
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_unary_negation_of_a_non_literal() {
        let source = "fn f(x: i32) -> i32 { -x }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_method_call_other_than_wrapping_ops() {
        let source = "fn f(x: i32) -> i32 { x.checked_add(1).unwrap() }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_wrapping_call_with_the_wrong_argument_count() {
        let source = "fn f(x: i32) -> i32 { x.wrapping_add() }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_positive_literal_with_a_non_i32_suffix() {
        let source = "fn f() -> i32 { 5u64 }";
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }

    #[test]
    fn rejects_a_positive_literal_out_of_i32_range() {
        let source = "fn f() -> i32 { 2147483648 }"; // i32::MAX + 1
        assert!(lower_rust_source(&path(), source, &["f"]).is_err());
    }
}

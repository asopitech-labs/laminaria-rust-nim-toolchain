//! Source-derived semantics for a narrow Rust prime predicate and its
//! inclusive-range/filter/collect consumer. Unsupported syntax is rejected;
//! the fixture's numeric results are never embedded in the IR.

use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::{BinOp, Expr, FnArg, Item, Lit, Pat, Stmt, Type};

use crate::rust_frontend::to_source_position;
use crate::types::{Provenance, SourceLanguage, SourceSpan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimeSemanticsIr {
    pub predicate_name: String,
    pub sequence_name: String,
    pub minimum_candidate: u64,
    pub initial_divisor: u64,
    pub divisor_step: u64,
    pub remainder_target: u64,
    pub range_start: u64,
    pub predicate_provenance: Provenance,
    pub sequence_provenance: Provenance,
}

#[derive(Debug)]
pub enum PrimeSemanticError {
    Parse(syn::Error),
    MissingFunction(String),
    DuplicateFunction(String),
    UnsupportedSignature(String),
    UnsupportedBody(String),
    ArithmeticOverflow,
}

impl std::fmt::Display for PrimeSemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid Rust source: {error}"),
            Self::MissingFunction(name) => write!(f, "function {name:?} is absent"),
            Self::DuplicateFunction(name) => write!(f, "function {name:?} is ambiguous"),
            Self::UnsupportedSignature(name) => write!(f, "unsupported signature of {name:?}"),
            Self::UnsupportedBody(name) => write!(f, "unsupported body of {name:?}"),
            Self::ArithmeticOverflow => write!(f, "checked u64 arithmetic overflowed"),
        }
    }
}

impl std::error::Error for PrimeSemanticError {}

/// Lower two linked functions only after checking the actual source bodies.
/// The accepted predicate is `if n < MIN { return false; } let mut i = START;
/// while i * i <= n { if n % i == REM { return false; } i += STEP; } true`.
/// The sequence is `(RANGE..=limit).filter(|&n| predicate(n)).collect()`.
pub fn lower_prime_semantics(
    source_file: &Path,
    source_text: &str,
    predicate_name: &str,
    sequence_name: &str,
) -> Result<PrimeSemanticsIr, PrimeSemanticError> {
    let file = syn::parse_file(source_text).map_err(PrimeSemanticError::Parse)?;
    if file.attrs.iter().any(|attr| !attr.path().is_ident("doc")) {
        return Err(PrimeSemanticError::UnsupportedSignature(
            predicate_name.into(),
        ));
    }
    // A local `Vec` or imported trait can change resolution of the sequence
    // signature and iterator methods. This subset does not resolve imports.
    if file.items.iter().any(|item| match item {
        Item::Use(_) | Item::ExternCrate(_) | Item::Macro(_) => true,
        Item::Struct(value) => value.ident == "Vec",
        Item::Enum(value) => value.ident == "Vec",
        Item::Union(value) => value.ident == "Vec",
        Item::Type(value) => value.ident == "Vec",
        Item::Mod(value) => value.ident == "Vec",
        Item::Trait(value) => value.ident == "Vec",
        _ => false,
    }) {
        return Err(PrimeSemanticError::UnsupportedSignature(
            sequence_name.into(),
        ));
    }
    let predicate = find_function(&file.items, predicate_name)?;
    let sequence = find_function(&file.items, sequence_name)?;
    let candidate = argument_name(predicate, "u64", "bool")
        .ok_or_else(|| PrimeSemanticError::UnsupportedSignature(predicate_name.into()))?;
    let limit = argument_name(sequence, "u64", "Vec<u64>")
        .ok_or_else(|| PrimeSemanticError::UnsupportedSignature(sequence_name.into()))?;
    let (minimum_candidate, initial_divisor, divisor_step, remainder_target) =
        predicate_body(predicate, &candidate)
            .ok_or_else(|| PrimeSemanticError::UnsupportedBody(predicate_name.into()))?;
    let range_start = sequence_body(sequence, &limit, predicate_name)
        .ok_or_else(|| PrimeSemanticError::UnsupportedBody(sequence_name.into()))?;
    // A zero divisor or non-progressing loop would be valid Rust syntax but
    // would never produce a useful prime predicate; fail before evaluation.
    if initial_divisor == 0 || divisor_step == 0 || minimum_candidate < 2 {
        return Err(PrimeSemanticError::UnsupportedBody(predicate_name.into()));
    }
    Ok(PrimeSemanticsIr {
        predicate_name: predicate_name.into(),
        sequence_name: sequence_name.into(),
        minimum_candidate,
        initial_divisor,
        divisor_step,
        remainder_target,
        range_start,
        predicate_provenance: provenance(source_file, predicate.span()),
        sequence_provenance: provenance(source_file, sequence.span()),
    })
}

fn find_function<'a>(items: &'a [Item], name: &str) -> Result<&'a syn::ItemFn, PrimeSemanticError> {
    let matches: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == name => Some(function),
            _ => None,
        })
        .collect();
    match matches.as_slice() {
        [] => Err(PrimeSemanticError::MissingFunction(name.into())),
        [function] => Ok(function),
        _ => Err(PrimeSemanticError::DuplicateFunction(name.into())),
    }
}

fn argument_name(function: &syn::ItemFn, input: &str, output: &str) -> Option<String> {
    let sig = &function.sig;
    if sig.constness.is_some()
        || sig.asyncness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
        || sig.variadic.is_some()
        || function
            .attrs
            .iter()
            .any(|attr| !attr.path().is_ident("doc"))
    {
        return None;
    }
    let [FnArg::Typed(arg)] = sig.inputs.iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    let Pat::Ident(name) = arg.pat.as_ref() else {
        return None;
    };
    if !arg.attrs.is_empty()
        || !name.attrs.is_empty()
        || name.by_ref.is_some()
        || name.mutability.is_some()
        || name.subpat.is_some()
        || !type_path(&arg.ty, input)
    {
        return None;
    }
    let syn::ReturnType::Type(_, result) = &sig.output else {
        return None;
    };
    let valid_output = if output == "Vec<u64>" {
        matches!(result.as_ref(), Type::Path(path)
            if path.qself.is_none() && path.path.segments.len() == 1
            && path.path.segments[0].ident == "Vec"
            && matches!(&path.path.segments[0].arguments, syn::PathArguments::AngleBracketed(args)
                if args.args.len() == 1 && matches!(args.args.first(), Some(syn::GenericArgument::Type(ty)) if type_path(ty, "u64"))))
    } else {
        type_path(result, output)
    };
    valid_output.then(|| name.ident.to_string())
}

fn type_path(ty: &Type, ident: &str) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.is_ident(ident))
}

fn expr(value: &Expr) -> &Expr {
    match value {
        Expr::Paren(paren) if paren.attrs.is_empty() => expr(&paren.expr),
        Expr::Group(group) if group.attrs.is_empty() => expr(&group.expr),
        other => other,
    }
}

fn ident(expr_value: &Expr, expected: &str) -> bool {
    matches!(expr(expr_value), Expr::Path(path)
        if path.attrs.is_empty() && path.qself.is_none() && path.path.is_ident(expected))
}

fn literal(expr_value: &Expr) -> Option<u64> {
    let Expr::Lit(value) = expr(expr_value) else {
        return None;
    };
    let Lit::Int(number) = &value.lit else {
        return None;
    };
    (value.attrs.is_empty() && (number.suffix().is_empty() || number.suffix() == "u64"))
        .then(|| number.base10_parse().ok())
        .flatten()
}

fn boolean(expr_value: &Expr, expected: bool) -> bool {
    matches!(expr(expr_value), Expr::Lit(value)
        if value.attrs.is_empty() && matches!(&value.lit, Lit::Bool(value) if value.value == expected))
}

fn return_boolean(block: &syn::Block, expected: bool) -> bool {
    matches!(block.stmts.as_slice(), [Stmt::Expr(Expr::Return(ret), Some(_))]
        if ret.attrs.is_empty() && ret.expr.as_deref().is_some_and(|value| boolean(value, expected)))
}

fn predicate_body(function: &syn::ItemFn, candidate: &str) -> Option<(u64, u64, u64, u64)> {
    let [Stmt::Expr(Expr::If(first), None), Stmt::Local(local), Stmt::Expr(Expr::While(loop_expr), None), Stmt::Expr(last, None)] =
        function.block.stmts.as_slice()
    else {
        return None;
    };
    if !first.attrs.is_empty()
        || first.else_branch.is_some()
        || !return_boolean(&first.then_branch, false)
        || !boolean(last, true)
    {
        return None;
    }
    let Expr::Binary(minimum) = expr(&first.cond) else {
        return None;
    };
    if !minimum.attrs.is_empty()
        || !matches!(minimum.op, BinOp::Lt(_))
        || !ident(&minimum.left, candidate)
    {
        return None;
    }
    let minimum_candidate = literal(&minimum.right)?;
    let Pat::Ident(divisor) = &local.pat else {
        return None;
    };
    if !local.attrs.is_empty()
        || !divisor.attrs.is_empty()
        || divisor.by_ref.is_some()
        || divisor.mutability.is_none()
        || divisor.subpat.is_some()
    {
        return None;
    }
    let divisor_name = divisor.ident.to_string();
    if divisor_name == candidate {
        return None;
    }
    let initial_divisor = match &local.init {
        Some(init) if init.diverge.is_none() => literal(&init.expr)?,
        _ => return None,
    };
    if !loop_expr.attrs.is_empty() || loop_expr.label.is_some() {
        return None;
    }
    let Expr::Binary(bound) = expr(&loop_expr.cond) else {
        return None;
    };
    let Expr::Binary(square) = expr(&bound.left) else {
        return None;
    };
    if !bound.attrs.is_empty()
        || !square.attrs.is_empty()
        || !matches!(bound.op, BinOp::Le(_))
        || !matches!(square.op, BinOp::Mul(_))
        || !ident(&square.left, &divisor_name)
        || !ident(&square.right, &divisor_name)
        || !ident(&bound.right, candidate)
    {
        return None;
    }
    let [Stmt::Expr(Expr::If(test), None), Stmt::Expr(Expr::Binary(step), Some(_))] =
        loop_expr.body.stmts.as_slice()
    else {
        return None;
    };
    if !test.attrs.is_empty()
        || test.else_branch.is_some()
        || !return_boolean(&test.then_branch, false)
    {
        return None;
    }
    let Expr::Binary(equal) = expr(&test.cond) else {
        return None;
    };
    let Expr::Binary(remainder) = expr(&equal.left) else {
        return None;
    };
    if !equal.attrs.is_empty()
        || !remainder.attrs.is_empty()
        || !matches!(equal.op, BinOp::Eq(_))
        || !matches!(remainder.op, BinOp::Rem(_))
        || !ident(&remainder.left, candidate)
        || !ident(&remainder.right, &divisor_name)
        || !ident(&step.left, &divisor_name)
        || !matches!(step.op, BinOp::AddAssign(_))
        || !step.attrs.is_empty()
    {
        return None;
    }
    Some((
        minimum_candidate,
        initial_divisor,
        literal(&step.right)?,
        literal(&equal.right)?,
    ))
}

fn sequence_body(function: &syn::ItemFn, limit: &str, predicate_name: &str) -> Option<u64> {
    let [Stmt::Expr(body, None)] = function.block.stmts.as_slice() else {
        return None;
    };
    let Expr::MethodCall(collect) = expr(body) else {
        return None;
    };
    if !collect.attrs.is_empty()
        || collect.method != "collect"
        || collect.turbofish.is_some()
        || !collect.args.is_empty()
    {
        return None;
    }
    let Expr::MethodCall(filter) = expr(&collect.receiver) else {
        return None;
    };
    if !filter.attrs.is_empty()
        || filter.method != "filter"
        || filter.turbofish.is_some()
        || filter.args.len() != 1
    {
        return None;
    }
    let Expr::Range(range) = expr(&filter.receiver) else {
        return None;
    };
    if !range.attrs.is_empty()
        || !matches!(range.limits, syn::RangeLimits::Closed(_))
        || !range.end.as_deref().is_some_and(|end| ident(end, limit))
    {
        return None;
    }
    let range_start = literal(range.start.as_deref()?)?;
    let Expr::Closure(closure) = expr(&filter.args[0]) else {
        return None;
    };
    if !closure.attrs.is_empty()
        || closure.asyncness.is_some()
        || closure.movability.is_some()
        || closure.capture.is_some()
        || closure.inputs.len() != 1
        || !matches!(closure.output, syn::ReturnType::Default)
    {
        return None;
    }
    let Some(Pat::Reference(reference)) = closure.inputs.first() else {
        return None;
    };
    let Pat::Ident(candidate) = reference.pat.as_ref() else {
        return None;
    };
    if !reference.attrs.is_empty()
        || reference.mutability.is_some()
        || !candidate.attrs.is_empty()
        || candidate.by_ref.is_some()
        || candidate.mutability.is_some()
        || candidate.subpat.is_some()
    {
        return None;
    }
    let Expr::Call(call) = expr(&closure.body) else {
        return None;
    };
    if !call.attrs.is_empty()
        || call.args.len() != 1
        || !ident(&call.func, predicate_name)
        || !ident(&call.args[0], &candidate.ident.to_string())
    {
        return None;
    }
    Some(range_start)
}

fn provenance(source_file: &Path, span: proc_macro2::Span) -> Provenance {
    Provenance {
        source_file: PathBuf::from(source_file),
        span: SourceSpan {
            start: to_source_position(span.start()),
            end: to_source_position(span.end()),
        },
        language: SourceLanguage::Rust,
    }
}

impl PrimeSemanticsIr {
    pub fn is_prime(&self, candidate: u64) -> Result<bool, PrimeSemanticError> {
        if candidate < self.minimum_candidate {
            return Ok(false);
        }
        let mut divisor = self.initial_divisor;
        while divisor
            .checked_mul(divisor)
            .ok_or(PrimeSemanticError::ArithmeticOverflow)?
            <= candidate
        {
            if candidate % divisor == self.remainder_target {
                return Ok(false);
            }
            divisor = divisor
                .checked_add(self.divisor_step)
                .ok_or(PrimeSemanticError::ArithmeticOverflow)?;
        }
        Ok(true)
    }

    pub fn primes_up_to(&self, limit: u64) -> Result<Vec<u64>, PrimeSemanticError> {
        let mut result = Vec::new();
        for candidate in self.range_start..=limit {
            if self.is_prime(candidate)? {
                result.push(candidate);
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "../../../fixtures/rust-heavy-workspace/crates/fixture-core/src/lib.rs"]
mod reference;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_derived_semantics_match_reference_function() {
        let source_file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rust-heavy-workspace/crates/fixture-core/src/lib.rs");
        let source = std::fs::read_to_string(&source_file).unwrap();
        let ir = lower_prime_semantics(&source_file, &source, "is_prime", "primes_up_to").unwrap();
        assert_eq!(ir.predicate_provenance.source_file, source_file);
        assert_eq!(ir.sequence_provenance.language, SourceLanguage::Rust);
        for value in 0..=400 {
            assert_eq!(ir.is_prime(value).unwrap(), reference::is_prime(value));
        }
        for limit in [0, 1, 2, 20, 200] {
            assert_eq!(
                ir.primes_up_to(limit).unwrap(),
                reference::primes_up_to(limit)
            );
        }
    }

    #[test]
    fn changed_source_semantics_fail_closed() {
        let source_file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rust-heavy-workspace/crates/fixture-core/src/lib.rs");
        let source = std::fs::read_to_string(&source_file).unwrap();
        let changed = source.replacen("n % i == 0", "n % i != 0", 1);
        assert!(matches!(
            lower_prime_semantics(&source_file, &changed, "is_prime", "primes_up_to"),
            Err(PrimeSemanticError::UnsupportedBody(_))
        ));
    }
}

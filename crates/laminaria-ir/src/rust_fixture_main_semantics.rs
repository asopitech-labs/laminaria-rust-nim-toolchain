//! Source-derived entry-function facts for the currently supported Rust
//! collection pipeline. Every recognized operation retains its source span;
//! unsupported expressions fail before an entry IR is published.

use std::path::{Path, PathBuf};

use quote::ToTokens;
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::{Expr, Item, Pat, Stmt};

use crate::rust_frontend::to_source_position;
use crate::types::{Provenance, SourceLanguage, SourceSpan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Cluster,
    I64,
    OptionalPoint,
    I64Vector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryBinding {
    pub name: String,
    pub ty: EntryType,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryOperation {
    PrimeGrid { limit: u64 },
    TotalPerimeter { cluster: String },
    Centroid { cluster: String },
    CollectXCoordinates { cluster: String },
    SumGenericI64 { values: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryComputation {
    pub binding: EntryBinding,
    pub operation: EntryOperation,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryOutputValue {
    PointsLength { cluster: String },
    Perimeter { binding: String },
    CentroidDebug { binding: String },
    SumX { binding: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPrint {
    /// The actual format string from source, not a rendered oracle value.
    pub format: String,
    pub value: EntryOutputValue,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustEntryIr {
    pub computations: Vec<EntryComputation>,
    pub prints: Vec<EntryPrint>,
    pub provenance: Provenance,
}

#[derive(Debug)]
pub enum EntryLoweringError {
    Parse(syn::Error),
    MissingMain,
    AmbiguousMain,
    UnsupportedImports,
    UnsupportedMainSignature,
    UnsupportedStatement { index: usize },
}

impl std::fmt::Display for EntryLoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid Rust entry source: {error}"),
            Self::MissingMain => write!(f, "Rust entry source has no main function"),
            Self::AmbiguousMain => write!(f, "Rust entry source has multiple main functions"),
            Self::UnsupportedImports => write!(f, "unsupported Rust entry items or imports"),
            Self::UnsupportedMainSignature => write!(f, "unsupported Rust main signature"),
            Self::UnsupportedStatement { index } => {
                write!(f, "unsupported Rust entry statement at index {index}")
            }
        }
    }
}

impl std::error::Error for EntryLoweringError {}

/// Lower the declared entry pipeline from actual source. The recognizer is
/// deliberately narrow: changed operations, bindings, or formatting reject
/// instead of becoming an inaccurate owned-compiler claim.
pub fn lower_rust_entry(
    source_file: &Path,
    source_text: &str,
) -> Result<RustEntryIr, EntryLoweringError> {
    let file = syn::parse_file(source_text).map_err(EntryLoweringError::Parse)?;
    if file.attrs.iter().any(|attr| !attr.path().is_ident("doc")) {
        return Err(EntryLoweringError::UnsupportedImports);
    }
    let mains: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "main" => Some(function),
            _ => None,
        })
        .collect();
    let main = match mains.as_slice() {
        [] => return Err(EntryLoweringError::MissingMain),
        [main] => main,
        _ => return Err(EntryLoweringError::AmbiguousMain),
    };
    if file.items.len() != 3
        || !import(&file.items[0], &["fixture_core", "sum_generic"])
        || !import(&file.items[1], &["fixture_mid", "Cluster"])
        || !matches!(&file.items[2], Item::Fn(function) if function.sig.ident == "main")
    {
        return Err(EntryLoweringError::UnsupportedImports);
    }
    let sig = &main.sig;
    if !main.attrs.is_empty()
        || sig.constness.is_some()
        || sig.asyncness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
        || !sig.inputs.is_empty()
        || !matches!(sig.output, syn::ReturnType::Default)
        || main.block.stmts.len() != 9
    {
        return Err(EntryLoweringError::UnsupportedMainSignature);
    }

    let steps = &main.block.stmts;
    let cluster = local_init(&steps[0], "cluster", None).ok_or(unsupported(0))?;
    let limit = prime_grid_limit(cluster).ok_or(unsupported(0))?;
    let perimeter = local_init(&steps[1], "perimeter", None).ok_or(unsupported(1))?;
    if !method_on_local(perimeter, "cluster", "total_perimeter") {
        return Err(unsupported(1));
    }
    let centroid = local_init(&steps[2], "centroid", None).ok_or(unsupported(2))?;
    if !method_on_local(centroid, "cluster", "centroid") {
        return Err(unsupported(2));
    }
    let xs = local_init(&steps[3], "xs", Some("Vec<i64>")).ok_or(unsupported(3))?;
    if !collect_x_coordinates(xs, "cluster") {
        return Err(unsupported(3));
    }
    let sum_x = local_init(&steps[4], "sum_x", None).ok_or(unsupported(4))?;
    if !sum_i64(sum_x, "xs") {
        return Err(unsupported(4));
    }

    let descriptions = [
        (
            "cluster",
            EntryType::Cluster,
            EntryOperation::PrimeGrid { limit },
        ),
        (
            "perimeter",
            EntryType::I64,
            EntryOperation::TotalPerimeter {
                cluster: "cluster".into(),
            },
        ),
        (
            "centroid",
            EntryType::OptionalPoint,
            EntryOperation::Centroid {
                cluster: "cluster".into(),
            },
        ),
        (
            "xs",
            EntryType::I64Vector,
            EntryOperation::CollectXCoordinates {
                cluster: "cluster".into(),
            },
        ),
        (
            "sum_x",
            EntryType::I64,
            EntryOperation::SumGenericI64 {
                values: "xs".into(),
            },
        ),
    ];
    let computations = descriptions
        .into_iter()
        .enumerate()
        .map(|(index, (name, ty, operation))| EntryComputation {
            binding: EntryBinding {
                name: name.into(),
                ty,
                provenance: provenance(source_file, steps[index].span()),
            },
            operation,
            provenance: provenance(source_file, steps[index].span()),
        })
        .collect();

    let print_specs = [
        (
            "points={}",
            EntryOutputValue::PointsLength {
                cluster: "cluster".into(),
            },
        ),
        (
            "perimeter={perimeter}",
            EntryOutputValue::Perimeter {
                binding: "perimeter".into(),
            },
        ),
        (
            "centroid={centroid:?}",
            EntryOutputValue::CentroidDebug {
                binding: "centroid".into(),
            },
        ),
        (
            "sum_x={sum_x}",
            EntryOutputValue::SumX {
                binding: "sum_x".into(),
            },
        ),
    ];
    let mut prints = Vec::with_capacity(4);
    for (offset, (expected_format, value)) in print_specs.into_iter().enumerate() {
        let index = offset + 5;
        let (format, arg) = print_call(&steps[index]).ok_or(unsupported(index))?;
        if format != expected_format || (index == 5) != arg.is_some() {
            return Err(unsupported(index));
        }
        if index == 5 && !arg.is_some_and(|arg| method_on_field(&arg, "cluster", "points", "len")) {
            return Err(unsupported(index));
        }
        prints.push(EntryPrint {
            format,
            value,
            provenance: provenance(source_file, steps[index].span()),
        });
    }
    Ok(RustEntryIr {
        computations,
        prints,
        provenance: provenance(source_file, main.span()),
    })
}

fn unsupported(index: usize) -> EntryLoweringError {
    EntryLoweringError::UnsupportedStatement { index }
}

fn provenance(path: &Path, span: proc_macro2::Span) -> Provenance {
    Provenance {
        source_file: PathBuf::from(path),
        span: SourceSpan {
            start: to_source_position(span.start()),
            end: to_source_position(span.end()),
        },
        language: SourceLanguage::Rust,
    }
}

fn import(item: &Item, segments: &[&str]) -> bool {
    let Item::Use(item) = item else { return false };
    if !item.attrs.is_empty() || item.leading_colon.is_some() {
        return false;
    }
    let mut tree = &item.tree;
    for (index, expected) in segments.iter().enumerate() {
        tree = match (tree, index + 1 == segments.len()) {
            (syn::UseTree::Path(path), false) if path.ident == *expected => &path.tree,
            (syn::UseTree::Name(name), true) if name.ident == *expected => return true,
            _ => return false,
        };
    }
    false
}

fn local_init<'a>(stmt: &'a Stmt, name: &str, annotation: Option<&str>) -> Option<&'a Expr> {
    let Stmt::Local(local) = stmt else {
        return None;
    };
    if !local.attrs.is_empty()
        || local
            .init
            .as_ref()
            .is_some_and(|init| init.diverge.is_some())
    {
        return None;
    }
    let Pat::Ident(ident) = local.pat.clone().into_untyped() else {
        return None;
    };
    if !ident.attrs.is_empty()
        || ident.by_ref.is_some()
        || ident.mutability.is_some()
        || ident.subpat.is_some()
        || ident.ident != name
    {
        return None;
    }
    match (&local.pat, annotation) {
        (Pat::Ident(_), None) => {}
        (Pat::Type(typed), Some(expected)) if compact(&typed.ty) == expected => {}
        _ => return None,
    }
    Some(&local.init.as_ref()?.expr)
}

trait UntypedPattern {
    fn into_untyped(self) -> Pat;
}

impl UntypedPattern for Pat {
    fn into_untyped(self) -> Pat {
        if let Pat::Type(typed) = self {
            *typed.pat
        } else {
            self
        }
    }
}

fn compact(value: &impl ToTokens) -> String {
    value.to_token_stream().to_string().replace(' ', "")
}

fn path(expr: &Expr, segments: &[&str]) -> bool {
    let Expr::Path(p) = expr else { return false };
    p.attrs.is_empty()
        && p.qself.is_none()
        && p.path.leading_colon.is_none()
        && p.path.segments.len() == segments.len()
        && p.path.segments.iter().zip(segments).all(|(part, name)| {
            part.ident == *name && matches!(part.arguments, syn::PathArguments::None)
        })
}

fn field(expr: &Expr, base: &str, member: &str) -> bool {
    let Expr::Field(f) = expr else { return false };
    f.attrs.is_empty()
        && path(&f.base, &[base])
        && matches!(&f.member, syn::Member::Named(name) if name == member)
}

fn method<'a>(expr: &'a Expr, name: &str) -> Option<&'a Expr> {
    let Expr::MethodCall(call) = expr else {
        return None;
    };
    (call.attrs.is_empty()
        && call.method == name
        && call.turbofish.is_none()
        && call.args.is_empty())
    .then_some(&call.receiver)
}

fn method_on_local(expr: &Expr, local: &str, name: &str) -> bool {
    method(expr, name).is_some_and(|receiver| path(receiver, &[local]))
}

fn method_on_field(expr: &Expr, local: &str, member: &str, name: &str) -> bool {
    method(expr, name).is_some_and(|receiver| field(receiver, local, member))
}

fn prime_grid_limit(expr: &Expr) -> Option<u64> {
    let Expr::Call(call) = expr else { return None };
    if !call.attrs.is_empty()
        || !path(&call.func, &["Cluster", "from_prime_grid"])
        || call.args.len() != 1
    {
        return None;
    }
    let Expr::Lit(lit) = call.args.first()? else {
        return None;
    };
    let syn::Lit::Int(value) = &lit.lit else {
        return None;
    };
    if !lit.attrs.is_empty() || !value.suffix().is_empty() && value.suffix() != "u64" {
        return None;
    }
    value.base10_parse().ok()
}

fn collect_x_coordinates(expr: &Expr, cluster: &str) -> bool {
    let Some(mapped) = method(expr, "collect") else {
        return false;
    };
    let Expr::MethodCall(map) = mapped else {
        return false;
    };
    if !map.attrs.is_empty()
        || map.method != "map"
        || map.turbofish.is_some()
        || map.args.len() != 1
        || !method_on_field(&map.receiver, cluster, "points", "iter")
    {
        return false;
    }
    let Some(Expr::Closure(closure)) = map.args.first() else {
        return false;
    };
    if !closure.attrs.is_empty()
        || closure.asyncness.is_some()
        || closure.movability.is_some()
        || closure.capture.is_some()
        || closure.inputs.len() != 1
        || !matches!(closure.output, syn::ReturnType::Default)
    {
        return false;
    }
    let Some(Pat::Ident(param)) = closure.inputs.first() else {
        return false;
    };
    param.attrs.is_empty()
        && param.by_ref.is_none()
        && param.mutability.is_none()
        && param.subpat.is_none()
        && field(&closure.body, &param.ident.to_string(), "x")
}

fn sum_i64(expr: &Expr, values: &str) -> bool {
    let Expr::Call(call) = expr else { return false };
    if !call.attrs.is_empty() || !path(&call.func, &["sum_generic"]) || call.args.len() != 1 {
        return false;
    }
    matches!(call.args.first(), Some(Expr::Reference(reference))
        if reference.attrs.is_empty()
        && reference.mutability.is_none()
        && path(&reference.expr, &[values]))
}

fn print_call(stmt: &Stmt) -> Option<(String, Option<Expr>)> {
    let Stmt::Macro(stmt) = stmt else { return None };
    if !stmt.attrs.is_empty()
        || stmt.semi_token.is_none()
        || !matches!(stmt.mac.delimiter, syn::MacroDelimiter::Paren(_))
        || stmt.mac.path.leading_colon.is_some()
        || stmt.mac.path.segments.len() != 1
        || !stmt.mac.path.is_ident("println")
    {
        return None;
    }
    let args = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated
        .parse2(stmt.mac.tokens.clone())
        .ok()?;
    let args: Vec<_> = args.iter().collect();
    let [Expr::Lit(lit), rest @ ..] = args.as_slice() else {
        return None;
    };
    let syn::Lit::Str(format) = &lit.lit else {
        return None;
    };
    if !lit.attrs.is_empty() || rest.len() > 1 {
        return None;
    }
    Some((format.value(), rest.first().map(|arg| (*arg).clone())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, String) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rust-heavy-workspace/crates/fixture-bin/src/main.rs");
        let source = std::fs::read_to_string(&path).unwrap();
        (path, source)
    }

    #[test]
    fn lowers_real_fixture_entry_pipeline_with_source_provenance() {
        let (path, source) = fixture();
        let ir = lower_rust_entry(&path, &source).unwrap();
        assert_eq!(ir.computations.len(), 5);
        assert_eq!(ir.prints.len(), 4);
        assert_eq!(
            ir.computations[0].operation,
            EntryOperation::PrimeGrid { limit: 200 }
        );
        assert_eq!(ir.computations[3].binding.ty, EntryType::I64Vector);
        assert_eq!(ir.prints[2].format, "centroid={centroid:?}");
        assert_eq!(ir.provenance.source_file, path);
        assert_eq!(ir.provenance.language, SourceLanguage::Rust);
        assert!(
            ir.computations[0].provenance.span.start.line < ir.prints[0].provenance.span.start.line
        );
    }

    #[test]
    fn rejects_changed_computation_or_output_semantics() {
        let (path, source) = fixture();
        for changed in [
            source.replace("from_prime_grid(200)", "from_prime_grid(-200)"),
            source.replace("cluster.total_perimeter()", "cluster.centroid()"),
            source.replace(".map(|p| p.x)", ".map(|p| p.y)"),
            source.replace("sum_generic(&xs)", "sum_generic(&other)"),
            source.replace("centroid={centroid:?}", "centroid={centroid}"),
            source.replace("cluster.points.len()", "xs.len()"),
        ] {
            assert!(lower_rust_entry(&path, &changed).is_err());
        }
    }

    #[test]
    fn source_limit_is_data_in_ir() {
        let (path, source) = fixture();
        let changed = source.replace("from_prime_grid(200)", "from_prime_grid(20)");
        let ir = lower_rust_entry(&path, &changed).unwrap();
        assert_eq!(
            ir.computations[0].operation,
            EntryOperation::PrimeGrid { limit: 20 }
        );
    }
}

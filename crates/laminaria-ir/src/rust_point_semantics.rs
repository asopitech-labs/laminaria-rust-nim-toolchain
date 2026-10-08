//! Source-derived semantics for a two-dimensional integer record and its
//! constructor/distance method. `syn` supplies syntax; this module checks the
//! supported meaning and rejects a changed source body before emitting IR.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use syn::spanned::Spanned;
use syn::{Expr, FnArg, ImplItem, Item, Pat, ReturnType, Stmt, Type};

use crate::rust_frontend::to_source_position;
use crate::rust_generic_fold::OverflowPolicy;
use crate::types::{Provenance, SourceLanguage, SourceSpan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointSemanticsIr {
    pub struct_name: String,
    /// Declaration order is retained for consumers that lay out this record.
    pub fields: Vec<String>,
    pub constructor_parameters: Vec<String>,
    /// Field name and the parameter that initializes it, in source order.
    pub constructor_fields: Vec<(String, String)>,
    pub distance: PointExpr,
    pub provenance: Provenance,
}

type ConstructorMapping = (Vec<String>, Vec<(String, String)>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointExpr {
    Field {
        receiver: PointReceiver,
        name: String,
        provenance: Provenance,
    },
    Subtract(Box<PointExpr>, Box<PointExpr>, Provenance),
    Absolute(Box<PointExpr>, Provenance),
    Add(Box<PointExpr>, Box<PointExpr>, Provenance),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointReceiver {
    SelfValue,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointValue {
    pub struct_name: String,
    pub fields: BTreeMap<String, i64>,
}

impl PointValue {
    pub fn field(&self, name: &str) -> Option<i64> {
        self.fields.get(name).copied()
    }
}

#[derive(Debug)]
pub enum PointSemanticsError {
    Parse(syn::Error),
    MissingStruct(String),
    AmbiguousStruct(String),
    UnsupportedShape(String),
    WrongArgumentCount { expected: usize, actual: usize },
    WrongPointType(String),
    MissingField(String),
    ArithmeticOverflow,
}

impl std::fmt::Display for PointSemanticsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid Rust source: {error}"),
            Self::MissingStruct(name) => write!(f, "struct {name:?} is absent"),
            Self::AmbiguousStruct(name) => write!(f, "struct {name:?} is ambiguous"),
            Self::UnsupportedShape(detail) => write!(f, "unsupported point semantics: {detail}"),
            Self::WrongArgumentCount { expected, actual } => {
                write!(
                    f,
                    "constructor expects {expected} arguments, received {actual}"
                )
            }
            Self::WrongPointType(name) => write!(f, "expected a point value of type {name:?}"),
            Self::MissingField(name) => write!(f, "point value lacks field {name:?}"),
            Self::ArithmeticOverflow => write!(f, "point arithmetic overflowed"),
        }
    }
}

impl std::error::Error for PointSemanticsError {}

fn provenance(source_file: &Path, span: proc_macro2::Span) -> Provenance {
    Provenance {
        source_file: source_file.to_path_buf(),
        span: SourceSpan {
            start: to_source_position(span.start()),
            end: to_source_position(span.end()),
        },
        language: SourceLanguage::Rust,
    }
}

fn supported_attrs(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .all(|attribute| attribute.path().is_ident("doc"))
}

fn type_name(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.is_ident(name))
}

fn bare_ident(pattern: &Pat) -> Option<String> {
    let Pat::Ident(ident) = pattern else {
        return None;
    };
    if !ident.attrs.is_empty()
        || ident.by_ref.is_some()
        || ident.mutability.is_some()
        || ident.subpat.is_some()
    {
        return None;
    }
    Some(ident.ident.to_string())
}

fn plain_expr(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(paren) if paren.attrs.is_empty() => plain_expr(&paren.expr),
        Expr::Group(group) if group.attrs.is_empty() => plain_expr(&group.expr),
        other => other,
    }
}

fn expression_path(expr: &Expr, expected: &str) -> bool {
    matches!(plain_expr(expr), Expr::Path(path) if path.attrs.is_empty() && path.qself.is_none() && path.path.is_ident(expected))
}

/// Parse one named record from the actual provider source. The supported
/// subset is a named-field `i64` struct, an inherent `new` constructor that
/// assigns each field from a same-typed argument, and an inherent
/// `manhattan_distance(&self, other: &Struct) -> i64` expression made from
/// field reads, subtraction, `abs`, and addition. Unsupported changes fail
/// before an IR value is published.
pub fn lower_point_semantics(
    source_file: &Path,
    source_text: &str,
    struct_name: &str,
) -> Result<PointSemanticsIr, PointSemanticsError> {
    let file = syn::parse_file(source_text).map_err(PointSemanticsError::Parse)?;
    if !supported_attrs(&file.attrs) {
        return Err(PointSemanticsError::UnsupportedShape(
            "crate attributes".into(),
        ));
    }
    let structs: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(item) if item.ident == struct_name => Some(item),
            _ => None,
        })
        .collect();
    let point = match structs.as_slice() {
        [] => return Err(PointSemanticsError::MissingStruct(struct_name.into())),
        [point] => point,
        _ => return Err(PointSemanticsError::AmbiguousStruct(struct_name.into())),
    };
    if !point.generics.params.is_empty()
        || point.generics.where_clause.is_some()
        || !point
            .attrs
            .iter()
            .all(|attr| attr.path().is_ident("doc") || accepted_builtin_derives(attr))
    {
        return Err(PointSemanticsError::UnsupportedShape(
            "struct declaration".into(),
        ));
    }
    let syn::Fields::Named(named) = &point.fields else {
        return Err(PointSemanticsError::UnsupportedShape(
            "unnamed fields".into(),
        ));
    };
    if named.named.len() != 2 {
        return Err(PointSemanticsError::UnsupportedShape(
            "expected two i64 fields".into(),
        ));
    }
    let mut fields = Vec::new();
    for field in &named.named {
        if !supported_attrs(&field.attrs) || !type_name(&field.ty, "i64") {
            return Err(PointSemanticsError::UnsupportedShape(
                "field type or attribute".into(),
            ));
        }
        fields.push(field.ident.as_ref().expect("named field").to_string());
    }
    let field_set: BTreeSet<_> = fields.iter().cloned().collect();
    if field_set.len() != fields.len() {
        return Err(PointSemanticsError::UnsupportedShape(
            "duplicate fields".into(),
        ));
    }

    let impls: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Impl(item) if type_name(&item.self_ty, struct_name) && item.trait_.is_none() => {
                Some(item)
            }
            _ => None,
        })
        .collect();
    let [implementation] = impls.as_slice() else {
        return Err(PointSemanticsError::UnsupportedShape(
            "expected one inherent impl".into(),
        ));
    };
    if !supported_attrs(&implementation.attrs)
        || implementation.unsafety.is_some()
        || !implementation.generics.params.is_empty()
        || implementation.generics.where_clause.is_some()
    {
        return Err(PointSemanticsError::UnsupportedShape(
            "inherent impl declaration".into(),
        ));
    }
    let constructors: Vec<_> = implementation
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(method) if method.sig.ident == "new" => Some(method),
            _ => None,
        })
        .collect();
    let distances: Vec<_> = implementation
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(method) if method.sig.ident == "manhattan_distance" => Some(method),
            _ => None,
        })
        .collect();
    let ([constructor], [distance]) = (constructors.as_slice(), distances.as_slice()) else {
        return Err(PointSemanticsError::UnsupportedShape(
            "constructor or distance method is absent or ambiguous".into(),
        ));
    };
    let (parameters, assignments) = lower_constructor(constructor, &field_set, struct_name)?;
    let distance = lower_distance(distance, source_file, &field_set, struct_name)?;
    Ok(PointSemanticsIr {
        struct_name: struct_name.into(),
        fields,
        constructor_parameters: parameters,
        constructor_fields: assignments,
        distance,
        provenance: provenance(source_file, point.span()),
    })
}

fn accepted_builtin_derives(attr: &syn::Attribute) -> bool {
    if !attr.path().is_ident("derive") {
        return false;
    }
    attr.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        .map(|paths| {
            !paths.is_empty()
                && paths.iter().all(|path| {
                    path.get_ident().is_some_and(|ident| {
                        matches!(
                            ident.to_string().as_str(),
                            "Debug" | "Clone" | "Copy" | "PartialEq" | "Eq"
                        )
                    })
                })
        })
        .unwrap_or(false)
}

fn plain_method(method: &syn::ImplItemFn) -> bool {
    supported_attrs(&method.attrs)
        && method.sig.asyncness.is_none()
        && method.sig.constness.is_none()
        && method.sig.unsafety.is_none()
        && method.sig.abi.is_none()
        && method.sig.generics.params.is_empty()
        && method.sig.generics.where_clause.is_none()
}

fn lower_constructor(
    method: &syn::ImplItemFn,
    fields: &BTreeSet<String>,
    struct_name: &str,
) -> Result<ConstructorMapping, PointSemanticsError> {
    let fail = || PointSemanticsError::UnsupportedShape("new signature or body".into());
    if !plain_method(method)
        || !matches!(&method.sig.output, ReturnType::Type(_, ty) if type_name(ty, "Self") || type_name(ty, struct_name))
    {
        return Err(fail());
    }
    let mut parameters = Vec::new();
    for input in &method.sig.inputs {
        let FnArg::Typed(input) = input else {
            return Err(fail());
        };
        if !supported_attrs(&input.attrs) || !type_name(&input.ty, "i64") {
            return Err(fail());
        }
        parameters.push(bare_ident(&input.pat).ok_or_else(fail)?);
    }
    if parameters.len() != fields.len()
        || parameters.iter().collect::<BTreeSet<_>>().len() != parameters.len()
    {
        return Err(fail());
    }
    let [Stmt::Expr(body, None)] = method.block.stmts.as_slice() else {
        return Err(fail());
    };
    let Expr::Struct(record) = plain_expr(body) else {
        return Err(fail());
    };
    if !record.attrs.is_empty()
        || !record.path.is_ident("Self") && !record.path.is_ident(struct_name)
        || record.rest.is_some()
        || record.fields.len() != fields.len()
    {
        return Err(fail());
    }
    let mut assignments = Vec::new();
    for field in &record.fields {
        if !field.attrs.is_empty() {
            return Err(fail());
        }
        let syn::Member::Named(member) = &field.member else {
            return Err(fail());
        };
        let field_name = member.to_string();
        let Expr::Path(value) = plain_expr(&field.expr) else {
            return Err(fail());
        };
        if !value.attrs.is_empty() || value.qself.is_some() {
            return Err(fail());
        }
        let Some(parameter) = value.path.get_ident().map(ToString::to_string) else {
            return Err(fail());
        };
        if !fields.contains(&field_name) || !parameters.contains(&parameter) {
            return Err(fail());
        }
        assignments.push((field_name, parameter));
    }
    if assignments
        .iter()
        .map(|(name, _)| name)
        .collect::<BTreeSet<_>>()
        .len()
        != fields.len()
    {
        return Err(fail());
    }
    Ok((parameters, assignments))
}

fn lower_distance(
    method: &syn::ImplItemFn,
    source_file: &Path,
    fields: &BTreeSet<String>,
    struct_name: &str,
) -> Result<PointExpr, PointSemanticsError> {
    let fail =
        || PointSemanticsError::UnsupportedShape("manhattan_distance signature or body".into());
    if !plain_method(method)
        || !matches!(&method.sig.output, ReturnType::Type(_, ty) if type_name(ty, "i64"))
    {
        return Err(fail());
    }
    let [FnArg::Receiver(receiver), FnArg::Typed(other)] =
        method.sig.inputs.iter().collect::<Vec<_>>().as_slice()
    else {
        return Err(fail());
    };
    if !supported_attrs(&receiver.attrs)
        || receiver.reference.is_none()
        || receiver.mutability.is_some()
        || receiver.colon_token.is_some()
        || !supported_attrs(&other.attrs)
        || bare_ident(&other.pat).as_deref() != Some("other")
    {
        return Err(fail());
    }
    let Type::Reference(other_type) = other.ty.as_ref() else {
        return Err(fail());
    };
    if other_type.mutability.is_some() || !type_name(&other_type.elem, struct_name) {
        return Err(fail());
    }
    let [Stmt::Expr(body, None)] = method.block.stmts.as_slice() else {
        return Err(fail());
    };
    lower_expr(body, source_file, fields).ok_or_else(fail)
}

fn lower_expr(expr: &Expr, source_file: &Path, fields: &BTreeSet<String>) -> Option<PointExpr> {
    let expr = plain_expr(expr);
    match expr {
        Expr::Field(field) if field.attrs.is_empty() => {
            let syn::Member::Named(name) = &field.member else {
                return None;
            };
            if !fields.contains(&name.to_string()) {
                return None;
            }
            let receiver = if expression_path(&field.base, "self") {
                PointReceiver::SelfValue
            } else if expression_path(&field.base, "other") {
                PointReceiver::Other
            } else {
                return None;
            };
            Some(PointExpr::Field {
                receiver,
                name: name.to_string(),
                provenance: provenance(source_file, field.span()),
            })
        }
        Expr::Binary(binary) if binary.attrs.is_empty() => {
            let left = Box::new(lower_expr(&binary.left, source_file, fields)?);
            let right = Box::new(lower_expr(&binary.right, source_file, fields)?);
            let at = provenance(source_file, binary.span());
            match binary.op {
                syn::BinOp::Sub(_) => Some(PointExpr::Subtract(left, right, at)),
                syn::BinOp::Add(_) => Some(PointExpr::Add(left, right, at)),
                _ => None,
            }
        }
        Expr::MethodCall(call)
            if call.attrs.is_empty()
                && call.method == "abs"
                && call.turbofish.is_none()
                && call.args.is_empty() =>
        {
            Some(PointExpr::Absolute(
                Box::new(lower_expr(&call.receiver, source_file, fields)?),
                provenance(source_file, call.span()),
            ))
        }
        _ => None,
    }
}

impl PointSemanticsIr {
    /// Execute the lowered constructor using source-declared parameter order.
    pub fn construct(
        &self,
        args: &[i64],
        _overflow: OverflowPolicy,
    ) -> Result<PointValue, PointSemanticsError> {
        if args.len() != self.constructor_parameters.len() {
            return Err(PointSemanticsError::WrongArgumentCount {
                expected: self.constructor_parameters.len(),
                actual: args.len(),
            });
        }
        let arguments: BTreeMap<_, _> = self
            .constructor_parameters
            .iter()
            .cloned()
            .zip(args.iter().copied())
            .collect();
        let fields = self
            .constructor_fields
            .iter()
            .map(|(field, parameter)| (field.clone(), arguments[parameter]))
            .collect();
        Ok(PointValue {
            struct_name: self.struct_name.clone(),
            fields,
        })
    }

    /// Execute the source-derived expression under the selected Cargo
    /// profile's integer overflow semantics.
    pub fn manhattan_distance(
        &self,
        left: &PointValue,
        right: &PointValue,
        overflow: OverflowPolicy,
    ) -> Result<i64, PointSemanticsError> {
        if left.struct_name != self.struct_name || right.struct_name != self.struct_name {
            return Err(PointSemanticsError::WrongPointType(
                self.struct_name.clone(),
            ));
        }
        self.eval(&self.distance, left, right, overflow)
    }

    fn eval(
        &self,
        expr: &PointExpr,
        left: &PointValue,
        right: &PointValue,
        overflow: OverflowPolicy,
    ) -> Result<i64, PointSemanticsError> {
        match expr {
            PointExpr::Field { receiver, name, .. } => {
                let value = match receiver {
                    PointReceiver::SelfValue => left,
                    PointReceiver::Other => right,
                };
                value
                    .field(name)
                    .ok_or_else(|| PointSemanticsError::MissingField(name.clone()))
            }
            PointExpr::Subtract(a, b, _) => {
                let a = self.eval(a, left, right, overflow)?;
                let b = self.eval(b, left, right, overflow)?;
                match overflow {
                    OverflowPolicy::Checked => a
                        .checked_sub(b)
                        .ok_or(PointSemanticsError::ArithmeticOverflow),
                    OverflowPolicy::Wrapping => Ok(a.wrapping_sub(b)),
                }
            }
            PointExpr::Absolute(value, _) => {
                let value = self.eval(value, left, right, overflow)?;
                match overflow {
                    OverflowPolicy::Checked => value
                        .checked_abs()
                        .ok_or(PointSemanticsError::ArithmeticOverflow),
                    OverflowPolicy::Wrapping => Ok(value.wrapping_abs()),
                }
            }
            PointExpr::Add(a, b, _) => {
                let a = self.eval(a, left, right, overflow)?;
                let b = self.eval(b, left, right, overflow)?;
                match overflow {
                    OverflowPolicy::Checked => a
                        .checked_add(b)
                        .ok_or(PointSemanticsError::ArithmeticOverflow),
                    OverflowPolicy::Wrapping => Ok(a.wrapping_add(b)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_point_lowers_and_matches_independent_reference() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rust-heavy-workspace/crates/fixture-core/src/lib.rs");
        let source = std::fs::read_to_string(&path).unwrap();
        let ir = lower_point_semantics(&path, &source, "Point").unwrap();
        assert_eq!(ir.fields, ["x", "y"]);
        assert!(ir.provenance.span.start.line > 1);
        for (a, b) in [([0, 0], [3, 4]), ([17, -9], [-2, 5]), ([-8, 12], [-8, 12])] {
            let left = ir.construct(&a, OverflowPolicy::Checked).unwrap();
            let right = ir.construct(&b, OverflowPolicy::Checked).unwrap();
            let owned = ir
                .manhattan_distance(&left, &right, OverflowPolicy::Checked)
                .unwrap();
            let reference = (a[0] - b[0]).abs() + (a[1] - b[1]).abs();
            assert_eq!(owned, reference);
            assert_eq!(
                owned,
                ir.manhattan_distance(&right, &left, OverflowPolicy::Checked)
                    .unwrap()
            );
        }
    }

    #[test]
    fn changed_distance_body_fails_closed() {
        let source = "struct P { x: i64, y: i64 } impl P { fn new(x:i64,y:i64)->Self { Self{x,y} } fn manhattan_distance(&self, other:&P)->i64 { (self.x - other.x).abs() * (self.y - other.y).abs() } }";
        assert!(matches!(
            lower_point_semantics(Path::new("p.rs"), source, "P"),
            Err(PointSemanticsError::UnsupportedShape(_))
        ));
    }

    #[test]
    fn constructor_mapping_is_taken_from_source() {
        let source = "struct P { latitude: i64, longitude: i64 } impl P { fn new(a:i64,b:i64)->Self { Self { latitude: b, longitude: a } } fn manhattan_distance(&self, other:&P)->i64 { (self.latitude - other.latitude).abs() + (self.longitude - other.longitude).abs() } }";
        let ir = lower_point_semantics(Path::new("p.rs"), source, "P").unwrap();
        let value = ir.construct(&[11, 29], OverflowPolicy::Checked).unwrap();
        assert_eq!(value.field("latitude"), Some(29));
        assert_eq!(value.field("longitude"), Some(11));
    }

    #[test]
    fn unsupported_constructor_assignment_fails_closed() {
        let source = "struct P { x: i64, y: i64 } impl P { fn new(x:i64,y:i64)->Self { Self { x: x + 1, y } } fn manhattan_distance(&self, other:&P)->i64 { (self.x - other.x).abs() + (self.y - other.y).abs() } }";
        assert!(matches!(
            lower_point_semantics(Path::new("p.rs"), source, "P"),
            Err(PointSemanticsError::UnsupportedShape(_))
        ));
    }

    #[test]
    fn checked_and_wrapping_arithmetic_are_explicit() {
        let source = "struct P { x: i64, y: i64 } impl P { fn new(x:i64,y:i64)->Self { Self{x,y} } fn manhattan_distance(&self, other:&P)->i64 { (self.x - other.x).abs() + (self.y - other.y).abs() } }";
        let ir = lower_point_semantics(Path::new("p.rs"), source, "P").unwrap();
        let a = ir
            .construct(&[i64::MIN, 0], OverflowPolicy::Checked)
            .unwrap();
        let b = ir.construct(&[0, 0], OverflowPolicy::Checked).unwrap();
        assert!(matches!(
            ir.manhattan_distance(&a, &b, OverflowPolicy::Checked),
            Err(PointSemanticsError::ArithmeticOverflow)
        ));
        assert_eq!(
            ir.manhattan_distance(&a, &b, OverflowPolicy::Wrapping)
                .unwrap(),
            i64::MIN
        );
    }
}

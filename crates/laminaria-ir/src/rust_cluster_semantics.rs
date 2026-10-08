//! Source-derived semantics for a cluster constructed from prime pairs.
//! The Rust iterator chains are checked before this module publishes IR;
//! execution consumes the lowered prime and point semantics.

use std::path::Path;

use quote::ToTokens;
use syn::spanned::Spanned;
use syn::{Expr, FnArg, ImplItem, Item, Pat, ReturnType, Stmt, Type};

use crate::rust_frontend::to_source_position;
use crate::rust_generic_fold::OverflowPolicy;
use crate::rust_point_semantics::{PointSemanticsError, PointSemanticsIr, PointValue};
use crate::rust_prime_semantics::{PrimeSemanticError, PrimeSemanticsIr};
use crate::types::{Provenance, SourceLanguage, SourceSpan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterSemanticsIr {
    pub struct_name: String,
    pub point_name: String,
    pub prime_function_name: String,
    pub points_field: String,
    pub zip_skip: usize,
    pub perimeter_window: usize,
    pub perimeter_first: usize,
    pub perimeter_second: usize,
    pub centroid_zero: i64,
    pub centroid_fields: [String; 2],
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterValue {
    pub points: Vec<PointValue>,
}

#[derive(Debug)]
pub enum ClusterSemanticsError {
    Parse(syn::Error),
    MissingStruct(String),
    UnsupportedShape(String),
    Prime(PrimeSemanticError),
    Point(PointSemanticsError),
    ArithmeticOverflow,
    WrongPointType(String),
    MissingPointField(String),
}

impl std::fmt::Display for ClusterSemanticsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid Rust source: {error}"),
            Self::MissingStruct(name) => write!(f, "struct {name:?} is absent"),
            Self::UnsupportedShape(detail) => write!(f, "unsupported cluster semantics: {detail}"),
            Self::Prime(error) => write!(f, "prime semantics: {error}"),
            Self::Point(error) => write!(f, "point semantics: {error}"),
            Self::ArithmeticOverflow => write!(f, "cluster arithmetic overflowed"),
            Self::WrongPointType(name) => write!(f, "expected point type {name:?}"),
            Self::MissingPointField(name) => write!(f, "point value lacks field {name:?}"),
        }
    }
}

impl std::error::Error for ClusterSemanticsError {}

impl From<PrimeSemanticError> for ClusterSemanticsError {
    fn from(value: PrimeSemanticError) -> Self {
        Self::Prime(value)
    }
}

impl From<PointSemanticsError> for ClusterSemanticsError {
    fn from(value: PointSemanticsError) -> Self {
        Self::Point(value)
    }
}

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

fn plain(value: &Expr) -> &Expr {
    match value {
        Expr::Paren(inner) if inner.attrs.is_empty() => plain(&inner.expr),
        Expr::Group(inner) if inner.attrs.is_empty() => plain(&inner.expr),
        other => other,
    }
}

fn path(value: &Expr, name: &str) -> bool {
    matches!(plain(value), Expr::Path(value)
        if value.attrs.is_empty() && value.qself.is_none() && value.path.is_ident(name))
}

fn field(value: &Expr, receiver: &str) -> Option<String> {
    let Expr::Field(value) = plain(value) else {
        return None;
    };
    if !value.attrs.is_empty() || !path(&value.base, receiver) {
        return None;
    }
    match &value.member {
        syn::Member::Named(name) => Some(name.to_string()),
        _ => None,
    }
}

fn integer(value: &Expr) -> Option<u64> {
    let Expr::Lit(value) = plain(value) else {
        return None;
    };
    let syn::Lit::Int(number) = &value.lit else {
        return None;
    };
    (value.attrs.is_empty() && matches!(number.suffix(), "" | "u64" | "usize" | "i64"))
        .then(|| number.base10_parse().ok())
        .flatten()
}

fn method<'a>(value: &'a Expr, name: &str, args: usize) -> Option<&'a syn::ExprMethodCall> {
    let Expr::MethodCall(value) = plain(value) else {
        return None;
    };
    (value.attrs.is_empty()
        && value.method == name
        && value.turbofish.is_none()
        && value.args.len() == args)
        .then_some(value)
}

fn call<'a>(value: &'a Expr, name: &str, args: usize) -> Option<&'a syn::ExprCall> {
    let Expr::Call(value) = plain(value) else {
        return None;
    };
    (value.attrs.is_empty() && path(&value.func, name) && value.args.len() == args).then_some(value)
}

fn named_call<'a>(value: &'a Expr, ty: &str, name: &str, args: usize) -> Option<&'a syn::ExprCall> {
    let Expr::Call(value) = plain(value) else {
        return None;
    };
    let Expr::Path(function) = plain(&value.func) else {
        return None;
    };
    let segments: Vec<_> = function.path.segments.iter().collect();
    (value.attrs.is_empty()
        && function.attrs.is_empty()
        && function.qself.is_none()
        && segments.len() == 2
        && segments[0].ident == ty
        && segments[1].ident == name
        && segments
            .iter()
            .all(|part| matches!(part.arguments, syn::PathArguments::None))
        && value.args.len() == args)
        .then_some(value)
}

fn bare(pattern: &Pat) -> Option<String> {
    let Pat::Ident(value) = pattern else {
        return None;
    };
    (value.attrs.is_empty()
        && value.by_ref.is_none()
        && value.mutability.is_none()
        && value.subpat.is_none())
    .then(|| value.ident.to_string())
}

fn type_name(value: &Type, name: &str) -> bool {
    matches!(value, Type::Path(value) if value.qself.is_none() && value.path.is_ident(name))
}

fn vec_type(value: &Type, item_name: &str) -> bool {
    let Type::Path(value) = value else {
        return false;
    };
    let segments: Vec<_> = value.path.segments.iter().collect();
    let [segment] = segments.as_slice() else {
        return false;
    };
    value.qself.is_none()
        && segment.ident == "Vec"
        && matches!(&segment.arguments, syn::PathArguments::AngleBracketed(args)
            if args.args.len() == 1 && matches!(args.args.first(), Some(syn::GenericArgument::Type(ty)) if type_name(ty, item_name)))
}

fn option_point(value: &Type, point_name: &str) -> bool {
    let Type::Path(value) = value else {
        return false;
    };
    let segments: Vec<_> = value.path.segments.iter().collect();
    let [segment] = segments.as_slice() else {
        return false;
    };
    value.qself.is_none()
        && segment.ident == "Option"
        && matches!(&segment.arguments, syn::PathArguments::AngleBracketed(args)
            if args.args.len() == 1 && matches!(args.args.first(), Some(syn::GenericArgument::Type(ty)) if type_name(ty, point_name)))
}

fn plain_method(method: &syn::ImplItemFn) -> bool {
    method.attrs.iter().all(|attr| attr.path().is_ident("doc"))
        && method.sig.asyncness.is_none()
        && method.sig.constness.is_none()
        && method.sig.unsafety.is_none()
        && method.sig.abi.is_none()
        && method.sig.generics.params.is_empty()
        && method.sig.generics.where_clause.is_none()
        && method.sig.variadic.is_none()
}

fn local<'a>(statement: &'a Stmt, name: Option<&str>) -> Option<(&'a Pat, &'a Expr)> {
    let Stmt::Local(value) = statement else {
        return None;
    };
    if !value.attrs.is_empty() {
        return None;
    }
    let init = value.init.as_ref()?;
    if init.diverge.is_some() {
        return None;
    }
    if let Some(name) = name {
        if bare(&value.pat).as_deref() != Some(name) {
            return None;
        }
    }
    Some((&value.pat, &init.expr))
}

/// Parse the actual declaration and all three methods. The accepted source
/// subset is intentionally narrow; no Rust parser result alone proves the
/// iterator and method-call meanings.
pub fn lower_cluster_semantics(
    source_file: &Path,
    source_text: &str,
    cluster_name: &str,
    point_name: &str,
    prime_function_name: &str,
) -> Result<ClusterSemanticsIr, ClusterSemanticsError> {
    let fail = |detail: &str| ClusterSemanticsError::UnsupportedShape(detail.into());
    let file = syn::parse_file(source_text).map_err(ClusterSemanticsError::Parse)?;
    if !file.attrs.iter().all(|attr| attr.path().is_ident("doc")) {
        return Err(fail("crate attributes"));
    }
    // The only import this subset resolves is the declared core provider.
    let imports: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Use(value) => Some(value),
            _ => None,
        })
        .collect();
    if imports.len() != 1
        || imports[0].to_token_stream().to_string()
            != format!("use fixture_core :: {{ {prime_function_name} , {point_name} }} ;")
    {
        return Err(fail("core provider import"));
    }
    let structs: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(value) if value.ident == cluster_name => Some(value),
            _ => None,
        })
        .collect();
    let [record] = structs.as_slice() else {
        return Err(ClusterSemanticsError::MissingStruct(cluster_name.into()));
    };
    if !record.attrs.iter().all(|attr| attr.path().is_ident("doc"))
        || !record.generics.params.is_empty()
        || record.generics.where_clause.is_some()
    {
        return Err(fail("cluster declaration"));
    }
    let syn::Fields::Named(fields) = &record.fields else {
        return Err(fail("cluster fields"));
    };
    let point_fields: Vec<_> = fields.named.iter().collect();
    let [point_field] = point_fields.as_slice() else {
        return Err(fail("cluster fields"));
    };
    if !point_field.attrs.is_empty() || !vec_type(&point_field.ty, point_name) {
        return Err(fail("cluster point field"));
    }
    let points_field = point_field.ident.as_ref().expect("named field").to_string();
    let impls: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Impl(value)
                if value.trait_.is_none() && type_name(&value.self_ty, cluster_name) =>
            {
                Some(value)
            }
            _ => None,
        })
        .collect();
    let [implementation] = impls.as_slice() else {
        return Err(fail("cluster inherent impl"));
    };
    if !implementation.attrs.is_empty()
        || implementation.unsafety.is_some()
        || !implementation.generics.params.is_empty()
        || implementation.generics.where_clause.is_some()
    {
        return Err(fail("cluster inherent impl"));
    }
    let find_method = |name: &str| -> Result<&syn::ImplItemFn, ClusterSemanticsError> {
        let methods: Vec<_> = implementation
            .items
            .iter()
            .filter_map(|item| match item {
                ImplItem::Fn(value) if value.sig.ident == name => Some(value),
                _ => None,
            })
            .collect();
        match methods.as_slice() {
            [method] => Ok(method),
            _ => Err(fail(name)),
        }
    };
    let constructor = find_method("from_prime_grid")?;
    let perimeter = find_method("total_perimeter")?;
    let centroid = find_method("centroid")?;
    let zip_skip = lower_constructor(constructor, &points_field, point_name, prime_function_name)
        .ok_or_else(|| fail("from_prime_grid"))?;
    let (perimeter_window, perimeter_first, perimeter_second) =
        lower_perimeter(perimeter, &points_field).ok_or_else(|| fail("total_perimeter"))?;
    let (centroid_zero, centroid_fields) =
        lower_centroid(centroid, &points_field, point_name).ok_or_else(|| fail("centroid"))?;
    Ok(ClusterSemanticsIr {
        struct_name: cluster_name.into(),
        point_name: point_name.into(),
        prime_function_name: prime_function_name.into(),
        points_field,
        zip_skip,
        perimeter_window,
        perimeter_first,
        perimeter_second,
        centroid_zero,
        centroid_fields,
        provenance: provenance(source_file, record.span()),
    })
}

fn checked_receiver(method: &syn::ImplItemFn, point_name: Option<&str>) -> bool {
    if !plain_method(method) {
        return false;
    }
    let [FnArg::Receiver(receiver)] = method.sig.inputs.iter().collect::<Vec<_>>().as_slice()
    else {
        return false;
    };
    if !receiver.attrs.is_empty()
        || receiver.reference.is_none()
        || receiver.mutability.is_some()
        || receiver.colon_token.is_some()
    {
        return false;
    }
    match (point_name, &method.sig.output) {
        (None, ReturnType::Type(_, result)) => type_name(result, "i64"),
        (Some(name), ReturnType::Type(_, result)) => option_point(result, name),
        _ => false,
    }
}

fn tuple_pair(pattern: &Pat) -> Option<(String, String)> {
    let Pat::Tuple(tuple) = pattern else {
        return None;
    };
    let tuple_elems: Vec<_> = tuple.elems.iter().collect();
    let [left, right] = tuple_elems.as_slice() else {
        return None;
    };
    let (Pat::Reference(left), Pat::Reference(right)) = (left, right) else {
        return None;
    };
    if !tuple.attrs.is_empty()
        || !left.attrs.is_empty()
        || !right.attrs.is_empty()
        || left.mutability.is_some()
        || right.mutability.is_some()
    {
        return None;
    }
    Some((bare(&left.pat)?, bare(&right.pat)?))
}

fn cast_ident(value: &Expr, name: &str) -> bool {
    matches!(plain(value), Expr::Cast(cast)
        if cast.attrs.is_empty() && path(&cast.expr, name) && type_name(&cast.ty, "i64"))
}

fn lower_constructor(
    method: &syn::ImplItemFn,
    points_field: &str,
    point_name: &str,
    prime_name: &str,
) -> Option<usize> {
    if !plain_method(method)
        || !matches!(&method.sig.output, ReturnType::Type(_, result) if type_name(result, "Self"))
    {
        return None;
    }
    let [FnArg::Typed(limit)] = method.sig.inputs.iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    let limit_name = bare(&limit.pat)?;
    if !limit.attrs.is_empty() || !type_name(&limit.ty, "u64") {
        return None;
    }
    let [prime_stmt, points_stmt, Stmt::Expr(result, None)] = method.block.stmts.as_slice() else {
        return None;
    };
    let (prime_pattern, prime_expr) = local(prime_stmt, None)?;
    let primes_name = bare(prime_pattern)?;
    let prime_call = call(prime_expr, prime_name, 1)?;
    if !path(&prime_call.args[0], &limit_name) {
        return None;
    }
    let (points_pattern, points_expr) = local(points_stmt, None)?;
    let points_name = bare(points_pattern)?;
    let collect = method_call_chain(points_expr, "collect", 0)?;
    let map = method_call_chain(&collect.receiver, "map", 1)?;
    let zip = method_call_chain(&map.receiver, "zip", 1)?;
    let first_iter = method_call_chain(&zip.receiver, "iter", 0)?;
    if !path(&first_iter.receiver, &primes_name) {
        return None;
    }
    let second_iter = method_call_chain(&zip.args[0], "skip", 1)?;
    let second_source = method_call_chain(&second_iter.receiver, "iter", 0)?;
    if !path(&second_source.receiver, &primes_name) {
        return None;
    }
    let zip_skip = usize::try_from(integer(&second_iter.args[0])?).ok()?;
    if zip_skip == 0 {
        return None;
    }
    let Expr::Closure(closure) = plain(&map.args[0]) else {
        return None;
    };
    if !plain_closure(closure) || closure.inputs.len() != 1 {
        return None;
    }
    let (left_name, right_name) = tuple_pair(closure.inputs.first()?)?;
    let constructor = named_call(&closure.body, point_name, "new", 2)?;
    if !cast_ident(&constructor.args[0], &left_name)
        || !cast_ident(&constructor.args[1], &right_name)
    {
        return None;
    }
    let Expr::Struct(record) = plain(result) else {
        return None;
    };
    if !record.attrs.is_empty()
        || !record.path.is_ident("Self")
        || record.rest.is_some()
        || record.fields.len() != 1
    {
        return None;
    }
    let field = record.fields.first()?;
    if !field.attrs.is_empty()
        || !matches!(&field.member, syn::Member::Named(name) if name == points_field)
        || !path(&field.expr, &points_name)
    {
        return None;
    }
    Some(zip_skip)
}

fn method_call_chain<'a>(
    value: &'a Expr,
    name: &str,
    args: usize,
) -> Option<&'a syn::ExprMethodCall> {
    method(value, name, args)
}

fn plain_closure(closure: &syn::ExprClosure) -> bool {
    closure.attrs.is_empty()
        && closure.asyncness.is_none()
        && closure.movability.is_none()
        && closure.capture.is_none()
        && matches!(closure.output, ReturnType::Default)
}

fn indexed(value: &Expr, receiver: &str) -> Option<usize> {
    let Expr::Index(index) = plain(value) else {
        return None;
    };
    if !index.attrs.is_empty() || !path(&index.expr, receiver) {
        return None;
    }
    usize::try_from(integer(&index.index)?).ok()
}

fn lower_perimeter(method: &syn::ImplItemFn, points_field: &str) -> Option<(usize, usize, usize)> {
    if !checked_receiver(method, None) {
        return None;
    }
    let [Stmt::Expr(result, None)] = method.block.stmts.as_slice() else {
        return None;
    };
    let sum = method_call_chain(result, "sum", 0)?;
    let map = method_call_chain(&sum.receiver, "map", 1)?;
    let windows = method_call_chain(&map.receiver, "windows", 1)?;
    if field(&windows.receiver, "self")?.as_str() != points_field {
        return None;
    }
    let window_size = usize::try_from(integer(&windows.args[0])?).ok()?;
    let Expr::Closure(closure) = plain(&map.args[0]) else {
        return None;
    };
    if !plain_closure(closure) || closure.inputs.len() != 1 {
        return None;
    }
    let window_name = bare(closure.inputs.first()?)?;
    let distance = method_call_chain(&closure.body, "manhattan_distance", 1)?;
    let first = indexed(&distance.receiver, &window_name)?;
    let Expr::Reference(reference) = plain(&distance.args[0]) else {
        return None;
    };
    if !reference.attrs.is_empty() || reference.mutability.is_some() {
        return None;
    }
    let second = indexed(&reference.expr, &window_name)?;
    (window_size > 0 && first < window_size && second < window_size).then_some((
        window_size,
        first,
        second,
    ))
}

fn plain_pair(pattern: &Pat) -> Option<(String, String)> {
    let Pat::Tuple(tuple) = pattern else {
        return None;
    };
    let tuple_elems: Vec<_> = tuple.elems.iter().collect();
    let [first, second] = tuple_elems.as_slice() else {
        return None;
    };
    if !tuple.attrs.is_empty() {
        return None;
    }
    Some((bare(first)?, bare(second)?))
}

fn expr_pair(value: &Expr) -> Option<(&Expr, &Expr)> {
    let Expr::Tuple(tuple) = plain(value) else {
        return None;
    };
    let tuple_elems: Vec<_> = tuple.elems.iter().collect();
    let [first, second] = tuple_elems.as_slice() else {
        return None;
    };
    tuple.attrs.is_empty().then_some((first, second))
}

fn added_field(value: &Expr, accumulator: &str, point_var: &str) -> Option<String> {
    let Expr::Binary(add) = plain(value) else {
        return None;
    };
    if !add.attrs.is_empty()
        || !matches!(add.op, syn::BinOp::Add(_))
        || !path(&add.left, accumulator)
    {
        return None;
    }
    field(&add.right, point_var)
}

fn divided(value: &Expr, numerator: &str, denominator: &str) -> bool {
    matches!(plain(value), Expr::Binary(div)
        if div.attrs.is_empty() && matches!(div.op, syn::BinOp::Div(_))
        && path(&div.left, numerator) && path(&div.right, denominator))
}

fn lower_centroid(
    method: &syn::ImplItemFn,
    points_field: &str,
    point_name: &str,
) -> Option<(i64, [String; 2])> {
    if !checked_receiver(method, Some(point_name)) {
        return None;
    }
    let [Stmt::Expr(Expr::If(empty), None), fold_stmt, count_stmt, Stmt::Expr(result, None)] =
        method.block.stmts.as_slice()
    else {
        return None;
    };
    if !empty.attrs.is_empty() || empty.else_branch.is_some() {
        return None;
    }
    let is_empty = method_call_chain(&empty.cond, "is_empty", 0)?;
    if field(&is_empty.receiver, "self")?.as_str() != points_field {
        return None;
    }
    let [Stmt::Expr(Expr::Return(ret), Some(_))] = empty.then_branch.stmts.as_slice() else {
        return None;
    };
    if !ret.attrs.is_empty() || !ret.expr.as_deref().is_some_and(|value| path(value, "None")) {
        return None;
    }

    let (fold_pattern, fold_expr) = local(fold_stmt, None)?;
    let (sum_a, sum_b) = plain_pair(fold_pattern)?;
    if sum_a == sum_b {
        return None;
    }
    let fold = method_call_chain(fold_expr, "fold", 2)?;
    let iter = method_call_chain(&fold.receiver, "iter", 0)?;
    if field(&iter.receiver, "self")?.as_str() != points_field {
        return None;
    }
    let (zero_a, zero_b) = expr_pair(&fold.args[0])?;
    let zero = i64::try_from(integer(zero_a)?).ok()?;
    if integer(zero_b)? != zero as u64 {
        return None;
    }
    let Expr::Closure(closure) = plain(&fold.args[1]) else {
        return None;
    };
    if !plain_closure(closure) || closure.inputs.len() != 2 {
        return None;
    }
    let (acc_a, acc_b) = plain_pair(&closure.inputs[0])?;
    let point_var = bare(&closure.inputs[1])?;
    let (add_a, add_b) = expr_pair(&closure.body)?;
    let field_a = added_field(add_a, &acc_a, &point_var)?;
    let field_b = added_field(add_b, &acc_b, &point_var)?;
    if field_a == field_b {
        return None;
    }

    let (count_pattern, count_expr) = local(count_stmt, None)?;
    let count_name = bare(count_pattern)?;
    let Expr::Cast(count_cast) = plain(count_expr) else {
        return None;
    };
    if !count_cast.attrs.is_empty() || !type_name(&count_cast.ty, "i64") {
        return None;
    }
    let len = method_call_chain(&count_cast.expr, "len", 0)?;
    if field(&len.receiver, "self")?.as_str() != points_field {
        return None;
    }
    let some = call(result, "Some", 1)?;
    let constructor = named_call(&some.args[0], point_name, "new", 2)?;
    if !divided(&constructor.args[0], &sum_a, &count_name)
        || !divided(&constructor.args[1], &sum_b, &count_name)
    {
        return None;
    }
    Some((zero, [field_a, field_b]))
}

impl ClusterSemanticsIr {
    pub fn from_prime_grid(
        &self,
        limit: u64,
        prime: &PrimeSemanticsIr,
        point: &PointSemanticsIr,
        overflow: OverflowPolicy,
    ) -> Result<ClusterValue, ClusterSemanticsError> {
        if prime.sequence_name != self.prime_function_name || point.struct_name != self.point_name {
            return Err(ClusterSemanticsError::UnsupportedShape(
                "provider identity".into(),
            ));
        }
        let primes = prime.primes_up_to(limit)?;
        let mut points = Vec::new();
        for (left, right) in primes.iter().zip(primes.iter().skip(self.zip_skip)) {
            let left = *left as i64;
            let right = *right as i64;
            points.push(point.construct(&[left, right], overflow)?);
        }
        Ok(ClusterValue { points })
    }

    pub fn total_perimeter(
        &self,
        cluster: &ClusterValue,
        point: &PointSemanticsIr,
        overflow: OverflowPolicy,
    ) -> Result<i64, ClusterSemanticsError> {
        if point.struct_name != self.point_name {
            return Err(ClusterSemanticsError::WrongPointType(
                self.point_name.clone(),
            ));
        }
        let mut total = 0_i64;
        for window in cluster.points.windows(self.perimeter_window) {
            let distance = point.manhattan_distance(
                &window[self.perimeter_first],
                &window[self.perimeter_second],
                overflow,
            )?;
            total = match overflow {
                OverflowPolicy::Checked => total
                    .checked_add(distance)
                    .ok_or(ClusterSemanticsError::ArithmeticOverflow)?,
                OverflowPolicy::Wrapping => total.wrapping_add(distance),
            };
        }
        Ok(total)
    }

    pub fn centroid(
        &self,
        cluster: &ClusterValue,
        point: &PointSemanticsIr,
        overflow: OverflowPolicy,
    ) -> Result<Option<PointValue>, ClusterSemanticsError> {
        if point.struct_name != self.point_name {
            return Err(ClusterSemanticsError::WrongPointType(
                self.point_name.clone(),
            ));
        }
        if cluster.points.is_empty() {
            return Ok(None);
        }
        let mut sums = [self.centroid_zero; 2];
        for value in &cluster.points {
            if value.struct_name != self.point_name {
                return Err(ClusterSemanticsError::WrongPointType(
                    self.point_name.clone(),
                ));
            }
            for (index, field_name) in self.centroid_fields.iter().enumerate() {
                let field = value
                    .field(field_name)
                    .ok_or_else(|| ClusterSemanticsError::MissingPointField(field_name.clone()))?;
                sums[index] = match overflow {
                    OverflowPolicy::Checked => sums[index]
                        .checked_add(field)
                        .ok_or(ClusterSemanticsError::ArithmeticOverflow)?,
                    OverflowPolicy::Wrapping => sums[index].wrapping_add(field),
                };
            }
        }
        let count = cluster.points.len() as i64;
        let values = [sums[0] / count, sums[1] / count];
        Ok(Some(point.construct(&values, overflow)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_point_semantics::lower_point_semantics;
    use crate::rust_prime_semantics::lower_prime_semantics;

    fn source_ir() -> (ClusterSemanticsIr, PrimeSemanticsIr, PointSemanticsIr) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rust-heavy-workspace/crates");
        let core_path = root.join("fixture-core/src/lib.rs");
        let mid_path = root.join("fixture-mid/src/lib.rs");
        let core = std::fs::read_to_string(&core_path).unwrap();
        let mid = std::fs::read_to_string(&mid_path).unwrap();
        let prime = lower_prime_semantics(&core_path, &core, "is_prime", "primes_up_to").unwrap();
        let point = lower_point_semantics(&core_path, &core, "Point").unwrap();
        let cluster =
            lower_cluster_semantics(&mid_path, &mid, "Cluster", "Point", "primes_up_to").unwrap();
        (cluster, prime, point)
    }

    #[test]
    fn fixture_cluster_lowers_and_executes_across_owned_providers() {
        let (ir, prime, point) = source_ir();
        assert_eq!(ir.provenance.language, SourceLanguage::Rust);
        for limit in [0, 1, 2, 20, 200] {
            let result = ir
                .from_prime_grid(limit, &prime, &point, OverflowPolicy::Checked)
                .unwrap();
            let prime_values = prime.primes_up_to(limit).unwrap();
            assert_eq!(
                result.points.len(),
                prime_values.len().saturating_sub(ir.zip_skip)
            );
            let perimeter = ir
                .total_perimeter(&result, &point, OverflowPolicy::Checked)
                .unwrap();
            assert!(perimeter >= 0);
            let centroid = ir
                .centroid(&result, &point, OverflowPolicy::Checked)
                .unwrap();
            assert_eq!(centroid.is_some(), !result.points.is_empty());
        }
    }

    #[test]
    fn changed_iterator_semantics_fail_closed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rust-heavy-workspace/crates/fixture-mid/src/lib.rs");
        let source = std::fs::read_to_string(&root).unwrap();
        let changed = source.replacen(".windows(2)", ".chunks(2)", 1);
        assert!(matches!(
            lower_cluster_semantics(&root, &changed, "Cluster", "Point", "primes_up_to"),
            Err(ClusterSemanticsError::UnsupportedShape(_))
        ));
    }
}

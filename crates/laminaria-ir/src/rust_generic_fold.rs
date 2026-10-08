//! Owned semantic lowering for a narrow Rust generic iterator fold.
//!
//! This module recognizes the source semantics of `&[T]::iter().fold` with
//! `T: Copy + Add<Output=T> + Default` and a scalar `acc + element` body.
//! It rejects other signatures and bodies before publishing an IR value.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::{
    Expr, FnArg, GenericArgument, GenericParam, Item, Pat, PathArguments, Type, TypeParamBound,
};

use crate::rust_frontend::to_source_position;
use crate::rust_generic_demand::RustGenericInstance;
use crate::types::{Provenance, SourceLanguage, SourceSpan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    I32,
    I64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    Checked,
    Wrapping,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericFoldIr {
    pub package: String,
    pub function: String,
    pub scalar: ScalarType,
    pub overflow: OverflowPolicy,
    pub provenance: Provenance,
}

#[derive(Debug)]
pub enum GenericFoldError {
    Parse(syn::Error),
    MissingFunction(String),
    DuplicateFunction(String),
    UnsupportedScalar(String),
    UnsupportedSignature(String),
    UnsupportedBody(String),
    WidthMismatch,
    ArithmeticOverflow,
}

impl std::fmt::Display for GenericFoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid Rust source: {error}"),
            Self::MissingFunction(name) => write!(f, "generic function {name:?} is absent"),
            Self::DuplicateFunction(name) => write!(f, "generic function {name:?} is ambiguous"),
            Self::UnsupportedScalar(name) => write!(f, "unsupported scalar instance {name:?}"),
            Self::UnsupportedSignature(name) => {
                write!(f, "unsupported generic signature of {name:?}")
            }
            Self::UnsupportedBody(name) => write!(f, "unsupported generic body of {name:?}"),
            Self::WidthMismatch => write!(f, "input width does not match the lowered instance"),
            Self::ArithmeticOverflow => write!(f, "checked Rust addition overflowed"),
        }
    }
}

impl std::error::Error for GenericFoldError {}

/// Lowers one selected, concrete generic instance from its provider source.
/// The caller supplies the Cargo profile's overflow policy explicitly; it is
/// retained in the resulting semantic identity instead of inferred from the
/// host build's own debug/release mode.
pub fn lower_generic_fold(
    source_file: &Path,
    source_text: &str,
    instance: &RustGenericInstance,
    overflow: OverflowPolicy,
) -> Result<GenericFoldIr, GenericFoldError> {
    let file = syn::parse_file(source_text).map_err(GenericFoldError::Parse)?;
    if file.attrs.iter().any(|attr| !attr.path().is_ident("doc")) {
        return Err(GenericFoldError::UnsupportedSignature(
            instance.function.clone(),
        ));
    }
    let functions: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == instance.function => Some(function),
            _ => None,
        })
        .collect();
    let function = match functions.as_slice() {
        [] => return Err(GenericFoldError::MissingFunction(instance.function.clone())),
        [function] => function,
        _ => {
            return Err(GenericFoldError::DuplicateFunction(
                instance.function.clone(),
            ))
        }
    };
    let scalar = match instance.type_arguments.as_slice() {
        [name] if name == "i32" => ScalarType::I32,
        [name] if name == "i64" => ScalarType::I64,
        _ => {
            return Err(GenericFoldError::UnsupportedScalar(
                instance.type_arguments.join(","),
            ))
        }
    };
    let (type_name, input_name) = signature(function)
        .ok_or_else(|| GenericFoldError::UnsupportedSignature(instance.function.clone()))?;
    if !fold_body(function, &type_name, &input_name) {
        return Err(GenericFoldError::UnsupportedBody(instance.function.clone()));
    }
    let span = function.span();
    Ok(GenericFoldIr {
        package: instance.package.clone(),
        function: instance.function.clone(),
        scalar,
        overflow,
        provenance: Provenance {
            source_file: PathBuf::from(source_file),
            span: SourceSpan {
                start: to_source_position(span.start()),
                end: to_source_position(span.end()),
            },
            language: SourceLanguage::Rust,
        },
    })
}

fn signature(function: &syn::ItemFn) -> Option<(String, String)> {
    let sig = &function.sig;
    if sig.asyncness.is_some()
        || sig.constness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || function
            .attrs
            .iter()
            .any(|attr| !attr.path().is_ident("doc"))
    {
        return None;
    }
    let [GenericParam::Type(type_param)] =
        sig.generics.params.iter().collect::<Vec<_>>().as_slice()
    else {
        return None;
    };
    if !type_param.attrs.is_empty() || type_param.default.is_some() {
        return None;
    }
    let type_name = type_param.ident.to_string();
    let [FnArg::Typed(input)] = sig.inputs.iter().collect::<Vec<_>>().as_slice() else {
        return None;
    };
    if !input.attrs.is_empty() {
        return None;
    }
    let Pat::Ident(pattern) = input.pat.as_ref() else {
        return None;
    };
    if !pattern.attrs.is_empty()
        || pattern.by_ref.is_some()
        || pattern.mutability.is_some()
        || pattern.subpat.is_some()
    {
        return None;
    }
    let input_name = pattern.ident.to_string();
    let Type::Reference(reference) = input.ty.as_ref() else {
        return None;
    };
    if reference.mutability.is_some() || reference.lifetime.is_some() {
        return None;
    }
    let Type::Slice(slice) = reference.elem.as_ref() else {
        return None;
    };
    if !type_ident(&slice.elem, &type_name) {
        return None;
    }
    let syn::ReturnType::Type(_, result) = &sig.output else {
        return None;
    };
    if !type_ident(result, &type_name) {
        return None;
    }
    let mut bounds: BTreeSet<&str> = BTreeSet::new();
    if !type_param.bounds.is_empty() && !collect_bounds(&type_param.bounds, &type_name, &mut bounds)
    {
        return None;
    }
    if let Some(where_clause) = &sig.generics.where_clause {
        let [syn::WherePredicate::Type(predicate)] = where_clause
            .predicates
            .iter()
            .collect::<Vec<_>>()
            .as_slice()
        else {
            return None;
        };
        if !type_ident(&predicate.bounded_ty, &type_name)
            || predicate.lifetimes.is_some()
            || !collect_bounds(&predicate.bounds, &type_name, &mut bounds)
        {
            return None;
        }
    }
    if bounds != BTreeSet::from(["Copy", "Add", "Default"]) {
        return None;
    }
    Some((type_name, input_name))
}

fn collect_bounds<'a>(
    source: &'a syn::punctuated::Punctuated<TypeParamBound, syn::Token![+]>,
    type_name: &str,
    output: &mut BTreeSet<&'a str>,
) -> bool {
    for bound in source {
        let TypeParamBound::Trait(trait_bound) = bound else {
            return false;
        };
        if trait_bound.lifetimes.is_some() || trait_bound.modifier != syn::TraitBoundModifier::None
        {
            return false;
        }
        let names: Vec<_> = trait_bound
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let path = names.join("::");
        let kind = match path.as_str() {
            "Copy" | "std::marker::Copy" | "core::marker::Copy" => "Copy",
            "Default" | "std::default::Default" | "core::default::Default" => "Default",
            "std::ops::Add" | "core::ops::Add" => "Add",
            _ => return false,
        };
        if kind == "Add" {
            let Some(last) = trait_bound.path.segments.last() else {
                return false;
            };
            let PathArguments::AngleBracketed(args) = &last.arguments else {
                return false;
            };
            let [GenericArgument::AssocType(output_type)] =
                args.args.iter().collect::<Vec<_>>().as_slice()
            else {
                return false;
            };
            if output_type.ident != "Output" || !type_ident(&output_type.ty, type_name) {
                return false;
            }
        } else if trait_bound
            .path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, PathArguments::None))
        {
            return false;
        }
        if !output.insert(kind) {
            return false;
        }
    }
    true
}

fn type_ident(ty: &Type, expected: &str) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.is_ident(expected))
}

fn expression(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(paren) => expression(&paren.expr),
        Expr::Group(group) => expression(&group.expr),
        other => other,
    }
}

fn path_ident(expr: &Expr, expected: &str) -> bool {
    matches!(expression(expr), Expr::Path(path) if path.attrs.is_empty() && path.qself.is_none() && path.path.is_ident(expected))
}

fn fold_body(function: &syn::ItemFn, type_name: &str, input_name: &str) -> bool {
    let [syn::Stmt::Expr(body, None)] = function.block.stmts.as_slice() else {
        return false;
    };
    let Expr::MethodCall(fold) = expression(body) else {
        return false;
    };
    if !fold.attrs.is_empty()
        || fold.method != "fold"
        || fold.turbofish.is_some()
        || fold.args.len() != 2
    {
        return false;
    }
    let Expr::MethodCall(iter) = expression(&fold.receiver) else {
        return false;
    };
    if !iter.attrs.is_empty()
        || iter.method != "iter"
        || iter.turbofish.is_some()
        || !iter.args.is_empty()
        || !path_ident(&iter.receiver, input_name)
    {
        return false;
    }
    let Expr::Call(default) = expression(&fold.args[0]) else {
        return false;
    };
    let Expr::Path(default_path) = expression(&default.func) else {
        return false;
    };
    if !default.attrs.is_empty()
        || !default_path.attrs.is_empty()
        || default_path.qself.is_some()
        || !default.args.is_empty()
    {
        return false;
    }
    let segments: Vec<_> = default_path.path.segments.iter().collect();
    if segments.len() != 2
        || segments[0].ident != type_name
        || segments[1].ident != "default"
        || segments
            .iter()
            .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return false;
    }
    let Expr::Closure(closure) = expression(&fold.args[1]) else {
        return false;
    };
    if !closure.attrs.is_empty()
        || closure.asyncness.is_some()
        || closure.movability.is_some()
        || closure.capture.is_some()
        || closure.inputs.len() != 2
        || !matches!(closure.output, syn::ReturnType::Default)
    {
        return false;
    }
    let Pat::Ident(accumulator) = &closure.inputs[0] else {
        return false;
    };
    let Pat::Reference(element_reference) = &closure.inputs[1] else {
        return false;
    };
    let Pat::Ident(element) = element_reference.pat.as_ref() else {
        return false;
    };
    if !accumulator.attrs.is_empty()
        || !element_reference.attrs.is_empty()
        || !element.attrs.is_empty()
        || accumulator.by_ref.is_some()
        || accumulator.mutability.is_some()
        || accumulator.subpat.is_some()
        || element_reference.mutability.is_some()
        || element.by_ref.is_some()
        || element.mutability.is_some()
        || element.subpat.is_some()
    {
        return false;
    }
    let Expr::Binary(add) = expression(&closure.body) else {
        return false;
    };
    add.attrs.is_empty()
        && matches!(add.op, syn::BinOp::Add(_))
        && path_ident(&add.left, &accumulator.ident.to_string())
        && path_ident(&add.right, &element.ident.to_string())
}

impl GenericFoldIr {
    pub fn evaluate_i64(&self, items: &[i64]) -> Result<i64, GenericFoldError> {
        if self.scalar != ScalarType::I64 {
            return Err(GenericFoldError::WidthMismatch);
        }
        items
            .iter()
            .try_fold(0_i64, |sum, &value| match self.overflow {
                OverflowPolicy::Checked => sum
                    .checked_add(value)
                    .ok_or(GenericFoldError::ArithmeticOverflow),
                OverflowPolicy::Wrapping => Ok(sum.wrapping_add(value)),
            })
    }

    pub fn evaluate_i32(&self, items: &[i32]) -> Result<i32, GenericFoldError> {
        if self.scalar != ScalarType::I32 {
            return Err(GenericFoldError::WidthMismatch);
        }
        items
            .iter()
            .try_fold(0_i32, |sum, &value| match self.overflow {
                OverflowPolicy::Checked => sum
                    .checked_add(value)
                    .ok_or(GenericFoldError::ArithmeticOverflow),
                OverflowPolicy::Wrapping => Ok(sum.wrapping_add(value)),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_generic_demand::{discover_generic_functions, discover_generic_instances};

    #[test]
    fn locked_fixture_generic_instances_lower_and_execute_from_source() {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rust-heavy-workspace");
        let core_path = root.join("crates/fixture-core/src/lib.rs");
        let bin_path = root.join("crates/fixture-bin/src/main.rs");
        let core = std::fs::read_to_string(&core_path).unwrap();
        let bin = std::fs::read_to_string(&bin_path).unwrap();
        let known = discover_generic_functions("fixture-core", &core).unwrap();
        let binary_instance = discover_generic_instances("fixture-bin", &bin, &known, false)
            .unwrap()
            .pop_first()
            .unwrap();
        let test_instance = discover_generic_instances("fixture-core", &core, &known, true)
            .unwrap()
            .pop_first()
            .unwrap();
        let production =
            lower_generic_fold(&core_path, &core, &binary_instance, OverflowPolicy::Checked)
                .unwrap();
        let test =
            lower_generic_fold(&core_path, &core, &test_instance, OverflowPolicy::Checked).unwrap();
        assert_eq!(production.scalar, ScalarType::I64);
        assert_eq!(test.scalar, ScalarType::I32);
        assert_eq!(production.evaluate_i64(&[12, 30]).unwrap(), 42);
        assert_eq!(test.evaluate_i32(&[1, 2, 3, 4]).unwrap(), 10);
        assert!(matches!(
            production.evaluate_i64(&[i64::MAX, 1]),
            Err(GenericFoldError::ArithmeticOverflow)
        ));
    }

    #[test]
    fn changed_generic_semantics_fail_closed() {
        let source = "fn sum<T>(items: &[T]) -> T where T: Copy + std::ops::Add<Output=T> + Default { items.iter().fold(T::default(), |acc, &x| acc - x) }";
        let instance = RustGenericInstance {
            package: "p".into(),
            function: "sum".into(),
            type_arguments: vec!["i64".into()],
        };
        assert!(matches!(
            lower_generic_fold(
                Path::new("sum.rs"),
                source,
                &instance,
                OverflowPolicy::Checked
            ),
            Err(GenericFoldError::UnsupportedBody(_))
        ));
    }
}

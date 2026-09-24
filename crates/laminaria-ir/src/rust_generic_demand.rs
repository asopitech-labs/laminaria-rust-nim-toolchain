//! Narrow Rust source facts for issue #50's first generic-demand slice.
//!
//! This is deliberately not a type checker. It handles only direct calls to
//! known generic functions where the type argument is explicit, comes from a
//! locally annotated `Vec<T>`, or comes from an unsuffixed integer array
//! literal. Anything outside that small, declared subset is rejected rather
//! than guessed.

use std::collections::{BTreeMap, BTreeSet};

use syn::parse::Parser;
use syn::visit::Visit;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RustGenericFunction {
    pub package: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RustGenericInstance {
    pub package: String,
    pub function: String,
    pub type_arguments: Vec<String>,
}

#[derive(Debug)]
pub enum RustGenericDemandError {
    Parse(syn::Error),
    CannotInferGenericArgument { function: String },
    UnsupportedMacro { name: String },
}

impl std::fmt::Display for RustGenericDemandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid Rust source: {error}"),
            Self::CannotInferGenericArgument { function } => write!(
                f,
                "cannot soundly infer the generic argument for call to `{function}`"
            ),
            Self::UnsupportedMacro { name } => {
                write!(f, "cannot soundly inspect Rust macro `{name}`")
            }
        }
    }
}

impl std::error::Error for RustGenericDemandError {}

/// Finds function declarations with at least one type/const/lifetime generic.
pub fn discover_generic_functions(
    package: &str,
    source_text: &str,
) -> Result<BTreeSet<RustGenericFunction>, RustGenericDemandError> {
    let file = syn::parse_file(source_text).map_err(RustGenericDemandError::Parse)?;
    let mut collector = GenericFunctionCollector {
        package,
        functions: BTreeSet::new(),
    };
    collector.visit_file(&file);
    Ok(collector.functions)
}

struct GenericFunctionCollector<'a> {
    package: &'a str,
    functions: BTreeSet<RustGenericFunction>,
}

impl<'ast> Visit<'ast> for GenericFunctionCollector<'_> {
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        if !function.sig.generics.params.is_empty()
            && function
                .sig
                .generics
                .params
                .iter()
                .all(|parameter| matches!(parameter, syn::GenericParam::Type(_)))
        {
            self.functions.insert(RustGenericFunction {
                package: self.package.to_string(),
                name: function.sig.ident.to_string(),
            });
        }
        syn::visit::visit_item_fn(self, function);
    }
}

/// Finds demands for known generic functions in one source file. `include_test_cfg`
/// selects whether items under `#[cfg(test)]` participate in this demand root.
pub fn discover_generic_instances(
    owner_package: &str,
    source_text: &str,
    known_functions: &BTreeSet<RustGenericFunction>,
    include_test_cfg: bool,
) -> Result<BTreeSet<RustGenericInstance>, RustGenericDemandError> {
    let file = syn::parse_file(source_text).map_err(RustGenericDemandError::Parse)?;
    let imports = imported_functions(&file);
    let mut collector = GenericCallCollector {
        owner_package,
        known_functions,
        imports,
        include_test_cfg,
        local_types: BTreeMap::new(),
        ambiguous_locals: BTreeSet::new(),
        instances: BTreeSet::new(),
        error: None,
    };
    collector.visit_file(&file);
    if let Some(error) = collector.error {
        return Err(error);
    }
    Ok(collector.instances)
}

fn imported_functions(file: &syn::File) -> BTreeMap<String, (String, String)> {
    fn flatten(tree: &syn::UseTree, prefix: &mut Vec<String>, output: &mut Vec<Vec<String>>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                flatten(&path.tree, prefix, output);
                prefix.pop();
            }
            syn::UseTree::Name(name) => {
                let mut full = prefix.clone();
                full.push(name.ident.to_string());
                output.push(full);
            }
            syn::UseTree::Rename(rename) => {
                let mut full = prefix.clone();
                full.push(rename.ident.to_string());
                full.push(rename.rename.to_string());
                output.push(full);
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    flatten(item, prefix, output);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }

    let mut flattened = Vec::new();
    for item in &file.items {
        if let syn::Item::Use(item_use) = item {
            flatten(&item_use.tree, &mut Vec::new(), &mut flattened);
        }
    }
    flattened
        .into_iter()
        .filter(|path| path.len() >= 2)
        .filter_map(|path| {
            let provider = path.first()?.clone();
            if matches!(
                provider.as_str(),
                "crate" | "self" | "super" | "std" | "core" | "alloc"
            ) {
                return None;
            }
            let local_name = if path.len() >= 3 {
                path[path.len() - 1].clone()
            } else {
                path[1].clone()
            };
            let function = if path.len() >= 3 {
                path[path.len() - 2].clone()
            } else {
                path[1].clone()
            };
            Some((local_name, (provider, function)))
        })
        .collect()
}

struct GenericCallCollector<'a> {
    owner_package: &'a str,
    known_functions: &'a BTreeSet<RustGenericFunction>,
    imports: BTreeMap<String, (String, String)>,
    include_test_cfg: bool,
    local_types: BTreeMap<String, Option<String>>,
    ambiguous_locals: BTreeSet<String>,
    instances: BTreeSet<RustGenericInstance>,
    error: Option<RustGenericDemandError>,
}

impl GenericCallCollector<'_> {
    fn known_function(&self, package: &str, name: &str) -> bool {
        self.known_functions.iter().any(|function| {
            function.name == name
                && (function.package.replace('-', "_") == package || function.package == package)
        })
    }

    fn resolve_call(&self, call: &syn::ExprCall) -> Option<(String, String, Vec<String>)> {
        let syn::Expr::Path(path) = call.func.as_ref() else {
            return None;
        };
        let segments = &path.path.segments;
        let name = segments.last()?.ident.to_string();
        let explicit_types = match &segments.last()?.arguments {
            syn::PathArguments::AngleBracketed(args) => args
                .args
                .iter()
                .filter_map(|arg| match arg {
                    syn::GenericArgument::Type(ty) => type_name(ty),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let (package, function) = if segments.len() >= 2 {
            (segments.first()?.ident.to_string(), name)
        } else if let Some((package, function)) = self.imports.get(&name) {
            (package.clone(), function.clone())
        } else {
            (self.owner_package.replace('-', "_"), name)
        };
        self.known_function(&package, &function)
            .then_some((package, function, explicit_types))
    }

    fn inferred_argument(&self, expression: &syn::Expr) -> Option<String> {
        match expression {
            syn::Expr::Reference(reference) => self.inferred_argument(&reference.expr),
            syn::Expr::Path(path) => {
                let name = path.path.get_ident()?.to_string();
                if self.ambiguous_locals.contains(&name) {
                    None
                } else {
                    self.local_types
                        .get(&name)?
                        .clone()
                        .and_then(|ty| generic_type_argument(&ty).or(Some(ty)))
                }
            }
            syn::Expr::Array(array) => {
                let mut suffix = None;
                for element in &array.elems {
                    let syn::Expr::Lit(lit) = element else {
                        return None;
                    };
                    let syn::Lit::Int(integer) = &lit.lit else {
                        return None;
                    };
                    let current = if integer.suffix().is_empty() {
                        "i32".to_string()
                    } else {
                        integer.suffix().to_string()
                    };
                    if suffix.as_ref().is_some_and(|prior| prior != &current) {
                        return None;
                    }
                    suffix = Some(current);
                }
                suffix
            }
            syn::Expr::Cast(cast) => type_name(&cast.ty),
            _ => None,
        }
    }
}

impl<'ast> Visit<'ast> for GenericCallCollector<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let is_test = item.attrs.iter().any(|attribute| {
            attribute.path().is_ident("cfg")
                && matches!(attribute.parse_args::<syn::Ident>(), Ok(ident) if ident == "test")
        });
        if is_test && !self.include_test_cfg {
            return;
        }
        syn::visit::visit_item_mod(self, item);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        match &local.pat {
            syn::Pat::Ident(pattern) => {
                self.record_local(pattern.ident.to_string(), None);
            }
            syn::Pat::Type(typed) => {
                if let syn::Pat::Ident(pattern) = typed.pat.as_ref() {
                    self.record_local(pattern.ident.to_string(), type_name(&typed.ty));
                }
            }
            _ => {}
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if self.error.is_some() {
            return;
        }
        if let Some((package, function, explicit_types)) = self.resolve_call(call) {
            let type_arguments = if !explicit_types.is_empty() {
                explicit_types
            } else {
                match call
                    .args
                    .first()
                    .and_then(|arg| self.inferred_argument(arg))
                {
                    Some(argument) => vec![argument],
                    None => {
                        self.error =
                            Some(RustGenericDemandError::CannotInferGenericArgument { function });
                        return;
                    }
                }
            };
            self.instances.insert(RustGenericInstance {
                package,
                function,
                type_arguments,
            });
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, macro_call: &'ast syn::Macro) {
        if self.error.is_some() {
            return;
        }
        let name = macro_call
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        if !matches!(
            name.as_str(),
            "assert"
                | "assert_eq"
                | "assert_ne"
                | "debug_assert"
                | "debug_assert_eq"
                | "debug_assert_ne"
                | "format"
                | "format_args"
                | "print"
                | "println"
                | "eprint"
                | "eprintln"
                | "vec"
        ) {
            self.error = Some(RustGenericDemandError::UnsupportedMacro { name });
            return;
        }
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        let expressions = match parser.parse2(macro_call.tokens.clone()) {
            Ok(expressions) => expressions,
            Err(error) => {
                self.error = Some(RustGenericDemandError::Parse(error));
                return;
            }
        };
        for expression in &expressions {
            self.visit_expr(expression);
        }
    }
}

impl GenericCallCollector<'_> {
    fn record_local(&mut self, name: String, ty: Option<String>) {
        if self.local_types.contains_key(&name) {
            self.ambiguous_locals.insert(name.clone());
        }
        self.local_types.insert(name, ty);
    }
}

fn generic_type_argument(ty: &str) -> Option<String> {
    let parsed: syn::Type = syn::parse_str(ty).ok()?;
    let syn::Type::Path(path) = parsed else {
        return None;
    };
    let segment = path.path.segments.last()?;
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(argument) => type_name(argument),
        _ => None,
    })
}

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => {
            let segment = path.path.segments.last()?;
            let name = segment.ident.to_string();
            match &segment.arguments {
                syn::PathArguments::None => Some(name),
                syn::PathArguments::AngleBracketed(arguments) => {
                    let arguments = arguments
                        .args
                        .iter()
                        .filter_map(|argument| match argument {
                            syn::GenericArgument::Type(argument) => type_name(argument),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    Some(format!("{name}<{}>", arguments.join(",")))
                }
                syn::PathArguments::Parenthesized(_) => None,
            }
        }
        syn::Type::Reference(reference) => type_name(&reference.elem),
        syn::Type::Array(array) => type_name(&array.elem).map(|element| format!("[{element}]")),
        syn::Type::Slice(slice) => type_name(&slice.elem).map(|element| format!("[{element}]")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_source(relative: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("fixtures/rust-heavy-workspace")
                .join(relative),
        )
        .expect("the checked-in Rust workload source is readable")
    }

    #[test]
    fn real_fixture_distinguishes_binary_and_test_generic_demands() {
        let core = fixture_source("crates/fixture-core/src/lib.rs");
        let binary = fixture_source("crates/fixture-bin/src/main.rs");
        let known = discover_generic_functions("fixture-core", &core).expect("valid core source");

        let binary_demand = discover_generic_instances("fixture-bin", &binary, &known, false)
            .expect("the annotated Vec element type determines the binary instantiation");
        let core_production_demand =
            discover_generic_instances("fixture-core", &core, &known, false)
                .expect("the production core source is analyzable");
        let core_test_demand = discover_generic_instances("fixture-core", &core, &known, true)
            .expect("the test source's integer fallback is supported");

        assert_eq!(binary_demand.len(), 1);
        assert_eq!(binary_demand.iter().next().unwrap().type_arguments, ["i64"]);
        assert!(core_production_demand.is_empty());
        assert_eq!(core_test_demand.len(), 1);
        assert_eq!(
            core_test_demand.iter().next().unwrap().type_arguments,
            ["i32"]
        );
    }

    #[test]
    fn unsupported_generic_argument_inference_fails_closed() {
        let known = BTreeSet::from([RustGenericFunction {
            package: "core-lib".to_string(),
            name: "convert".to_string(),
        }]);
        let error = discover_generic_instances(
            "app",
            "use core_lib::convert; fn run(value: impl Trait) { convert(value); }",
            &known,
            false,
        )
        .expect_err("an opaque impl Trait value must not be guessed");
        assert!(matches!(
            error,
            RustGenericDemandError::CannotInferGenericArgument { .. }
        ));

        let error = discover_generic_instances(
            "app",
            "use core_lib::convert; fn run() { custom_expansion! { convert(1) } }",
            &known,
            false,
        )
        .expect_err("unknown macros must not hide or fabricate generic work");
        assert!(matches!(
            error,
            RustGenericDemandError::UnsupportedMacro { .. }
        ));
    }
}

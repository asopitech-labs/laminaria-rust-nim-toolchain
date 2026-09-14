//! Issue #48 (G1): a shallow syn-based scan for `extern "C" { ... }` FFI
//! declarations and their `#[link(name = "...")]` provider hints and
//! `#[cfg(...)]` activation gates -- the same "best-effort fact finder
//! over whatever syntax the file actually contains, never a
//! correctness gate" discipline
//! [`crate::discover::discover_called_functions`] already established for
//! T0 §1.8's call-graph scan, applied here to G1's own need: a
//! source-derived FFI requirement (a real `extern "C"` block naming a
//! required symbol, linkage, arity, return type, and activation
//! condition) that can feed back into which candidate native-artifact
//! provider a dependency-obligation graph selects or rejects. This
//! module never calls
//! [`crate::rust_frontend::lower_rust_source`] and knows nothing about
//! `laminaria-plan`'s obligation graph -- it only turns real Rust source
//! text into a small, typed fact list. Evaluating the `cfg_predicate`
//! this module reports against a real target/feature configuration is
//! the caller's own job ([`crate::cfg_predicate`]); this module only
//! ever parses and reports it, never evaluates it, since it has no
//! opinion on which configuration is "real."

use syn::visit::Visit;
use syn::{Item, Lit};

use crate::cfg_predicate::{combined_cfg_predicate, CfgPredicate};
use crate::diagnostics::{Diagnostic, LoweringError};
use crate::rust_frontend::to_source_position;
use crate::types::{SourceLanguage, SourceSpan};

/// The `#[link(name = "...", kind = "...")]` attribute LAMINARIA's own
/// candidate-provider matching reads as a hint at which native artifact
/// is expected to satisfy the enclosing `extern` block's symbols --
/// `kind` is `None` when the attribute omits it (Rust's own default is
/// `dylib`, but this scan records only what the source text actually
/// says).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkHint {
    pub name: String,
    pub kind: Option<String>,
}

/// One function declared inside a real `extern "<abi>" { ... }` block --
/// a genuine source-derived FFI requirement, not an invented one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignFunctionRequirement {
    pub name: String,
    /// The ABI string literal (`"C"`, `"system"`, ...) syn parsed from
    /// `extern "..."`; Rust's own unmarked `extern` defaults to `"C"`,
    /// recorded as `"C"` here too since that is the real linkage in
    /// force, not merely omitted text.
    pub abi: String,
    pub param_count: usize,
    /// The real declared return type, stringified from the actual
    /// syntax tree (`"()"` for no return type, e.g. `-> i32` becomes
    /// `"i32"`) -- never inferred or assumed.
    pub return_type: String,
    /// The nearest enclosing `#[link(...)]` hint, if the `extern` block
    /// (or, for a single-block file, the function's own block) carries
    /// one -- `None` when the source declares the function with no such
    /// attribute at all, which is itself a fact worth recording (an
    /// unhinted FFI requirement still needs *some* provider, discovered
    /// by name alone).
    pub link_hint: Option<LinkHint>,
    /// The real `#[cfg(...)]` activation condition on the enclosing
    /// `extern` block, if any -- `None` means unconditionally active.
    /// This module only parses and reports it; evaluating it against a
    /// real configuration is the caller's job.
    pub cfg_predicate: Option<CfgPredicate>,
}

struct ForeignModVisitor {
    found: Vec<ForeignFunctionRequirement>,
    parse_errors: Vec<syn::Error>,
}

fn link_hint_from_attrs(attrs: &[syn::Attribute]) -> Option<LinkHint> {
    for attr in attrs {
        if !attr.path().is_ident("link") {
            continue;
        }
        let mut name: Option<String> = None;
        let mut kind: Option<String> = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                if let Ok(value) = meta.value() {
                    if let Ok(Lit::Str(s)) = value.parse::<Lit>() {
                        name = Some(s.value());
                    }
                }
            } else if meta.path.is_ident("kind") {
                if let Ok(value) = meta.value() {
                    if let Ok(Lit::Str(s)) = value.parse::<Lit>() {
                        kind = Some(s.value());
                    }
                }
            }
            Ok(())
        });
        if let Some(name) = name {
            return Some(LinkHint { name, kind });
        }
    }
    None
}

fn return_type_text(output: &syn::ReturnType) -> String {
    match output {
        syn::ReturnType::Default => "()".to_string(),
        syn::ReturnType::Type(_, ty) => quote::quote!(#ty).to_string().replace(' ', ""),
    }
}

impl<'ast> Visit<'ast> for ForeignModVisitor {
    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        let abi = node
            .abi
            .name
            .as_ref()
            .map(|lit| lit.value())
            .unwrap_or_else(|| "C".to_string());
        let link_hint = link_hint_from_attrs(&node.attrs);
        let cfg_predicate = match combined_cfg_predicate(&node.attrs) {
            Ok(p) => p,
            Err(e) => {
                self.parse_errors.push(e);
                None
            }
        };
        for item in &node.items {
            if let syn::ForeignItem::Fn(f) = item {
                self.found.push(ForeignFunctionRequirement {
                    name: f.sig.ident.to_string(),
                    abi: abi.clone(),
                    param_count: f.sig.inputs.len(),
                    return_type: return_type_text(&f.sig.output),
                    link_hint: link_hint.clone(),
                    cfg_predicate: cfg_predicate.clone(),
                });
            }
        }
        syn::visit::visit_item_foreign_mod(self, node);
    }
}

/// Scans real Rust source text for every `extern "<abi>" { fn ...; }`
/// declaration, in file order, together with its real `#[cfg(...)]`
/// activation condition. Every declaration the file's own syntax
/// contains is reported regardless of any particular feature/target
/// configuration -- a caller with a real [`crate::cfg_predicate::CfgContext`]
/// decides which requirements are actually in force for a given
/// closure by evaluating each requirement's own `cfg_predicate`.
/// `#[link]` outside a `Meta::List` (e.g. a bare `#[link]` with no
/// arguments) is silently treated as no hint, the same "don't reject
/// unfamiliar syntax" stance `discover_called_functions` already takes;
/// an unparseable `#[cfg(...)]` is reported as a diagnostic, since a
/// cfg gate this module cannot understand must never be silently
/// treated as "always active."
pub fn discover_foreign_function_requirements(
    source_text: &str,
) -> Result<Vec<ForeignFunctionRequirement>, Vec<Diagnostic>> {
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

    let mut visitor = ForeignModVisitor {
        found: Vec::new(),
        parse_errors: Vec::new(),
    };
    for item in &file.items {
        if let Item::ForeignMod(fm) = item {
            visitor.visit_item_foreign_mod(fm);
        }
    }
    if !visitor.parse_errors.is_empty() {
        return Err(visitor
            .parse_errors
            .into_iter()
            .map(|e| {
                Diagnostic::from_lowering_error(
                    LoweringError::ParseError {
                        detail: e.to_string(),
                        span: SourceSpan {
                            start: to_source_position(e.span().start()),
                            end: to_source_position(e.span().end()),
                        },
                    },
                    SourceLanguage::Rust,
                )
            })
            .collect());
    }
    Ok(visitor.found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_with_no_extern_block_discovers_nothing() {
        let source = "fn f(x: i32) -> i32 { x }";
        let found = discover_foreign_function_requirements(source).expect("must parse");
        assert!(found.is_empty());
    }

    #[test]
    fn a_hinted_extern_c_block_is_discovered_with_its_link_hint() {
        let source = r#"
            #[link(name = "cadd", kind = "static")]
            extern "C" {
                fn c_add(a: i32, b: i32) -> i32;
            }
        "#;
        let found = discover_foreign_function_requirements(source).expect("must parse");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "c_add");
        assert_eq!(found[0].abi, "C");
        assert_eq!(found[0].param_count, 2);
        assert_eq!(found[0].return_type, "i32");
        assert_eq!(
            found[0].link_hint,
            Some(LinkHint {
                name: "cadd".to_string(),
                kind: Some("static".to_string()),
            })
        );
        assert_eq!(found[0].cfg_predicate, None);
    }

    #[test]
    fn an_unhinted_extern_block_is_still_discovered_with_no_link_hint() {
        let source = r#"
            extern "C" {
                fn nim_double(x: i32) -> i32;
            }
        "#;
        let found = discover_foreign_function_requirements(source).expect("must parse");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "nim_double");
        assert_eq!(found[0].link_hint, None);
    }

    #[test]
    fn multiple_hinted_blocks_each_keep_their_own_hint() {
        let source = r#"
            #[link(name = "cadd", kind = "static")]
            extern "C" {
                fn c_add(a: i32, b: i32) -> i32;
            }
            #[link(name = "cppmax", kind = "static")]
            extern "C" {
                fn cpp_max_i32(a: i32, b: i32) -> i32;
            }
        "#;
        let found = discover_foreign_function_requirements(source).expect("must parse");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "c_add");
        assert_eq!(found[0].link_hint.as_ref().unwrap().name, "cadd");
        assert_eq!(found[1].name, "cpp_max_i32");
        assert_eq!(found[1].link_hint.as_ref().unwrap().name, "cppmax");
    }

    #[test]
    fn an_unparseable_file_is_a_diagnostic_not_a_panic() {
        let result = discover_foreign_function_requirements("extern \"C\" { fn f(");
        assert!(result.is_err());
    }

    #[test]
    fn a_cfg_gated_block_reports_its_real_predicate() {
        let source = r#"
            #[cfg(all(unix, feature = "use_nim_double"))]
            extern "C" {
                fn nim_double(x: i32) -> i32;
            }
        "#;
        let found = discover_foreign_function_requirements(source).expect("must parse");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].cfg_predicate,
            Some(CfgPredicate::All(vec![
                CfgPredicate::Ident("unix".to_string()),
                CfgPredicate::KeyValue("feature".to_string(), "use_nim_double".to_string()),
            ]))
        );
    }

    #[test]
    fn a_function_returning_unit_reports_return_type_unit() {
        let source = r#"
            extern "C" {
                fn f(x: i32);
            }
        "#;
        let found = discover_foreign_function_requirements(source).expect("must parse");
        assert_eq!(found[0].return_type, "()");
    }
}

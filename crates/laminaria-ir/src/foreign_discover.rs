//! Issue #48 (G1): a shallow syn-based scan for `extern "C" { ... }` FFI
//! declarations and their `#[link(name = "...")]` provider hints -- the
//! same "best-effort fact finder over whatever syntax the file actually
//! contains, never a correctness gate" discipline
//! [`crate::discover::discover_called_functions`] already established for
//! T0 §1.8's call-graph scan, applied here to G1's own need: a
//! source-derived FFI requirement (a real `extern "C"` block naming a
//! required symbol, linkage, and arity) that can feed back into which
//! candidate native-artifact provider a dependency-obligation graph
//! selects or rejects. This module never calls
//! [`crate::rust_frontend::lower_rust_source`] and knows nothing about
//! `laminaria-plan`'s obligation graph -- it only turns real Rust source
//! text into a small, typed fact list.

use syn::visit::Visit;
use syn::{Item, Lit};

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
    /// The nearest enclosing `#[link(...)]` hint, if the `extern` block
    /// (or, for a single-block file, the function's own block) carries
    /// one -- `None` when the source declares the function with no such
    /// attribute at all, which is itself a fact worth recording (an
    /// unhinted FFI requirement still needs *some* provider, discovered
    /// by name alone).
    pub link_hint: Option<LinkHint>,
}

struct ForeignModVisitor {
    found: Vec<ForeignFunctionRequirement>,
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

impl<'ast> Visit<'ast> for ForeignModVisitor {
    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        let abi = node
            .abi
            .name
            .as_ref()
            .map(|lit| lit.value())
            .unwrap_or_else(|| "C".to_string());
        let link_hint = link_hint_from_attrs(&node.attrs);
        for item in &node.items {
            if let syn::ForeignItem::Fn(f) = item {
                self.found.push(ForeignFunctionRequirement {
                    name: f.sig.ident.to_string(),
                    abi: abi.clone(),
                    param_count: f.sig.inputs.len(),
                    link_hint: link_hint.clone(),
                });
            }
        }
        syn::visit::visit_item_foreign_mod(self, node);
    }
}

/// Scans real Rust source text for every `extern "<abi>" { fn ...; }`
/// declaration, in file order. Deliberately does not attempt to resolve
/// `#[cfg(feature = "...")]`-gated foreign blocks against any particular
/// feature set -- every declaration the file's own syntax contains is
/// reported; a caller matching this against a specific Cargo feature
/// selection decides which requirements are actually in force for a
/// given closure. `#[link]` outside a `Meta::List`
/// (e.g. a bare `#[link]` with no arguments) is silently treated as no
/// hint, the same "don't reject unfamiliar syntax" stance
/// `discover_called_functions` already takes.
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

    let mut visitor = ForeignModVisitor { found: Vec::new() };
    for item in &file.items {
        if let Item::ForeignMod(fm) = item {
            visitor.visit_item_foreign_mod(fm);
        }
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
        assert_eq!(
            found[0].link_hint,
            Some(LinkHint {
                name: "cadd".to_string(),
                kind: Some("static".to_string()),
            })
        );
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
}

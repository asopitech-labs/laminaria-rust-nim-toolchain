//! Shallow, owned discovery of external Rust crate references for issue #50.
//!
//! This source-fact collector is not a type checker or name resolver. It
//! records roots introduced by `use` trees and qualified calls. Imported item
//! names are tracked so calls such as `Cluster::new()` do not get mistaken for
//! an external package named `Cluster`.

use std::collections::BTreeSet;

use syn::visit::Visit;

fn is_builtin_root(name: &str) -> bool {
    matches!(name, "crate" | "self" | "super" | "std" | "core" | "alloc")
}

fn flatten_use_tree(tree: &syn::UseTree, prefix: &mut Vec<String>, output: &mut Vec<Vec<String>>) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            flatten_use_tree(&path.tree, prefix, output);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            let mut path = prefix.clone();
            path.push(name.ident.to_string());
            output.push(path);
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix.clone();
            path.push(rename.rename.to_string());
            output.push(path);
        }
        syn::UseTree::Glob(_) => {
            let mut path = prefix.clone();
            path.push("*".to_string());
            output.push(path);
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                flatten_use_tree(item, prefix, output);
            }
        }
    }
}

#[derive(Default)]
struct ImportCollector {
    roots: BTreeSet<String>,
    imported_names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for ImportCollector {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut flattened = Vec::new();
        flatten_use_tree(&item.tree, &mut Vec::new(), &mut flattened);
        for path in flattened {
            let Some(root) = path.first() else {
                continue;
            };
            if is_builtin_root(root) {
                continue;
            }
            self.roots.insert(root.clone());
            if let Some(imported_name) = path.last() {
                if imported_name != "*" {
                    self.imported_names.insert(imported_name.clone());
                }
            }
        }
        syn::visit::visit_item_use(self, item);
    }
}

struct CallCollector<'a> {
    references: BTreeSet<String>,
    imported_names: &'a BTreeSet<String>,
}

impl<'ast> Visit<'ast> for CallCollector<'_> {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let segments = &path.path.segments;
            if segments.len() >= 2 {
                if let Some(first) = segments.first() {
                    let name = first.ident.to_string();
                    if !is_builtin_root(&name) && !self.imported_names.contains(&name) {
                        self.references.insert(name);
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// Returns external roots introduced by Rust `use` trees and qualified calls
/// like `crate_name::item(...)`. Unqualified calls and Rust's built-in roots
/// are not package requirements. This remains conservative syntax discovery;
/// Cargo metadata and later semantic analysis decide which roots are providers.
pub fn discover_external_crate_references(
    source_text: &str,
) -> Result<BTreeSet<String>, syn::Error> {
    let file = syn::parse_file(source_text)?;
    let mut imports = ImportCollector::default();
    imports.visit_file(&file);
    let mut calls = CallCollector {
        references: imports.roots,
        imported_names: &imports.imported_names,
    };
    calls.visit_file(&file);
    Ok(calls.references)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_qualified_external_calls() {
        let references = discover_external_crate_references(
            "fn main() { local(); crate::local(); std::hint::black_box(0); used_core::value(); used_util::value(); }",
        )
        .expect("valid Rust source");
        assert_eq!(
            references,
            ["used_core", "used_util"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[test]
    fn import_roots_cover_associated_calls_through_imported_types_and_aliases() {
        let references = discover_external_crate_references(
            "use fixture_core::sum_generic; use fixture_mid::{Cluster as PrimeCluster}; fn main() { let _ = PrimeCluster::from_prime_grid(20); let _ = fixture_core::sum_generic::<i64>(&[]); sum_generic(&[]); }",
        )
        .expect("valid Rust source");
        assert_eq!(
            references,
            ["fixture_core", "fixture_mid"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }
}

//! Shallow, owned discovery of external Rust crate references for issue #50.
//!
//! This is deliberately a source-fact collector, not a type checker.  The
//! planner conservatively rejects an unresolved or ambiguous reference before
//! it schedules candidate compiler work.

use std::collections::BTreeSet;

use syn::visit::Visit;

/// Returns the first path segment of calls written as `crate_name::item(...)`.
/// Unqualified calls and Rust's built-in roots are not package requirements.
pub fn discover_external_crate_references(
    source_text: &str,
) -> Result<BTreeSet<String>, syn::Error> {
    struct Collector {
        references: BTreeSet<String>,
    }

    impl<'ast> Visit<'ast> for Collector {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(path) = call.func.as_ref() {
                let segments = &path.path.segments;
                if segments.len() >= 2 && path.path.leading_colon.is_none() {
                    if let Some(first) = segments.first() {
                        let name = first.ident.to_string();
                        if !matches!(
                            name.as_str(),
                            "crate" | "self" | "super" | "std" | "core" | "alloc"
                        ) {
                            self.references.insert(name);
                        }
                    }
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
    }

    let file = syn::parse_file(source_text)?;
    let mut collector = Collector {
        references: BTreeSet::new(),
    };
    collector.visit_file(&file);
    Ok(collector.references)
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
}

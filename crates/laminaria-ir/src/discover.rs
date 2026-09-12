//! Issue #36 T0 §1.8: `DiscoverSourceDependencies`, a shallow call-graph
//! scan over real Rust source -- **never** calls [`crate::rust_frontend::lower_rust_source`]
//! itself, so it never hits that function's own precondition
//! (`rust_frontend.rs:747-752`: a call to a name outside the requested
//! set is a hard `Diagnostic`, with no partial `Program` produced). This
//! is exactly the fixed fixture bug the accepted T0 revision found and
//! corrected: `add_or_double` cannot be lowered alone to *discover* that
//! it calls `double` -- the closure must be known *before* a single
//! `lower_rust_source` call can succeed at all.
//!
//! Deliberately **more lenient** than `lower_rust_source`: this scan
//! never rejects a construct it doesn't recognize (it isn't validating
//! the file, only trying to find `Expr::Call` sites among whatever
//! syntax the file actually contains), and it only needs to be a
//! reasonable, best-effort superset-finder -- `lower_rust_source` itself
//! remains the actual gatekeeper of correctness once the closure this
//! scan finds is handed to it as a closed `requested_functions` set. If
//! this scan ever missed a real call site, `lower_rust_source` would
//! still catch the gap (a call outside the request), just one round
//! later -- it would never silently under-lower.
//!
//! Local/parameter shadowing is approximated conservatively: every name
//! bound by *any* parameter or `let` pattern anywhere in a scanned
//! function is excluded from its call sites, regardless of which nested
//! block actually shadows it (whole-function flat set, not a real nested
//! scope stack) -- correct for this crate's own declared subset (no
//! function-valued locals, so a shadowed name is never itself callable
//! anyway, `rust_frontend.rs:738-746`'s same exclusion), and simpler than
//! reimplementing full lowering's own scope tracking for a scan that
//! only needs to find candidate names, not lower them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use syn::visit::Visit;
use syn::{Expr as SynExpr, Item, ItemFn, Pat};

use crate::diagnostics::{Diagnostic, LoweringError};
use crate::rust_frontend::to_source_position;
use crate::types::{SourceLanguage, SourceSpan};

/// Every parameter/`let`-bound name anywhere inside one function -- see
/// this module's own doc comment on why a flat, whole-function set is a
/// deliberate, sufficient approximation here.
struct ShadowCollector {
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for ShadowCollector {
    fn visit_pat(&mut self, pat: &'ast Pat) {
        if let Pat::Ident(ident) = pat {
            self.names.insert(ident.ident.to_string());
        }
        syn::visit::visit_pat(self, pat);
    }
}

/// Collects every `Expr::Call` whose callee is a single, unqualified
/// identifier not in `shadowed` -- the same identifier-resolution rule
/// `rust_frontend.rs:716-752`'s own `Expr::Call` arm applies (reject a
/// qualified path, reject a non-identifier callee, reject a
/// local/parameter of the same name), except this scan *skips* a
/// disqualified call site instead of raising a diagnostic: it is
/// searching for candidates, not validating the file.
struct CallCollector<'a> {
    shadowed: &'a BTreeSet<String>,
    found: BTreeSet<String>,
}

impl<'a, 'ast> Visit<'ast> for CallCollector<'a> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let SynExpr::Path(p) = node.func.as_ref() {
            if let Some(ident) = p.path.get_ident() {
                let name = ident.to_string();
                if !self.shadowed.contains(&name) {
                    self.found.insert(name);
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

fn scan_function(item_fn: &ItemFn) -> BTreeSet<String> {
    let mut shadows = ShadowCollector {
        names: BTreeSet::new(),
    };
    shadows.visit_item_fn(item_fn);

    let mut calls = CallCollector {
        shadowed: &shadows.names,
        found: BTreeSet::new(),
    };
    calls.visit_item_fn(item_fn);
    calls.found
}

/// Determines which functions are reachable (directly or transitively)
/// from `known_functions` but not already in that set -- the exact
/// `newly_required_function_names` payload issue #36 T0 §1.8 describes.
/// Returns them sorted, for a deterministic wire order. Never calls
/// [`crate::rust_frontend::lower_rust_source`]/
/// [`crate::nim_frontend::lower_nim_source`].
pub fn discover_called_functions(
    // Kept for interface consistency with `lower_rust_source`'s own
    // signature (a caller building both from the same file path) --
    // this scan's own diagnostics don't yet tag a per-file provenance
    // the way lowering's IR nodes do.
    _source_file: &Path,
    source_text: &str,
    known_functions: &[&str],
) -> Result<Vec<String>, Vec<Diagnostic>> {
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

    let mut by_name: BTreeMap<String, &ItemFn> = BTreeMap::new();
    for item in &file.items {
        if let Item::Fn(f) = item {
            // First declaration wins for scanning purposes -- a genuine
            // duplicate declaration is `lower_rust_source`'s own
            // diagnostic to raise (`rust_frontend.rs:195-212`) once the
            // closure this scan finds is actually lowered; this scan
            // only needs *a* body to search for call sites in.
            by_name.entry(f.sig.ident.to_string()).or_insert(f);
        }
    }

    let mut known: BTreeSet<String> = known_functions.iter().map(|s| s.to_string()).collect();
    let mut frontier: Vec<String> = known_functions.iter().map(|s| s.to_string()).collect();
    let mut newly_discovered: BTreeSet<String> = BTreeSet::new();

    while let Some(name) = frontier.pop() {
        let Some(item_fn) = by_name.get(name.as_str()) else {
            // Not declared in this file at all -- nothing to scan from
            // here; `lower_rust_source` will diagnose the absence itself
            // if this name ends up in a later closed request.
            continue;
        };
        for callee in scan_function(item_fn) {
            if known.insert(callee.clone()) {
                newly_discovered.insert(callee.clone());
                frontier.push(callee);
            }
        }
    }

    Ok(newly_discovered.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn source_file() -> PathBuf {
        PathBuf::from("test.rs")
    }

    #[test]
    fn a_function_calling_nothing_discovers_nothing_new() {
        let source = "fn f(x: i32) -> i32 { x }";
        let discovered =
            discover_called_functions(&source_file(), source, &["f"]).expect("must parse");
        assert_eq!(discovered, Vec::<String>::new());
    }

    #[test]
    fn a_direct_call_is_discovered() {
        let source = "fn f(x: i32) -> i32 { g(x) }\nfn g(x: i32) -> i32 { x }";
        let discovered =
            discover_called_functions(&source_file(), source, &["f"]).expect("must parse");
        assert_eq!(discovered, vec!["g".to_string()]);
    }

    /// The exact real fixture (issue #35 D0's `add_or_double.rs`) this
    /// discovery mechanism exists for: `add_or_double` calls `double`
    /// only inside the `if` branch, so the call site is nested inside a
    /// block/if-expression, not at the function's own top level -- this
    /// exercises the `Visit` trait's own recursive descent, not just a
    /// flat scan of a function's immediate statements.
    #[test]
    fn the_real_add_or_double_fixture_discovers_double() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let path = repo_root
            .join("fixtures/laminaria-semantic-substrate-prototype/rust-src/add_or_double.rs");
        let source = std::fs::read_to_string(&path).unwrap();
        let discovered =
            discover_called_functions(&path, &source, &["add_or_double"]).expect("must parse");
        assert_eq!(discovered, vec!["double".to_string()]);
    }

    #[test]
    fn a_transitively_reachable_function_is_also_discovered() {
        let source = "fn f(x: i32) -> i32 { g(x) }\n\
                       fn g(x: i32) -> i32 { h(x) }\n\
                       fn h(x: i32) -> i32 { x }";
        let discovered =
            discover_called_functions(&source_file(), source, &["f"]).expect("must parse");
        assert_eq!(discovered, vec!["g".to_string(), "h".to_string()]);
    }

    #[test]
    fn a_call_to_an_already_known_function_is_not_reported_as_newly_discovered() {
        let source = "fn f(x: i32) -> i32 { g(x) }\nfn g(x: i32) -> i32 { x }";
        let discovered =
            discover_called_functions(&source_file(), source, &["f", "g"]).expect("must parse");
        assert_eq!(discovered, Vec::<String>::new());
    }

    /// Mirrors `rust_frontend.rs`'s own
    /// `rejects_calling_a_local_that_shadows_a_declared_functions_name`
    /// exclusion (a local/parameter of the same name is never a callable
    /// value in this subset) -- a `let`-bound name shadowing a real
    /// sibling function must not be reported as a discovered dependency.
    #[test]
    fn a_local_shadowing_a_sibling_functions_name_is_not_discovered() {
        // `g(x)` here is a call-syntax expression whose callee identifier
        // is the `let`-bound local `g`, not the sibling function `g` --
        // syn parses this at the syntax level regardless of real Rust's
        // own type rules (an i32 isn't callable), which is exactly what
        // this scan must not confuse with a real dependency on `fn g`.
        let source = "fn f(x: i32) -> i32 {\n\
                       let g = x;\n\
                       g(x)\n\
                       }\n\
                       fn g(x: i32) -> i32 { x }";
        let discovered =
            discover_called_functions(&source_file(), source, &["f"]).expect("must parse");
        assert_eq!(
            discovered,
            Vec::<String>::new(),
            "the let-bound local `g` shadows the sibling function of the same name, so `g(x)` \
             must never be reported as a discovered dependency on `fn g`"
        );
    }

    #[test]
    fn an_unparseable_file_is_a_diagnostic_not_a_panic() {
        let result = discover_called_functions(&source_file(), "fn f(x: i32 -> i32 { x }", &["f"]);
        assert!(result.is_err());
    }
}

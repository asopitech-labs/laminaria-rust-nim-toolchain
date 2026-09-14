//! Issue #48 (G1) Checkpoint A: a real, generic evaluator for the
//! `#[cfg(...)]` predicates a foreign-function declaration can carry --
//! `unix`, `feature = "..."`, and `all`/`any`/`not` combinations of
//! those. This is what lets ingestion answer "is this FFI requirement
//! actually active for the real target/feature configuration this
//! build uses," rather than reporting every `extern` block a source
//! file's syntax contains regardless of whether it is ever compiled.
//!
//! Deliberately small: this project's own fixtures only ever gate
//! foreign declarations on `unix` and Cargo features, so this module
//! supports exactly that subset plus the boolean combinators, not the
//! full `cfg` grammar (`target_os`, `any`/`all` nesting is supported,
//! `target_feature`, version-numbered `cfg_attr`, etc. are not). Never
//! hard-codes a feature or target name -- the active set comes from the
//! caller's own [`CfgContext`].

use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{LitStr, Token};

use std::collections::BTreeSet;

/// A parsed `#[cfg(...)]` predicate tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgPredicate {
    /// A bare identifier, e.g. `unix`.
    Ident(String),
    /// A `key = "value"` pair, e.g. `feature = "use_nim_double"`.
    KeyValue(String, String),
    All(Vec<CfgPredicate>),
    Any(Vec<CfgPredicate>),
    Not(Box<CfgPredicate>),
}

impl Parse for CfgPredicate {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident: syn::Ident = input.parse()?;
        let name = ident.to_string();
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            let lit: LitStr = input.parse()?;
            return Ok(CfgPredicate::KeyValue(name, lit.value()));
        }
        if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            let items: Vec<CfgPredicate> =
                Punctuated::<CfgPredicate, Token![,]>::parse_terminated(&content)?
                    .into_iter()
                    .collect();
            return match name.as_str() {
                "all" => Ok(CfgPredicate::All(items)),
                "any" => Ok(CfgPredicate::Any(items)),
                "not" => {
                    let mut it = items.into_iter();
                    let inner = it.next().ok_or_else(|| {
                        syn::Error::new(
                            ident.span(),
                            "cfg(not(...)) requires exactly one predicate",
                        )
                    })?;
                    if it.next().is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "cfg(not(...)) requires exactly one predicate",
                        ));
                    }
                    Ok(CfgPredicate::Not(Box::new(inner)))
                }
                other => Err(syn::Error::new(
                    ident.span(),
                    format!(
                        "unsupported cfg combinator '{other}' (only all/any/not are supported)"
                    ),
                )),
            };
        }
        Ok(CfgPredicate::Ident(name))
    }
}

/// The real, observed activation facts a `CfgPredicate` evaluates
/// against -- never invented, always supplied by a caller that
/// actually determined them (e.g. from a real target triple and real
/// `cargo metadata` default-feature resolution).
#[derive(Debug, Clone, Default)]
pub struct CfgContext {
    pub true_idents: BTreeSet<String>,
    pub active_features: BTreeSet<String>,
}

impl CfgPredicate {
    pub fn evaluate(&self, ctx: &CfgContext) -> bool {
        match self {
            CfgPredicate::Ident(name) => ctx.true_idents.contains(name),
            CfgPredicate::KeyValue(key, value) => {
                key == "feature" && ctx.active_features.contains(value)
            }
            CfgPredicate::All(items) => items.iter().all(|p| p.evaluate(ctx)),
            CfgPredicate::Any(items) => items.iter().any(|p| p.evaluate(ctx)),
            CfgPredicate::Not(inner) => !inner.evaluate(ctx),
        }
    }
}

/// Parses the single `#[cfg(...)]` predicate a real attribute carries,
/// or `None` when the attribute is not `cfg` at all.
fn cfg_predicate_from_attr(attr: &syn::Attribute) -> Option<syn::Result<CfgPredicate>> {
    if !attr.path().is_ident("cfg") {
        return None;
    }
    Some(attr.parse_args::<CfgPredicate>())
}

/// Combines every real `#[cfg(...)]` attribute on one item into a
/// single predicate -- multiple stacked `#[cfg(...)]` attributes are
/// Rust's own AND semantics, not an invented convention. `None` means
/// no `#[cfg(...)]` attribute is present at all (unconditionally
/// active).
pub fn combined_cfg_predicate(attrs: &[syn::Attribute]) -> syn::Result<Option<CfgPredicate>> {
    let mut combined: Option<CfgPredicate> = None;
    for attr in attrs {
        if let Some(parsed) = cfg_predicate_from_attr(attr) {
            let predicate = parsed?;
            combined = Some(match combined {
                None => predicate,
                Some(existing) => CfgPredicate::All(vec![existing, predicate]),
            });
        }
    }
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse::Parser;

    fn ctx(true_idents: &[&str], features: &[&str]) -> CfgContext {
        CfgContext {
            true_idents: true_idents.iter().map(|s| s.to_string()).collect(),
            active_features: features.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn parse_one(attr_src: &str) -> CfgPredicate {
        let attrs: Vec<syn::Attribute> = syn::Attribute::parse_outer
            .parse_str(attr_src)
            .expect("must parse attribute");
        combined_cfg_predicate(&attrs)
            .expect("must parse cfg predicate")
            .expect("must find a cfg predicate")
    }

    #[test]
    fn a_bare_ident_evaluates_against_true_idents() {
        let p = parse_one("#[cfg(unix)]");
        assert!(p.evaluate(&ctx(&["unix"], &[])));
        assert!(!p.evaluate(&ctx(&[], &[])));
    }

    #[test]
    fn a_feature_key_value_evaluates_against_active_features() {
        let p = parse_one(r#"#[cfg(feature = "use_nim_double")]"#);
        assert!(p.evaluate(&ctx(&[], &["use_nim_double"])));
        assert!(!p.evaluate(&ctx(&[], &[])));
    }

    #[test]
    fn all_requires_every_branch_true() {
        let p = parse_one(r#"#[cfg(all(unix, feature = "use_nim_double"))]"#);
        assert!(p.evaluate(&ctx(&["unix"], &["use_nim_double"])));
        assert!(!p.evaluate(&ctx(&["unix"], &[])));
        assert!(!p.evaluate(&ctx(&[], &["use_nim_double"])));
    }

    #[test]
    fn not_inverts_its_inner_predicate() {
        let p = parse_one(r#"#[cfg(not(feature = "use_nim_double"))]"#);
        assert!(p.evaluate(&ctx(&[], &[])));
        assert!(!p.evaluate(&ctx(&[], &["use_nim_double"])));
    }

    #[test]
    fn multiple_stacked_cfg_attributes_combine_as_and() {
        let attrs: Vec<syn::Attribute> = syn::Attribute::parse_outer
            .parse_str("#[cfg(unix)] #[cfg(feature = \"use_nim_double\")]")
            .expect("must parse");
        let p = combined_cfg_predicate(&attrs).unwrap().unwrap();
        assert!(p.evaluate(&ctx(&["unix"], &["use_nim_double"])));
        assert!(!p.evaluate(&ctx(&["unix"], &[])));
    }

    #[test]
    fn no_cfg_attribute_means_unconditionally_active() {
        let attrs: Vec<syn::Attribute> = syn::Attribute::parse_outer
            .parse_str(r#"#[link(name = "cadd")]"#)
            .expect("must parse");
        assert_eq!(combined_cfg_predicate(&attrs).unwrap(), None);
    }
}

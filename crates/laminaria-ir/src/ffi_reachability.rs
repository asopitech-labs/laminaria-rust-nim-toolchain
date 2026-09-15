//! Issue #47 (G3, Lane B): a name-matching FFI reachability computation
//! that answers "which of a C source's declared functions does the
//! Rust/Nim caller side actually require?" using only
//! [`crate::foreign_discover::discover_foreign_function_requirements`]
//! and [`crate::c_header_discover::discover_c_declared_functions`] --
//! both pure syntax scans, no compiler invocation, no linker, no
//! `build.rs`. This is the Phase-1-only ("semantic-determinable")
//! reachability computation issue #64's `bcm.c` worked example showed
//! Cargo's own `cargo check` cannot obtain without also paying its
//! native build cost (see
//! `docs/02-research-areas/toolchains/dependency-resolved-artifact-closure_ja.md`,
//! "root集合確定コストの実測"): confirming which C functions a caller
//! reaches by matching real, source-derived name/arity facts, entirely
//! before any `cc` invocation.
//!
//! This module owns only the *matching*, never the discovery: it takes
//! already-discovered fact lists as plain data and reports which
//! declared names the requirement list actually reaches by name, which
//! required names have no matching declaration at all (a genuine
//! provider gap, not this module's business to resolve), and which
//! declared names no requirement reaches (candidates a caller may choose
//! not to materialize). Name-only matching is a real, stated limitation:
//! two C functions sharing a name in different translation units, or a
//! Rust requirement whose true provider is decided by a `#[link]` hint
//! this module does not yet cross-reference, are both out of scope here
//! -- this is the smallest fact this crate can report without inventing
//! anything the input source text does not say.

use crate::c_header_discover::CDeclaredFunction;
use crate::foreign_discover::ForeignFunctionRequirement;

/// One requirement's reachability verdict against a declared-function
/// set: `Reached` names the declaration it matched by name; `Unmatched`
/// means no declaration in the given set has that name at all (the
/// requirement is not satisfiable from this source alone -- a fact the
/// caller must externalize or reject, this module makes no such call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachabilityVerdict {
    Reached { declared_name: String },
    Unmatched,
}

/// The result of matching a set of Rust/Nim-side FFI requirements
/// against a set of C-side declared functions by name: every
/// requirement's own verdict (in the same order the caller supplied
/// them), and the subset of declared functions no requirement reached
/// by name -- the candidate set a summary/body-separation strategy
/// (B-H3) could choose not to materialize, pending whatever further
/// evidence (e.g. cross-translation-unit static coupling, per
/// `scripts/research/measure-bcm-decomposability.sh`'s findings) makes
/// omitting a given one actually safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachabilityReport {
    pub verdicts: Vec<(String, ReachabilityVerdict)>,
    pub unreached_declarations: Vec<String>,
}

/// Matches every `requirement` against `declarations` by name alone.
/// Declaration order and requirement order are both preserved; a name
/// appearing more than once in `declarations` (e.g. re-declared in two
/// headers) is still just "reached" once its name has any match --
/// this module reports reachability, not declaration multiplicity.
pub fn compute_ffi_reachability(
    requirements: &[ForeignFunctionRequirement],
    declarations: &[CDeclaredFunction],
) -> ReachabilityReport {
    let declared_names: std::collections::BTreeSet<&str> =
        declarations.iter().map(|d| d.name.as_str()).collect();

    let mut reached_names: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut verdicts = Vec::with_capacity(requirements.len());
    for req in requirements {
        if declared_names.contains(req.name.as_str()) {
            reached_names.insert(req.name.as_str());
            verdicts.push((
                req.name.clone(),
                ReachabilityVerdict::Reached {
                    declared_name: req.name.clone(),
                },
            ));
        } else {
            verdicts.push((req.name.clone(), ReachabilityVerdict::Unmatched));
        }
    }

    let unreached_declarations: Vec<String> = declarations
        .iter()
        .map(|d| d.name.as_str())
        .filter(|name| !reached_names.contains(name))
        .map(|s| s.to_string())
        .collect();

    ReachabilityReport {
        verdicts,
        unreached_declarations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_header_discover::discover_c_declared_functions;
    use crate::foreign_discover::discover_foreign_function_requirements;

    #[test]
    fn a_single_matching_requirement_is_reached() {
        let rust_src = r#"
            #[link(name = "cadd", kind = "static")]
            extern "C" {
                fn c_add(a: i32, b: i32) -> i32;
            }
        "#;
        let c_src = "int c_add(int a, int b);";
        let requirements = discover_foreign_function_requirements(rust_src).expect("must parse");
        let declarations = discover_c_declared_functions(c_src);
        let report = compute_ffi_reachability(&requirements, &declarations);
        assert_eq!(
            report.verdicts,
            vec![(
                "c_add".to_string(),
                ReachabilityVerdict::Reached {
                    declared_name: "c_add".to_string()
                }
            )]
        );
        assert!(report.unreached_declarations.is_empty());
    }

    #[test]
    fn a_declared_function_no_requirement_names_is_unreached() {
        let rust_src = r#"
            extern "C" {
                fn c_add(a: i32, b: i32) -> i32;
            }
        "#;
        let c_src = r#"
            int c_add(int a, int b);
            int c_subtract(int a, int b);
        "#;
        let requirements = discover_foreign_function_requirements(rust_src).expect("must parse");
        let declarations = discover_c_declared_functions(c_src);
        let report = compute_ffi_reachability(&requirements, &declarations);
        assert_eq!(
            report.unreached_declarations,
            vec!["c_subtract".to_string()]
        );
    }

    #[test]
    fn a_requirement_with_no_matching_declaration_is_unmatched() {
        let rust_src = r#"
            extern "C" {
                fn does_not_exist_anywhere(x: i32) -> i32;
            }
        "#;
        let c_src = "int c_add(int a, int b);";
        let requirements = discover_foreign_function_requirements(rust_src).expect("must parse");
        let declarations = discover_c_declared_functions(c_src);
        let report = compute_ffi_reachability(&requirements, &declarations);
        assert_eq!(
            report.verdicts,
            vec![(
                "does_not_exist_anywhere".to_string(),
                ReachabilityVerdict::Unmatched
            )]
        );
        assert_eq!(report.unreached_declarations, vec!["c_add".to_string()]);
    }

    #[test]
    fn no_requirements_leaves_every_declaration_unreached() {
        let declarations = discover_c_declared_functions("int a(void);\nint b(void);");
        let report = compute_ffi_reachability(&[], &declarations);
        assert!(report.verdicts.is_empty());
        assert_eq!(
            report.unreached_declarations,
            vec!["a".to_string(), "b".to_string()]
        );
    }

    /// Issue #47 (G3): the real G1/G2 `cadd`/`app` fixture pair, not a
    /// synthetic string -- `app/src/main.rs` requires `c_add`,
    /// `cpp_max_i32`, and `nim_double` by real `extern "C"` blocks, but
    /// `cadd.h` only ever declares `c_add` (the C++ and Nim requirements
    /// are satisfied by their own providers, not this header) -- so a
    /// correct reachability computation over this one C header must
    /// report `c_add` reached and the other two requirements unmatched
    /// *by this header alone*, never silently drop them or misreport a
    /// false match.
    #[test]
    fn the_real_cadd_fixture_reaches_exactly_c_add_and_nothing_else() {
        let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let rust_src = std::fs::read_to_string(
            repo_root.join("fixtures/cross-ecosystem-native-executable/app/src/main.rs"),
        )
        .unwrap();
        let c_src = std::fs::read_to_string(
            repo_root.join("fixtures/cross-ecosystem-native-executable/c/cadd/v1/cadd.h"),
        )
        .unwrap();

        let requirements = discover_foreign_function_requirements(&rust_src).expect("must parse");
        let declarations = discover_c_declared_functions(&c_src);
        let report = compute_ffi_reachability(&requirements, &declarations);

        let names: Vec<&str> = requirements.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["c_add", "cpp_max_i32", "nim_double"]);

        assert_eq!(
            report.verdicts,
            vec![
                (
                    "c_add".to_string(),
                    ReachabilityVerdict::Reached {
                        declared_name: "c_add".to_string()
                    }
                ),
                ("cpp_max_i32".to_string(), ReachabilityVerdict::Unmatched),
                ("nim_double".to_string(), ReachabilityVerdict::Unmatched),
            ]
        );
        assert!(report.unreached_declarations.is_empty());
    }
}

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
//! not to materialize).
//!
//! Issue #75 extends the original name-only match with a `param_count`
//! check: a name match whose declared parameter count disagrees is
//! reported as `Reached { arity_mismatch: true, .. }` rather than
//! silently treated as the same match strength as a full agreement --
//! `param_count` is a real, source-derived fact both
//! [`ForeignFunctionRequirement`] and [`CDeclaredFunction`] already
//! carry, so this is refining what this module already had rather than
//! inventing anything. Return-type text is deliberately *not* compared
//! across languages here: Rust's `i32`, C's `int` and (for the Nim
//! variant below) Nim's `cint` are different source vocabularies for
//! what may be the same ABI type, and this module has no cross-language
//! type-equivalence table -- comparing the raw strings would produce
//! false mismatches (e.g. Rust `i32` vs C `int`) more often than it
//! would catch real ones, which is worse than not checking at all. What
//! remains a real, stated limitation: two functions sharing a name in
//! different translation units, or a Rust requirement whose true
//! provider is decided by a `#[link]` hint this module does not yet
//! cross-reference, are both out of scope here -- this is the smallest
//! fact this crate can report without inventing anything the input
//! source text does not say.

use crate::c_header_discover::CDeclaredFunction;
use crate::foreign_discover::ForeignFunctionRequirement;
use crate::nim_export_discover::NimExportedProc;

/// One requirement's reachability verdict against a declared-function
/// set: `Reached` names the declaration it matched by name, and reports
/// whether the two sides' declared parameter counts actually agree
/// (`arity_mismatch: false` for full agreement); `Unmatched` means no
/// declaration in the given set has that name at all (the requirement
/// is not satisfiable from this source alone -- a fact the caller must
/// externalize or reject, this module makes no such call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachabilityVerdict {
    Reached {
        declared_name: String,
        arity_mismatch: bool,
    },
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

/// Matches every `requirement` against `declarations` by name, then
/// checks `param_count` agreement for each name match. Declaration
/// order and requirement order are both preserved; a name appearing
/// more than once in `declarations` (e.g. re-declared in two headers)
/// is still just "reached" once its name has any match -- this module
/// reports reachability, not declaration multiplicity. When a name
/// matches more than one declaration with disagreeing `param_count`,
/// the requirement is reported as reached with `arity_mismatch: true`
/// as soon as at least one candidate disagrees, since this module has
/// no further fact (e.g. a `#[link]` hint) to pick among same-named
/// candidates -- see the module doc's stated limitation.
pub fn compute_ffi_reachability(
    requirements: &[ForeignFunctionRequirement],
    declarations: &[CDeclaredFunction],
) -> ReachabilityReport {
    let mut declared_by_name: std::collections::BTreeMap<&str, Vec<usize>> =
        std::collections::BTreeMap::new();
    for d in declarations {
        declared_by_name
            .entry(d.name.as_str())
            .or_default()
            .push(d.param_count);
    }

    let mut reached_names: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut verdicts = Vec::with_capacity(requirements.len());
    for req in requirements {
        if let Some(arities) = declared_by_name.get(req.name.as_str()) {
            reached_names.insert(req.name.as_str());
            let arity_mismatch = arities.iter().any(|&count| count != req.param_count);
            verdicts.push((
                req.name.clone(),
                ReachabilityVerdict::Reached {
                    declared_name: req.name.clone(),
                    arity_mismatch,
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

/// The Rust↔Nim counterpart to [`compute_ffi_reachability`]: matches
/// Rust `extern` requirements against Nim `{.exportc.}` declarations by
/// `exported_symbol` (the real link-time name, mirroring how
/// [`ForeignFunctionRequirement::name`] is the real `extern` symbol
/// name -- not `declared_name`, which may differ under
/// `{.exportc: "other_name".}`), with the same `param_count` agreement
/// check. No Rust↔Nim matching existed in this crate before issue #75;
/// this is new, not a refinement of an existing check.
pub fn compute_ffi_reachability_nim(
    requirements: &[ForeignFunctionRequirement],
    declarations: &[NimExportedProc],
) -> ReachabilityReport {
    let mut declared_by_symbol: std::collections::BTreeMap<&str, Vec<usize>> =
        std::collections::BTreeMap::new();
    for d in declarations {
        declared_by_symbol
            .entry(d.exported_symbol.as_str())
            .or_default()
            .push(d.param_count);
    }

    let mut reached_symbols: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut verdicts = Vec::with_capacity(requirements.len());
    for req in requirements {
        if let Some(arities) = declared_by_symbol.get(req.name.as_str()) {
            reached_symbols.insert(req.name.as_str());
            let arity_mismatch = arities.iter().any(|&count| count != req.param_count);
            verdicts.push((
                req.name.clone(),
                ReachabilityVerdict::Reached {
                    declared_name: req.name.clone(),
                    arity_mismatch,
                },
            ));
        } else {
            verdicts.push((req.name.clone(), ReachabilityVerdict::Unmatched));
        }
    }

    let unreached_declarations: Vec<String> = declarations
        .iter()
        .map(|d| d.exported_symbol.as_str())
        .filter(|symbol| !reached_symbols.contains(symbol))
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
                    declared_name: "c_add".to_string(),
                    arity_mismatch: false,
                }
            )]
        );
        assert!(report.unreached_declarations.is_empty());
    }

    /// Issue #75: a name match whose declared parameter count actually
    /// disagrees must not be reported the same as full agreement --
    /// name-only matching alone cannot tell `c_add(a, b)` (2 params)
    /// apart from a C declaration accidentally sharing the name with a
    /// different arity (a real hazard the module doc's original
    /// "stated limitation" left unaddressed).
    #[test]
    fn a_name_match_with_disagreeing_arity_is_reached_with_mismatch() {
        let rust_src = r#"
            extern "C" {
                fn c_add(a: i32, b: i32) -> i32;
            }
        "#;
        let c_src = "int c_add(int a);";
        let requirements = discover_foreign_function_requirements(rust_src).expect("must parse");
        let declarations = discover_c_declared_functions(c_src);
        let report = compute_ffi_reachability(&requirements, &declarations);
        assert_eq!(
            report.verdicts,
            vec![(
                "c_add".to_string(),
                ReachabilityVerdict::Reached {
                    declared_name: "c_add".to_string(),
                    arity_mismatch: true,
                }
            )]
        );
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
                        declared_name: "c_add".to_string(),
                        arity_mismatch: false,
                    }
                ),
                ("cpp_max_i32".to_string(), ReachabilityVerdict::Unmatched),
                ("nim_double".to_string(), ReachabilityVerdict::Unmatched),
            ]
        );
        assert!(report.unreached_declarations.is_empty());
    }

    // -- Rust↔Nim matching (issue #75) --------------------------------

    #[test]
    fn a_rust_requirement_reaches_a_matching_nim_export_by_symbol() {
        let rust_src = r#"
            extern "C" {
                fn nim_double(x: i32) -> i32;
            }
        "#;
        let requirements = discover_foreign_function_requirements(rust_src).expect("must parse");
        let declarations = vec![NimExportedProc {
            declared_name: "double".to_string(),
            exported_symbol: "nim_double".to_string(),
            param_count: 1,
            return_type: "cint".to_string(),
        }];
        let report = compute_ffi_reachability_nim(&requirements, &declarations);
        assert_eq!(
            report.verdicts,
            vec![(
                "nim_double".to_string(),
                ReachabilityVerdict::Reached {
                    declared_name: "nim_double".to_string(),
                    arity_mismatch: false,
                }
            )]
        );
        assert!(report.unreached_declarations.is_empty());
    }

    #[test]
    fn a_rust_requirement_matching_by_symbol_but_disagreeing_arity_is_flagged() {
        let rust_src = r#"
            extern "C" {
                fn nim_add(a: i32, b: i32) -> i32;
            }
        "#;
        let requirements = discover_foreign_function_requirements(rust_src).expect("must parse");
        let declarations = vec![NimExportedProc {
            declared_name: "add".to_string(),
            exported_symbol: "nim_add".to_string(),
            param_count: 1,
            return_type: "cint".to_string(),
        }];
        let report = compute_ffi_reachability_nim(&requirements, &declarations);
        assert_eq!(
            report.verdicts,
            vec![(
                "nim_add".to_string(),
                ReachabilityVerdict::Reached {
                    declared_name: "nim_add".to_string(),
                    arity_mismatch: true,
                }
            )]
        );
    }

    /// A Nim proc's `declared_name` differing from its `exported_symbol`
    /// (`{.exportc: "other_name".}`) must not itself cause a spurious
    /// match or mismatch -- matching goes by `exported_symbol` only,
    /// the real link-time name, never `declared_name`.
    #[test]
    fn matching_uses_exported_symbol_not_declared_name() {
        let rust_src = r#"
            extern "C" {
                fn nim_scale(x: i32, factor: i32) -> i32;
            }
        "#;
        let requirements = discover_foreign_function_requirements(rust_src).expect("must parse");
        let declarations = vec![NimExportedProc {
            declared_name: "scaleValue".to_string(),
            exported_symbol: "nim_scale".to_string(),
            param_count: 2,
            return_type: "cint".to_string(),
        }];
        let report = compute_ffi_reachability_nim(&requirements, &declarations);
        assert_eq!(
            report.verdicts,
            vec![(
                "nim_scale".to_string(),
                ReachabilityVerdict::Reached {
                    declared_name: "nim_scale".to_string(),
                    arity_mismatch: false,
                }
            )]
        );
    }

    /// Issue #75's real fixture measurement: `nimlib.nim`'s two
    /// `{.exportc.}` procs (`nim_array_stats`, `nim_array_scale_evens`)
    /// against a synthetic Rust `extern` block naming both plus one the
    /// fixture does not export -- confirms the new matcher works over
    /// real, unmodified fixture Nim source, not just synthetic strings.
    #[test]
    fn the_real_mixed_rust_nim_fixture_nim_side_matches_by_symbol_and_arity() {
        let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let nim_src = std::fs::read_to_string(
            repo_root.join("fixtures/mixed-rust-nim-executable/nim-lib/nimlib.nim"),
        )
        .unwrap();
        let declarations = crate::nim_export_discover::discover_exportc_declarations(&nim_src)
            .expect("fixture must have well-formed signatures");

        let rust_src = r#"
            extern "C" {
                fn nim_array_stats(data: *const i32, len: i32, out_sum: *mut i64, out_min: *mut i32, out_max: *mut i32, out_mean_x1000: *mut i64);
                fn nim_array_scale_evens(data: *mut i32, len: i32, factor: i32);
                fn nim_not_exported(x: i32) -> i32;
            }
        "#;
        let requirements = discover_foreign_function_requirements(rust_src).expect("must parse");
        let report = compute_ffi_reachability_nim(&requirements, &declarations);

        assert_eq!(
            report.verdicts,
            vec![
                (
                    "nim_array_stats".to_string(),
                    ReachabilityVerdict::Reached {
                        declared_name: "nim_array_stats".to_string(),
                        arity_mismatch: false,
                    }
                ),
                (
                    "nim_array_scale_evens".to_string(),
                    ReachabilityVerdict::Reached {
                        declared_name: "nim_array_scale_evens".to_string(),
                        arity_mismatch: false,
                    }
                ),
                (
                    "nim_not_exported".to_string(),
                    ReachabilityVerdict::Unmatched
                ),
            ]
        );
        assert!(report.unreached_declarations.is_empty());
    }

    /// Issue #75's real-measurement pass: run the Rust↔Nim matcher over
    /// every fixture pair this repo has that names its Nim `{.exportc.}`
    /// procs from a Rust `extern "C"` block, and record (via test
    /// output, `cargo test -- --nocapture`) whether the previously
    /// unimplemented Rust↔Nim path finds any arity mismatch a
    /// name-only check would have missed. This is not a synthetic
    /// worked example -- it is the actual fixture corpus.
    #[test]
    fn real_fixture_corpus_rust_nim_arity_measurement() {
        let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();

        // (rust source, nim source) pairs -- every fixture directory in
        // this repo that has both a Rust `extern "C"` side and a Nim
        // `{.exportc.}` side as of issue #75.
        let pairs = [
            (
                "rust-nim-c-abi-baseline",
                "rust-bin/src/main.rs",
                "nim-lib/nimlib.nim",
            ),
            (
                "boundary-heavy-workload",
                "rust-bin/src/main.rs",
                "nim-lib/nimlib.nim",
            ),
            (
                "mixed-rust-nim-executable",
                "rust-bin/src/main.rs",
                "nim-lib/nimlib.nim",
            ),
            (
                "llvm-rediscovery-semantic-workload",
                "rust-src/add.rs",
                "nim-src/add.nim",
            ),
            (
                "rust-nim-llvm-lto-compatibility",
                "rust-src/lib.rs",
                "nim-src/main.nim",
            ),
        ];

        let mut total_reached = 0usize;
        let mut total_mismatched = 0usize;
        let mut total_unmatched = 0usize;

        for (fixture, rust_rel, nim_rel) in pairs {
            let rust_src =
                std::fs::read_to_string(repo_root.join("fixtures").join(fixture).join(rust_rel))
                    .unwrap_or_else(|e| panic!("{fixture}/{rust_rel}: {e}"));
            let nim_src =
                std::fs::read_to_string(repo_root.join("fixtures").join(fixture).join(nim_rel))
                    .unwrap_or_else(|e| panic!("{fixture}/{nim_rel}: {e}"));

            let requirements = discover_foreign_function_requirements(&rust_src)
                .unwrap_or_else(|e| panic!("{fixture}: Rust side failed to parse: {e:?}"));
            let declarations = crate::nim_export_discover::discover_exportc_declarations(&nim_src)
                .unwrap_or_else(|e| panic!("{fixture}: Nim side failed to parse: {e}"));

            let report = compute_ffi_reachability_nim(&requirements, &declarations);

            println!("=== fixture: {fixture} ===");
            println!(
                "  Rust extern requirements: {}",
                requirements
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "  Nim exportc declarations: {}",
                declarations
                    .iter()
                    .map(|d| d.exported_symbol.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for (name, verdict) in &report.verdicts {
                match verdict {
                    ReachabilityVerdict::Reached {
                        arity_mismatch: false,
                        ..
                    } => {
                        total_reached += 1;
                        println!("  {name}: reached, arity agrees");
                    }
                    ReachabilityVerdict::Reached {
                        arity_mismatch: true,
                        ..
                    } => {
                        total_mismatched += 1;
                        println!("  {name}: reached, ARITY MISMATCH");
                    }
                    ReachabilityVerdict::Unmatched => {
                        total_unmatched += 1;
                        println!(
                            "  {name}: unmatched (no same-named Nim exportc symbol in this file -- \
                             may be a Rust-runtime symbol like NimMain(), or provided from elsewhere)"
                        );
                    }
                }
            }
            if !report.unreached_declarations.is_empty() {
                println!(
                    "  Nim exports no Rust extern in this file reaches: {}",
                    report.unreached_declarations.join(", ")
                );
            }
        }

        println!(
            "=== summary: {total_reached} reached/agree, {total_mismatched} reached/MISMATCH, {total_unmatched} unmatched across {} fixture pairs ===",
            pairs.len()
        );

        // The real finding this measurement pass exists to record: with
        // real, unmodified fixture source, no arity mismatch was found
        // in this repo's current corpus -- the fixtures are internally
        // consistent. The value of this test is in the `Unmatched`
        // count instead: `NimMain()` (Nim's own runtime-initialization
        // symbol, not a `{.exportc.}` proc in `nimlib.nim`) reliably
        // shows up as `Unmatched` in `rust-nim-c-abi-baseline`,
        // `boundary-heavy-workload` and `mixed-rust-nim-executable`,
        // demonstrating the matcher correctly reports "no same-named
        // Nim export in this file" rather than silently or incorrectly
        // matching a runtime-provided symbol.
        assert_eq!(
            total_mismatched, 0,
            "expected no arity mismatches in this repo's current fixture corpus; \
             found some -- see stdout for which fixture (run with --nocapture)"
        );
        assert!(
            total_unmatched > 0,
            "expected at least one Unmatched verdict (NimMain() has no same-named \
             {{.exportc.}} proc) -- if this now fails, the fixture corpus changed \
             and this test's own docs need re-checking, not silently relaxing"
        );
    }
}

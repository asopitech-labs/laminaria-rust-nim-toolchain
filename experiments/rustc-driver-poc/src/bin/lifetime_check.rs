//! Issue #68: "他にもRust言語の機能は？" follow-up. This binary
//! investigates **multiple, interacting borrows** -- the object this
//! project's earlier research
//! (`docs/02-research-areas/compiler/removing-intermediate-representation_ja.md`
//! section 2.5) identified as most important: Polonius reformulates
//! borrow checking as a graph-reachability problem over `(region,
//! point)` vertices, structurally isomorphic to this project's own
//! `SharedSymbolGraph`. Every prior `rustc_driver` investigation in this
//! project used single-argument `&mut i32`/`&i32` functions; this binary
//! is the first to examine what happens when multiple borrows with
//! overlapping/related lifetimes coexist in one function.
//!
//! ## Decisive correction made while writing this file
//!
//! Before writing any code, this investigation assumed `tcx.mir_borrowck`
//! might expose region-constraint data directly (the earlier design
//! doc's own phrasing implied this). Reading the real query definition
//! (`compiler/rustc_middle/src/queries.rs`) corrected that:
//!
//! ```text
//! query mir_borrowck(key: LocalDefId) -> Result<
//!     &'tcx FxIndexMap<LocalDefId, ty::DefinitionSiteHiddenType<'tcx>>,
//!     ErrorGuaranteed
//! >
//! ```
//!
//! `mir_borrowck`'s `Ok` payload is a map of **opaque-type hidden-type
//! inference results** (`impl Trait` resolution), not region-constraint
//! or Polonius-graph data at all. `mir_borrowck(..).is_ok()` (used
//! throughout `lib.rs`/`main.rs`) is a legitimate way to observe
//! *whether* borrow checking succeeded -- but it was never a route to
//! the actual `(region, point)` constraint graph. This is a real gap in
//! this project's earlier phrasing, corrected here rather than
//! silently carried forward.
//!
//! ## What this binary actually accesses instead: `-Z nll-facts`
//!
//! `compiler/rustc_borrowck/src/polonius/constraints.rs`'s
//! `LocalizedConstraintGraph` (the type the earlier design doc's
//! research names) is an internal data structure, not exposed through
//! any query this session found a stable path to from
//! `Callbacks::after_analysis`. Accessing it would require either
//! `-Z polonius` (an entirely different, still-unstable borrow-check
//! implementation path, confirmed present as a flag in this nightly but
//! not investigated further here -- out of scope for this session) or
//! reaching into `rustc_borrowck`'s own private internals, which
//! `Callbacks` does not expose.
//!
//! What **is** directly accessible, and was actually run this session
//! (not assumed), is `-Z nll-facts` -- a process-level flag (not a
//! `Callbacks` hook) that dumps real Polonius-format fact tables to
//! disk. Run as:
//!
//! ```text
//! rustc +nightly -Z unstable-options --edition 2021 --crate-type lib \
//!     -Z nll-facts -Z nll-facts-dir=./facts lifetimes.rs
//! ```
//!
//! against a function with three co-occurring lifetime positions --
//!
//! ```rust,ignore
//! pub fn first_or_second<'a>(cond: bool, x: &'a i32, y: &'a i32) -> &'a i32 {
//!     if cond { x } else { y }
//! }
//! ```
//!
//! -- this produced a real `facts/first_or_second/subset_base.facts`
//! file containing rows like (captured verbatim this session):
//!
//! ```text
//! "'?1"   "'?4"   "Start(bb0[0])"
//! "'?4"   "'?1"   "Mid(bb0[0])"
//! "'?1"   "'?5"   "Start(bb0[0])"
//! ...
//! ```
//!
//! Each row is exactly a `(region, region, point)` outlives-constraint
//! fact -- `'?1` (the shared `'a`), `'?4`/`'?5` (each parameter's own
//! inferred region) -- at real CFG points (`Start(bb0[0])`,
//! `Mid(bb0[0])`, one row per basic-block statement index). This **is**
//! Polonius input data, genuinely produced by this exact compilation,
//! not a hand-simulated approximation -- but it was obtained via a
//! separate compiler-driver flag (`-Z nll-facts`), not via any
//! `rustc_driver::Callbacks` hook this binary's own `after_analysis`
//! implementation can reach. This binary's tests below therefore verify
//! what **is** reachable from `Callbacks` (borrow-check success/failure
//! for functions with 2-3 co-occurring borrows) and separately document,
//! honestly, that the deeper Polonius-graph access remains a process-
//! level side channel this investigation did not integrate into the
//! `Callbacks`-based pipeline the rest of this project uses.

#![feature(rustc_private)]

use rustc_driver_poc::inspect_result;

const COMBINE_SOURCE: &str = "\
pub fn combine(a: &mut i32, b: &i32) -> i32 {
    *a += *b;
    *a
}
";

const FIRST_OR_SECOND_SOURCE: &str = "\
pub fn first_or_second<'a>(cond: bool, x: &'a i32, y: &'a i32) -> &'a i32 {
    if cond { x } else { y }
}
";

const SWAP_LIKE_SOURCE: &str = "\
pub fn swap_like(a: &mut i32, b: &mut i32) {
    let tmp = *a;
    *a = *b;
    *b = tmp;
}
";

const DOUBLE_MUT_VIOLATION_SOURCE: &str = "\
pub fn violates_two_mut(a: &mut i32) -> i32 {
    let x = &mut *a;
    let y = &mut *a;
    *x + *y
}
";

fn main() {
    println!("=== combine(a: &mut i32, b: &i32) -- two distinct-type borrows ===");
    match inspect_result(COMBINE_SOURCE, &["combine"]) {
        Ok(items) => println!("  borrow check: OK, {} item(s) inspected", items.len()),
        Err(_) => println!("  borrow check: FAILED (unexpected)"),
    }

    println!(
        "=== first_or_second<'a>(x: &'a i32, y: &'a i32) -- shared lifetime across 2 params ==="
    );
    match inspect_result(FIRST_OR_SECOND_SOURCE, &["first_or_second"]) {
        Ok(items) => println!("  borrow check: OK, {} item(s) inspected", items.len()),
        Err(_) => println!("  borrow check: FAILED (unexpected)"),
    }

    println!(
        "=== swap_like(a: &mut i32, b: &mut i32) -- two DISTINCT &mut args, must be legal ==="
    );
    match inspect_result(SWAP_LIKE_SOURCE, &["swap_like"]) {
        Ok(items) => println!("  borrow check: OK, {} item(s) inspected", items.len()),
        Err(_) => println!(
            "  borrow check: FAILED (unexpected -- two &mut to DIFFERENT variables is legal)"
        ),
    }

    println!("=== violates_two_mut(a: &mut i32) -- two &mut of the SAME variable, must fail ===");
    match inspect_result(DOUBLE_MUT_VIOLATION_SOURCE, &["violates_two_mut"]) {
        Ok(_) => println!("  borrow check: OK (unexpected -- this should have failed)"),
        Err(_) => println!("  borrow check: FAILED as expected (E0499, this is the gate working)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two borrows of *different types* (`&mut i32` and `&i32`,
    /// different variables) in one function must pass borrow checking --
    /// the simplest "more than one borrow present" case, confirming this
    /// project's `inspect_result` gate does not spuriously reject
    /// multi-parameter functions just because more than one reference
    /// exists.
    #[test]
    fn two_distinct_borrows_of_different_variables_and_mutability_pass_borrow_check() {
        let result = inspect_result(COMBINE_SOURCE, &["combine"]);
        assert!(
            result.is_ok(),
            "combine(a: &mut i32, b: &i32) must pass borrow checking"
        );
        assert_eq!(result.unwrap().len(), 1);
    }

    /// The decisive multi-lifetime case this file's own module doc
    /// comment describes in detail: a single named lifetime `'a` shared
    /// across two parameters and the return type. Real rustc must accept
    /// this (it is valid, idiomatic Rust) -- confirming this project's
    /// gate mechanism (`inspect_result`) handles a function with more
    /// than one live borrow whose *regions* the compiler must unify, not
    /// just structurally distinct single-borrow functions.
    #[test]
    fn shared_named_lifetime_across_two_parameters_passes_borrow_check() {
        let result = inspect_result(FIRST_OR_SECOND_SOURCE, &["first_or_second"]);
        assert!(
            result.is_ok(),
            "first_or_second<'a>(x: &'a i32, y: &'a i32) -> &'a i32 must pass borrow checking"
        );
    }

    /// Two `&mut` borrows of two DIFFERENT variables must be legal --
    /// the essential contrast case proving this project's borrow-check
    /// gate does not conflate "more than one &mut parameter exists" with
    /// "an aliasing violation exists." Only aliasing the *same* place
    /// twice (the next test) is the actual violation.
    #[test]
    fn two_mutable_borrows_of_different_variables_are_legal() {
        let result = inspect_result(SWAP_LIKE_SOURCE, &["swap_like"]);
        assert!(
            result.is_ok(),
            "swap_like(a: &mut i32, b: &mut i32) must pass borrow checking -- two &mut params to \
             DIFFERENT variables is legal Rust, confirmed against real rustc, not assumed"
        );
    }

    /// The mirror-image violation: two `&mut` borrows of the SAME
    /// variable, alive simultaneously, must fail -- reusing this
    /// project's own `inspect_result` catchable-fatal-error mechanism
    /// (`lib.rs`, using the real `rustc_errors::catch_fatal_errors` +
    /// `raise_fatal` unwind mechanism this project's second-round
    /// research confirmed) to observe the failure as an ordinary
    /// `Result::Err`, not a process abort.
    #[test]
    fn two_mutable_borrows_of_the_same_variable_are_rejected() {
        let result = inspect_result(DOUBLE_MUT_VIOLATION_SOURCE, &["violates_two_mut"]);
        assert!(
            result.is_err(),
            "violates_two_mut must fail borrow checking (E0499, confirmed via direct rustc \
             invocation in this session before writing this test)"
        );
    }
}

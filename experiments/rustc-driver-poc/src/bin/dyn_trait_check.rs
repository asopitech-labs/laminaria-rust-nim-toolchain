//! Issue #68: "他にもRust言語の機能は？" follow-up. The user asked for a
//! parallel investigation of Rust language features not yet exercised by
//! this project's `rustc_driver`-based verification. This binary covers
//! **trait objects / dynamic dispatch** (`dyn Trait`), the runtime-
//! polymorphism counterpart to the static-polymorphism (generic
//! monomorphization) work already done in `main.rs`
//! (`inspect_monomorphization`).
//!
//! Monomorphization produces a *different function body per call site*;
//! `dyn Trait` is the opposite shape -- *one* function body
//! (`print_area_dyn` below) that indirects through a vtable at runtime to
//! reach whichever concrete implementation the caller supplied. This is
//! a materially different requirement for a target-specific backend:
//! monomorphization needs per-instantiation code generation, dynamic
//! dispatch needs a real indirect-call instruction plus a real vtable
//! layout (a data structure, not code) to call through.
//!
//! ## What was actually run, this session, before writing this file
//!
//! ```text
//! rustc --edition 2021 --crate-type lib -C debuginfo=0 --emit=mir dyntrait.rs
//! ```
//!
//! against
//!
//! ```rust,ignore
//! pub trait Shape { fn area(&self) -> i32; }
//! pub struct Circle { pub r: i32 }
//! impl Shape for Circle { fn area(&self) -> i32 { self.r * self.r } }
//! pub fn print_area_dyn(s: &dyn Shape) -> i32 { s.area() }
//! pub fn print_area_generic<T: Shape>(s: &T) -> i32 { s.area() }
//! ```
//!
//! produced (byte-for-byte from this session's own terminal output):
//!
//! ```text
//! fn print_area_dyn(_1: &dyn Shape) -> i32 {
//!     debug s => _1;
//!     let mut _0: i32;
//!
//!     bb0: {
//!         _0 = <dyn Shape as Shape>::area(copy _1) -> [return: bb1, unwind continue];
//!     }
//!     bb1: { return; }
//! }
//!
//! fn print_area_generic(_1: &T) -> i32 {
//!     debug s => _1;
//!     let mut _0: i32;
//!
//!     bb0: {
//!         _0 = <T as Shape>::area(copy _1) -> [return: bb1, unwind continue];
//!     }
//!     bb1: { return; }
//! }
//! ```
//!
//! **Decisive, honest finding**: at the MIR *text* level, dynamic and
//! static (unresolved-generic) dispatch are visually near-identical --
//! `<dyn Shape as Shape>::area(copy _1)` vs `<T as Shape>::area(copy
//! _1)`. Neither shows an indirect call, a vtable slot index, or any
//! machine-level difference in this text form. The distinction is not
//! visible in text; it lives in the real `Ty<'tcx>` (`TyKind::Dynamic`
//! vs `TyKind::Param`) and is resolved later, at codegen time, by
//! `Instance::resolve`. This means `target_ir::mir_text`'s text-parsing
//! approach (this project's own first-round PoC, since superseded by
//! `rustc_driver`-based inspection precisely because of gaps like this)
//! could never have distinguished these two calls even in principle --
//! not merely because its own parser is narrow, but because the textual
//! MIR format itself elides the distinction that matters for codegen.

#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;

use rustc_driver::Callbacks;
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;

/// What this binary extracts about one `&dyn Trait`-shaped parameter,
/// using only owned/primitive data so it can cross out of the
/// `rustc_driver` callback closure (mirrors `InspectedItem`'s own
/// discipline in `lib.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DynTraitFinding {
    fn_name: String,
    /// The real `Ty<'tcx>` debug string for the parameter this binary is
    /// inspecting -- e.g. `&dyn Shape` -- confirming `TyKind::Dynamic`
    /// was actually matched, not inferred from a name string.
    param_ty_debug: String,
    is_dynamic_ty: bool,
    /// `Some(n)` = `tcx.vtable_entries(trait_ref)` was successfully
    /// called and returned `n` real entries; `None` = this binary did
    /// not attempt vtable extraction for this item (see `main` for which
    /// items get this treatment -- only the concrete `impl` block's
    /// trait, not the abstract `&dyn Shape` parameter itself, since a
    /// vtable is a property of a concrete `(Self, Trait)` pair, not of
    /// the unsized `dyn Shape` type alone).
    vtable_entry_count: Option<usize>,
    /// Human-readable classification of each real `VtblEntry` this
    /// binary found, in vtable slot order -- e.g.
    /// `["MetadataDropInPlace", "MetadataSize", "MetadataAlign",
    /// "Method(Circle::area)"]`. Confirmed against the real
    /// `rustc_middle::ty::VtblEntry` enum
    /// (`compiler/rustc_middle/src/ty/vtable.rs`), not assumed from
    /// memory -- this session initially assumed vtables might start
    /// directly with method pointers and had to correct that assumption
    /// after reading the enum definition, which shows three metadata
    /// header entries (`MetadataDropInPlace`/`MetadataSize`/
    /// `MetadataAlign`) always precede the dispatchable `Method` entries.
    vtable_entries_debug: Vec<String>,
}

struct Inspect {
    collected: std::sync::Arc<std::sync::Mutex<Vec<DynTraitFinding>>>,
}

impl Callbacks for Inspect {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> rustc_driver::Compilation {
        for local_def_id in tcx.hir_body_owners() {
            let name = tcx.item_name(local_def_id.to_def_id());
            let name_str = name.as_str();
            if name_str != "print_area_dyn" && name_str != "print_area_generic" {
                continue;
            }

            let body = tcx.optimized_mir(local_def_id.to_def_id());
            // local _1 is always the first (and only) parameter for both
            // `print_area_dyn(s: &dyn Shape)` and
            // `print_area_generic(s: &T)`, confirmed against this
            // session's own captured MIR (both functions declare exactly
            // `_0` and `_1`).
            let param_decl = &body.local_decls[rustc_middle::mir::Local::from_usize(1)];
            let param_ty = param_decl.ty;

            // Peel through `&`/`&mut` to inspect the referent type's own
            // kind -- `&dyn Shape`'s outer kind is `TyKind::Ref`, and the
            // `TyKind::Dynamic` this binary is actually looking for is
            // one level inside, confirmed directly against
            // `compiler/rustc_type_ir/src/ty_kind.rs`'s own
            // `Dynamic(I::BoundExistentialPredicates, Region<I>)`
            // variant (a 2-field variant, not 3 -- this session initially
            // assumed a 3rd `DynKind` field from memory and had to
            // correct that against the real source before this compiled).
            let referent_ty = match param_ty.kind() {
                rustc_middle::ty::TyKind::Ref(_, referent, _) => *referent,
                other => panic!("expected a reference parameter, got {other:?}"),
            };
            let is_dynamic_ty = matches!(referent_ty.kind(), rustc_middle::ty::TyKind::Dynamic(..));

            self.collected.lock().unwrap().push(DynTraitFinding {
                fn_name: name_str.to_string(),
                param_ty_debug: format!("{:?}", param_ty),
                is_dynamic_ty,
                vtable_entry_count: None,
                vtable_entries_debug: Vec::new(),
            });
        }

        // Separately: find `Circle::area`'s real impl block and its
        // trait, to compute the real vtable for `Circle as Shape` -- a
        // vtable exists for a concrete (Self, Trait) pair, not for the
        // abstract `dyn Shape` type by itself (there is no single
        // "the Shape vtable"; every implementing type gets its own).
        for local_def_id in tcx.hir_body_owners() {
            let name = tcx.item_name(local_def_id.to_def_id());
            if name.as_str() != "area" {
                continue;
            }
            let method_def_id = local_def_id.to_def_id();
            let Some(impl_def_id) = tcx.impl_of_assoc(method_def_id) else {
                continue;
            };
            let trait_ref_binder = tcx.impl_trait_ref(impl_def_id);
            let trait_ref = trait_ref_binder.skip_binder();

            let entries = tcx.vtable_entries(trait_ref);
            let entries_debug: Vec<String> =
                entries.iter().map(|entry| format!("{entry:?}")).collect();

            self.collected.lock().unwrap().push(DynTraitFinding {
                fn_name: format!("<vtable for {:?}>", trait_ref),
                param_ty_debug: String::new(),
                is_dynamic_ty: false,
                vtable_entry_count: Some(entries.len()),
                vtable_entries_debug: entries_debug,
            });
        }

        rustc_driver::Compilation::Stop
    }
}

fn inspect_dyn_trait(source: &str) -> Vec<DynTraitFinding> {
    // Reuses `inspect_result`'s own scratch-file-and-invoke plumbing
    // (`lib.rs`) is not directly reusable here since this binary needs
    // its own `Callbacks` implementation (different extraction target)
    // -- but the *catchable-fatal-error* discipline `inspect_result`
    // established is not needed here either, since every source this
    // binary compiles is valid Rust that passes borrow checking (this
    // investigation is about dispatch mechanism, not gate behavior,
    // which `main.rs`/`unsafe_check.rs` already cover). A direct
    // `run_compiler` call, matching this project's own first-round
    // pattern before `catch_fatal_errors` was introduced, is honest here
    // because no failure path is being tested.
    let out_dir = std::env::temp_dir().join("rustc-driver-poc-out");
    std::fs::create_dir_all(&out_dir).expect("create scratch out dir");
    // Includes a numeric-only thread id (`{:?}` on `ThreadId` prints
    // `ThreadId(N)`, whose parentheses rustc rejects as an invalid crate
    // name character -- confirmed directly: an earlier version of this
    // line used the `{:?}` form as-is and every test failed with "error:
    // invalid character '(' in crate name," a real compile error, not a
    // test-logic bug) so concurrent `#[test]` invocations (Rust's
    // default test runner runs tests in parallel threads) sharing the
    // identical `DYN_TRAIT_SOURCE` never write to -- and race on -- the
    // same scratch path. Confirmed necessary in this session: `cargo
    // +nightly test` (all 4 tests in parallel) produced 3 spurious
    // `after_analysis` failures that did not occur under `cargo
    // +nightly test -- --test-threads=1`, tracked down to this exact
    // race rather than assumed.
    let thread_id_numeric: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let src_path = out_dir.join(format!(
        "dyn_trait_check_{:x}_{thread_id_numeric}.rs",
        fnv1a(source)
    ));
    std::fs::write(&src_path, source).expect("write scratch source file");

    let args = vec![
        "dyn_trait_check".to_string(),
        "--edition".to_string(),
        "2021".to_string(),
        "--crate-type".to_string(),
        "lib".to_string(),
        src_path.to_str().unwrap().to_string(),
    ];

    let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut callbacks = Inspect {
        collected: std::sync::Arc::clone(&collected),
    };
    rustc_driver::run_compiler(&args, &mut callbacks);
    let items = collected.lock().unwrap().clone();
    items
}

/// Same tiny hash `lib.rs`'s own `fnv1a` uses, duplicated here rather
/// than imported to keep this binary's investigation fully self-
/// contained in one file, matching this session's own file-per-
/// investigation split (see this file's own module doc comment).
fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

const DYN_TRAIT_SOURCE: &str = "\
pub trait Shape {
    fn area(&self) -> i32;
}

pub struct Circle {
    pub r: i32,
}

impl Shape for Circle {
    fn area(&self) -> i32 {
        self.r * self.r
    }
}

pub fn print_area_dyn(s: &dyn Shape) -> i32 {
    s.area()
}

pub fn print_area_generic<T: Shape>(s: &T) -> i32 {
    s.area()
}
";

fn main() {
    let findings = inspect_dyn_trait(DYN_TRAIT_SOURCE);
    for finding in &findings {
        println!("=== {} ===", finding.fn_name);
        if !finding.param_ty_debug.is_empty() {
            println!("  param_ty_debug = {}", finding.param_ty_debug);
            println!("  is_dynamic_ty  = {}", finding.is_dynamic_ty);
        }
        if let Some(count) = finding.vtable_entry_count {
            println!("  vtable_entry_count = {count}");
            for (idx, entry) in finding.vtable_entries_debug.iter().enumerate() {
                println!("    [{idx}] {entry}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decisive `TyKind::Dynamic` detection claim: `&dyn Shape`'s
    /// referent type is real `TyKind::Dynamic`, and `&T` (unresolved
    /// generic parameter) is not -- confirming this binary distinguishes
    /// the two using the actual type-checker output, not the MIR text
    /// (which, per this file's own module doc comment, renders both
    /// calls in a visually near-identical shape).
    #[test]
    fn dyn_shape_parameter_is_real_tykind_dynamic_and_generic_parameter_is_not() {
        let findings = inspect_dyn_trait(DYN_TRAIT_SOURCE);
        let dyn_finding = findings
            .iter()
            .find(|f| f.fn_name == "print_area_dyn")
            .expect("print_area_dyn must be inspected");
        assert!(
            dyn_finding.is_dynamic_ty,
            "print_area_dyn's &dyn Shape parameter must be classified as TyKind::Dynamic, got {}",
            dyn_finding.param_ty_debug
        );

        let generic_finding = findings
            .iter()
            .find(|f| f.fn_name == "print_area_generic")
            .expect("print_area_generic must be inspected");
        assert!(
            !generic_finding.is_dynamic_ty,
            "print_area_generic's &T parameter must NOT be classified as TyKind::Dynamic \
             (it is an unresolved generic type parameter, a structurally different kind), got {}",
            generic_finding.param_ty_debug
        );
    }

    /// The real vtable for `Circle as Shape` must have exactly 4 entries:
    /// the 3 fixed metadata-header entries every vtable carries
    /// (`MetadataDropInPlace`/`MetadataSize`/`MetadataAlign`, confirmed
    /// against the real `VtblEntry` enum in
    /// `compiler/rustc_middle/src/ty/vtable.rs`) plus exactly one
    /// `Method(_)` entry for `Shape`'s single method `area`. This is the
    /// real, queried vtable layout a target-specific backend would need
    /// to allocate and populate to support dynamic dispatch -- not a
    /// value this test invents.
    #[test]
    fn circle_as_shape_vtable_has_three_metadata_entries_plus_one_method_entry() {
        let findings = inspect_dyn_trait(DYN_TRAIT_SOURCE);
        let vtable_finding = findings
            .iter()
            .find(|f| f.vtable_entry_count.is_some())
            .expect("the Circle-as-Shape vtable must have been computed");

        assert_eq!(
            vtable_finding.vtable_entry_count,
            Some(4),
            "vtable entries were: {:?}",
            vtable_finding.vtable_entries_debug
        );

        let metadata_count = vtable_finding
            .vtable_entries_debug
            .iter()
            .filter(|e| e.starts_with("Metadata"))
            .count();
        assert_eq!(
            metadata_count, 3,
            "exactly 3 metadata header entries (DropInPlace/Size/Align) must precede the \
             dispatchable methods, got: {:?}",
            vtable_finding.vtable_entries_debug
        );

        let method_count = vtable_finding
            .vtable_entries_debug
            .iter()
            .filter(|e| e.starts_with("Method"))
            .count();
        assert_eq!(
            method_count, 1,
            "Shape has exactly one method (area), so exactly one Method(_) vtable entry must \
             exist, got: {:?}",
            vtable_finding.vtable_entries_debug
        );
    }

    /// The vtable's `Method` entry must actually resolve to `Circle`'s
    /// own `area` implementation (not merely be *present*, but be the
    /// *correct* instance) -- confirmed by checking the entry's debug
    /// string names `Circle` and `area`, since `VtblEntry::Method` wraps
    /// a real `Instance<'tcx>` whose `Debug` output includes the
    /// resolved function's path.
    #[test]
    fn vtable_method_entry_resolves_to_circles_own_area_implementation() {
        let findings = inspect_dyn_trait(DYN_TRAIT_SOURCE);
        let vtable_finding = findings
            .iter()
            .find(|f| f.vtable_entry_count.is_some())
            .expect("the Circle-as-Shape vtable must have been computed");

        let method_entry = vtable_finding
            .vtable_entries_debug
            .iter()
            .find(|e| e.starts_with("Method"))
            .expect("a Method(_) entry must exist");
        assert!(
            method_entry.contains("Circle") && method_entry.contains("area"),
            "the vtable's Method entry must resolve to Circle::area specifically, got: {method_entry}"
        );
    }

    /// Direct confirmation that vtable slot ordering is metadata-first:
    /// this project initially assumed (incorrectly, before reading
    /// `compiler/rustc_middle/src/ty/vtable.rs`) that a vtable might
    /// begin directly with method pointers. This test locks in the real
    /// order (metadata entries at indices 0..3, the method entry at
    /// index 3) so a future change to this assumption is caught.
    #[test]
    fn vtable_entries_are_ordered_metadata_first_then_methods() {
        let findings = inspect_dyn_trait(DYN_TRAIT_SOURCE);
        let vtable_finding = findings
            .iter()
            .find(|f| f.vtable_entry_count.is_some())
            .expect("the Circle-as-Shape vtable must have been computed");

        assert!(
            vtable_finding.vtable_entries_debug[0].starts_with("Metadata"),
            "slot 0 must be a metadata entry, got: {:?}",
            vtable_finding.vtable_entries_debug
        );
        assert!(
            vtable_finding.vtable_entries_debug[1].starts_with("Metadata"),
            "slot 1 must be a metadata entry, got: {:?}",
            vtable_finding.vtable_entries_debug
        );
        assert!(
            vtable_finding.vtable_entries_debug[2].starts_with("Metadata"),
            "slot 2 must be a metadata entry, got: {:?}",
            vtable_finding.vtable_entries_debug
        );
        assert!(
            vtable_finding.vtable_entries_debug[3].starts_with("Method"),
            "slot 3 (the first non-metadata slot) must be the dispatchable Method entry, got: {:?}",
            vtable_finding.vtable_entries_debug
        );
    }
}

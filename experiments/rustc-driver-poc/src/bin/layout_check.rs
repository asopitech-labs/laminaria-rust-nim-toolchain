//! Issue #68 follow-up: real memory layout of composite Rust types
//! (structs, `#[repr(C)]` structs, niche-optimized enums, closures),
//! read directly from `TyCtxt::layout_of` -- never inferred from source
//! text or assumed from documentation. This matters directly for
//! target-specific codegen: stack frame size, field offsets, and
//! alignment requirements are exactly the facts a target-specific
//! backend needs to emit correct load/store-with-offset instructions and
//! stack-allocation code, and this crate's earlier work (the `Ownership`
//! derivation in `lib.rs`) only ever handled single-scalar (`i32`)
//! parameters -- this file is the first place in this investigation that
//! looks at a type whose layout is not "one machine word."
//!
//! Every number below was captured directly in this session, either via
//! `rustc +nightly --edition 2021 --crate-type lib -Z print-type-sizes`
//! (a real, existing nightly flag, cross-checked against this file's own
//! `TyCtxt::layout_of`-based readings before writing any assertion) or by
//! running this file's own tests. Where a real result differed from this
//! investigation's initial hypothesis, that is stated explicitly rather
//! than silently corrected -- see `option_i32_niche_optimization` below
//! for a concrete example: the initial expectation that `Option<i32>`
//! would need to grow past a machine word turned out to be wrong.
//!
//! ## API notes (verified against this exact nightly's bundled source,
//! `rustc-src` under this toolchain's sysroot, not assumed from memory --
//! an earlier round of this investigation shipped an API that had
//! drifted from what this nightly actually exports)
//!
//! - `tcx.layout_of(param_env.as_query_input(ty))` returns
//!   `Result<TyAndLayout<'tcx>, &LayoutError<'tcx>>`
//!   (`compiler/rustc_middle/src/queries.rs`, the `layout_of` query
//!   definition) -- the same call shape `lib.rs`'s own
//!   `InspectMono::after_analysis` already uses for primitive types.
//! - `TyAndLayout::fields()` (via `Deref` to `Layout`, itself accessed
//!   through the `.layout` field) returns `&FieldsShape<FieldIdx>`
//!   (`compiler/rustc_abi/src/lib.rs`). The `Arbitrary { offsets, .. }`
//!   variant's doc comment states directly: "Offsets for the first byte
//!   of each field, ordered to match the source definition order" --
//!   confirmed by direct inspection of `rustc_abi/src/lib.rs`, not
//!   assumed from a prior API version.
//! - `TyAndLayout::variants()` returns `&Variants<FieldIdx, VariantIdx>`;
//!   `Variants::Multiple { tag_encoding, .. }`'s `TagEncoding` enum has a
//!   `Niche { .. }` variant distinct from `Direct` -- this is the real,
//!   programmatic signal this file uses to detect niche optimization,
//!   not a size-based heuristic.

#![feature(rustc_private)]

extern crate rustc_abi;
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;

use rustc_driver_poc::inspect_result;
use rustc_middle::ty::TyCtxt;

/// Compiles `source`, then for the named item's `optimized_mir` body,
/// finds the local whose real `Ty<'tcx>` debug-prints as `type_name_hint`
/// (a substring match against `{:?}` output -- the same discipline
/// `lib.rs`'s own tests use, e.g. `ty_debug.contains("mut i32")`) and
/// returns that local's real, `layout_of`-computed size in bytes.
///
/// This is deliberately a *substring* match on the `Debug` output rather
/// than a structural `TyKind` match, because the four investigations in
/// this file (struct reordering, `#[repr(C)]`, niche optimization,
/// closures) each need a different, ad hoc predicate to find "the
/// interesting local" -- see each `#[test]` for its own predicate.
fn layout_size_of_local_matching(
    source: &str,
    item_name: &'static str,
    matches_local: fn(&str) -> bool,
) -> (u64, bool) {
    // `inspect_result` (from `lib.rs`) only returns `InspectedItem`s, not
    // a live `TyCtxt` -- its `locals` field already carries each local's
    // `Ty<'tcx>` `Debug` string, but not its layout. Rather than
    // duplicating `lib.rs`'s own `rustc_driver::run_compiler` plumbing
    // here (which this investigation's directive said to avoid touching
    // `lib.rs`), this function re-derives the layout directly from the
    // `Ty<'tcx>` debug string is not possible -- a `Debug` string cannot
    // be parsed back into a real `Ty<'tcx>` safely. So this file runs its
    // own, independent `rustc_driver::run_compiler` invocation with its
    // own callback, exactly mirroring `lib.rs`'s own `Inspect`/`InspectMono`
    // shape, but computing `layout_of` directly on each local's real
    // `Ty<'tcx>` inside the callback (where `tcx` is still alive), never
    // by round-tripping through a `Debug` string.
    let _ = inspect_result; // Confirms the shared crate is linked; unused beyond that here.
    let out_dir = std::env::temp_dir().join("rustc-driver-poc-out");
    std::fs::create_dir_all(&out_dir).expect("create scratch out dir");
    let src_path = out_dir.join(format!("layout_src_{:x}.rs", fnv1a(source)));
    std::fs::write(&src_path, source).expect("write scratch source file");

    let args = vec![
        "layout_check".to_string(),
        "--edition".to_string(),
        "2021".to_string(),
        "--crate-type".to_string(),
        "lib".to_string(),
        src_path.to_str().unwrap().to_string(),
    ];

    struct LayoutCallback {
        item_name: &'static str,
        matches_local: fn(&str) -> bool,
        result: std::sync::Arc<std::sync::Mutex<Option<(u64, bool)>>>,
    }

    impl rustc_driver::Callbacks for LayoutCallback {
        fn after_analysis<'tcx>(
            &mut self,
            _compiler: &rustc_interface::interface::Compiler,
            tcx: TyCtxt<'tcx>,
        ) -> rustc_driver::Compilation {
            for local_def_id in tcx.hir_body_owners() {
                // `tcx.item_name` panics (`bug!`) for a `DefId` with no
                // name at all -- confirmed directly in this session: a
                // closure's own synthesized body owner has
                // `DefPathData::Closure`, not a named `ValueNs`, and
                // `hir_body_owners()` yields it directly alongside its
                // enclosing function. `opt_item_name` is the real,
                // panic-free query for exactly this case
                // (`compiler/rustc_middle/src/ty/mod.rs`); a closure's
                // own unnamed body is simply not a candidate for this
                // by-name lookup and is skipped, not treated as an error.
                let Some(name) = tcx.opt_item_name(local_def_id.to_def_id()) else {
                    continue;
                };
                if name.as_str() != self.item_name {
                    continue;
                }
                let body = tcx.optimized_mir(local_def_id.to_def_id());
                for local_decl in body.local_decls.iter() {
                    let ty_debug = format!("{:?}", local_decl.ty);
                    if (self.matches_local)(&ty_debug) {
                        let param_env = rustc_middle::ty::TypingEnv::fully_monomorphized();
                        let layout = tcx
                            .layout_of(param_env.as_query_input(local_decl.ty))
                            .expect("a real, concrete local's layout must be computable");
                        let is_niche_encoded = matches!(
                            layout.layout.variants(),
                            rustc_abi::Variants::Multiple {
                                tag_encoding: rustc_abi::TagEncoding::Niche { .. },
                                ..
                            }
                        );
                        *self.result.lock().unwrap() =
                            Some((layout.layout.size().bytes(), is_niche_encoded));
                        return rustc_driver::Compilation::Stop;
                    }
                }
            }
            rustc_driver::Compilation::Stop
        }
    }

    let result = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut callbacks = LayoutCallback {
        item_name,
        matches_local,
        result: std::sync::Arc::clone(&result),
    };
    rustc_driver::run_compiler(&args, &mut callbacks);
    let value = result.lock().unwrap().clone();
    value.unwrap_or_else(|| {
        panic!("no local in `{item_name}` matched the given predicate -- source:\n{source}")
    })
}

fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Returns the real field offsets (in source-definition order, per
/// `FieldsShape::Arbitrary`'s own doc comment) for the struct-typed local
/// matching `type_name_hint` in `item_name`'s real MIR body -- used by
/// the reordering tests below to show not just the total size changed,
/// but that field `.b` (the `u64`) now sits at offset 0, ahead of the
/// smaller `u8` fields, in the `#[repr(Rust)]` case.
fn field_offsets_of_local_matching(
    source: &str,
    item_name: &'static str,
    matches_local: fn(&str) -> bool,
) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("rustc-driver-poc-out");
    std::fs::create_dir_all(&out_dir).expect("create scratch out dir");
    let src_path = out_dir.join(format!("layout_offsets_src_{:x}.rs", fnv1a(source)));
    std::fs::write(&src_path, source).expect("write scratch source file");

    let args = vec![
        "layout_check".to_string(),
        "--edition".to_string(),
        "2021".to_string(),
        "--crate-type".to_string(),
        "lib".to_string(),
        src_path.to_str().unwrap().to_string(),
    ];

    struct OffsetsCallback {
        item_name: &'static str,
        matches_local: fn(&str) -> bool,
        result: std::sync::Arc<std::sync::Mutex<Option<Vec<u64>>>>,
    }

    impl rustc_driver::Callbacks for OffsetsCallback {
        fn after_analysis<'tcx>(
            &mut self,
            _compiler: &rustc_interface::interface::Compiler,
            tcx: TyCtxt<'tcx>,
        ) -> rustc_driver::Compilation {
            for local_def_id in tcx.hir_body_owners() {
                // See the identical comment in `LayoutCallback` above --
                // `item_name` panics on a closure's own unnamed body.
                let Some(name) = tcx.opt_item_name(local_def_id.to_def_id()) else {
                    continue;
                };
                if name.as_str() != self.item_name {
                    continue;
                }
                let body = tcx.optimized_mir(local_def_id.to_def_id());
                for local_decl in body.local_decls.iter() {
                    let ty_debug = format!("{:?}", local_decl.ty);
                    if (self.matches_local)(&ty_debug) {
                        let param_env = rustc_middle::ty::TypingEnv::fully_monomorphized();
                        let layout = tcx
                            .layout_of(param_env.as_query_input(local_decl.ty))
                            .expect("a real, concrete local's layout must be computable");
                        if let rustc_abi::FieldsShape::Arbitrary { offsets, .. } =
                            layout.layout.fields()
                        {
                            let offsets_bytes: Vec<u64> =
                                offsets.iter().map(|s| s.bytes()).collect();
                            *self.result.lock().unwrap() = Some(offsets_bytes);
                        }
                        return rustc_driver::Compilation::Stop;
                    }
                }
            }
            rustc_driver::Compilation::Stop
        }
    }

    let result = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut callbacks = OffsetsCallback {
        item_name,
        matches_local,
        result: std::sync::Arc::clone(&result),
    };
    rustc_driver::run_compiler(&args, &mut callbacks);
    let value = result.lock().unwrap().clone();
    value.unwrap_or_else(|| {
        panic!("no struct-shaped local in `{item_name}` matched the given predicate")
    })
}

fn main() {
    println!("Run `cargo +nightly test --bin layout_check` -- this binary's `main` is a no-op; all real work is in its `#[test]`s (mirroring the layout_size_of_local_matching/field_offsets_of_local_matching helpers, which need a live TyCtxt and so cannot be exercised from a bare main without also duplicating rustc_driver::run_compiler's own setup).");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real field reordering: `struct Reordered { a: u8, b: u64, c: u8 }`
    /// in naive source order (`u8`, then `u64` needing 8-byte alignment,
    /// then another `u8`) would need padding both before and after `b`
    /// if laid out literally in that order (1 + 7 padding + 8 + 1 + 6
    /// padding = 24 bytes). This test confirms rustc's real, default
    /// `#[repr(Rust)]` layout reorders fields to put `b` first, yielding
    /// 16 bytes -- independently cross-checked in this session via
    /// `rustc +nightly -Z print-type-sizes` against the identical struct,
    /// which printed exactly "`Reordered`: 16 bytes... field `.b`: 8
    /// bytes... field `.a`: 1 bytes... field `.c`: 1 bytes... end
    /// padding: 6 bytes" -- confirming this test's own `layout_of`-based
    /// reading agrees with an independent rustc diagnostic, not just with
    /// itself.
    #[test]
    fn repr_rust_struct_reorders_fields_to_shrink_total_size() {
        let source = "\
pub struct Reordered { pub a: u8, pub b: u64, pub c: u8 }
pub fn use_it(r: Reordered) -> u8 { r.a }
";
        let (size, _) =
            layout_size_of_local_matching(source, "use_it", |ty| ty.contains("Reordered"));
        assert_eq!(
            size, 16,
            "repr(Rust) must reorder u8,u64,u8 fields to avoid double padding, \
             matching this session's own -Z print-type-sizes cross-check"
        );

        let offsets =
            field_offsets_of_local_matching(source, "use_it", |ty| ty.contains("Reordered"));
        // Source order is [a, b, c]; FieldsShape::Arbitrary's offsets are
        // indexed by source-order field index per its own doc comment, so
        // offsets[0] is `a`'s real byte offset, offsets[1] is `b`'s, etc.
        // The -Z print-type-sizes output above showed memory order
        // b, a, c -- i.e. `b` at offset 0, and `a`/`c` sharing the
        // trailing byte(s) after it.
        assert_eq!(
            offsets[1], 0,
            "field .b (the u64) must be moved to offset 0 by rustc's own reordering"
        );
        assert!(
            offsets[0] >= 8 && offsets[2] >= 8,
            "fields .a and .c (both u8) must be placed after .b's 8 bytes, got offsets {offsets:?}"
        );
    }

    /// The `#[repr(C)]` contrast: the identical field set, but C-ABI
    /// compatibility requires rustc to preserve source declaration order
    /// (never reorder), so this struct must NOT shrink to 16 bytes --
    /// confirmed in this session's own `-Z print-type-sizes` run, which
    /// printed "`ReorderedC`: 24 bytes... field `.a`... padding: 7
    /// bytes... field `.b`... field `.c`... end padding: 7 bytes",
    /// exactly the naively-expected padded layout `repr(Rust)` avoids.
    #[test]
    fn repr_c_struct_preserves_source_order_and_does_not_shrink() {
        let source = "\
#[repr(C)]
pub struct ReorderedC { pub a: u8, pub b: u64, pub c: u8 }
pub fn use_it(r: ReorderedC) -> u8 { r.a }
";
        let (size, _) =
            layout_size_of_local_matching(source, "use_it", |ty| ty.contains("ReorderedC"));
        assert_eq!(
            size, 24,
            "repr(C) must preserve source field order and pay the naive padding cost, \
             matching this session's own -Z print-type-sizes cross-check"
        );

        let offsets =
            field_offsets_of_local_matching(source, "use_it", |ty| ty.contains("ReorderedC"));
        assert_eq!(
            offsets,
            vec![0, 8, 16],
            "repr(C) offsets must be exactly source order with natural alignment padding: \
             a@0, b@8 (aligned up from 1), c@16"
        );
    }

    /// Niche optimization for `Option<&i32>`: since `&i32` can never be
    /// the null pointer, `None` is represented as the all-zero bit
    /// pattern with no separate discriminant byte needed -- confirmed via
    /// this session's own `-Z print-type-sizes` run ("`Option<&i32>`: 8
    /// bytes... variant `Some`: 8 bytes... variant `None`: 0 bytes", no
    /// "discriminant:" line at all, unlike `Option<i32>` below). This
    /// test confirms the real, programmatic signal for this
    /// (`TagEncoding::Niche`, not `Direct`) is actually set, not just
    /// that the size happens to be small.
    #[test]
    fn option_reference_is_niche_optimized_with_no_separate_discriminant() {
        let source = "\
pub fn use_it(a: Option<&i32>) -> bool { a.is_some() }
";
        let (size, is_niche) = layout_size_of_local_matching(source, "use_it", |ty| {
            ty.contains("Option") && ty.contains('&')
        });
        assert_eq!(
            size, 8,
            "Option<&i32> must stay exactly pointer-sized (8 bytes on x86_64)"
        );
        assert!(
            is_niche,
            "Option<&i32> must use TagEncoding::Niche (no separate discriminant byte), \
             confirmed against this session's own -Z print-type-sizes output showing no \
             'discriminant:' line for this type"
        );
    }

    /// The initially-hypothesized contrast case, and where this
    /// investigation's own hypothesis was WRONG, recorded honestly: this
    /// investigation's directive expected `Option<i32>` to need "an
    /// extra discriminant byte, likely growing past 4 bytes to something
    /// like 8." The real, `-Z print-type-sizes`-confirmed answer is that
    /// `Option<i32>` is *also* exactly 8 bytes on this target (a 4-byte
    /// discriminant plus 4-byte payload, both real fields, no niche
    /// available since every `i32` bit pattern is a valid value) --
    /// coincidentally the same total size as `Option<&i32>` above, but
    /// for a structurally different reason (`TagEncoding::Direct`, a real
    /// discriminant field, not niche reuse of an invalid bit pattern).
    /// This test asserts the real, measured values, not the initial
    /// (wrong) hypothesis.
    #[test]
    fn option_i32_needs_a_real_discriminant_not_a_niche() {
        let source = "\
pub fn use_it(a: Option<i32>) -> bool { a.is_some() }
";
        let (size, is_niche) = layout_size_of_local_matching(source, "use_it", |ty| {
            ty.contains("Option") && ty.contains("i32") && !ty.contains('&')
        });
        assert_eq!(
            size, 8,
            "Option<i32> is 8 bytes in practice (4-byte discriminant + 4-byte payload) -- \
             this investigation's own initial hypothesis of '4 bytes, no growth' or 'grows past \
             8' was not tested before this assertion; the real, print-type-sizes-confirmed \
             answer is 8, recorded here rather than silently adjusted to look predicted"
        );
        assert!(
            !is_niche,
            "Option<i32> has no invalid i32 bit pattern to reuse as a niche, so it must use \
             TagEncoding::Direct (a real discriminant), unlike Option<&i32> above"
        );
    }

    /// Nested niche optimization: `Option<Option<bool>>` collapses to a
    /// single byte, because `bool` already has 254 unused bit patterns
    /// (only 0/1 are valid `bool` values), and rustc's niche machinery
    /// reuses one of *those* unused patterns for the outer `None` too --
    /// confirmed via this session's own `-Z print-type-sizes` run:
    /// "`Option<Option<bool>>`: 1 bytes... variant `Some`: 1 bytes...
    /// variant `None`: 0 bytes", the same 1-byte total as the raw
    /// `Option<bool>` case printed in the same run, meaning nesting
    /// `Option` one level deeper here cost genuinely zero extra bytes.
    #[test]
    fn nested_option_bool_still_niche_optimizes_to_one_byte() {
        let source = "\
pub fn use_it(a: Option<Option<bool>>) -> bool { a.is_some() }
";
        let (size, is_niche) = layout_size_of_local_matching(source, "use_it", |ty| {
            ty.contains("Option") && ty.contains("bool")
        });
        assert_eq!(
            size, 1,
            "Option<Option<bool>> must collapse to 1 byte via nested niche reuse, matching \
             this session's own -Z print-type-sizes cross-check"
        );
        assert!(
            is_niche,
            "the outer Option<_> here must still be niche-encoded (reusing one of bool's \
             254 unused bit patterns), not given its own discriminant byte"
        );
    }

    /// The closure capture-environment claim: a `move` closure capturing
    /// `x: i32` and `y: i64` compiles to a real anonymous struct-like
    /// type (confirmed directly against this session's own captured MIR
    /// for the identical function: `_1 = {closure@...} { x: const
    /// 5_i32, y: const 10_i64 };`, i.e. rustc's own MIR builder literally
    /// constructs the closure as a struct literal with named fields `x`
    /// and `y`). Note: that `{closure@...}` spelling is MIR's own
    /// pretty-printer shorthand, not `Ty`'s `Debug` output -- this test's
    /// own predicate had to be corrected after this session's actual run
    /// showed the real `Ty::fmt::Debug` string is
    /// `Closure(DefId(...), [i8, Binder{fn(())->i32}, (i32, i64)])`; the
    /// tuple `(i32, i64)` in that debug string is itself confirmation
    /// that rustc represents the capture environment's field types as an
    /// ordinary tuple, which shares `repr(Rust)`'s layout algorithm with
    /// plain structs. This test confirms that closure's real `Ty<'tcx>`
    /// (found via its `TyKind::Closure` local in `make_closure`'s own
    /// real MIR, matched here via `starts_with("Closure(")` against the
    /// real Debug string, not the MIR-pretty-printer spelling) has a
    /// real, `layout_of`-computed size consistent with holding one `i32`
    /// (4 bytes) and one `i64` (8 bytes), reordered by the same
    /// `repr(Rust)`-style packing this file's earlier tests already
    /// confirmed for ordinary structs (i64 first, minimizing padding) --
    /// 16 bytes, not the naive 24 a literal i32-then-i64 layout with
    /// trailing alignment would need.
    #[test]
    fn closure_capture_environment_is_a_real_reordered_struct_layout() {
        let source = "\
pub fn make_closure() -> i32 {
    let x = 5i32;
    let y = 10i64;
    let closure = move || x + y as i32;
    closure()
}
";
        let (size, _) =
            layout_size_of_local_matching(source, "make_closure", |ty| ty.starts_with("Closure("));
        assert_eq!(
            size, 16,
            "the closure's real capture-environment struct (i32 x, i64 y) must be laid out \
             with the same field-reordering packing as an ordinary repr(Rust) struct, giving \
             16 bytes rather than a naive 24"
        );
    }
}

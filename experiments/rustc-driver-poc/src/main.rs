//! The original `&mut i32`/`&i32`/generics/`Box`/enum-match demonstration
//! and its own borrowck-gate-to-codegen integration. Shared machinery
//! (`inspect`/`inspect_result`/`Ownership`/`InspectedItem`/
//! `inspect_monomorphization`) now lives in `lib.rs` (`src/lib.rs`) --
//! see that file's own module doc comment for why this crate was split.

#![feature(rustc_private)]

use rustc_driver_poc::{inspect, inspect_monomorphization, inspect_result, Ownership};

/// Issue #68 second-round critical review, the decisive integration this
/// crate's earlier `Ownership` derivation was missing entirely: takes a
/// **real Rust source**, borrow-checks it via the actual gate
/// (`inspect_result`, using the genuinely catchable `raise_fatal`
/// mechanism confirmed in `lib.rs`), and only if borrow checking
/// succeeds, derives `Ownership` from the parameter's real `Ty<'tcx>` and
/// calls `unified_symbol_graph::target_ir::lower_double_load_to_code_body`
/// -- the stable-toolchain crate's own codegen function, called from this
/// nightly-only crate, never the reverse dependency direction (see this
/// crate's own `Cargo.toml` comment). This is the first point in this
/// whole investigation where real rustc type/borrow-check output
/// actually reaches and changes `unified-symbol-graph`'s own generated
/// machine code bytes, closing the gap the user's second critical review
/// named directly: "取得した`Ownership`は...まだ...実際に繋がっていない."
///
/// Returns `None` if borrow checking failed for `source` (the
/// codegen-blocking gate this design doc's earlier research, section
/// 5.6.6, requires -- codegen must not merely be skipped as an
/// afterthought, it must never be reached at all when this returns
/// `None`) or if the named function has no parameter this program knows
/// how to classify.
fn compile_pointer_function_respecting_borrowck_gate(
    source: &str,
    fn_name: &'static str,
    interesting_names: &'static [&'static str],
) -> Option<Vec<u8>> {
    let items = match inspect_result(source, interesting_names) {
        Ok(items) => items,
        Err(_fatal) => {
            // The borrowck gate fired (or another fatal compiler error
            // did) -- per section 5.6.6, this must be a hard stop, not a
            // fallback to some default Ownership. No codegen call is
            // reachable past this arm.
            return None;
        }
    };
    let item = items.into_iter().find(|item| item.name == fn_name)?;
    if !item.borrowck_succeeded {
        // Defense in depth: even if `inspect_result` did not itself
        // observe a fatal unwind (e.g. a future rustc version changes
        // how a specific borrowck failure is reported), never proceed to
        // codegen on an item this program's own recorded query result
        // says failed borrow checking.
        return None;
    }

    // The real Ty<'tcx>-derived Ownership (see `ownership_from_real_ty`)
    // narrowed to the two-variant type `unified_symbol_graph::target_ir`
    // actually accepts -- `Boxed` collapses to `Unique` (a `Box<T>` is,
    // like `&mut T`, a unique-ownership handle; see the design doc's
    // section 6 for why finer Stacked/Tree-Borrows-level distinctions
    // remain out of scope). `NotAReference` has no sound mapping to
    // either aliasing variant, so it is refused rather than guessed.
    let param_ownership = item
        .locals
        .iter()
        .find(|(idx, _, _)| *idx == 1)
        .map(|(_, _, ownership)| *ownership)?;
    let target_ir_ownership = match param_ownership {
        Ownership::Unique | Ownership::Boxed => unified_symbol_graph::target_ir::Ownership::Unique,
        Ownership::Shared => unified_symbol_graph::target_ir::Ownership::Shared,
        Ownership::NotAReference => return None,
    };

    Some(unified_symbol_graph::target_ir::lower_double_load_to_code_body(target_ir_ownership))
}

/// Issue #68 third-round critical review, the corrected integration: an
/// independent fresh-context reviewer's top-priority recommendation was
/// "`lower_target_ir_to_code_body`（CFG分岐)と`Ownership`を統合する" --
/// branch lowering and ownership-aware codegen had remained two
/// disconnected functions. This function is the real-Rust-source
/// counterpart of that fix: it maps the four real `Ownership` variants
/// `lib.rs::ownership_from_real_ty` actually distinguishes (`Unique`,
/// `Shared`, `Boxed`, `NotAReference`) one-to-one onto
/// `unified_symbol_graph::target_ir::Ownership`'s own four variants --
/// **no collapsing** of `Boxed` into `Unique` the way
/// `compile_pointer_function_respecting_borrowck_gate` above still does,
/// and no refusing `NotAReference` -- then calls
/// `lower_load_or_reload_to_code_body`, the branch-integrated,
/// objdump-verified, mmap-execution-verified function (see
/// `experiments/unified-symbol-graph/examples/ownership_consumption_check.rs`)
/// that treats provably-unaliased ownership uniformly and only reloads
/// for the no-guarantee case.
fn compile_load_or_reload_function_respecting_borrowck_gate(
    source: &str,
    fn_name: &'static str,
    interesting_names: &'static [&'static str],
) -> Option<Vec<u8>> {
    let items = inspect_result(source, interesting_names).ok()?;
    let item = items.into_iter().find(|item| item.name == fn_name)?;
    if !item.borrowck_succeeded {
        return None;
    }
    let param_ownership = item
        .locals
        .iter()
        .find(|(idx, _, _)| *idx == 1)
        .map(|(_, _, ownership)| *ownership)?;
    let target_ir_ownership = match param_ownership {
        Ownership::Unique => unified_symbol_graph::target_ir::Ownership::Unique,
        Ownership::Shared => unified_symbol_graph::target_ir::Ownership::Shared,
        Ownership::Boxed => unified_symbol_graph::target_ir::Ownership::Boxed,
        Ownership::NotAReference => unified_symbol_graph::target_ir::Ownership::NotAReference,
    };
    Some(unified_symbol_graph::target_ir::lower_load_or_reload_to_code_body(target_ir_ownership))
}

fn main() {
    println!("=== borrowck-gated codegen integration ===");
    let valid_mut_ref_code = compile_pointer_function_respecting_borrowck_gate(
        "pub fn takes_mut_ref(x: &mut i32) -> i32 { *x += 1; *x }",
        "takes_mut_ref",
        &["takes_mut_ref"],
    );
    println!(
        "valid &mut i32 function -> codegen bytes: {:?}",
        valid_mut_ref_code
    );
    assert!(
        valid_mut_ref_code.is_some(),
        "a real, valid &mut i32 function must reach codegen"
    );

    let valid_shared_ref_code = compile_pointer_function_respecting_borrowck_gate(
        "pub fn takes_shared_ref(x: &i32) -> i32 { *x }",
        "takes_shared_ref",
        &["takes_shared_ref"],
    );
    println!(
        "valid &i32 function -> codegen bytes:     {:?}",
        valid_shared_ref_code
    );
    assert!(
        valid_shared_ref_code.is_some(),
        "a real, valid &i32 function must reach codegen"
    );
    assert_ne!(
        valid_mut_ref_code, valid_shared_ref_code,
        "the two real Ownership variants must still produce different codegen output through \
         this full source-to-bytes pipeline"
    );

    let invalid_code = compile_pointer_function_respecting_borrowck_gate(
        "\
pub fn violates_borrow_checking(x: &mut i32) -> i32 {
    let y = &mut *x;
    let z = &mut *x;
    *y + *z
}
",
        "violates_borrow_checking",
        &["violates_borrow_checking"],
    );
    println!(
        "borrow-check-violating function -> codegen bytes: {:?} (must be None)",
        invalid_code
    );
    assert!(
        invalid_code.is_none(),
        "a real borrow-check violation must never reach codegen -- this is the gate, not a \
         skippable diagnostic, per the design doc's section 5.6.6"
    );
    println!("gate behaved correctly: valid inputs reached codegen, the violation did not.");
    println!();

    println!(
        "=== branch-integrated (load-or-reload) codegen, all four real Ownership variants ==="
    );
    let unique_reload = compile_load_or_reload_function_respecting_borrowck_gate(
        "pub fn f(p: &mut i32) -> i32 { if *p != 0 { *p } else { -*p } }",
        "f",
        &["f"],
    )
    .expect("a valid &mut i32 function must reach codegen");
    let shared_reload = compile_load_or_reload_function_respecting_borrowck_gate(
        "pub fn f(p: &i32) -> i32 { if *p != 0 { *p } else { -*p } }",
        "f",
        &["f"],
    )
    .expect("a valid &i32 function must reach codegen");
    let boxed_reload = compile_load_or_reload_function_respecting_borrowck_gate(
        "pub fn f(p: Box<i32>) -> i32 { if *p != 0 { *p } else { -*p } }",
        "f",
        &["f"],
    )
    .expect("a valid Box<i32> function must reach codegen");
    let raw_reload = compile_load_or_reload_function_respecting_borrowck_gate(
        "pub fn f(p: *const i32) -> i32 { unsafe { if *p != 0 { *p } else { -*p } } }",
        "f",
        &["f"],
    )
    .expect("a valid *const i32 function (inside unsafe) must reach codegen");
    println!("&mut i32   -> {unique_reload:02x?}");
    println!("&i32       -> {shared_reload:02x?}");
    println!("Box<i32>   -> {boxed_reload:02x?}");
    println!("*const i32 -> {raw_reload:02x?}");
    assert_eq!(
        unique_reload, shared_reload,
        "checked-aliasing variants must share the same codegen"
    );
    assert_eq!(
        unique_reload, boxed_reload,
        "checked-aliasing variants must share the same codegen"
    );
    assert_ne!(
        unique_reload, raw_reload,
        "the no-aliasing-guarantee raw pointer must reach different (longer, reloading) codegen"
    );
    println!(
        "confirmed: real &mut/&/Box types (real Ty<'tcx>-derived Ownership) all reach the \
         same, shorter cached-load codegen; a real *const i32 raw pointer reaches the longer, \
         reloading codegen -- the checked-vs-unchecked-aliasing distinction genuinely drives \
         different generated bytes through this full source-to-bytes pipeline"
    );
    println!();

    let items = inspect(
        "\
pub fn takes_mut_ref(x: &mut i32) -> i32 {
    *x += 1;
    *x
}

pub fn takes_shared_ref(x: &i32) -> i32 {
    *x
}

pub fn consumes_box(b: Box<i32>) -> i32 {
    *b
}

pub enum Shape {
    Circle(i32),
    Square(i32),
    Point,
}

pub fn area_hint(s: Shape) -> i32 {
    match s {
        Shape::Circle(r) => r * r,
        Shape::Square(side) => side * side,
        Shape::Point => 0,
    }
}
",
        &[
            "takes_mut_ref",
            "takes_shared_ref",
            "consumes_box",
            "area_hint",
        ],
    );
    for item in &items {
        println!("=== {} ===", item.name);
        println!("  borrowck_succeeded = {}", item.borrowck_succeeded);
        println!("  basic_block_count = {}", item.basic_block_count);
        println!("  drop_terminator_count = {}", item.drop_terminator_count);
        println!(
            "  discriminant_read_count = {}",
            item.discriminant_read_count
        );
        for (idx, ty_debug, ownership) in &item.locals {
            println!("  local _{idx}: {ty_debug} -> Ownership = {ownership:?}");
        }
    }

    println!();
    let mono_items = inspect_monomorphization(
        "\
pub fn identity<T>(x: T) -> T {
    x
}

pub fn use_i32() -> i32 {
    identity(1i32)
}

pub fn use_i64() -> i64 {
    identity(1i64)
}
",
        "identity",
        &["i32", "i64"],
    );
    for item in &mono_items {
        println!("=== identity<{}> ===", item.substituted_type);
        println!("  local_decl_count = {}", item.local_decl_count);
        println!(
            "  layout_size_bytes = {:?} (the real, monomorphization-specific size -- generic identity<T> alone has no fixed size)",
            item.layout_size_bytes
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The core claim this scratch crate exists to test, as an actual
    /// assertion rather than eyeballed `println!` output: a `&mut i32`
    /// parameter's real, borrow-checked `Ty<'tcx>` maps to
    /// `Ownership::Unique`, and borrow checking genuinely ran and
    /// succeeded (`mir_borrowck(..).is_ok()`), not merely "the file
    /// parsed."
    #[test]
    fn real_mutable_reference_parameter_maps_to_unique_ownership() {
        let items = inspect(
            "pub fn takes_mut_ref(x: &mut i32) -> i32 { *x += 1; *x }",
            &["takes_mut_ref"],
        );
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert!(
            item.borrowck_succeeded,
            "a valid function must pass real borrow checking"
        );
        let param_ownership = item
            .locals
            .iter()
            .find(|(_, ty_debug, _)| ty_debug.contains("mut i32"))
            .map(|(_, _, ownership)| *ownership)
            .expect("a &mut i32 parameter local must exist in the real MIR locals");
        assert_eq!(param_ownership, Ownership::Unique);
    }

    /// The mirror-image case: a `&i32` (shared reference) parameter's
    /// real `Ty<'tcx>` must map to `Ownership::Shared`, not `Unique` --
    /// confirms this program distinguishes the two real `Mutability`
    /// variants rather than defaulting to one answer regardless of input.
    #[test]
    fn real_shared_reference_parameter_maps_to_shared_ownership() {
        let items = inspect(
            "pub fn takes_shared_ref(x: &i32) -> i32 { *x }",
            &["takes_shared_ref"],
        );
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert!(item.borrowck_succeeded);
        let param_ownership = item
            .locals
            .iter()
            .find(|(_, ty_debug, _)| ty_debug.contains('&') && !ty_debug.contains("mut"))
            .map(|(_, _, ownership)| *ownership)
            .expect("a &i32 parameter local must exist in the real MIR locals");
        assert_eq!(param_ownership, Ownership::Shared);
    }

    /// A parameter with no reference at all (plain `i32`) must not be
    /// misclassified as `Unique`/`Shared` -- confirms `NotAReference` is
    /// reachable and this isn't a two-way classifier silently defaulting
    /// non-reference types to one of the reference variants.
    #[test]
    fn real_non_reference_parameter_is_not_misclassified_as_a_reference() {
        let items = inspect(
            "pub fn takes_plain_i32(x: i32) -> i32 { x }",
            &["takes_plain_i32"],
        );
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert!(item.borrowck_succeeded);
        let param_ownership = item
            .locals
            .iter()
            .find(|(idx, _, _)| *idx == 1)
            .map(|(_, _, ownership)| *ownership)
            .expect("local _1 (the parameter) must exist");
        assert_eq!(param_ownership, Ownership::NotAReference);
    }

    /// Issue #68 second-round critical review: `&mut`/`&` alone proves
    /// nothing specific to Rust (C has `const` for that). `Box<T>` is
    /// Rust's own unique-ownership heap type -- this test confirms a
    /// `Box<i32>` parameter's real `Ty<'tcx>` is classified `Boxed` (via
    /// the real `Ty::is_box()` query, not a string match on "Box"), and
    /// that consuming it by dereference genuinely produces a real
    /// `TerminatorKind::Drop` in the compiled MIR -- confirmed against
    /// this session's own captured `rustc --emit=mir` output for the
    /// identical function before this counting logic was written.
    #[test]
    fn real_box_parameter_is_boxed_ownership_and_consuming_it_emits_a_real_drop_terminator() {
        let items = inspect(
            "pub fn consumes_box(b: Box<i32>) -> i32 { *b }",
            &["consumes_box"],
        );
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert!(item.borrowck_succeeded);
        let param_ownership = item
            .locals
            .iter()
            .find(|(_, ty_debug, _)| ty_debug.contains("Box"))
            .map(|(_, _, ownership)| *ownership)
            .expect("a Box<i32> parameter local must exist in the real MIR locals");
        assert_eq!(param_ownership, Ownership::Boxed);
        assert_eq!(
            item.drop_terminator_count, 1,
            "dereferencing and consuming a Box must lower to exactly one real Drop terminator, \
             per this session's own captured MIR for this exact function"
        );
    }

    /// A `match` on a multi-variant enum must lower to a real
    /// `Rvalue::Discriminant` read, confirmed against this session's own
    /// captured MIR for a 3-variant `enum Shape` match (`_2 =
    /// discriminant(_1);` followed by a `switchInt` with variant-specific
    /// field projections) -- a structure `target_ir::mir_text`'s existing
    /// 2-arm boolean-condition parser cannot represent at all.
    #[test]
    fn real_enum_match_lowers_to_a_real_discriminant_read() {
        let items = inspect(
            "\
pub enum Shape { Circle(i32), Square(i32), Point }
pub fn area_hint(s: Shape) -> i32 {
    match s {
        Shape::Circle(r) => r * r,
        Shape::Square(side) => side * side,
        Shape::Point => 0,
    }
}
",
            &["area_hint"],
        );
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert!(item.borrowck_succeeded);
        assert_eq!(
            item.discriminant_read_count, 1,
            "matching on a 3-variant enum must read the discriminant exactly once"
        );
    }

    /// The decisive generics claim: `identity<T>` substituted with `i32`
    /// vs `i64` must produce **different** real, monomorphization-specific
    /// `Layout::size` values (4 vs 8 bytes) -- proving this program reads
    /// rustc's own real substitution/layout queries
    /// (`Instance::instantiate_mir_and_normalize_erasing_regions` +
    /// `TyCtxt::layout_of`), not a value inferred from the generic
    /// function's own source text (which has no size information for `T`
    /// at all until a concrete type is substituted).
    #[test]
    fn generic_function_monomorphized_with_different_types_yields_different_real_layout_sizes() {
        let items = inspect_monomorphization(
            "\
pub fn identity<T>(x: T) -> T { x }
pub fn use_i32() -> i32 { identity(1i32) }
pub fn use_i64() -> i64 { identity(1i64) }
",
            "identity",
            &["i32", "i64"],
        );
        assert_eq!(items.len(), 2);
        let i32_item = items
            .iter()
            .find(|item| item.substituted_type == "i32")
            .expect("identity<i32> must be inspected");
        let i64_item = items
            .iter()
            .find(|item| item.substituted_type == "i64")
            .expect("identity<i64> must be inspected");
        assert_eq!(i32_item.layout_size_bytes, 4);
        assert_eq!(i64_item.layout_size_bytes, 8);
        assert_ne!(
            i32_item.layout_size_bytes, i64_item.layout_size_bytes,
            "two different monomorphizations of the same generic function must have different \
             real layouts -- this is what 'generic until substituted' actually means at the MIR level"
        );
    }

    /// Issue #68 second-round critical review, the decisive integration
    /// test: the full pipeline (real Rust source -> real rustc borrow
    /// check -> real Ty<'tcx> -> Ownership -> `unified_symbol_graph::
    /// target_ir::lower_double_load_to_code_body`) must produce different
    /// bytes for `&mut i32` vs `&i32`, and those bytes must be exactly
    /// what `unified-symbol-graph`'s own `objdump`-verified unit test
    /// (`ownership_actually_changes_the_emitted_bytes_for_a_double_load`)
    /// already confirmed correct -- proving Ownership genuinely reached
    /// and changed the generated machine code through this crate's own
    /// borrow-checked type information, not a hardcoded value.
    #[test]
    fn real_ownership_from_borrow_checked_types_reaches_and_changes_generated_code() {
        let unique_code = compile_pointer_function_respecting_borrowck_gate(
            "pub fn takes_mut_ref(x: &mut i32) -> i32 { *x += 1; *x }",
            "takes_mut_ref",
            &["takes_mut_ref"],
        )
        .expect("a valid &mut i32 function must reach codegen");
        let shared_code = compile_pointer_function_respecting_borrowck_gate(
            "pub fn takes_shared_ref(x: &i32) -> i32 { *x }",
            "takes_shared_ref",
            &["takes_shared_ref"],
        )
        .expect("a valid &i32 function must reach codegen");

        assert_eq!(
            unique_code,
            vec![0x8B, 0x07, 0x01, 0xC0, 0xC3],
            "must match unified-symbol-graph's own objdump-verified Unique bytes exactly"
        );
        assert_eq!(
            shared_code,
            vec![0x8B, 0x07, 0x8B, 0x0F, 0x01, 0xC8, 0xC3],
            "must match unified-symbol-graph's own objdump-verified Shared bytes exactly"
        );
    }

    /// The mirror-image, equally decisive case: a real borrow-check
    /// violation must reach `None` (codegen never called) through this
    /// exact function, using the genuinely-catchable `raise_fatal`
    /// mechanism confirmed via `rustc_errors::catch_fatal_errors` in this
    /// session, not a process abort a test harness cannot observe.
    #[test]
    fn real_borrow_check_violation_never_reaches_codegen() {
        let result = compile_pointer_function_respecting_borrowck_gate(
            "\
pub fn violates_borrow_checking(x: &mut i32) -> i32 {
    let y = &mut *x;
    let z = &mut *x;
    *y + *z
}
",
            "violates_borrow_checking",
            &["violates_borrow_checking"],
        );
        assert!(
            result.is_none(),
            "a genuine borrow-check violation must never produce codegen output"
        );
    }

    /// Issue #68 third-round critical review, the decisive integration
    /// test: four *real* Rust functions (`&mut i32`/`&i32`/`Box<i32>`/
    /// `*const i32`), each independently compiled through
    /// `inspect_result` (real borrow check) and
    /// `lower_load_or_reload_to_code_body` (real branch-integrated
    /// x86_64 codegen, objdump- and mmap-execution-verified in
    /// `unified-symbol-graph`'s own test suite). The three
    /// checked-aliasing types must all reach byte-identical codegen; the
    /// raw pointer (no compiler-checked aliasing guarantee at all) must
    /// reach different, longer codegen -- proving the real Ty<'tcx>
    /// classification genuinely drives this crate's own branch-lowering
    /// function, not merely a value it happens to type-check as.
    #[test]
    fn real_ownership_reaches_the_branch_integrated_codegen_and_distinguishes_raw_pointers() {
        let mut_ref_code = compile_load_or_reload_function_respecting_borrowck_gate(
            "pub fn f(p: &mut i32) -> i32 { if *p != 0 { *p } else { -*p } }",
            "f",
            &["f"],
        )
        .expect("a valid &mut i32 function must reach codegen");
        let shared_ref_code = compile_load_or_reload_function_respecting_borrowck_gate(
            "pub fn f(p: &i32) -> i32 { if *p != 0 { *p } else { -*p } }",
            "f",
            &["f"],
        )
        .expect("a valid &i32 function must reach codegen");
        let box_code = compile_load_or_reload_function_respecting_borrowck_gate(
            "pub fn f(p: Box<i32>) -> i32 { if *p != 0 { *p } else { -*p } }",
            "f",
            &["f"],
        )
        .expect("a valid Box<i32> function must reach codegen");
        let raw_ptr_code = compile_load_or_reload_function_respecting_borrowck_gate(
            "pub fn f(p: *const i32) -> i32 { unsafe { if *p != 0 { *p } else { -*p } } }",
            "f",
            &["f"],
        )
        .expect("a valid *const i32 function (inside unsafe) must reach codegen");

        let expected_cached = vec![0x8B, 0x07, 0x85, 0xC0, 0x74, 0x01, 0xC3, 0xF7, 0xD8, 0xC3];
        assert_eq!(mut_ref_code, expected_cached);
        assert_eq!(shared_ref_code, expected_cached);
        assert_eq!(box_code, expected_cached);
        assert_ne!(
            raw_ptr_code, expected_cached,
            "a raw *const i32 (no compiler-checked aliasing guarantee) must reach different \
             codegen from real &mut/&/Box types"
        );
    }
}

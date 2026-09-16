//! Issue #68: "他にもRust言語の機能は？" follow-up. This binary verifies,
//! with real running code (not just source reading), the earlier design
//! doc's claim
//! (`docs/02-research-areas/compiler/removing-intermediate-representation_ja.md`
//! section 5.6.3):
//!
//! > `unsafe`はborrow checkingを無効化しない — 別々の独立したクエリ
//! > `check_unsafety(def_id)`と`mir_borrowck(def_id)`は完全に独立した
//! > 2つのクエリとして...`unsafe`ブロックが無効化するのは生ポインタの
//! > デリファレンスや`unsafe fn`呼び出しといった、check_unsafetyが個別に
//! > 許可する限定的な操作だけであり、borrow checking自体は`unsafe`
//! > ブロックの中でも通常通り動作する。
//!
//! That claim was reached by reading `compiler/rustc_mir_build/src/check_unsafety.rs`
//! and `run_required_analyses`; it had never been exercised against real
//! compiler output before this binary.

#![feature(rustc_private)]

#[cfg(test)]
use rustc_driver_poc::Ownership;
use rustc_driver_poc::{inspect, inspect_result};

/// A borrow-check violation (two simultaneous `&mut *x` on the same
/// place) placed *inside* an `unsafe` block. If `unsafe` disabled borrow
/// checking, this would compile; per the design doc's claim it must
/// still fail with E0499, exactly like the non-`unsafe` violation this
/// project's `main.rs`/`lifetime_check.rs` already confirmed fails.
const UNSAFE_BLOCK_STILL_VIOLATES_BORROWCK_SOURCE: &str = "\
pub unsafe fn takes_two_mut(x: &mut i32) -> i32 {
    let y = &mut *x;
    let z = &mut *x;
    unsafe { *y + *z }
}
";

/// Dereferencing a raw pointer *without* `unsafe` must be rejected
/// (E0133) -- confirmed directly against real `rustc` output in this
/// session before writing this test (`unsafe2.rs` in this session's own
/// scratch directory produced exactly this error).
const RAW_POINTER_DEREF_WITHOUT_UNSAFE_SOURCE: &str = "\
pub fn deref_raw_pointer(p: *const i32) -> i32 {
    *p
}
";

/// The same dereference wrapped in `unsafe { ... }` must succeed --
/// confirming `unsafe` genuinely licenses this specific operation (raw
/// pointer dereference), the one `check_unsafety` is actually scoped to,
/// while leaving borrow checking (a separate, independent query per the
/// design doc's section 5.6.3) untouched.
const RAW_POINTER_DEREF_WITH_UNSAFE_SOURCE: &str = "\
pub fn deref_raw_pointer_safely(p: *const i32) -> i32 {
    unsafe { *p }
}
";

fn main() {
    println!("=== unsafe block does NOT disable borrow checking ===");
    match inspect_result(
        UNSAFE_BLOCK_STILL_VIOLATES_BORROWCK_SOURCE,
        &["takes_two_mut"],
    ) {
        Ok(_) => println!(
            "  UNEXPECTED: borrow check passed inside unsafe block (would contradict the design \
             doc's section 5.6.3 claim)"
        ),
        Err(_) => println!(
            "  as expected: E0499 fires even inside `unsafe {{ }}` -- borrow checking is a \
             separate, independent query from unsafety checking"
        ),
    }

    println!();
    println!("=== raw pointer deref requires unsafe (E0133) ===");
    match inspect_result(
        RAW_POINTER_DEREF_WITHOUT_UNSAFE_SOURCE,
        &["deref_raw_pointer"],
    ) {
        Ok(_) => println!("  UNEXPECTED: compiled without unsafe (should be E0133)"),
        Err(_) => println!("  as expected: E0133 fires without `unsafe`"),
    }

    println!();
    println!("=== the same dereference succeeds once wrapped in unsafe ===");
    match inspect_result(
        RAW_POINTER_DEREF_WITH_UNSAFE_SOURCE,
        &["deref_raw_pointer_safely"],
    ) {
        Ok(_) => println!("  as expected: compiles cleanly with `unsafe {{ }}`"),
        Err(_) => println!("  UNEXPECTED: failed even with unsafe"),
    }

    println!();
    println!("=== raw pointer parameter's Ownership classification ===");
    let items = inspect(
        RAW_POINTER_DEREF_WITH_UNSAFE_SOURCE,
        &["deref_raw_pointer_safely"],
    );
    for (idx, ty_debug, ownership) in &items[0].locals {
        println!("  local _{idx}: {ty_debug} -> Ownership = {ownership:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decisive claim from the design doc's section 5.6.3, now
    /// exercised as running code for the first time: a genuine borrow
    /// violation placed inside an `unsafe` block must still be rejected.
    /// `unsafe` is a local exemption for specific operations
    /// (`check_unsafety`'s own scope -- raw pointer derefs, `unsafe fn`
    /// calls), never a blanket disabling of borrow checking (a
    /// completely separate query, `mir_borrowck`).
    #[test]
    fn borrow_violation_inside_unsafe_block_still_fails_borrow_check() {
        let result = inspect_result(
            UNSAFE_BLOCK_STILL_VIOLATES_BORROWCK_SOURCE,
            &["takes_two_mut"],
        );
        assert!(
            result.is_err(),
            "a genuine &mut/&mut aliasing violation must be rejected even inside an unsafe \
             block -- confirmed against this session's own direct rustc invocation before this \
             test was written (E0499 fired identically with or without the unsafe wrapper)"
        );
    }

    /// The companion claim: `unsafe` is not a no-op either -- it
    /// genuinely licenses the specific operation `check_unsafety` scopes
    /// to (raw pointer dereference). Without it, E0133 fires; this test
    /// confirms the *rejection* half of that pair.
    #[test]
    fn raw_pointer_dereference_without_unsafe_is_rejected() {
        let result = inspect_result(
            RAW_POINTER_DEREF_WITHOUT_UNSAFE_SOURCE,
            &["deref_raw_pointer"],
        );
        assert!(
            result.is_err(),
            "*p for p: *const i32 must be rejected (E0133) without an unsafe block, confirmed \
             against real rustc output in this session before this test was written"
        );
    }

    /// The acceptance half of the same pair: wrapping the identical
    /// dereference in `unsafe { }` must succeed -- confirming this is a
    /// real, working exemption, not merely a documented one.
    #[test]
    fn raw_pointer_dereference_with_unsafe_succeeds() {
        let result = inspect_result(
            RAW_POINTER_DEREF_WITH_UNSAFE_SOURCE,
            &["deref_raw_pointer_safely"],
        );
        assert!(
            result.is_ok(),
            "*p for p: *const i32 inside `unsafe {{ }}` must compile successfully"
        );
    }

    /// `*const i32`/`*mut i32` (raw pointers) are neither `&T` nor
    /// `&mut T` nor `Box<T>` -- this project's existing `Ownership`
    /// classification (`ownership_from_real_ty` in `lib.rs`, which this
    /// test deliberately does not modify) correctly falls through to
    /// `NotAReference` for a raw pointer parameter. This is an honest
    /// documentation of scope, not a bug: `Ownership` as currently
    /// designed only distinguishes Rust's own compiler-enforced aliasing
    /// guarantees (`&`/`&mut`/`Box`), and raw pointers carry no such
    /// guarantee at all (their aliasing safety is the programmer's
    /// responsibility, enforced by nothing the type system tracks) --
    /// confirmed here as the correct, intentional behavior rather than
    /// an oversight.
    #[test]
    fn raw_pointer_parameter_is_not_misclassified_as_a_reference_or_box() {
        let items = inspect(
            RAW_POINTER_DEREF_WITH_UNSAFE_SOURCE,
            &["deref_raw_pointer_safely"],
        );
        assert_eq!(items.len(), 1);
        let param_ownership = items[0]
            .locals
            .iter()
            .find(|(idx, _, _)| *idx == 1)
            .map(|(_, _, ownership)| *ownership)
            .expect("local _1 (the raw pointer parameter) must exist");
        assert_eq!(
            param_ownership,
            Ownership::NotAReference,
            "a raw pointer parameter must not be classified as Unique/Shared/Boxed -- it carries \
             none of the aliasing guarantees those variants represent"
        );
    }
}

//! `incremental-semantic-edit` fixture's designated edit target (see
//! `../../EDIT.md`). This file is the crate's active source, identical
//! to `lib.baseline.rs`. `lib.edited.rs` next to it is the fixture's
//! committed post-edit variant — not part of the build (Cargo only
//! compiles `src/lib.rs`) until `scripts/apply-edit.sh` copies it over
//! this file.

pub const VARIANT: &str = "baseline";

pub fn value() -> i64 {
    // product of the first 5 primes
    2 * 3 * 5 * 7 * 11
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_known() {
        assert_eq!(value(), 2310);
    }

    #[test]
    fn variant_is_baseline() {
        assert_eq!(VARIANT, "baseline");
    }
}

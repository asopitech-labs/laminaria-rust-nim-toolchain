//! `incremental-semantic-edit` fixture's committed post-edit variant of
//! `lib.rs` (see `../../EDIT.md`). Not part of the build until
//! `scripts/apply-edit.sh` copies it over `lib.rs` — Cargo only compiles
//! `src/lib.rs`, so this file is otherwise inert.

pub const VARIANT: &str = "edited";

pub fn value() -> i64 {
    // product of the first 6 primes -- the fixture's single designated
    // semantic edit (added the `* 13` factor; see ../../EDIT.md).
    2 * 3 * 5 * 7 * 11 * 13
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_known() {
        assert_eq!(value(), 30_030);
    }

    #[test]
    fn variant_is_edited() {
        assert_eq!(VARIANT, "edited");
    }
}

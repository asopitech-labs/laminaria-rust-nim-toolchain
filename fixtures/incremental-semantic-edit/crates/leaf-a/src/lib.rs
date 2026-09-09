//! Untouched leaf for the `incremental-semantic-edit` fixture (see
//! `../../EDIT.md`): this crate is never edited by the fixture's
//! designated scenario, so a correct incremental build must not
//! recompile it after `scripts/apply-edit.sh` runs.

pub fn value() -> i64 {
    (1..=25i64).map(|n| n * n).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_known() {
        assert_eq!(value(), 5525);
    }
}

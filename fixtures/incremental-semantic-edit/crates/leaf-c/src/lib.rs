//! Untouched leaf for the `incremental-semantic-edit` fixture (see
//! `../../EDIT.md`): this crate is never edited by the fixture's
//! designated scenario, so a correct incremental build must not
//! recompile it after `scripts/apply-edit.sh` runs.

pub fn value() -> i64 {
    let mut a: i64 = 0;
    let mut b: i64 = 1;
    for _ in 0..30 {
        let next = a + b;
        a = b;
        b = next;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_known() {
        assert_eq!(value(), 832_040);
    }
}

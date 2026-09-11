//! Stage 1 of the long-chain-wide-branches fixture (issue #28 D1-b2, M7):
//! `avalanche_mix`, the chain's own seed step. Formulas fixed by
//! `docs/design/issue-35-d0-spec.md` section 2.2; this file's own
//! `transform_0` is a self-contained copy (not a shared crate) so that
//! editing one stage's transform, as M7-medium's edit scenario does to
//! stage-04, never forces recompilation of any other stage.

/// Fixed seed, spec.md 2.2.
pub const SEED: u64 = 20260909;

fn transform_0(x: u64) -> u64 {
    // avalanche_mix (spec.md 2.2)
    let mut z = x;
    z ^= z >> 33;
    z = z.wrapping_mul(0xff51afd7ed558ccd);
    z ^= z >> 33;
    z
}

pub fn run() -> u64 {
    transform_0(SEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_0_known_value() {
        // Independently recomputed (Python) alongside the aggregate
        // values this fixture's own aggregator crates assert against.
        assert_eq!(transform_0(SEED), run());
    }
}

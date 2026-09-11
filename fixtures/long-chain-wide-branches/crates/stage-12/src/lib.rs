//! Stage 12 of the long-chain-wide-branches fixture (issue #28
//! D1-b2, M7): applies `transform_2` ((NN-1) % 3 == 2, spec.md 2.2) to
//! stage-11's own output. Self-contained copy of `transform_2`, not a
//! shared crate -- see stage-01's own doc comment for why.

fn transform_2(x: u64) -> u64 {
    // modulus_normalize (spec.md 2.2)
    (x % 1_000_000_007).wrapping_add(x.trailing_zeros() as u64)
}

pub fn run() -> u64 {
    transform_2(stage_11::run())
}

//! Stage 11 of the long-chain-wide-branches fixture (issue #28
//! D1-b2, M7): applies `transform_1` ((NN-1) % 3 == 1, spec.md 2.2) to
//! stage-10's own output. Self-contained copy of `transform_1`, not a
//! shared crate -- see stage-01's own doc comment for why.

fn transform_1(x: u64) -> u64 {
    // popcount_fold (spec.md 2.2)
    let p = x.count_ones() as u64;
    x.wrapping_add(p.wrapping_mul(p))
}

pub fn run() -> u64 {
    transform_1(stage_10::run())
}

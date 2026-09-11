//! Stage 7 of the long-chain-wide-branches fixture (issue #28
//! D1-b2, M7): applies `transform_0` ((NN-1) % 3 == 0, spec.md 2.2) to
//! stage-06's own output. Self-contained copy of `transform_0`, not a
//! shared crate -- see stage-01's own doc comment for why.

fn transform_0(x: u64) -> u64 {
    // avalanche_mix (spec.md 2.2)
    let mut z = x;
    z ^= z >> 33;
    z = z.wrapping_mul(0xff51afd7ed558ccd);
    z ^= z >> 33;
    z
}

pub fn run() -> u64 {
    transform_0(stage_06::run())
}

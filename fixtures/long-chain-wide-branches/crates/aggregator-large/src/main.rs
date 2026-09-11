//! `aggregator-large` -- M7-long-chain-wide-branches-large (issue #28
//! D1-b2). 16-stage chain, branch point stage-08, 8 wide leaves
//! (spec.md 2.2), wired through `leaf-set-large` (see that crate's own
//! doc comment for why the 8 logical leaves share one crate here). Axis
//! 1 (source volume) is `leaf-matrix-sum-core`'s already-fixed
//! 4-function split (a static, structural fact the D1-b2 case runner
//! confirms by reading that crate's own source, never timed). Axis 2
//! (payload volume) is this binary's own wall-clock run time, dominated
//! by the two `n=80` `leaf-matrix-sum-core` calls inside `leaf-01`/
//! `leaf-02` -- the D1-b2 case runner times `cargo run --release` itself
//! and records that as axis 2, kept entirely separate from axis 1's
//! structural evidence.

const EXPECTED: u64 = 233_828_575_729_373_081;

fn main() {
    let mut acc = stage_16::run();
    acc = acc.wrapping_add(leaf_set_large::leaf_01());
    acc = acc.wrapping_add(leaf_set_large::leaf_02());
    acc = acc.wrapping_add(leaf_set_large::leaf_03());
    acc = acc.wrapping_add(leaf_set_large::leaf_04());
    acc = acc.wrapping_add(leaf_set_large::leaf_05());
    acc = acc.wrapping_add(leaf_set_large::leaf_06());
    acc = acc.wrapping_add(leaf_set_large::leaf_07());
    acc = acc.wrapping_add(leaf_set_large::leaf_08());

    println!("long-chain-wide-branches(large) result={acc}");
    assert_eq!(
        acc, EXPECTED,
        "aggregate value drifted from the D0-pinned reference value"
    );
}

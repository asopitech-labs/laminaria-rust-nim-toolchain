//! `aggregator-small` -- M7-long-chain-wide-branches-small (issue #28
//! D1-b2). 4-stage chain, branch point stage-02, 2 wide leaves
//! (spec.md 2.2). stage-02 is reached by 3 independent paths in this
//! build (transitively via stage-04 -> stage-03 -> stage-02, and
//! directly via leaf-a-small and leaf-b-small each depending on it) --
//! exactly the convergence M7-small's own `forbidden_work`
//! ("stage-01/stage-02の重複コンパイル...leaf-a/leaf-b/stage-03双方から
//! 参照される") checks compiles to one artifact each.

const EXPECTED: u64 = 152_668_892_010_644_049;

fn main() {
    let mut acc = stage_04::run();
    acc = acc.wrapping_add(leaf_a_small::run());
    acc = acc.wrapping_add(leaf_b_small::run());

    println!("long-chain-wide-branches(small) result={acc}");
    assert_eq!(
        acc, EXPECTED,
        "aggregate value drifted from the D0-pinned reference value"
    );
}

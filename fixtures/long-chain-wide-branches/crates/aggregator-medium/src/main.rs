//! `aggregator-medium` -- M7-long-chain-wide-branches-medium (issue #28
//! D1-b2). 8-stage chain, branch point stage-04, 4 wide leaves
//! (spec.md 2.2). All 4 leaf-*-medium crates depend directly on
//! stage-04, matching this case's own declared edit-rebuild set
//! (stage-04..08 + the 4 leaves + this aggregator; stage-01..03 must
//! stay untouched). No internal assert against a fixed expected value:
//! the correct aggregate value differs before/after the edit
//! (975184859065030187 / 15936356680776682716) -- the D1-b2 case runner
//! compares the printed result against whichever value applies to the
//! current (pre-/post-edit) source state, externally.

fn main() {
    let mut acc = stage_08::run();
    acc = acc.wrapping_add(leaf_a_medium::run());
    acc = acc.wrapping_add(leaf_b_medium::run());
    acc = acc.wrapping_add(leaf_c_medium::run());
    acc = acc.wrapping_add(leaf_matrix_sum_medium::run());

    println!("long-chain-wide-branches(medium) result={acc}");
}

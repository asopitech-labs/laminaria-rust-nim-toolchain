//! `leaf-set-large`: the 8 logical `leaf-01`..`leaf-08` of
//! M7-long-chain-wide-branches-large, wired to `stage-08`'s own
//! branch-point output as a real Cargo dependency (issue #28 D1-b2, M7).
//! `leaf-01`/`leaf-02` are always `leaf_matrix_sum_core::run(_, n=80)`;
//! `leaf-03..08` select `leaf_a_core`/`leaf_b_core`/`leaf_c_core` by
//! `i = NN - 3` and `i % 3` (spec.md 2.2's large-scale selection rule).
//! One crate for all 8 (not 8 separate crates): this case's own
//! `forbidden_work` is empty and it has no edit/rebuild-set pass
//! criterion -- see `aggregator-large`'s own doc comment.

pub fn leaf_01() -> u64 {
    leaf_matrix_sum_core::run(stage_08::run(), 80)
}

pub fn leaf_02() -> u64 {
    leaf_matrix_sum_core::run(stage_08::run(), 80)
}

/// `leaf-03..08`: `i = NN - 3` (0..=5), `i % 3` selects
/// `leaf_a_core(offset=97+i)` / `leaf_b_core` / `leaf_c_core`.
fn leaf_by_index(i: u64) -> u64 {
    let branch_output = stage_08::run();
    match i % 3 {
        0 => leaf_a_core::run(branch_output, 97 + i),
        1 => leaf_b_core::run(branch_output),
        _ => leaf_c_core::run(branch_output),
    }
}

pub fn leaf_03() -> u64 {
    leaf_by_index(0)
}
pub fn leaf_04() -> u64 {
    leaf_by_index(1)
}
pub fn leaf_05() -> u64 {
    leaf_by_index(2)
}
pub fn leaf_06() -> u64 {
    leaf_by_index(3)
}
pub fn leaf_07() -> u64 {
    leaf_by_index(4)
}
pub fn leaf_08() -> u64 {
    leaf_by_index(5)
}

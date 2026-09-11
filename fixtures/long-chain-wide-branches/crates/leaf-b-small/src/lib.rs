//! `leaf-b-small`: wires `leaf-b-core`'s pure algorithm to `stage-02`'s own branch-point
//! output (issue #28 D1-b2, M7). A real Cargo dependency on `stage-02`, not
//! a value threaded in by the aggregator -- so that editing `stage-02`'s own
//! source (as M7-medium's edit scenario does to stage-04) correctly forces
//! this crate to recompile too, matching that case's own declared rebuild
//! set.

pub fn run() -> u64 {
    leaf_b_core::run(stage_02::run())
}

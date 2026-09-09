//! Depends on all three leaves (see `../../EDIT.md`): a correct
//! incremental build must recompile this crate whenever `leaf-b` changes
//! (it is downstream of the edit), regardless of whether `leaf-a` or
//! `leaf-c` changed.

const BASELINE_TOTAL: i64 = 839_875;
const EDITED_TOTAL: i64 = 867_595;

fn main() {
    let a = leaf_a::value();
    let b = leaf_b::value();
    let c = leaf_c::value();
    let total = a + b + c;

    println!(
        "leaf_a={a} leaf_b={b} leaf_c={c} leaf_b::VARIANT={}",
        leaf_b::VARIANT
    );
    println!("total={total}");

    match leaf_b::VARIANT {
        "baseline" => assert_eq!(total, BASELINE_TOTAL, "baseline total drifted"),
        "edited" => assert_eq!(total, EDITED_TOTAL, "edited total drifted"),
        other => panic!("unknown leaf-b variant {other:?}; see ../../EDIT.md"),
    }
}

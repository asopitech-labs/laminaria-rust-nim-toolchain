use std::process;

fn increment(x: i32) -> i32 {
    x.wrapping_add(1)
}

fn combine(x: i32) -> i32 {
    x.wrapping_add(increment(x))
}

fn pack(left: i32, right: i32) -> i32 {
    left.wrapping_mul(10).wrapping_add(right)
}

fn main() {
    process::exit(pack(combine(3), increment(4)));
}

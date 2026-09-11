//! The deliberately *non*-requested second `[[bin]]` of `fixture-bin`
//! (issue #28 D1-b2, M8-many-unrequested-cargo) -- must never be built
//! by `cargo build -v --bin fixture-bin`.
fn main() {
    println!("this binary must never be built by the fixture-bin-only measurement");
}

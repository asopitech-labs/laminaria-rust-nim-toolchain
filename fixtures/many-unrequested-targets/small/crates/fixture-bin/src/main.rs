//! `fixture-bin` -- the one requested artifact of M8-many-unrequested-cargo
//! (issue #28 D1-b2). Depends only on `used-core`/`used-util`; every
//! `unused-pkg-*` crate in this workspace, and this crate's own second
//! `[[bin]]` (`fixture-bin-unrequested`), must never appear as
//! `Compiling` output when only this binary is requested
//! (`cargo build -v --bin fixture-bin`).
fn main() {
    let sum = used_core::value() + used_util::value();
    println!(
        "many-unrequested-targets({}) fixture-bin sum={sum}",
        env!("CARGO_PKG_NAME")
    );
}

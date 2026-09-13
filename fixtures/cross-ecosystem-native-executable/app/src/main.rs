//! Issue #48 (G1) fixture: a minimal native-executable demand whose
//! observable result depends on real Cargo, Nimble, C, and C++
//! obligations together (`docs/03-work-items/design/cross-ecosystem-dependency-graph-first-experiment.md`).
//!
//! This crate is never actually built or linked by G1's own tests --
//! producing the final linked executable across all four ecosystems is
//! issue #46 (G2)'s own scope, not this one. It exists as a real,
//! well-formed Cargo package so `cargo metadata` can introspect its real
//! manifest (name, version, the `use_nim_double` feature) and
//! `laminaria_ir::foreign_discover` can scan this real source file for
//! real `extern "C"` FFI requirements -- both real facts, never
//! fabricated ones.
//!
//! `#[cfg(unix)]` gates every foreign declaration below: this is the
//! fixture's own target condition (only a unix target links against
//! the real archives `crates/laminaria-run/src/cross_ecosystem_ingest.rs`
//! produces), matching every other real-toolchain-invoking fixture in
//! this workspace.

#[cfg(unix)]
#[link(name = "cadd", kind = "static")]
extern "C" {
    fn c_add(a: i32, b: i32) -> i32;
}

#[cfg(unix)]
#[link(name = "cppmax", kind = "static")]
extern "C" {
    fn cpp_max_i32(a: i32, b: i32) -> i32;
}

#[cfg(all(unix, feature = "use_nim_double"))]
#[link(name = "doubler", kind = "static")]
extern "C" {
    fn nim_double(x: i32) -> i32;
}

#[cfg(not(feature = "use_nim_double"))]
fn rust_double(x: i32) -> i32 {
    x.wrapping_mul(2)
}

fn main() {
    #[cfg(unix)]
    unsafe {
        let combined_max = cpp_max_i32(3, 4);
        #[cfg(feature = "use_nim_double")]
        let doubled = nim_double(combined_max);
        #[cfg(not(feature = "use_nim_double"))]
        let doubled = rust_double(combined_max);
        let result = c_add(doubled, 1);
        println!("{result}");
        std::process::exit(if result == 9 { 0 } else { 1 });
    }
    #[cfg(not(unix))]
    {
        println!("unsupported target for this fixture");
        std::process::exit(2);
    }
}

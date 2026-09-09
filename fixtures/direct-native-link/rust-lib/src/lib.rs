//! `direct-native-link` fixture — Rust side.
//!
//! #11's "direct native-link workload" Core workload: the minimal Layer 1
//! proof from `docs/rust-nim-native-linking.md` ("one Rust-produced
//! object and one Nim-produced object in the same link, with an
//! intentionally simple symbol relationship and no generated C header
//! contract"). Every other Rust/Nim fixture here has Rust as the build
//! orchestrator calling into a Nim static library; this one reverses the
//! direction — Nim is the final linked binary, and it calls directly
//! into this crate's exported symbol. Neither side goes through a
//! generated header: both just agree, by hand, on the raw C-compatible
//! symbol name and signature (see `../nim-bin/main.nim`).
//!
//! This is the fixture future direct native-link research (issue #4)
//! builds its deeper Layer 1-6 experiments on top of; it is not that
//! research itself.

#[no_mangle]
pub extern "C" fn rust_transform(x: i32) -> i32 {
    x.wrapping_mul(2).wrapping_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(rust_transform(21), 43);
    }
}

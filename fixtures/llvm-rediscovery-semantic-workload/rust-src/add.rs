//! Semantic contract, see ../CONTRACT.md: given two 32-bit two's-complement
//! integers, compute their sum modulo 2^32 (wrapping on overflow), no other
//! observable effect.

#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

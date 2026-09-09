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

// --- Issue #4 Layer 2/3 feasibility: fixed-layout struct and pointer
// round-trips, extending past #11's own Layer-1-only scope above. See
// `NOTES.md` for the methodology and results, and
// `docs/rust-nim-native-linking.md` for what these Layers mean.
//
// `Point` is declared independently here and in `../nim-bin/main.nim` —
// neither side reads the other's definition or a shared header. Layer 3
// asks whether that's actually safe to rely on, not whether it merely
// happens to work once; `rust_point_layout_probe` exists so both sides
// can report their own size/alignment/offset understanding at runtime
// and a caller can compare them, rather than assuming agreement because
// the linker didn't complain.

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// By-value struct round-trip: takes a `Point` by value, returns a new
/// `Point` by value. Small-aggregate-by-value is well-trodden C ABI
/// territory (SysV/AAPCS both classify small structs into registers),
/// unlike a bare fixed-size array parameter, which C itself has no
/// by-value calling convention for at all — deliberately not attempted
/// here for that reason.
#[no_mangle]
pub extern "C" fn rust_point_translate(p: Point, dx: i32, dy: i32) -> Point {
    Point {
        x: p.x + dx,
        y: p.y + dy,
    }
}

/// Mutates a Nim-allocated `Point` in place through a raw pointer.
///
/// # Safety
///
/// `p` must point to a valid, aligned, writable `Point`.
#[no_mangle]
pub unsafe extern "C" fn rust_point_scale_in_place(p: *mut Point, factor: i32) {
    (*p).x *= factor;
    (*p).y *= factor;
}

/// Reports this side's own understanding of `Point`'s layout, so the
/// caller (Nim) can compare it against its own independently-computed
/// `sizeof`/offset values instead of assuming the two match.
///
/// # Safety
///
/// Every `out_*` pointer must point to a valid, aligned, writable `i32`.
#[no_mangle]
pub unsafe extern "C" fn rust_point_layout_probe(
    out_size: *mut i32,
    out_align: *mut i32,
    out_offset_x: *mut i32,
    out_offset_y: *mut i32,
) {
    *out_size = std::mem::size_of::<Point>() as i32;
    *out_align = std::mem::align_of::<Point>() as i32;
    *out_offset_x = std::mem::offset_of!(Point, x) as i32;
    *out_offset_y = std::mem::offset_of!(Point, y) as i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(rust_transform(21), 43);
    }

    #[test]
    fn point_translate_known_value() {
        let p = Point { x: 3, y: 4 };
        assert_eq!(rust_point_translate(p, 10, -1), Point { x: 13, y: 3 });
    }

    #[test]
    fn point_scale_in_place_known_value() {
        let mut p = Point { x: 3, y: 4 };
        unsafe { rust_point_scale_in_place(&mut p, 5) };
        assert_eq!(p, Point { x: 15, y: 20 });
    }

    #[test]
    fn point_layout_probe_matches_rust_reflection() {
        let (mut size, mut align, mut off_x, mut off_y) = (0, 0, 0, 0);
        unsafe { rust_point_layout_probe(&mut size, &mut align, &mut off_x, &mut off_y) };
        assert_eq!(size, std::mem::size_of::<Point>() as i32);
        assert_eq!(align, std::mem::align_of::<Point>() as i32);
        assert_eq!(off_x, std::mem::offset_of!(Point, x) as i32);
        assert_eq!(off_y, std::mem::offset_of!(Point, y) as i32);
    }
}

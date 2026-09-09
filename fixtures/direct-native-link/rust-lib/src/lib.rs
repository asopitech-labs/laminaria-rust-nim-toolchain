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

/// Same computation as `rust_point_translate`, but writes the result
/// through an output pointer instead of returning it by value. Exists
/// to isolate which half of a by-value round trip an ABI implementation
/// gets wrong: the *input* argument (`p` is still passed by value here)
/// or the *return* value (nlvm's own compiler warns its small-struct
/// *return* ABI is an incomplete TODO — see `../NOTES.md` — this
/// function's input-by-value/output-by-pointer split is the practical
/// workaround: avoid returning an aggregate by value, keep passing one
/// in by value).
///
/// # Safety
///
/// `out` must point to a valid, aligned, writable `Point`.
#[no_mangle]
pub unsafe extern "C" fn rust_point_translate_via_pointer(
    p: Point,
    dx: i32,
    dy: i32,
    out: *mut Point,
) {
    (*out).x = p.x + dx;
    (*out).y = p.y + dy;
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

// --- Issue #4 Layer 3/4 focal question: can Rust resolve a pointer
// obtained from *Nim-owned* memory (a `seq`'s buffer, reference-counted
// by ORC), not only from Rust-owned memory Nim was merely lent a
// pointer into (`mixed-rust-nim-executable`'s pattern)? See `NOTES.md`
// for the full discussion — this is the harder direction, and where two
// distinct validity caveats actually matter: reallocation on growth,
// and deallocation when Nim's last reference to the buffer goes away
// (ORC's actual GC-ness; it never relocates a live seq's buffer on its
// own).

/// Reads through a pointer into memory Rust does not own (may be a
/// Nim `seq`'s buffer) and sums it. Read-only: proves resolution alone,
/// independent of the mutation question below.
///
/// # Safety
///
/// `data` must point to `len` valid, initialized, readable `i32`s for
/// the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn rust_sum_via_pointer(data: *const i32, len: i32) -> i64 {
    let slice = std::slice::from_raw_parts(data, len as usize);
    slice.iter().map(|&x| x as i64).sum()
}

/// Mutates memory Rust does not own (may be a Nim `seq`'s buffer)
/// in place, through a raw pointer, doubling each element.
///
/// # Safety
///
/// `data` must point to `len` valid, aligned, writable `i32`s for the
/// duration of this call, and no other reference to that memory may be
/// live concurrently.
#[no_mangle]
pub unsafe extern "C" fn rust_double_in_place(data: *mut i32, len: i32) {
    let slice = std::slice::from_raw_parts_mut(data, len as usize);
    for v in slice.iter_mut() {
        *v = v.wrapping_mul(2);
    }
}

/// The symmetric, reverse-direction caveat: Rust owns a growable
/// `Vec<i32>`, takes its buffer address, forces a reallocation (a
/// `Vec` push/extend past capacity moves the buffer, exactly like a Nim
/// `seq` growing past capacity does), and reports both addresses as
/// plain integers — never as pointers Nim could be tempted to
/// dereference — so the caller can observe whether the address changed
/// without ever touching invalidated memory. `Vec` drops normally at
/// the end of this call; nothing is leaked or exposed past its lifetime.
///
/// # Safety
///
/// Every `out_*` pointer must point to a valid, aligned, writable
/// destination of the matching type.
#[no_mangle]
pub unsafe extern "C" fn rust_vec_growth_probe(
    len: i32,
    out_addr_before: *mut i64,
    out_addr_after: *mut i64,
    out_len_after: *mut i32,
) {
    let mut v: Vec<i32> = (0..len).map(|i| i * 10).collect();
    *out_addr_before = v.as_ptr() as i64;
    v.extend(std::iter::repeat_n(0, v.len() + 1000));
    *out_addr_after = v.as_ptr() as i64;
    *out_len_after = v.len() as i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_known_value() {
        assert_eq!(rust_transform(21), 43);
    }

    #[test]
    fn sum_via_pointer_known_value() {
        let data = [10i32, 20, 30, 40, 50];
        let sum = unsafe { rust_sum_via_pointer(data.as_ptr(), data.len() as i32) };
        assert_eq!(sum, 150);
    }

    #[test]
    fn double_in_place_known_value() {
        let mut data = [10i32, 20, 30, 40, 50];
        unsafe { rust_double_in_place(data.as_mut_ptr(), data.len() as i32) };
        assert_eq!(data, [20, 40, 60, 80, 100]);
    }

    #[test]
    fn vec_growth_probe_reports_len_and_runs_without_ub() {
        let (mut before, mut after, mut len_after) = (0i64, 0i64, 0i32);
        unsafe { rust_vec_growth_probe(5, &mut before, &mut after, &mut len_after) };
        assert_eq!(len_after, 1010);
        // Address equality/inequality is platform-allocator-dependent (a
        // small allocator could in principle grow in place); what this
        // fixture actually needs is that both addresses were captured
        // without UB, which the assertions above already exercise.
        let _ = (before, after);
    }

    #[test]
    fn point_translate_known_value() {
        let p = Point { x: 3, y: 4 };
        assert_eq!(rust_point_translate(p, 10, -1), Point { x: 13, y: 3 });
    }

    #[test]
    fn point_translate_via_pointer_known_value() {
        let p = Point { x: 3, y: 4 };
        let mut out = Point { x: 0, y: 0 };
        unsafe { rust_point_translate_via_pointer(p, 10, -1, &mut out) };
        assert_eq!(out, Point { x: 13, y: 3 });
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

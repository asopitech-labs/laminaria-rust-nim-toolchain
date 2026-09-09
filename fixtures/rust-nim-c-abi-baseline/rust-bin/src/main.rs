//! `rust-nim-c-abi-baseline` fixture — Rust side. Links the static library
//! `build.rs` compiles from `../nim-lib/nimlib.nim` and calls it purely
//! through scalar C ABI parameters/return values (no Nim `seq`/`string`
//! crosses the boundary) — the reference point future direct native-link
//! experiments (#4) are compared against.

extern "C" {
    fn NimMain();
    fn laminaria_is_prime(n: i32) -> i32;
    fn laminaria_cluster_point_count(limit: i32) -> i32;
    fn laminaria_cluster_perimeter(limit: i32) -> i64;
    fn laminaria_cluster_centroid_x(limit: i32) -> i64;
    fn laminaria_cluster_centroid_y(limit: i32) -> i64;
}

fn main() {
    // Required once before calling any Nim-compiled function: initializes
    // Nim's runtime globals (GC/exception bookkeeping), even though none of
    // the functions below allocate GC'd memory themselves.
    unsafe { NimMain() };

    let limit = 200;
    let points = unsafe { laminaria_cluster_point_count(limit) };
    let perimeter = unsafe { laminaria_cluster_perimeter(limit) };
    let cx = unsafe { laminaria_cluster_centroid_x(limit) };
    let cy = unsafe { laminaria_cluster_centroid_y(limit) };
    let is_17_prime = unsafe { laminaria_is_prime(17) };

    println!("points={points}");
    println!("perimeter={perimeter}");
    println!("centroid=({cx}, {cy})");
    println!("is_17_prime={is_17_prime}");
}

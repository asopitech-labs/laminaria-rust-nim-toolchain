//! Direct executable experiments and measurement runners for issue #28.
//! The tests and runners in this crate exercise production implementations;
//! the historical D0 case catalog is design provenance, not an executable
//! verification authority.

pub mod d1b1_planner_cases;
pub mod d1b1_preflight;
pub mod d1b1_reference_cases;
pub mod d1b2_new_fixtures;
pub mod m3_baseline;
pub mod m8_baseline;
pub mod owned_identity;
pub mod planner_binary;
pub mod rust_cross_layer_baseline;

/// `laminaria_run::generate_run_id()` (`<unix_ns>-<pid>`) alone collided
/// in practice: this crate's own tests call `m3_baseline::run`/
/// `m8_baseline::run` concurrently from multiple threads of the same
/// test binary (the default `cargo test` behavior), and two calls close
/// enough in time produced the *same* run_id -- `write_run` then
/// silently overwrote one repetition's persisted evidence with another's,
/// caught directly by a `regenerate()`-from-disk test reading back a
/// different `kernel_nanos` mean than the in-memory report it was built
/// from. An added monotonic, process-local counter makes every call's
/// id unique regardless of clock resolution or caller concurrency, the
/// same fix already applied to `m3_baseline::run`'s temp directory
/// naming for the identical underlying race.
pub fn unique_run_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{n}", laminaria_run::generate_run_id())
}

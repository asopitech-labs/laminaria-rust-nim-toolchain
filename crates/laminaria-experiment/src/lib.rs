//! Issue #28 D1-a: case-registry validation for
//! `docs/design/issue-35-d0-cases.yaml`, and owned-baseline measurement
//! runners for the two cases D1-a actually executes
//! (`M3-owned-independent-chains`, `M8-many-unrequested-nim-planner`).
//! See `docs/design/issue-35-d0-spec.md` section 10 for the accepted
//! D0 scope this crate implements against.

pub mod m3_baseline;
pub mod m8_baseline;
pub mod owned_identity;
pub mod planner_binary;
pub mod registry;

/// The repository-relative path to the case registry this crate
/// validates, resolved from this crate's own manifest directory so it
/// works regardless of the caller's current working directory.
pub fn default_cases_yaml_path() -> std::path::PathBuf {
    planner_binary::repo_root().join("docs/design/issue-35-d0-cases.yaml")
}

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

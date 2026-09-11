//! Issue #28 D1-a: case-registry validation for
//! `docs/design/issue-35-d0-cases.yaml`, and owned-baseline measurement
//! runners for the two cases D1-a actually executes
//! (`M3-owned-independent-chains`, `M8-many-unrequested-nim-planner`).
//! See `docs/design/issue-35-d0-spec.md` section 10 for the accepted
//! D0 scope this crate implements against.

pub mod m3_baseline;
pub mod m8_baseline;
pub mod planner_binary;
pub mod registry;

/// The repository-relative path to the case registry this crate
/// validates, resolved from this crate's own manifest directory so it
/// works regardless of the caller's current working directory.
pub fn default_cases_yaml_path() -> std::path::PathBuf {
    planner_binary::repo_root().join("docs/design/issue-35-d0-cases.yaml")
}

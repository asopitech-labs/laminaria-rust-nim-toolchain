//! Issue #65 (G3 follow-up, carried-forward item 2): peak-memory
//! measurement for the same E0-vs-E1 comparison issue #47's `g3_e0_vs_e1`
//! already covers for wall time.
//!
//! **The problem this solves**: `g3_e0_vs_e1` calls `resolve`/
//! `resolve_demand_driven` in-process. `crate::tracer`'s (in
//! `laminaria-run`) only peak-RSS accounting mechanism is `wait4`-derived
//! `rusage`, obtained by spawning and reaping an actual child process --
//! it has no way to attribute a slice of the *current* process's own peak
//! RSS to one in-process function call. Issue #65's carried-forward item 2
//! names two options: "an in-process memory-sampling approach or an
//! out-of-process harness that still exercises the real production path."
//! This module takes the out-of-process route: `src/bin/
//! g3_e0_or_e1_child.rs` is a real, separate binary that does exactly one
//! ingest + one resolve call + exit, so `wait4`'s `ru_maxrss` on that
//! child process is genuinely that process's own peak RSS, not a shared
//! or approximated figure.
//!
//! **What this does and does not prove -- measured, then corrected**:
//! this module's first version claimed E0's own `peak_rss_bytes` trends
//! upward with injected scale while E1's stays flat, based on a native
//! Ubuntu 22.04 host run at scale 10,000 (E0 ~60.6MB vs. E1 ~31.3MB).
//! Re-running the identical harness inside this repo's own
//! `docker/bootstrap.Dockerfile` container (Debian 12/glibc 2.36, the
//! reproducible environment this project's own measurement-foundation
//! doc requires bootstrap/correctness claims to be checked against) does
//! **not** reproduce that trend: both E0 and E1 report peak RSS pinned
//! around ~84.5-84.8MB regardless of injected scale (0 through 10,000),
//! with no consistent E0-vs-E1 ordering across repeated runs. The
//! difference is glibc malloc's arena/trim behavior, not this crate's
//! own allocation pattern -- confirmed by `MALLOC_ARENA_MAX=1` making no
//! difference either. `ru_maxrss` (this module's only available metric,
//! via `wait4`) is dominated by allocator/OS-page-retention noise that
//! differs enough between a native host and this project's own
//! reproducibility container to flip the measured direction entirely,
//! so this module draws **no absolute or trend conclusion from
//! `peak_rss_bytes` at all** -- it still records the real, observed
//! value on every sample (never fabricated), but the only assertions
//! this module's own tests make are the same structurally-guaranteed
//! claims `g3_e0_vs_e1` already establishes for wall time (E1's own
//! retained candidate/obligation counts are scale-invariant, which
//! follows directly from `resolve_demand_driven` pruning before
//! `resolve` ever runs, not from any allocator behavior). Issue #65's
//! carried-forward item 2 asked for "an adopt/reject/reformulate
//! decision with evidence" -- the evidence here is: **reformulate**.
//! `ru_maxrss` peak-RSS-per-process is not a reliable signal for this
//! comparison at this fixture's scale; a real peak-memory claim would
//! need either a heap-profiling instrument (e.g. `jemalloc`
//! `stats.allocated` sampling, or a custom global allocator that counts
//! live bytes) that isolates the resolver's own allocations from
//! process/runtime/allocator-arena overhead, or evidence at a much
//! larger injected scale where the resolver's own share of peak RSS
//! could plausibly dominate the noise floor this module measured.

use std::path::{Path, PathBuf};

use laminaria_run::clock::RunClock;
use laminaria_run::tracer::trace_root_command;
use laminaria_run::types::RootCommand;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "0.1.0";
pub const WORKLOAD_ID: &str = "issue65-g3-peak-memory-cadd-app@v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChildResult {
    resolver: String,
    unreachable_candidates_injected: usize,
    package_candidates_total: usize,
    package_candidates_considered: Option<usize>,
    obligations_in_closure: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeakMemorySample {
    pub unreachable_candidates_injected: usize,
    pub package_candidates_total: usize,
    pub package_candidates_considered: Option<usize>,
    pub obligations_in_closure: usize,
    /// `None` only if this platform's `wait4`/`ru_maxrss` support is
    /// unavailable (see `laminaria_run::tracer`'s own non-Unix fallback
    /// note) -- never a fabricated value.
    pub peak_rss_bytes: Option<u64>,
    pub wall_seconds: f64,
}

/// Locates the `laminaria-g3-e0-or-e1-child` binary next to this process's
/// own executable -- the same directory `cargo build` places every binary
/// target of a workspace member in (e.g. `target/debug/`). A *test*
/// binary's `current_exe()` instead resolves to `target/debug/deps/`
/// (Cargo's separate directory for test artifacts), one level below where
/// bin targets land -- verified directly: `cargo build --bins` places
/// `laminaria-g3-e0-or-e1-child` in `target/debug/`, not
/// `target/debug/deps/`. So this checks the executable's own directory
/// first, then that directory's parent, rather than guessing which one a
/// given caller is running from.
fn child_binary_path() -> Result<PathBuf, String> {
    let current_exe =
        std::env::current_exe().map_err(|e| format!("failed to resolve current_exe: {e}"))?;
    let dir = current_exe
        .parent()
        .ok_or_else(|| "current_exe has no parent directory".to_string())?;
    let name = if cfg!(windows) {
        "laminaria-g3-e0-or-e1-child.exe"
    } else {
        "laminaria-g3-e0-or-e1-child"
    };

    let same_dir = dir.join(name);
    if same_dir.is_file() {
        return Ok(same_dir);
    }
    if let Some(parent_dir) = dir.parent() {
        let parent_candidate = parent_dir.join(name);
        if parent_candidate.is_file() {
            return Ok(parent_candidate);
        }
    }
    Err(format!(
        "child binary {name:?} not found next to {} or its parent directory; build it first \
         (cargo build -p laminaria-experiment --bin laminaria-g3-e0-or-e1-child)",
        dir.display()
    ))
}

/// Spawns the child binary once for `resolver` ("e0" or "e1") at the given
/// injected scale, traces it with `trace_root_command` (real `wait4`-based
/// peak RSS on Unix), and parses the child's single-line JSON stdout
/// result.
fn measure_one(
    child_binary: &Path,
    resolver: &str,
    scale: usize,
) -> Result<PeakMemorySample, String> {
    let clock = RunClock::start();
    let out_dir = std::env::temp_dir().join(format!(
        "laminaria-g3-peak-memory-{}-{resolver}-{scale}",
        std::process::id()
    ));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("failed to create out dir: {e}"))?;
    let stdout_path = out_dir.join("stdout.log");
    let stderr_path = out_dir.join("stderr.log");

    let root = RootCommand {
        program: child_binary.to_string_lossy().to_string(),
        args: vec![resolver.to_string(), scale.to_string()],
        cwd: None,
        env_overrides: Default::default(),
    };

    let record = trace_root_command(&clock, &root, &stdout_path, &stderr_path)
        .map_err(|e| format!("failed to spawn/trace child: {e}"))?;

    let status = record
        .exit_status
        .as_ref()
        .ok_or_else(|| "child record has no exit status".to_string())?;
    if !status.success {
        let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&out_dir);
        return Err(format!(
            "child process ({resolver}, scale={scale}) failed: {stderr}"
        ));
    }

    let stdout = std::fs::read_to_string(&stdout_path)
        .map_err(|e| format!("failed to read child stdout: {e}"))?;
    let child_result: ChildResult = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("failed to parse child stdout {stdout:?}: {e}"))?;

    let wall_seconds = record
        .end_elapsed_ns
        .map(|end| (end - record.start_elapsed_ns) as f64 / 1_000_000_000.0)
        .unwrap_or(0.0);

    let _ = std::fs::remove_dir_all(&out_dir);

    Ok(PeakMemorySample {
        unreachable_candidates_injected: child_result.unreachable_candidates_injected,
        package_candidates_total: child_result.package_candidates_total,
        package_candidates_considered: child_result.package_candidates_considered,
        obligations_in_closure: child_result.obligations_in_closure,
        peak_rss_bytes: record.resource_usage.peak_rss_bytes,
        wall_seconds,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct G3PeakMemoryReport {
    pub schema_version: String,
    pub workload_id: String,
    pub scales: Vec<usize>,
    pub eager: Vec<PeakMemorySample>,
    pub demand_driven: Vec<PeakMemorySample>,
    /// Whether this run's platform actually reported `peak_rss_bytes`
    /// (Unix; see `laminaria_run::tracer`'s Level 1 support). `false`
    /// means every sample's `peak_rss_bytes` is `None` and no
    /// peak-memory comparison can be drawn from this report at all.
    pub peak_rss_supported: bool,
}

impl G3PeakMemoryReport {
    /// Whether E1's own retained package-candidate count (via
    /// `package_candidates_considered`) stays scale-invariant across
    /// every injected scale this report measured -- a structural
    /// consequence of `resolve_demand_driven` pruning unreachable
    /// candidates before `resolve` ever runs (already established by
    /// `g3_e0_vs_e1`'s in-process comparison; this method reconfirms it
    /// against the out-of-process child). Unlike a `peak_rss_bytes`
    /// comparison, this claim depends only on this crate's own resolver
    /// logic, not on allocator/OS memory-retention behavior -- see this
    /// module's own doc comment on why `peak_rss_bytes` itself is not
    /// used for a pass/fail claim here.
    pub fn e1_considered_candidates_are_scale_invariant(&self) -> bool {
        let mut considered = self
            .demand_driven
            .iter()
            .filter_map(|s| s.package_candidates_considered);
        let Some(first) = considered.next() else {
            return false;
        };
        considered.all(|c| c == first)
    }
}

/// Runs the full E0-vs-E1 peak-memory comparison: for each scale in
/// `scales`, spawns the child binary once for E0 and once for E1,
/// recording each child's own real peak RSS via `wait4`.
pub fn run(scales: &[usize]) -> Result<G3PeakMemoryReport, String> {
    assert!(!scales.is_empty(), "scales must be non-empty");
    let child_binary = child_binary_path()?;

    let mut eager = Vec::with_capacity(scales.len());
    let mut demand_driven = Vec::with_capacity(scales.len());

    for &scale in scales {
        eager.push(measure_one(&child_binary, "e0", scale)?);
        demand_driven.push(measure_one(&child_binary, "e1", scale)?);
    }

    let peak_rss_supported = eager
        .iter()
        .chain(demand_driven.iter())
        .all(|s| s.peak_rss_bytes.is_some());

    Ok(G3PeakMemoryReport {
        schema_version: SCHEMA_VERSION.to_string(),
        workload_id: WORKLOAD_ID.to_string(),
        scales: scales.to_vec(),
        eager,
        demand_driven,
        peak_rss_supported,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// The real end-to-end case: spawns the real child binary at
    /// increasing injected scale for both E0 and E1, records each
    /// sample's real peak RSS (asserting only that it is nonzero and was
    /// actually observed, never that it follows any particular
    /// direction -- see this module's own doc comment on why a
    /// native-host run and a run inside this repo's own
    /// `docker/bootstrap.Dockerfile` container produced opposite
    /// E0-vs-E1 peak-RSS orderings for the identical harness), and
    /// asserts only the claims that do not depend on allocator behavior:
    /// E1's own retained candidate count and obligation count are
    /// scale-invariant (the same structural claim `g3_e0_vs_e1`'s
    /// in-process comparison already established, reconfirmed here
    /// against the out-of-process child).
    ///
    /// Requires `laminaria-g3-e0-or-e1-child` to already be built next to
    /// this test binary -- true under `cargo test --workspace` (Cargo
    /// builds every bin target of a workspace member before running its
    /// tests) but not if this test binary is invoked in isolation without
    /// that build step.
    #[test]
    fn e1_considered_candidates_and_obligations_stay_flat_across_scale_at_the_real_fixture() {
        let report = run(&[0, 100, 1000, 10_000]).expect("must run against the real fixture");
        assert_eq!(report.eager.len(), 4);
        assert_eq!(report.demand_driven.len(), 4);

        if report.peak_rss_supported {
            for sample in report.eager.iter().chain(report.demand_driven.iter()) {
                assert!(
                    sample.peak_rss_bytes.unwrap() > 0,
                    "expected a nonzero peak RSS, got {sample:?}"
                );
            }
        } else {
            eprintln!(
                "peak_rss_bytes unsupported on this platform; recording no peak-memory figures"
            );
        }

        let e1_obligation_counts: Vec<usize> = report
            .demand_driven
            .iter()
            .map(|s| s.obligations_in_closure)
            .collect();
        assert!(
            e1_obligation_counts.windows(2).all(|w| w[0] == w[1]),
            "E1's obligation count must be flat across scales, got {e1_obligation_counts:?}"
        );
        assert!(
            report.e1_considered_candidates_are_scale_invariant(),
            "E1's considered-candidate count must be flat across scales, got {:?}",
            report
                .demand_driven
                .iter()
                .map(|s| s.package_candidates_considered)
                .collect::<Vec<_>>()
        );

        // E0's own obligation count, by contrast, must grow with
        // injected scale -- it walks every injected candidate and keeps
        // a Rejected obligation per one (this is the structural
        // candidate-pruning claim; unlike peak RSS it depends only on
        // this crate's own resolve() logic, not on allocator behavior).
        let e0_obligation_counts: Vec<usize> = report
            .eager
            .iter()
            .map(|s| s.obligations_in_closure)
            .collect();
        assert!(
            e0_obligation_counts.windows(2).all(|w| w[1] > w[0]),
            "E0's obligation count must strictly grow with injected scale, got {e0_obligation_counts:?}"
        );
    }
}

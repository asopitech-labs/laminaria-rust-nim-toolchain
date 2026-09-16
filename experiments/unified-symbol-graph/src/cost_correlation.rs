//! Issue #67 -- the last unaddressed verification question: does the
//! *degree* `durability_v2::HeapEvictingGraph`'s `dependents_of`-style
//! reasoning would use as an invalidation-cost proxy (how many other
//! crates depend on a given crate) actually correlate with the *real*
//! compilation cost (wall-clock `rustc` time) of that crate?
//!
//! Every prior cycle in this experiment approximated invalidation cost by
//! graph degree alone (`dependents_of`, added in `b510e0f`) without ever
//! measuring whether degree predicts real compile time. The issue #67
//! coordinator's own integration comment names this the single remaining
//! unmeasured verification question and the top-priority item for the
//! next cycle. This module answers it directly against two real,
//! already-checked-in Rust workspace fixtures with contrasting degree
//! distributions:
//!
//! - `fixtures/deep-critical-path-graph/`: a 12-stage strictly linear
//!   dependency chain (`stage-01` -> `stage-02` -> ... -> `stage-12` ->
//!   `fixture-bin`), where degree (number of crates depending on a given
//!   crate, directly or transitively) is maximal for `stage-01` and
//!   minimal for `stage-12`/`fixture-bin` -- degree and chain *position*
//!   are perfectly confounded by construction.
//! - `fixtures/wide-parallel-graph/`: 8 mutually-independent leaf crates
//!   (`leaf-fibonacci`, `leaf-bubble-sort`, ...) that only `aggregator`
//!   depends on, so every leaf has the SAME transitive-dependent count
//!   (1) despite doing very different amounts of computation
//!   (`fibonacci` is a 10-iteration loop; `bubble_sort` is nested loops)
//!   -- degree carries zero discriminating information among the leaves
//!   by construction, so if compile time still varies here, degree
//!   provably cannot be the explanation for that variance.
//!
//! **Method**: each crate is compiled in isolation with a direct `rustc
//! --edition 2021 --crate-type lib|bin` invocation (not `cargo build`,
//! whose own caching/fingerprinting/parallel scheduling would add
//! confounding variables this experiment does not want to measure),
//! timed with `std::time::Instant`, with each dependency's compiled
//! `.rlib` passed via `--extern` in topological order. Degree (transitive
//! dependent count) is computed by parsing each fixture's `Cargo.toml`
//! files for `name = { path = "../name" }` dependency lines (no `toml`
//! crate dependency exists in this workspace -- see `disk_tiering.rs`'s
//! own "serde非依存" precedent for the same self-imposed constraint) and
//! inverting the resulting direct-dependency edges, then taking the
//! transitive closure.
//!
//! **This module does not assume the hypothesis is true or false.** The
//! task instructions are explicit that a weak correlation is a valid,
//! reportable result, not a failure to fix. See each test's own
//! `eprintln!` output (`cargo test -- --nocapture`) for the actual
//! measured numbers, reported honestly regardless of which way they cut.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// One crate's real, measured build-cost data point: its transitive
/// dependent count (the degree `dependents_of`-style reasoning would use
/// as an invalidation-cost proxy) paired with its actual `rustc` wall
/// time.
#[derive(Debug, Clone)]
pub struct CrateCostSample {
    pub crate_name: String,
    /// Number of other crates in the fixture that depend on this crate,
    /// directly or transitively -- i.e. how many crates would need to be
    /// invalidated/recompiled if this crate changed. This is the exact
    /// quantity `durability_v2`'s degree-based reasoning approximates
    /// invalidation cost with.
    pub transitive_dependent_count: usize,
    pub compile_time: Duration,
}

/// A minimal, dependency-free `[dependencies]` block parser sufficient
/// for this repo's own fixture `Cargo.toml` files, which only ever use
/// the `name = { path = "../name" }` form (confirmed by reading every
/// fixture `Cargo.toml` in `deep-critical-path-graph` and
/// `wide-parallel-graph` directly before writing this parser -- no
/// version requirements, no workspace-inherited dependency tables, no
/// features). Deliberately does not attempt to be a general TOML parser;
/// scoped exactly to what these two fixtures actually contain, matching
/// this crate's existing "no toml/serde dependency" precedent
/// (`Cargo.toml`/`Cargo.lock` in this crate list neither).
fn parse_path_dependencies(cargo_toml: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_dependencies_table = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dependencies_table = trimmed == "[dependencies]";
            continue;
        }
        if !in_dependencies_table || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Expect `crate-name = { path = "../crate-name" }`.
        if let Some((name, _rest)) = trimmed.split_once('=') {
            let name = name.trim();
            if !name.is_empty() {
                deps.push(name.to_string());
            }
        }
    }
    deps
}

/// One crate's identity within a fixture workspace: its package name (as
/// declared in `Cargo.toml`, used for `--extern name=path.rlib`), its
/// source directory, and its direct path-dependency crate names.
#[derive(Debug, Clone)]
struct FixtureCrate {
    name: String,
    dir: PathBuf,
    is_bin: bool,
    direct_deps: Vec<String>,
}

/// Discovers every member crate of a fixture workspace by reading its
/// top-level `Cargo.toml` `[workspace] members` list directly (same
/// string-scanning approach as `parse_path_dependencies`, scoped to this
/// repo's own fixture format: a `members = [...]` array of
/// `"crates/xxx"` string literals, one per line).
fn discover_fixture_crates(fixture_root: &Path) -> Vec<FixtureCrate> {
    let root_toml = fs::read_to_string(fixture_root.join("Cargo.toml"))
        .expect("fixture root Cargo.toml must be readable");
    let mut member_dirs = Vec::new();
    let mut in_members = false;
    for line in root_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members") {
            in_members = true;
        }
        if in_members {
            for token in trimmed.split(['"', '\'']) {
                if token.starts_with("crates/") {
                    member_dirs.push(token.to_string());
                }
            }
        }
        if in_members && trimmed.contains(']') {
            in_members = false;
        }
    }

    member_dirs
        .into_iter()
        .map(|member| {
            let dir = fixture_root.join(&member);
            let toml_text = fs::read_to_string(dir.join("Cargo.toml"))
                .unwrap_or_else(|e| panic!("failed to read {}/Cargo.toml: {e}", dir.display()));
            let name_line = toml_text
                .lines()
                .find(|l| l.trim_start().starts_with("name"))
                .expect("crate Cargo.toml must declare [package] name");
            let name = name_line
                .split('=')
                .nth(1)
                .expect("name line must have a value")
                .trim()
                .trim_matches('"')
                .to_string();
            let is_bin = toml_text.contains("[[bin]]");
            let direct_deps = parse_path_dependencies(&toml_text);
            FixtureCrate {
                name,
                dir,
                is_bin,
                direct_deps,
            }
        })
        .collect()
}

/// Inverts direct-dependency edges and takes the transitive closure:
/// for every crate, how many OTHER crates depend on it (directly or
/// transitively). This is exactly the "how much would be invalidated if
/// this changed" quantity `durability_v2`'s degree-based reasoning
/// approximates invalidation cost with -- computed here from real
/// fixture dependency data, not synthesized.
fn transitive_dependent_counts(crates: &[FixtureCrate]) -> HashMap<String, usize> {
    // direct_dependents[x] = crates that directly depend on x
    let mut direct_dependents: HashMap<String, Vec<String>> = HashMap::new();
    for c in crates {
        for dep in &c.direct_deps {
            direct_dependents
                .entry(dep.clone())
                .or_default()
                .push(c.name.clone());
        }
    }

    let mut counts = HashMap::new();
    for c in crates {
        let mut visited: HashSet<String> = HashSet::new();
        let mut frontier: Vec<String> = direct_dependents.get(&c.name).cloned().unwrap_or_default();
        while let Some(next) = frontier.pop() {
            if visited.insert(next.clone()) {
                if let Some(more) = direct_dependents.get(&next) {
                    frontier.extend(more.iter().cloned());
                }
            }
        }
        counts.insert(c.name.clone(), visited.len());
    }
    counts
}

/// Compiles every crate in `crates` in dependency order (parents before
/// children) using a direct `rustc` invocation, timing each build with
/// `Instant`, and passing each already-built dependency's `.rlib` via
/// `--extern`. `out_dir` must be a scratch directory outside this
/// worktree (per this crate's `disk_tiering.rs` precedent of requiring an
/// externally supplied scratch root rather than writing into the
/// project) -- see `USG_COST_CORRELATION_SCRATCH_ROOT` in the test
/// module below.
fn build_and_time_all(crates: &[FixtureCrate], out_dir: &Path) -> HashMap<String, Duration> {
    fs::create_dir_all(out_dir).expect("scratch out_dir must be creatable");

    // Topological order via repeated Kahn's-algorithm-style passes: a
    // crate is buildable once every entry in its direct_deps has already
    // been built. Both fixtures here are small (<=13 crates) and acyclic
    // by construction (real Cargo workspaces, already known to build),
    // so a simple O(n^2) fixed-point pass is more than adequate and
    // avoids pulling in a graph crate for a one-off topo-sort.
    let mut remaining: Vec<&FixtureCrate> = crates.iter().collect();
    let mut built: HashMap<String, PathBuf> = HashMap::new();
    let mut timings: HashMap<String, Duration> = HashMap::new();

    while !remaining.is_empty() {
        let mut progressed = false;
        let mut still_remaining = Vec::new();
        for c in remaining {
            if c.direct_deps.iter().all(|d| built.contains_key(d)) {
                let mut cmd = Command::new("rustc");
                cmd.arg("--edition").arg("2021");
                if c.is_bin {
                    cmd.arg("--crate-type").arg("bin");
                } else {
                    cmd.arg("--crate-type").arg("lib");
                }
                cmd.arg("--crate-name").arg(c.name.replace('-', "_"));
                let src = if c.is_bin {
                    c.dir.join("src/main.rs")
                } else {
                    c.dir.join("src/lib.rs")
                };
                cmd.arg(&src);
                cmd.arg("--out-dir").arg(out_dir);
                for dep in &c.direct_deps {
                    let rlib_path = built.get(dep).expect("dependency must already be built");
                    cmd.arg("--extern").arg(format!(
                        "{}={}",
                        dep.replace('-', "_"),
                        rlib_path.display()
                    ));
                }
                cmd.arg("-L").arg(out_dir);

                let start = Instant::now();
                let output = cmd.output().expect("failed to spawn rustc");
                let elapsed = start.elapsed();
                assert!(
                    output.status.success(),
                    "rustc failed compiling {}: stderr={}",
                    c.name,
                    String::from_utf8_lossy(&output.stderr)
                );

                let produced_name = c.name.replace('-', "_");
                let artifact = if c.is_bin {
                    out_dir.join(&produced_name)
                } else {
                    out_dir.join(format!("lib{produced_name}.rlib"))
                };
                assert!(
                    artifact.exists(),
                    "expected rustc artifact not found: {}",
                    artifact.display()
                );

                built.insert(c.name.clone(), artifact);
                timings.insert(c.name.clone(), elapsed);
                progressed = true;
            } else {
                still_remaining.push(c);
            }
        }
        assert!(
            progressed,
            "topological build made no progress -- fixture dependency graph is not a DAG \
             or references an undiscovered crate name"
        );
        remaining = still_remaining;
    }

    timings
}

/// Full measurement for one fixture: discovers its crates, computes real
/// transitive-dependent-count degree for each, builds every crate with a
/// direct `rustc` invocation timed by `Instant`, and joins the two into
/// one `CrateCostSample` per crate.
pub fn measure_fixture(fixture_root: &Path, scratch_root: &Path) -> Vec<CrateCostSample> {
    let crates = discover_fixture_crates(fixture_root);
    let degrees = transitive_dependent_counts(&crates);
    let timings = build_and_time_all(&crates, scratch_root);

    crates
        .iter()
        .map(|c| CrateCostSample {
            crate_name: c.name.clone(),
            transitive_dependent_count: *degrees.get(&c.name).expect("degree must be computed"),
            compile_time: *timings.get(&c.name).expect("timing must be recorded"),
        })
        .collect()
}

/// Pearson correlation coefficient between transitive dependent count
/// (x) and compile time in nanoseconds (y), computed directly from the
/// definition (no external stats crate). Returns `None` if either series
/// has zero variance (e.g. `wide-parallel-graph`'s leaves, which all
/// share the same degree by construction -- correlation is undefined,
/// not zero, in that case, and this function says so explicitly rather
/// than silently returning a misleading 0.0).
pub fn pearson_correlation(samples: &[CrateCostSample]) -> Option<f64> {
    let n = samples.len();
    if n < 2 {
        return None;
    }
    let xs: Vec<f64> = samples
        .iter()
        .map(|s| s.transitive_dependent_count as f64)
        .collect();
    let ys: Vec<f64> = samples
        .iter()
        .map(|s| s.compile_time.as_nanos() as f64)
        .collect();

    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let x_mean = mean(&xs);
    let y_mean = mean(&ys);

    let mut cov = 0.0;
    let mut x_var = 0.0;
    let mut y_var = 0.0;
    for i in 0..n {
        let dx = xs[i] - x_mean;
        let dy = ys[i] - y_mean;
        cov += dx * dy;
        x_var += dx * dx;
        y_var += dy * dy;
    }

    if x_var == 0.0 || y_var == 0.0 {
        return None;
    }
    Some(cov / (x_var.sqrt() * y_var.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolves the two fixtures' absolute paths relative to this crate's
    /// own manifest dir (`experiments/unified-symbol-graph`), independent
    /// of the working directory `cargo test` happens to be invoked from.
    fn fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
            .canonicalize()
            .unwrap_or_else(|e| panic!("fixture '{name}' must exist and be canonicalizable: {e}"))
    }

    /// A scratch directory OUTSIDE this worktree/repo for `rustc` output
    /// artifacts, following this crate's own `disk_tiering.rs` precedent
    /// (`USG_DISK_TIERING_SCRATCH_ROOT`) of requiring the caller to
    /// supply a writable directory via an environment variable rather
    /// than writing into the project tree, per this project's own
    /// CLAUDE.md constraint against generating test artifacts under the
    /// project root.
    fn scratch_root(subdir: &str) -> PathBuf {
        let root = std::env::var("USG_COST_CORRELATION_SCRATCH_ROOT").expect(
            "USG_COST_CORRELATION_SCRATCH_ROOT must be set to a writable directory outside \
             this worktree (e.g. the harness-provided scratchpad directory) -- this test \
             compiles real crates with rustc and must not write build artifacts into the \
             project tree",
        );
        let dir = Path::new(&root).join("cost_correlation").join(subdir);
        fs::create_dir_all(&dir).expect("scratch subdirectory must be creatable");
        dir
    }

    /// The core measurement for `deep-critical-path-graph`: a strictly
    /// linear 12-stage chain where degree (transitive dependent count)
    /// and chain position are perfectly confounded by construction
    /// (stage-01 has the most dependents, stage-12/fixture-bin the
    /// fewest). Reports the real per-crate compile time next to the real
    /// degree, and the Pearson correlation between them, honestly --
    /// including if the correlation is weaker than the "deep chains
    /// should show strong degree/cost correlation" prior hypothesis
    /// (stated in the task instructions as a PRE-measurement guess, not
    /// an assumed conclusion) predicted.
    #[test]
    fn degree_vs_compile_time_correlation_on_deep_critical_path_chain() {
        let scratch = scratch_root("deep-critical-path-graph");
        let samples = measure_fixture(&fixture_path("deep-critical-path-graph"), &scratch);

        let mut sorted = samples.clone();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.transitive_dependent_count));
        for s in &sorted {
            eprintln!(
                "[cost_correlation][deep-critical-path-graph] crate={:<12} transitive_dependent_count={:>2} compile_time={:?}",
                s.crate_name, s.transitive_dependent_count, s.compile_time
            );
        }

        let r = pearson_correlation(&samples);
        eprintln!(
            "[cost_correlation][deep-critical-path-graph] Pearson r(degree, compile_time) = {}",
            r.map(|v| format!("{v:.4}"))
                .unwrap_or_else(|| "undefined (zero variance in one series)".to_string())
        );

        // Correctness-only assertions: every crate actually built and
        // was timed, and degree is a real, non-trivial gradient here
        // (13 crates, 13 distinct degree values from 12 down to 0),
        // confirming this fixture's degree/position confound is real
        // data, not an assumption. No assertion is made about the sign
        // or magnitude of the correlation itself -- that is reported,
        // not asserted, per the task's explicit instruction not to
        // steer toward a favorable-looking result.
        assert_eq!(
            samples.len(),
            13,
            "deep-critical-path-graph must have 12 stages + fixture-bin"
        );
        let distinct_degrees: HashSet<usize> = samples
            .iter()
            .map(|s| s.transitive_dependent_count)
            .collect();
        assert!(
            distinct_degrees.len() > 1,
            "deep-critical-path-graph must show a real degree gradient across stages, got only {} distinct value(s)",
            distinct_degrees.len()
        );
    }

    /// The core measurement for `wide-parallel-graph`: 8 independent
    /// leaves that all share the SAME transitive dependent count (1,
    /// since only `aggregator` depends on each) despite very different
    /// actual computation (`fibonacci`'s bounded loop vs
    /// `bubble_sort`'s nested loops vs `factorial`'s single loop, etc).
    /// Degree therefore carries no discriminating power among the
    /// leaves by construction; this test reports whether their real
    /// compile times nonetheless differ, which would demonstrate a case
    /// where degree-only invalidation-cost estimation provably cannot
    /// distinguish crates that a real build system would still see take
    /// different amounts of time.
    #[test]
    fn degree_vs_compile_time_correlation_on_wide_parallel_leaves() {
        let scratch = scratch_root("wide-parallel-graph");
        let samples = measure_fixture(&fixture_path("wide-parallel-graph"), &scratch);

        let mut sorted = samples.clone();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.compile_time));
        for s in &sorted {
            eprintln!(
                "[cost_correlation][wide-parallel-graph] crate={:<20} transitive_dependent_count={} compile_time={:?}",
                s.crate_name, s.transitive_dependent_count, s.compile_time
            );
        }

        let leaves: Vec<&CrateCostSample> = samples
            .iter()
            .filter(|s| s.crate_name != "aggregator")
            .collect();
        let leaf_degrees: HashSet<usize> = leaves
            .iter()
            .map(|s| s.transitive_dependent_count)
            .collect();
        eprintln!(
            "[cost_correlation][wide-parallel-graph] leaf degree set = {:?} (expect exactly {{1}} -- all \
             8 leaves share the same dependent count by construction, so degree alone cannot \
             distinguish their real compile costs)",
            leaf_degrees
        );

        let leaf_times: Vec<f64> = leaves
            .iter()
            .map(|s| s.compile_time.as_nanos() as f64)
            .collect();
        let min_t = leaf_times.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_t = leaf_times.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        eprintln!(
            "[cost_correlation][wide-parallel-graph] leaf compile_time range: min={:.0}ns max={:.0}ns \
             max/min ratio={:.3}x -- this is the real spread among crates degree treats as IDENTICAL",
            min_t,
            max_t,
            max_t / min_t.max(1.0)
        );

        let r = pearson_correlation(&samples);
        eprintln!(
            "[cost_correlation][wide-parallel-graph] Pearson r(degree, compile_time) across ALL 9 \
             crates (8 leaves + aggregator) = {} -- note this number is driven almost entirely by \
             aggregator being the one outlier with degree 0, not by any real relationship among the \
             leaves, since the 8 leaves are degree-tied",
            r.map(|v| format!("{v:.4}"))
                .unwrap_or_else(|| "undefined (zero variance in one series)".to_string())
        );

        // Correctness-only assertions. The whole point of this fixture
        // is that leaf degree is CONSTANT (=1) by construction -- assert
        // that fact (a structural property of the fixture, verified
        // from real parsed data, not assumed), then report the real
        // compile-time spread among degree-tied crates without steering
        // the outcome either way.
        assert_eq!(
            samples.len(),
            9,
            "wide-parallel-graph must have 8 leaves + aggregator"
        );
        assert_eq!(
            leaf_degrees,
            HashSet::from([1]),
            "all 8 leaves must share transitive_dependent_count=1 (only aggregator depends on them) -- \
             if this fails the fixture's fan-out structure changed"
        );
    }
}

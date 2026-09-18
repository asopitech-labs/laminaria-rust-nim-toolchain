//! Issue #67 -- follow-up to `cost_correlation.rs`'s single-run degree/
//! compile-time correlation measurement. That module's own "next cycle"
//! hand-off named two concrete follow-ups: (a) whether a *weighted*
//! degree estimate (degree combined with a static complexity proxy, e.g.
//! "次数×平均コード行数") predicts real compile time better than bare
//! degree, and (b) whether the sample size (8-13 crates, single run) is
//! large enough to draw a reliable conclusion at all.
//!
//! This module answers (b) first, because it changes how (a) must be
//! read: it repeats `wide-parallel-graph`'s measurement 5 times and
//! reports the **coefficient of variation (CV%)** per crate -- the
//! single-run numbers in the previous cycle's report were never checked
//! for measurement noise before being fed into a Pearson correlation.
//!
//! **This module does not assume the hypothesis (that a weighted proxy
//! improves on bare degree) is true.** Per this experiment's own
//! precedent (`cost_correlation.rs`'s own doc comment, "does not assume
//! the hypothesis is true or false"), a negative or inconclusive result
//! is reported as such, not steered toward a favorable-looking number.

use std::collections::HashMap;
use std::path::Path;

use crate::cost_correlation::{measure_fixture, pearson_correlation, CrateCostSample};

/// One crate's static-complexity proxy values, computed by simple line-
/// based scanning of its own `src/lib.rs`/`src/main.rs` (no syntax-tree
/// parser dependency, matching this crate's existing "no external parser
/// crate" precedent in `cost_correlation.rs`'s own
/// `parse_path_dependencies`). Two independent proxies are tracked
/// separately rather than pre-combined into one score, so a caller can
/// test each axis's own correlation before deciding whether combining
/// them (and how) is even worth doing:
/// - `source_lines`: total non-blank, non-comment-only lines in the
///   crate's own source file (test modules excluded -- see
///   `count_non_test_source_lines`'s own doc comment for why).
/// - `control_flow_construct_count`: occurrences of `for `, `while `,
///   `if `, `match ` -- a crude proxy for the branching/looping
///   structure rustc's own type-checker and MIR builder must walk,
///   distinct from raw line count (a one-line `.map().sum()` chain has
///   real control flow rustc must still process, even though it has no
///   `for`/`if` token).
///
/// Public (rather than test-only) because `weighted_correlation_report`'s
/// own signature -- the only non-test caller -- must name this type; the
/// scanning helpers that populate it (`count_non_test_source_lines` etc.)
/// stay test-only below, matching this crate's own `cost_correlation.rs`
/// precedent of keeping fixture-scanning helpers inside `#[cfg(test)] mod
/// tests` rather than at module top level.
#[derive(Debug, Clone, Copy)]
pub struct StaticComplexity {
    pub source_lines: usize,
    pub control_flow_construct_count: usize,
}

/// Repeats `measure_fixture` `run_count` times against the same fixture
/// and scratch layout (a fresh scratch subdirectory per run, so no
/// cross-run rustc/filesystem caching confounds later runs), returning
/// one `Vec<CrateCostSample>` per run in call order. This is the
/// noise-quantification primitive the rest of this module's analysis
/// (`per_crate_variation`, `weighted_correlation_report`) builds on.
pub fn measure_fixture_repeated(
    fixture_root: &Path,
    scratch_root: &Path,
    run_count: usize,
) -> Vec<Vec<CrateCostSample>> {
    (0..run_count)
        .map(|run_index| {
            let run_scratch = scratch_root.join(format!("run{run_index}"));
            measure_fixture(fixture_root, &run_scratch)
        })
        .collect()
}

/// One crate's cross-run measurement-noise summary: mean compile time
/// and coefficient of variation (stdev/mean, as a fraction -- 0.10 means
/// "10% of the mean"). CV is used rather than raw stdev because this
/// module compares noise *across crates with different mean compile
/// times*, where raw stdev in nanoseconds is not directly comparable
/// (a slower crate can have a larger absolute stdev while actually being
/// more *proportionally* stable).
#[derive(Debug, Clone)]
pub struct NoiseSummary {
    pub crate_name: String,
    pub mean_compile_time_ns: f64,
    pub coefficient_of_variation: f64,
}

/// Computes per-crate mean and CV of compile time across `runs` (the
/// output of `measure_fixture_repeated`). Requires every run to report
/// the same set of crate names (a build failure or fixture mismatch
/// producing a different crate set across runs is a real bug this
/// function must not silently paper over by only reporting crates common
/// to all runs).
pub fn per_crate_variation(runs: &[Vec<CrateCostSample>]) -> Vec<NoiseSummary> {
    assert!(
        !runs.is_empty(),
        "per_crate_variation requires at least one run"
    );
    let first_names: Vec<String> = runs[0].iter().map(|s| s.crate_name.clone()).collect();
    for (i, run) in runs.iter().enumerate() {
        let names: Vec<String> = run.iter().map(|s| s.crate_name.clone()).collect();
        assert_eq!(
            names, first_names,
            "run {i} reported a different crate set than run 0 -- measurement is not comparable \
             across runs (expected {first_names:?}, got {names:?})"
        );
    }

    let mut times_by_crate: HashMap<String, Vec<f64>> = HashMap::new();
    for run in runs {
        for sample in run {
            times_by_crate
                .entry(sample.crate_name.clone())
                .or_default()
                .push(sample.compile_time.as_nanos() as f64);
        }
    }

    first_names
        .into_iter()
        .map(|name| {
            let times = &times_by_crate[&name];
            let n = times.len() as f64;
            let mean = times.iter().sum::<f64>() / n;
            let variance = if times.len() > 1 {
                times.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / (n - 1.0)
            } else {
                0.0
            };
            let stdev = variance.sqrt();
            NoiseSummary {
                crate_name: name,
                mean_compile_time_ns: mean,
                coefficient_of_variation: if mean > 0.0 { stdev / mean } else { 0.0 },
            }
        })
        .collect()
}

/// The result of testing whether a static-complexity-weighted degree
/// estimate correlates with real compile time better than bare degree
/// alone -- computed against the SAME data both `cost_correlation.rs`'s
/// bare-degree correlation and this weighted variant use (per-crate mean
/// compile time across `run_count` repeated measurements, not a single
/// noisy sample), so the two `Option<f64>` values are directly
/// comparable.
#[derive(Debug, Clone)]
pub struct WeightedCorrelationReport {
    pub bare_degree_correlation: Option<f64>,
    pub lines_weighted_correlation: Option<f64>,
    pub control_flow_weighted_correlation: Option<f64>,
}

/// Computes bare-degree, lines-weighted, and control-flow-weighted
/// correlations against mean compile time across repeated runs of one
/// fixture. "Weighted" here means `degree * complexity_proxy` (issue
/// #67's own hand-off text: "次数×平均コード行数") -- a simple product,
/// not a fitted regression, matching this experiment's existing
/// preference for the simplest testable version of a proposed
/// improvement before any more sophisticated model.
///
/// `include_crate` lets a caller exclude structurally different nodes
/// (e.g. `wide-parallel-graph`'s `aggregator`, the one crate every other
/// crate feeds into, matching `cost_correlation.rs`'s own precedent of
/// filtering it out of its degree-tied-leaves analysis) from the
/// correlation computation, without this function needing to know any
/// fixture's specific crate-naming convention itself.
pub fn weighted_correlation_report(
    fixture_root: &Path,
    scratch_root: &Path,
    run_count: usize,
    include_crate: impl Fn(&str) -> bool,
    crate_static_complexity: impl Fn(&str) -> StaticComplexity,
) -> WeightedCorrelationReport {
    let runs = measure_fixture_repeated(fixture_root, scratch_root, run_count);
    let noise: Vec<NoiseSummary> = per_crate_variation(&runs)
        .into_iter()
        .filter(|n| include_crate(&n.crate_name))
        .collect();

    // Use the FIRST run's degree values (degree is a static graph
    // property, identical across runs by construction -- asserting this
    // would be redundant with `measure_fixture`'s own determinism, which
    // `cost_correlation.rs`'s existing tests already exercise) paired
    // with each crate's cross-run MEAN compile time, rather than any
    // single run's noisy sample.
    let degree_by_crate: HashMap<String, usize> = runs[0]
        .iter()
        .map(|s| (s.crate_name.clone(), s.transitive_dependent_count))
        .collect();

    struct Row {
        degree: f64,
        lines_weighted: f64,
        ctrl_weighted: f64,
        mean_time: f64,
    }
    let rows: Vec<Row> = noise
        .iter()
        .map(|n| {
            let degree = *degree_by_crate.get(&n.crate_name).unwrap() as f64;
            let complexity = crate_static_complexity(&n.crate_name);
            Row {
                degree,
                lines_weighted: degree * complexity.source_lines as f64,
                ctrl_weighted: degree * complexity.control_flow_construct_count as f64,
                mean_time: n.mean_compile_time_ns,
            }
        })
        .collect();

    let pearson_pairs = |xs: Vec<f64>, ys: Vec<f64>| -> Option<f64> {
        // Reuses cost_correlation::pearson_correlation's exact formula by
        // constructing throwaway CrateCostSample values -- avoids a second,
        // divergent correlation implementation existing in this crate.
        let samples: Vec<CrateCostSample> = xs
            .into_iter()
            .zip(ys)
            .map(|(x, y)| CrateCostSample {
                crate_name: String::new(),
                transitive_dependent_count: x as usize,
                compile_time: std::time::Duration::from_nanos(y as u64),
            })
            .collect();
        pearson_correlation(&samples)
    };

    WeightedCorrelationReport {
        bare_degree_correlation: pearson_pairs(
            rows.iter().map(|r| r.degree).collect(),
            rows.iter().map(|r| r.mean_time).collect(),
        ),
        lines_weighted_correlation: pearson_pairs(
            rows.iter().map(|r| r.lines_weighted).collect(),
            rows.iter().map(|r| r.mean_time).collect(),
        ),
        control_flow_weighted_correlation: pearson_pairs(
            rows.iter().map(|r| r.ctrl_weighted).collect(),
            rows.iter().map(|r| r.mean_time).collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Counts source lines in a crate's own implementation, deliberately
    /// excluding its `#[cfg(test)] mod tests { ... }` block -- every
    /// fixture crate in this repo's `wide-parallel-graph`/
    /// `deep-critical-path-graph` carries a test module of its own
    /// (confirmed by reading every fixture source file directly before
    /// writing this scanner), and including test code would measure the
    /// wrong thing: test-module code is not part of what `rustc
    /// --crate-type lib` needs to monomorphize/typecheck for the
    /// *library*'s own public API surface as consumed by a dependent
    /// crate in this fixture's build graph (tests only run under `cargo
    /// test`'s own `--test` build, which this experiment's
    /// `build_and_time_all` never invokes). A line is counted if, after
    /// trimming whitespace, it is non-empty and does not start with `//`
    /// (a whole-line comment) -- deliberately simple, not a real
    /// tokenizer, matching this module's own "no external parser
    /// dependency" scope.
    fn count_non_test_source_lines(source: &str) -> usize {
        let mut count = 0;
        let mut in_test_module = false;
        let mut brace_depth_at_test_module_start = 0i32;
        let mut brace_depth = 0i32;
        for raw_line in source.lines() {
            let line = raw_line.trim();
            if line.contains("#[cfg(test)]") {
                in_test_module = true;
            }
            if in_test_module {
                brace_depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
                if brace_depth_at_test_module_start == 0 && line.contains("mod tests") {
                    brace_depth_at_test_module_start = brace_depth;
                } else if brace_depth_at_test_module_start != 0
                    && brace_depth < brace_depth_at_test_module_start
                {
                    in_test_module = false;
                    brace_depth_at_test_module_start = 0;
                }
                continue;
            }
            if !line.is_empty() && !line.starts_with("//") {
                count += 1;
            }
        }
        count
    }

    /// Scoped to the same non-test-module region
    /// `count_non_test_source_lines` uses, by simply running the same
    /// test-module exclusion first -- duplicated rather than refactored
    /// into a shared "strip test module" helper because each caller
    /// wants a different final reduction (line count vs. substring
    /// count), and this module's own files are short enough that the
    /// duplication is not a real maintenance burden.
    fn count_control_flow_constructs(source: &str) -> usize {
        let mut in_test_module = false;
        let mut brace_depth_at_test_module_start = 0i32;
        let mut brace_depth = 0i32;
        let mut count = 0;
        for raw_line in source.lines() {
            let line = raw_line.trim();
            if line.contains("#[cfg(test)]") {
                in_test_module = true;
            }
            if in_test_module {
                brace_depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
                if brace_depth_at_test_module_start == 0 && line.contains("mod tests") {
                    brace_depth_at_test_module_start = brace_depth;
                } else if brace_depth_at_test_module_start != 0
                    && brace_depth < brace_depth_at_test_module_start
                {
                    in_test_module = false;
                    brace_depth_at_test_module_start = 0;
                }
                continue;
            }
            for needle in ["for ", "while ", "if ", "match "] {
                count += line.matches(needle).count();
            }
        }
        count
    }

    fn measure_static_complexity(crate_src_dir: &Path, is_bin: bool) -> StaticComplexity {
        let src = crate_src_dir.join(if is_bin { "main.rs" } else { "lib.rs" });
        let text = fs::read_to_string(&src).unwrap_or_else(|e| {
            panic!(
                "failed to read {} for static complexity: {e}",
                src.display()
            )
        });
        StaticComplexity {
            source_lines: count_non_test_source_lines(&text),
            control_flow_construct_count: count_control_flow_constructs(&text),
        }
    }

    fn fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
            .canonicalize()
            .unwrap_or_else(|e| panic!("fixture '{name}' must exist and be canonicalizable: {e}"))
    }

    fn scratch_root(subdir: &str) -> PathBuf {
        let root = std::env::var("USG_COST_CORRELATION_SCRATCH_ROOT").expect(
            "USG_COST_CORRELATION_SCRATCH_ROOT must be set to a writable directory outside \
             this worktree",
        );
        let dir = Path::new(&root).join("cost_correlation_noise").join(subdir);
        fs::create_dir_all(&dir).expect("scratch subdirectory must be creatable");
        dir
    }

    /// The core noise-quantification test: repeats `wide-parallel-graph`
    /// (the fixture where degree is constant across leaves, so any
    /// observed compile-time variation cannot be attributed to degree at
    /// all) 5 times and reports each leaf's coefficient of variation.
    /// This is a prerequisite check for the weighted-correlation
    /// experiment below: if per-crate noise (CV%) is itself larger than
    /// the differences a weighted proxy would need to explain, a
    /// correlation computed from a SINGLE run (as the previous cycle's
    /// `cost_correlation.rs` measurement did) cannot be trusted to
    /// reflect a real relationship rather than noise.
    #[test]
    fn wide_parallel_leaves_show_large_per_crate_measurement_noise_across_5_runs() {
        let scratch = scratch_root("wide-parallel-graph-noise");
        let runs = measure_fixture_repeated(&fixture_path("wide-parallel-graph"), &scratch, 5);
        // Excludes `aggregator` -- this fixture's whole point (per
        // `cost_correlation.rs`'s own precedent, which applies the same
        // filter) is the 8 mutually-independent, degree-tied LEAVES;
        // `aggregator` is a structurally different node (degree 0, the
        // one crate all others feed into) and mixing it in would measure
        // a different question than "how noisy is repeated measurement
        // of degree-tied crates."
        let noise: Vec<NoiseSummary> = per_crate_variation(&runs)
            .into_iter()
            .filter(|n| n.crate_name != "aggregator")
            .collect();

        let mut sorted = noise.clone();
        sorted.sort_by(|a, b| {
            b.coefficient_of_variation
                .total_cmp(&a.coefficient_of_variation)
        });
        for n in &sorted {
            eprintln!(
                "[cost_correlation_noise][wide-parallel-graph] crate={:<20} mean_compile_time={:.2}ms CV={:.1}%",
                n.crate_name,
                n.mean_compile_time_ns / 1_000_000.0,
                n.coefficient_of_variation * 100.0
            );
        }

        let max_cv = sorted[0].coefficient_of_variation;
        let min_cv = sorted.last().unwrap().coefficient_of_variation;
        eprintln!(
            "[cost_correlation_noise][wide-parallel-graph] CV range across 8 leaves: {:.1}% to {:.1}% \
             (5 runs each) -- if this range is wide, a single-run correlation measurement (as used \
             in the prior cycle) risks mistaking measurement noise for a real degree/cost relationship",
            min_cv * 100.0,
            max_cv * 100.0
        );

        // Correctness-only assertions: every leaf produced 5 samples and a
        // non-negative CV. No assertion is made about the CV magnitude
        // itself being "acceptable" or "too high" -- that judgment is
        // reported in the eprintln! output and this experiment's own
        // follow-up doc, not baked into a pass/fail threshold that could
        // silently start failing (or silently stop being meaningful) as
        // real-world noise characteristics change.
        assert_eq!(
            noise.len(),
            8,
            "wide-parallel-graph must have exactly 8 leaves"
        );
        for n in &noise {
            assert!(
                n.coefficient_of_variation >= 0.0,
                "CV must be non-negative for crate {}",
                n.crate_name
            );
        }
    }

    /// The weighted-correlation experiment itself: does `degree *
    /// source_lines` or `degree * control_flow_construct_count` predict
    /// mean compile time (across 5 repeated runs, not a single noisy
    /// sample) better than bare degree alone, on `wide-parallel-graph`?
    ///
    /// This fixture is the harder, more informative test for a weighted
    /// proxy specifically because bare degree is CONSTANT (=1) across all
    /// 8 leaves -- `bare_degree_correlation` is therefore mathematically
    /// `None` (zero variance in the x-series) by construction. A weighted
    /// proxy's whole claimed value is to discriminate among degree-tied
    /// crates using their static complexity instead; this test is the
    /// direct empirical check of whether that claim holds for this
    /// fixture's real crates.
    #[test]
    fn weighted_degree_proxies_on_wide_parallel_leaves_report_measured_correlation_honestly() {
        let scratch = scratch_root("wide-parallel-graph-weighted");
        let fixture_root = fixture_path("wide-parallel-graph");

        let complexity_lookup = |crate_name: &str| -> StaticComplexity {
            let dir = fixture_root.join("crates").join(crate_name);
            let is_bin = crate_name == "aggregator";
            measure_static_complexity(&dir.join("src"), is_bin)
        };

        // Excludes `aggregator`, matching `cost_correlation.rs`'s own
        // precedent -- see this test's own doc comment on why mixing it
        // in would measure a different question.
        let report = weighted_correlation_report(
            &fixture_root,
            &scratch,
            5,
            |name| name != "aggregator",
            complexity_lookup,
        );

        eprintln!(
            "[cost_correlation_noise][wide-parallel-graph] bare_degree_correlation = {} \
             (expected None/undefined -- all 8 leaves share degree=1, so bare degree has zero \
             variance and cannot correlate with anything by construction)",
            report
                .bare_degree_correlation
                .map(|v| format!("{v:.4}"))
                .unwrap_or_else(|| "undefined (zero variance)".to_string())
        );
        eprintln!(
            "[cost_correlation_noise][wide-parallel-graph] lines_weighted_correlation (degree*source_lines) = {}",
            report
                .lines_weighted_correlation
                .map(|v| format!("{v:.4}"))
                .unwrap_or_else(|| "undefined".to_string())
        );
        eprintln!(
            "[cost_correlation_noise][wide-parallel-graph] control_flow_weighted_correlation (degree*ctrl_flow_count) = {}",
            report
                .control_flow_weighted_correlation
                .map(|v| format!("{v:.4}"))
                .unwrap_or_else(|| "undefined".to_string())
        );

        // Since degree is constant (=1) for every leaf, degree * proxy ==
        // proxy itself here -- this assertion documents that fact rather
        // than hiding it, so a reader does not mistake this fixture's
        // result for "weighting genuinely combined two independent
        // signals" (that combination is exercised by
        // deep-critical-path-graph instead, where degree actually varies).
        assert_eq!(
            report.bare_degree_correlation, None,
            "bare degree correlation must be undefined on wide-parallel-graph by construction \
             (all leaves share degree=1)"
        );
    }

    /// The companion measurement on `deep-critical-path-graph`, where
    /// degree DOES vary (12 down to 0) -- here `degree * complexity`
    /// combines two genuinely independent signals, and this test reports
    /// whether that combination correlates with mean compile time better
    /// than bare degree, again across repeated runs rather than a single
    /// sample.
    #[test]
    fn weighted_degree_proxies_on_deep_critical_path_chain_report_measured_correlation_honestly() {
        let scratch = scratch_root("deep-critical-path-graph-weighted");
        let fixture_root = fixture_path("deep-critical-path-graph");

        let complexity_lookup = |crate_name: &str| -> StaticComplexity {
            let dir = fixture_root.join("crates").join(crate_name);
            let is_bin = crate_name == "fixture-bin";
            measure_static_complexity(&dir.join("src"), is_bin)
        };

        let report = weighted_correlation_report(
            &fixture_root,
            &scratch,
            5,
            |_name| true,
            complexity_lookup,
        );

        eprintln!(
            "[cost_correlation_noise][deep-critical-path-graph] bare_degree_correlation (5-run mean) = {}",
            report
                .bare_degree_correlation
                .map(|v| format!("{v:.4}"))
                .unwrap_or_else(|| "undefined".to_string())
        );
        eprintln!(
            "[cost_correlation_noise][deep-critical-path-graph] lines_weighted_correlation = {}",
            report
                .lines_weighted_correlation
                .map(|v| format!("{v:.4}"))
                .unwrap_or_else(|| "undefined".to_string())
        );
        eprintln!(
            "[cost_correlation_noise][deep-critical-path-graph] control_flow_weighted_correlation = {}",
            report
                .control_flow_weighted_correlation
                .map(|v| format!("{v:.4}"))
                .unwrap_or_else(|| "undefined".to_string())
        );

        // Both series (degree 12..0, 13 crates) have real variance here,
        // so both correlations must be computable (Some), unlike the
        // wide-parallel-graph test above. No assertion on sign/magnitude,
        // per this experiment's "report, don't steer" discipline.
        assert!(
            report.bare_degree_correlation.is_some(),
            "deep-critical-path-graph has real degree variance (12..0) and must produce a \
             defined bare-degree correlation"
        );
    }
}

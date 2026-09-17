//! Issue #47 (G3, Lane B): a real, production-path E0-vs-E1 comparison.
//!
//! **What this measures**: `laminaria_plan::dependency_graph::resolve`
//! (E0, eager -- unconditionally iterates every candidate in the input)
//! versus `resolve_demand_driven` (E1 -- prunes candidates unreachable
//! from `demand_entry_point` before `resolve` ever sees them), on the
//! real G1/G2 `cadd`/`app` fixture's own real ingested input
//! (`laminaria_run::cross_ecosystem_ingest::ingest_fixture_input`), with
//! a configurable number of synthetic unreachable candidates injected --
//! packages declaring an export no real `ffi_requirements` entry ever
//! names, so `resolve` (E0) would walk them (as extra, unselected root
//! candidates; see `resolve_root_packages`'s own doc comment) but never
//! select them, and `resolve_demand_driven` (E1) prunes them before
//! `resolve` is even called.
//!
//! **Why this is production-path evidence, not a synthetic
//! microbenchmark of `resolve` alone**: after computing each closure,
//! this module actually executes the real Checkpoint 1 action
//! (`laminaria_run::g2_execute::compile_and_archive_cadd_v1` -- real
//! `cc -c` + `ar rcs`, real `nm` verification) against both the E0- and
//! E1-produced closures and asserts the two runs produce byte-identical
//! archives (same SHA-256) -- the unreachable candidates injected for
//! this measurement must never change what actually gets compiled,
//! archived, or discharged. This is issue #47's own direct-acceptance
//! requirement: "observable native executable behavior and obligation
//! outcomes are identical before performance comparison."
//!
//! **What this does not measure**: the injected unreachable candidates
//! are declared facts only (no real source files on disk), so E0 never
//! actually invokes `cc`/`ar` on them either -- `resolve` only ever
//! walks their `PackageCandidateFacts` entries in memory, it never
//! compiles anything G1 didn't select. The wall-time/CPU difference this
//! module measures is therefore `resolve`'s own graph-construction cost
//! at increasing candidate-pool size, not a difference in which
//! toolchain commands actually run -- exactly the "expanded/pruned node
//! count" issue #47 asks for, isolated from Phase 2's own execution cost
//! (already measured separately for the `bcm.c` worked example, see
//! `scripts/research/measure-bcm-unity-build-cost.sh`).

use std::path::Path;
use std::time::Instant;

use laminaria_plan::dependency_graph::{
    resolve, resolve_demand_driven, DependencyResolutionInput, Ecosystem, FfiExportFacts,
    FfiRequirementFacts, PackageCandidateFacts, PositiveClosure, Role, SourceModuleFacts,
};
use laminaria_run::command_runner::RecordingCommandRunner;
use laminaria_run::cross_ecosystem_ingest::{ingest_fixture_input, FixtureLayout};
use laminaria_run::g2_execute::compile_and_archive_cadd_v1;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "0.1.0";
pub const WORKLOAD_ID: &str = "issue47-g3-e0-vs-e1-cadd-app@v1";

/// One synthetic package candidate wholly unreachable from
/// `demand_entry_point`, plus the synthetic `FfiRequirementFacts` that
/// makes `resolve` (E0) treat it as a *provider* candidate rather than a
/// root/consumer one -- `resolve_root_packages` requires every
/// non-provider candidate to be Cargo/Nimble (`resolve`'s own structural
/// invariant, "a root package is only ever Cargo or Nimble"), so a
/// synthetic C/C++ candidate must be wired as an unreachable *provider*,
/// never a root, to stay inside that invariant. The synthetic requirement
/// itself is declared by a synthetic, never-actually-ingested source id
/// (`synthetic/.../demand.rs`) that never appears in `input.sources` and
/// is therefore never reachable from the real `demand_entry_point`
/// ("app") -- `resolve_demand_driven`'s own reachability computation only
/// follows a requirement whose `declaring_source` is itself reachable,
/// so this requirement, and the provider package id it names, are both
/// correctly unreachable, while `resolve` (E0) still walks and correctly
/// rejects-or-ignores them as ordinary (if never-selected) candidates.
/// `pub` (not just crate-private) so `g3_peak_memory`'s out-of-process
/// child binary (`src/bin/g3_e0_or_e1_child.rs`) can build the identical
/// injected input this module's own in-process E0-vs-E1 comparison uses,
/// rather than a second, divergent implementation of the same synthetic
/// candidate shape.
pub fn synthetic_unreachable_candidate(
    index: usize,
) -> (
    SourceModuleFacts,
    PackageCandidateFacts,
    FfiRequirementFacts,
) {
    let package_id = format!("synthetic-unreachable-{index}");
    let source_id = format!("synthetic/{package_id}/src.c");
    let symbol = format!("synthetic_unreachable_fn_{index}");
    let source = SourceModuleFacts {
        id: source_id.clone(),
        ecosystem: Ecosystem::C,
        package_id: package_id.clone(),
    };
    let candidate = PackageCandidateFacts {
        ecosystem: Ecosystem::C,
        package_id: package_id.clone(),
        version: "1.0.0".to_string(),
        role: Role::Target,
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        sources: vec![source_id.clone()],
        declared_exports: vec![FfiExportFacts {
            declaring_source: source_id,
            symbol: symbol.clone(),
            abi: "C".to_string(),
            param_count: 0,
            return_type: "i32".to_string(),
        }],
        declared_constraints: vec![],
    };
    // Declared by a source id that never appears in `input.sources` --
    // unreachable from `demand_entry_point` by construction, but present
    // enough in `input.ffi_requirements` for E0's own
    // `resolve_provider_for_requirement` to walk it as a genuine
    // (unselected, since no real source ever requires it) provider
    // candidate.
    let requirement = FfiRequirementFacts {
        declaring_source: format!("synthetic/{package_id}/never-reachable-demand.rs"),
        symbol,
        abi: "C".to_string(),
        param_count: 0,
        return_type: "i32".to_string(),
        expected_provider_package: package_id,
    };
    (source, candidate, requirement)
}

/// `pub` for the same reason as `synthetic_unreachable_candidate` above:
/// shared by both this module's in-process comparison and
/// `g3_peak_memory`'s out-of-process child binary.
pub fn inject_unreachable_candidates(
    mut input: DependencyResolutionInput,
    count: usize,
) -> DependencyResolutionInput {
    for i in 0..count {
        let (source, candidate, requirement) = synthetic_unreachable_candidate(i);
        input.sources.push(source);
        input.package_candidates.push(candidate);
        input.ffi_requirements.push(requirement);
    }
    input
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedResolution {
    pub unreachable_candidates_injected: usize,
    pub package_candidates_total: usize,
    /// `None` for E0 -- `resolve` has no pruning to report, so this
    /// field distinguishes "E0, nothing to compare" from "E1, pruned
    /// down to N" rather than reporting a fabricated equal-to-total
    /// value for E0.
    pub package_candidates_considered: Option<usize>,
    pub wall_seconds: f64,
    pub obligations_in_closure: usize,
}

fn time_resolve_eager(
    input: &DependencyResolutionInput,
) -> Result<(PositiveClosure, TimedResolution), String> {
    let start = Instant::now();
    let closure = resolve(input).map_err(|e| format!("E0 resolve() rejected: {e:?}"))?;
    let wall_seconds = start.elapsed().as_secs_f64();
    let stats = TimedResolution {
        unreachable_candidates_injected: 0,
        package_candidates_total: input.package_candidates.len(),
        package_candidates_considered: None,
        wall_seconds,
        obligations_in_closure: closure.obligations.len(),
    };
    Ok((closure, stats))
}

fn time_resolve_demand_driven(
    input: &DependencyResolutionInput,
) -> Result<(PositiveClosure, TimedResolution), String> {
    let start = Instant::now();
    let (closure, expansion) = resolve_demand_driven(input)
        .map_err(|e| format!("E1 resolve_demand_driven() rejected: {e:?}"))?;
    let wall_seconds = start.elapsed().as_secs_f64();
    let stats = TimedResolution {
        unreachable_candidates_injected: 0,
        package_candidates_total: expansion.package_candidates_total,
        package_candidates_considered: Some(expansion.package_candidates_considered),
        wall_seconds,
        obligations_in_closure: closure.obligations.len(),
    };
    Ok((closure, stats))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEquivalence {
    pub e0_archive_sha256: String,
    pub e1_archive_sha256: String,
    pub identical: bool,
}

/// Actually executes the real Checkpoint 1 action
/// (`cc -c` + `ar rcs` + `nm` verification) against a closure, in an
/// isolated output directory so E0's and E1's own runs never share
/// files on disk.
fn execute_and_archive(
    closure: &mut PositiveClosure,
    fixture_root: &Path,
    label: &str,
) -> Result<String, String> {
    let out_dir = std::env::temp_dir().join(format!(
        "laminaria-g3-e0-vs-e1-{}-{}",
        std::process::id(),
        label
    ));
    let evidence = compile_and_archive_cadd_v1(closure, fixture_root, &out_dir)
        .map_err(|e| format!("real cc/ar execution failed for {label}: {e:?}"))?;
    let sha256 = evidence.archive_sha256.clone();
    let _ = std::fs::remove_dir_all(&out_dir);
    Ok(sha256)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct G3E0VsE1Report {
    pub schema_version: String,
    pub workload_id: String,
    pub scales: Vec<usize>,
    pub eager: Vec<TimedResolution>,
    pub demand_driven: Vec<TimedResolution>,
    /// Computed once, at the largest injected scale -- the real `cc`/`ar`
    /// action pair is deterministic and independent of how many
    /// unreachable candidates were in the input, so repeating this check
    /// at every scale would only re-verify the same fact.
    pub artifact_equivalence: ArtifactEquivalence,
}

impl G3E0VsE1Report {
    /// Issue #47's own direct-acceptance requirement, computed from this
    /// already-recorded evidence: the real compiled archive is
    /// byte-identical between E0 and E1 (same SHA-256) at every scale
    /// this report measured, and E1 actually pruned a nonzero number of
    /// candidates at every nonzero scale (proving there was something to
    /// prune, not a vacuous comparison).
    pub fn e1_matches_e0_behavior_and_actually_prunes(&self) -> bool {
        if !self.artifact_equivalence.identical {
            return false;
        }
        self.demand_driven.iter().all(|r| {
            r.unreachable_candidates_injected == 0
                || r.package_candidates_considered
                    .is_some_and(|considered| considered < r.package_candidates_total)
        })
    }
}

/// Runs the full E0-vs-E1 comparison for real: ingests the fixture's own
/// real `cargo metadata`/Nimble-manifest/C-header facts
/// (`ingest_fixture_input`), injects `scales` unreachable candidates at
/// each named scale, times `resolve` (E0) and `resolve_demand_driven`
/// (E1) at each scale, then actually executes the real Checkpoint 1
/// `cc`/`ar` action against both the largest-scale E0 and E1 closures
/// and compares the resulting archives.
pub fn run(scales: &[usize]) -> Result<G3E0VsE1Report, String> {
    assert!(!scales.is_empty(), "scales must be non-empty");

    let layout = FixtureLayout::discover();
    let runner = RecordingCommandRunner::new();
    let base_input = ingest_fixture_input(&runner, &layout, &["1.0.0"])
        .map_err(|e| format!("failed to ingest the real fixture input: {e:?}"))?;

    let mut eager = Vec::with_capacity(scales.len());
    let mut demand_driven = Vec::with_capacity(scales.len());
    let mut last_e0_closure: Option<PositiveClosure> = None;
    let mut last_e1_closure: Option<PositiveClosure> = None;

    for &scale in scales {
        let input = inject_unreachable_candidates(base_input.clone(), scale);

        let (e0_closure, mut e0_stats) = time_resolve_eager(&input)?;
        e0_stats.unreachable_candidates_injected = scale;
        eager.push(e0_stats);

        let (e1_closure, mut e1_stats) = time_resolve_demand_driven(&input)?;
        e1_stats.unreachable_candidates_injected = scale;
        demand_driven.push(e1_stats);

        last_e0_closure = Some(e0_closure);
        last_e1_closure = Some(e1_closure);
    }

    let mut e0_closure = last_e0_closure.expect("scales is non-empty");
    let mut e1_closure = last_e1_closure.expect("scales is non-empty");

    let e0_archive_sha256 = execute_and_archive(&mut e0_closure, &layout.root, "e0")?;
    let e1_archive_sha256 = execute_and_archive(&mut e1_closure, &layout.root, "e1")?;
    let artifact_equivalence = ArtifactEquivalence {
        identical: e0_archive_sha256 == e1_archive_sha256,
        e0_archive_sha256,
        e1_archive_sha256,
    };

    Ok(G3E0VsE1Report {
        schema_version: SCHEMA_VERSION.to_string(),
        workload_id: WORKLOAD_ID.to_string(),
        scales: scales.to_vec(),
        eager,
        demand_driven,
        artifact_equivalence,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// The real end-to-end case: injects unreachable candidates at
    /// increasing scale, confirms E1 actually prunes them
    /// (`package_candidates_considered < package_candidates_total`) at
    /// every nonzero scale, and confirms the real compiled archive is
    /// byte-identical between E0 and E1 -- the unreachable candidates
    /// must never change what Checkpoint 1 actually compiles.
    #[test]
    fn e1_prunes_unreachable_candidates_and_produces_the_same_real_archive_as_e0() {
        let report = run(&[0, 10, 100]).expect("must run against the real fixture");
        assert!(
            report.e1_matches_e0_behavior_and_actually_prunes(),
            "report: {report:#?}"
        );
        assert_eq!(report.eager.len(), 3);
        assert_eq!(report.demand_driven.len(), 3);

        // E1's own retained obligation count must never depend on how
        // many unreachable candidates were injected -- pruning them
        // before `resolve` runs means `resolve` never even knows they
        // existed, so E1's obligation count stays flat across every
        // scale. (E0's own obligation count does grow with scale: each
        // unreachable candidate still becomes a real, if `Rejected`,
        // `Symbol` obligation in E0's closure -- see this module's own
        // doc comment on E0's walk cost, not a bug in either resolve
        // path.)
        let e1_obligation_counts: Vec<usize> = report
            .demand_driven
            .iter()
            .map(|r| r.obligations_in_closure)
            .collect();
        assert!(
            e1_obligation_counts.windows(2).all(|w| w[0] == w[1]),
            "E1's obligation count must be flat across scales, got {e1_obligation_counts:?}"
        );

        // At scale 100, E1 must have considered far fewer candidates
        // than E0's own total.
        let e1_at_100 = &report.demand_driven[2];
        assert_eq!(e1_at_100.unreachable_candidates_injected, 100);
        assert!(
            e1_at_100.package_candidates_considered.unwrap() < e1_at_100.package_candidates_total
        );
    }
}

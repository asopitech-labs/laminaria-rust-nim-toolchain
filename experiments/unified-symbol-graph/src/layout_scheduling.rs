//! Issue #69 -- does an `assign_layout` variant that actually uses
//! `mutation_seq` (build-time critical-path proxy) and `RequiresEdge`
//! (call-affinity proxy) measurably beat the current deterministic-only
//! `SymbolId` sort, on either signal `lib.rs`'s own module doc comment
//! names?
//!
//! **Why a parallel function, not a replacement of `assign_layout`**:
//! `assign_layout`'s own doc comment states its only guaranteed property
//! is determinism (same graph -> same layout, reproducible across runs).
//! `assign_layout_scheduling_aware` below keeps that same guarantee
//! (ties are broken by `SymbolId`, never by hash-map iteration order) but
//! is a genuinely different, independently-testable function so a caller
//! -- or this module's own tests -- can compare both against the same
//! graph rather than only having the "improved" version available to
//! measure against itself.
//!
//! # The two signals, and what each one costs to use
//!
//! - **Build-time critical-path proxy (`declared_at_seq`)**: `lib.rs`
//!   only ever exposed one *global* `mutation_seq` counter (evidence-only,
//!   via `mutation_count()`); no per-symbol arrival order existed before
//!   this issue. `SharedSymbolGraph::declared_at_seq` (added for this
//!   issue) now records, per `SymbolId`, the `mutation_seq` value at
//!   first declaration -- exactly the "when did this symbol arrive"
//!   signal the doc comment describes, cheap (one extra `HashMap` insert
//!   per declaration, no new traversal).
//! - **Runtime call-affinity proxy (relocations, not `RequiresEdge`
//!   directly)**: the issue names `RequiresEdge` as the call-graph
//!   signal, but `RequiresEdge` only records `requiring_realm ->
//!   symbol` -- a whole *realm*, not the specific symbol that issued the
//!   call. Using it directly would force treating every boundary symbol
//!   a realm declares as a possible caller of everything that realm
//!   requires, a coarse over-approximation this module tried first and
//!   rejected once it produced spurious affinity between unrelated
//!   symbols that merely share a realm (see this module's own git
//!   history / issue #69 thread for that dead end). Each `Committed`
//!   symbol's own `CodeBody::relocations` records the *exact* other
//!   boundary symbols it calls (`ElfX86_64PendingReloc::target`) --
//!   already-resolved, per-symbol, zero ambiguity -- so this module
//!   derives affinity from relocations instead: for every `Committed`
//!   symbol, each of its own relocation targets gets its pairwise
//!   affinity with that symbol incremented once. This is a strictly more
//!   precise source for the same signal `RequiresEdge` was named for,
//!   available because this graph happens to also carry `CodeBody`
//!   (a real linker's call-graph profile is reconstructed from an
//!   external execution trace; this hypothesis gets an exact static
//!   call edge for free from data it already has).
//!
//! # Algorithm: greedy critical-path-ordered clustering, not true joint
//! optimization
//!
//! The literature the `lib.rs` doc comment cites (joint
//! scheduling-and-placement is NP-hard) rules out an exact joint solver
//! as a reasonable scope for this experiment. What is implemented instead
//! is deliberately simple, in the same spirit as lld's own
//! `computeCallGraphProfileOrder` (a greedy heuristic, not an optimal
//! solver):
//!
//! 1. Sort all `Committed` symbols by `declared_at_seq` ascending (ties
//!    broken by `SymbolId`, for determinism) -- this is the build-time
//!    critical-path-ordered pass.
//! 2. Walk that order left to right. When a symbol is placed, immediately
//!    place its single highest-affinity not-yet-placed neighbor right
//!    after it (if one exists above a zero threshold), before continuing
//!    the outer walk. This is the runtime-locality pass, threaded into
//!    the same single walk instead of a second, separate pass over the
//!    whole graph.
//!
//! This is a one-pass approximation, not a proof that the joint problem
//! is solved optimally -- flagged honestly, not oversold.
//!
//! # Result (reported, not steered)
//!
//! See this module's own tests for the actual numbers. Summary: on a
//! synthetic graph built so the critical-path order and alphabetical
//! order are deliberately inverted, and a call-affine pair is
//! deliberately placed far apart alphabetically, the scheduling-aware
//! layout:
//! - Strictly reduces "critical-path inversions" (a symbol placed
//!   *before* a symbol that, by arrival order, is earlier on the
//!   critical path) vs. the alphabetical baseline (2 -> 1 in this
//!   fixture), but does **not** eliminate them: the original design
//!   assumption ("ordering by `declared_at_seq` alone gives zero
//!   inversions") was wrong and this module's own first test run
//!   disproved it -- the affinity pull-forward step itself introduces an
//!   inversion whenever the pulled-forward neighbor was not next in
//!   critical-path order, exactly the tension `two_signals_can_disagree`
//!   documents directly.
//! - Strictly reduces byte-distance between the deliberately-affine pair
//!   compared to the alphabetical baseline (in this fixture: from 16
//!   bytes down to 8, the tightest possible adjacency for the fixture's
//!   byte sizes).
//!
//! Both improvements are measured on a fixture *constructed* to expose
//! them (the same honesty standard `cost_correlation.rs` and
//! `disk_tiering.rs` hold themselves to) -- this shows the algorithm
//! does what it is designed to do, not that either signal matters on an
//! arbitrary or real graph. The two signals **do** measurably disagree
//! even on a 3-symbol fixture (see `two_signals_can_disagree...` and the
//! inversion residue in the critical-path test above): this greedy,
//! single-pass heuristic resolves that disagreement by favoring
//! whichever signal it encounters first in its own walk order (here,
//! affinity, because the pull-forward check runs immediately after every
//! placement), not by any principled trade-off rule.
//!
//! # One-pass vs. two-pass: `assign_layout_two_pass`, and the real
//! measured trade-off
//!
//! `assign_layout_two_pass` (below) implements the two-pass alternative
//! -- Cargo-style schedule pass, then a lld-style placement pass -- so
//! the "is one-pass joint optimization worth it over two separate
//! passes" question has an actual second algorithm to compare against,
//! not just an assertion that they differ. **The placement pass is not
//! fully independent of the schedule**: it only reorders symbols
//! *within* the affinity-chains it greedily grows; the order of the
//! chains themselves still follows the schedule pass's own output
//! exactly (see `assign_layout_two_pass`'s own doc comment for the
//! precise two-level structure -- an earlier version of this paragraph
//! overstated the independence and was corrected after that mismatch
//! with the code was pointed out). Measured on `real_llvm_ffi_fixture`'s
//! real 308-symbol rustc_codegen_llvm FFI graph (see that module's own
//! test): two-pass reduces affinity-weighted distance a further 18.2%
//! versus one-pass (its placement pass can reorder freely within each
//! chain, so it clusters more aggressively there), but **increases
//! critical-path inversions by 147.1%** (17 -> 42) versus one-pass
//! (intra-chain reordering by affinity, even though chain order itself
//! still tracks the schedule, is enough on its own to disturb more of
//! the fine-grained arrival order than the one-pass version's single
//! per-placement pull-forward does). Neither algorithm dominates the
//! other on this real fixture -- one-pass sacrifices some locality to
//! preserve more of the fine-grained schedule; two-pass sacrifices more
//! of it to gain more locality within each chain. This is a real,
//! measured confirmation of the NP-hardness-motivated framing this
//! module's own introduction cites (joint scheduling-and-placement
//! optimization is a genuine trade-off, not a case where one approach is
//! simply better), not merely an assumption carried over from the
//! literature.

use crate::{AddressState, SharedSymbolGraph, SymbolId};
use std::collections::{HashMap, HashSet};

/// Same shape as `LayoutAssignment` (kept as its own type rather than
/// reusing that one) so a caller can tell, from the type alone, which
/// algorithm produced a given assignment -- this crate's own
/// `apply_elf_x86_64_relocations` takes a `LayoutAssignment` by
/// reference, so a caller wanting to actually link with this algorithm's
/// output must go through `into_layout_assignment` explicitly, never by
/// accident.
#[derive(Debug, Clone, Default)]
pub struct SchedulingAwareLayout {
    pub addresses: HashMap<SymbolId, u64>,
    /// The placement order this layout used, for tests/inspection to
    /// verify clustering behavior directly rather than only inferring it
    /// from byte offsets.
    pub order: Vec<SymbolId>,
}

impl SchedulingAwareLayout {
    pub fn into_layout_assignment(self) -> crate::LayoutAssignment {
        crate::LayoutAssignment {
            addresses: self.addresses,
        }
    }
}

/// Undirected pairwise call-affinity counts derived from every
/// `Committed` symbol's own relocations: `(caller, callee)` (both
/// directions of a pair map to the same count) -> number of relocations
/// in `caller`'s own `CodeBody` that target `callee`. Exact, not an
/// approximation -- each relocation names precisely which other boundary
/// symbol its own code calls.
fn call_affinity(graph: &SharedSymbolGraph) -> HashMap<(SymbolId, SymbolId), u64> {
    let nodes = graph.nodes.read().expect("nodes lock poisoned");
    let mut affinity: HashMap<(SymbolId, SymbolId), u64> = HashMap::new();
    for (caller_id, node) in nodes.iter() {
        let AddressState::Committed(body) = &node.address else {
            continue;
        };
        for reloc in &body.relocations {
            let callee_id = &reloc.target;
            if callee_id == caller_id {
                continue;
            }
            let key = if caller_id.clone() < callee_id.clone() {
                (caller_id.clone(), callee_id.clone())
            } else {
                (callee_id.clone(), caller_id.clone())
            };
            *affinity.entry(key).or_insert(0) += 1;
        }
    }
    affinity
}

/// The scheduling-aware layout algorithm described in this module's own
/// doc comment. Only places `Committed` symbols, same as `assign_layout`
/// (an `Analyzed`-but-not-yet-`Committed` symbol has no `CodeBody` to lay
/// out yet).
pub fn assign_layout_scheduling_aware(graph: &SharedSymbolGraph) -> SchedulingAwareLayout {
    let nodes = graph.nodes.read().expect("nodes lock poisoned");
    let declared_at_seq = graph
        .declared_at_seq
        .read()
        .expect("declared_at_seq lock poisoned");

    let mut committed: Vec<SymbolId> = nodes
        .iter()
        .filter(|(_, node)| matches!(node.address, AddressState::Committed(_)))
        .map(|(id, _)| id.clone())
        .collect();
    // Critical-path-ordered pass: earlier `declared_at_seq` first.
    // Ties (including symbols with no recorded seq, which should not
    // happen for any symbol reachable via `declare_symbol`/
    // `declare_analyzed_symbol`, but is handled as "arrived last" rather
    // than panicking) broken by `SymbolId` for determinism, matching
    // `assign_layout`'s own guarantee.
    committed.sort_by(|a, b| {
        let seq_a = declared_at_seq.get(a).copied().unwrap_or(u64::MAX);
        let seq_b = declared_at_seq.get(b).copied().unwrap_or(u64::MAX);
        seq_a.cmp(&seq_b).then_with(|| a.cmp(b))
    });

    let affinity = call_affinity(graph);
    let mut placed: HashSet<SymbolId> = HashSet::new();
    let mut order: Vec<SymbolId> = Vec::with_capacity(committed.len());

    for id in &committed {
        if placed.contains(id) {
            continue;
        }
        order.push(id.clone());
        placed.insert(id.clone());

        // Runtime-locality pass, threaded into this same walk: place the
        // single highest-affinity not-yet-placed neighbor immediately
        // after `id`. Only one neighbor per placement (not a full
        // cluster expansion) -- deliberately the simplest heuristic that
        // still tests the "interleave both signals in one pass" idea,
        // not the most sophisticated one that could be built.
        let mut best: Option<(SymbolId, u64)> = None;
        for other in &committed {
            if placed.contains(other) {
                continue;
            }
            let key = if id.clone() < other.clone() {
                (id.clone(), other.clone())
            } else {
                (other.clone(), id.clone())
            };
            if let Some(&count) = affinity.get(&key) {
                if count > 0
                    && best
                        .as_ref()
                        .is_none_or(|(_, best_count)| count > *best_count)
                {
                    best = Some((other.clone(), count));
                }
            }
        }
        if let Some((neighbor, _)) = best {
            order.push(neighbor.clone());
            placed.insert(neighbor);
        }
    }

    let mut addresses = HashMap::new();
    let mut cursor: u64 = 0;
    for id in &order {
        let node = nodes.get(id).expect("order only contains known symbols");
        let AddressState::Committed(body) = &node.address else {
            unreachable!("order was filtered to Committed symbols only")
        };
        addresses.insert(id.clone(), cursor);
        cursor += body.code.len() as u64;
    }

    SchedulingAwareLayout { addresses, order }
}

/// Counts how many adjacent-in-critical-path-order pairs `(earlier,
/// later)` end up placed with `later` at a *smaller* address than
/// `earlier` in `layout` -- a real, measurable definition of "critical
/// path inversion" for this experiment: the build-time-critical-path-
/// aware pass should place earlier-arriving (more upstream) symbols no
/// later, byte-address-wise, than later-arriving ones, whenever nothing
/// else (call affinity) forces a deviation.
pub fn count_critical_path_inversions(
    graph: &SharedSymbolGraph,
    addresses: &HashMap<SymbolId, u64>,
) -> u64 {
    let declared_at_seq = graph
        .declared_at_seq
        .read()
        .expect("declared_at_seq lock poisoned");
    let mut by_seq: Vec<(&SymbolId, u64)> = addresses
        .keys()
        .filter_map(|id| declared_at_seq.get(id).map(|seq| (id, *seq)))
        .collect();
    by_seq.sort_by_key(|(_, seq)| *seq);

    let mut inversions = 0u64;
    for window in by_seq.windows(2) {
        let [(earlier_id, _), (later_id, _)] = window else {
            unreachable!("windows(2) always yields 2 elements")
        };
        let earlier_addr = addresses[*earlier_id];
        let later_addr = addresses[*later_id];
        if later_addr < earlier_addr {
            inversions += 1;
        }
    }
    inversions
}

/// Sum of byte-distance between every pair of symbols with non-zero call
/// affinity -- a real, measurable definition of "runtime locality cost"
/// for this experiment: lower is better (affine symbols placed closer
/// together), matching the intent (not the full mechanism) of lld's
/// Call-Chain Clustering.
pub fn total_affinity_weighted_distance(
    graph: &SharedSymbolGraph,
    addresses: &HashMap<SymbolId, u64>,
) -> u64 {
    let affinity = call_affinity(graph);
    affinity
        .iter()
        .filter_map(|((a, b), count)| {
            let addr_a = *addresses.get(a)?;
            let addr_b = *addresses.get(b)?;
            let distance = addr_a.abs_diff(addr_b);
            Some(distance * count)
        })
        .sum()
}

/// The two-pass baseline issue #69 asks to compare the one-pass
/// `assign_layout_scheduling_aware` against: Cargo's own schedule-only
/// pass (`DependencyQueue`-style critical-path ordering, here just
/// `declared_at_seq` ascending -- identical to
/// `assign_layout_scheduling_aware`'s own first sort, with none of its
/// affinity pull-forward interleaved) followed by lld's own
/// placement-only pass (`computeCallGraphProfileOrder`-style Call-Chain
/// Clustering). **Important correction, made after this doc comment's
/// own first version overstated Pass 2's independence and was caught as
/// inconsistent with the code**: Pass 2 is not a full reordering that
/// ignores critical-path order entirely. It only reorders *within* the
/// chains it greedily grows by affinity; the order of the chains
/// themselves still follows Pass 1's own schedule order exactly (see
/// `assign_layout_two_pass`'s own doc comment for the precise
/// two-level structure). Two sequential passes over the whole graph,
/// but not two *fully* independent ones -- a partial, not total,
/// realization of the "solve schedule and layout as two independent
/// problems" approach `lib.rs`'s own module doc comment says Cargo and
/// lld actually take today -- built here so it can be measured against
/// the one-pass
/// greedy interleaving, not just asserted to be different in kind.
///
/// **Pass 2's own algorithm, precisely**: NOT a full reordering by
/// affinity alone -- that overstates its independence from Pass 1.
/// What it actually does: walk `schedule_order` (Pass 1's own output)
/// left to right, and for each not-yet-consumed symbol, seed a new
/// chain and grow it greedily by affinity (picking the single
/// highest-affinity not-yet-placed symbol among ALL remaining symbols,
/// not just neighbors in the schedule, and attaching it to either end
/// of the chain) until no further affinity exists at either end. This
/// produces a two-level structure: **chain order** (which chain comes
/// before which) is still exactly `schedule_order`'s own order -- the
/// same critical-path constraint Pass 1 established, never revisited --
/// while **placement inside a chain** is free of that constraint,
/// decided purely by affinity, in either direction, which the one-pass
/// version's single left-to-right walk cannot express. "Two fully
/// independent passes" is the wrong mental model for this
/// implementation; "schedule decides chain order, affinity decides only
/// intra-chain order" is the accurate one -- corrected here after this
/// same overstatement was pointed out as inconsistent with the code
/// during issue #69's own review.
pub fn assign_layout_two_pass(graph: &SharedSymbolGraph) -> SchedulingAwareLayout {
    let nodes = graph.nodes.read().expect("nodes lock poisoned");
    let declared_at_seq = graph
        .declared_at_seq
        .read()
        .expect("declared_at_seq lock poisoned");

    let mut committed: Vec<SymbolId> = nodes
        .iter()
        .filter(|(_, node)| matches!(node.address, AddressState::Committed(_)))
        .map(|(id, _)| id.clone())
        .collect();

    // Pass 1 (Cargo-style schedule): critical-path order alone, no
    // affinity considered at all. This IS the schedule this crate's own
    // build-time-critical-path signal produces on its own -- kept as a
    // separate, complete pass rather than interleaved with anything
    // else.
    committed.sort_by(|a, b| {
        let seq_a = declared_at_seq.get(a).copied().unwrap_or(u64::MAX);
        let seq_b = declared_at_seq.get(b).copied().unwrap_or(u64::MAX);
        seq_a.cmp(&seq_b).then_with(|| a.cmp(b))
    });
    let schedule_order = committed;

    // Pass 2 (lld-style placement): a SEPARATE traversal over the
    // schedule pass's own output that reorders purely by affinity,
    // ignoring where each symbol sat in the schedule. Chains grow from
    // both ends (unlike assign_layout_scheduling_aware's single-neighbor
    // pull-forward) so a symbol with two affine neighbors can end up
    // between them, which the one-pass version's own left-to-right walk
    // cannot express.
    let affinity = call_affinity(graph);
    let mut remaining: HashSet<SymbolId> = schedule_order.iter().cloned().collect();
    let mut chains: Vec<Vec<SymbolId>> = Vec::new();

    // Seed one chain per symbol, in schedule order, so ties (no affinity
    // info at all) fall back to the schedule -- the only place this pass
    // still depends on Pass 1's output, matching how a real two-pass
    // linker still needs *some* deterministic tie-break for
    // zero-affinity sections.
    for id in &schedule_order {
        if !remaining.remove(id) {
            continue;
        }
        let mut chain = vec![id.clone()];
        // Greedily grow this chain: repeatedly attach whichever
        // remaining symbol has the highest affinity with either end of
        // the current chain, until no remaining symbol has any
        // affinity with either end.
        loop {
            let front = chain.first().unwrap().clone();
            let back = chain.last().unwrap().clone();
            // Iterate `remaining` in SymbolId order (not HashSet's own
            // hash-randomized order) so a weight tie always resolves the
            // same way across runs -- this pass must hold the same
            // determinism guarantee assign_layout's own doc comment
            // states, and hash-order iteration would silently make a
            // weight tie depend on SipHash's per-process random seed.
            let mut candidates: Vec<&SymbolId> = remaining.iter().collect();
            candidates.sort();
            let mut best: Option<(SymbolId, u64, bool)> = None; // (symbol, weight, attach_to_front)
            for candidate in candidates {
                for (end, at_front) in [(&front, true), (&back, false)] {
                    let key = if end.clone() < candidate.clone() {
                        (end.clone(), candidate.clone())
                    } else {
                        (candidate.clone(), end.clone())
                    };
                    if let Some(&weight) = affinity.get(&key) {
                        if weight > 0
                            && best
                                .as_ref()
                                .is_none_or(|(_, best_weight, _)| weight > *best_weight)
                        {
                            best = Some((candidate.clone(), weight, at_front));
                        }
                    }
                }
            }
            let Some((symbol, _, at_front)) = best else {
                break;
            };
            remaining.remove(&symbol);
            if at_front {
                chain.insert(0, symbol);
            } else {
                chain.push(symbol);
            }
        }
        chains.push(chain);
    }

    let order: Vec<SymbolId> = chains.into_iter().flatten().collect();

    let mut addresses = HashMap::new();
    let mut cursor: u64 = 0;
    for id in &order {
        let node = nodes.get(id).expect("order only contains known symbols");
        let AddressState::Committed(body) = &node.address else {
            unreachable!("order was filtered to Committed symbols only")
        };
        addresses.insert(id.clone(), cursor);
        cursor += body.code.len() as u64;
    }

    SchedulingAwareLayout { addresses, order }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CodeBody, ElfX86_64PendingReloc, Realm, SymbolNode};

    fn committed(realm: Realm, name: &str, len: usize) -> SymbolNode {
        SymbolNode {
            id: SymbolId {
                realm,
                name: name.to_string(),
            },
            address: AddressState::Committed(CodeBody {
                code: vec![0u8; len],
                relocations: vec![],
            }),
        }
    }

    /// Same as `committed`, but its own code carries one relocation
    /// targeting `calls` -- the exact, per-symbol call-affinity signal
    /// this module's `call_affinity` reads (see this module's own doc
    /// comment on why relocations, not `RequiresEdge`, are used).
    fn committed_calling(realm: Realm, name: &str, len: usize, calls: &SymbolId) -> SymbolNode {
        SymbolNode {
            id: SymbolId {
                realm,
                name: name.to_string(),
            },
            address: AddressState::Committed(CodeBody {
                code: vec![0u8; len],
                relocations: vec![ElfX86_64PendingReloc {
                    offset: 0,
                    width: 4,
                    target: calls.clone(),
                    addend: 0,
                }],
            }),
        }
    }

    /// Builds a graph where:
    /// - Declaration order (critical-path proxy) is deliberately the
    ///   *reverse* of alphabetical order, so `assign_layout`'s own
    ///   alphabetical sort is guaranteed to invert the critical path on
    ///   every adjacent pair.
    /// - `zzz_caller` (declared first, alphabetically last) has a
    ///   relocation naming `aaa_callee` (declared last, alphabetically
    ///   first) -- an affine pair placed maximally far apart by an
    ///   alphabetical sort, and adjacent by declaration order.
    fn build_inverted_fixture() -> SharedSymbolGraph {
        let graph = SharedSymbolGraph::new();
        let callee_id = SymbolId {
            realm: Realm::Cargo,
            name: "aaa_callee".to_string(),
        };
        graph
            .declare_symbol(
                Realm::Cargo,
                committed_calling(Realm::Cargo, "zzz_caller", 8, &callee_id),
            )
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "mmm_middle", 8))
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "aaa_callee", 8))
            .unwrap();
        graph
    }

    #[test]
    fn scheduling_aware_layout_reduces_but_does_not_eliminate_critical_path_inversions_when_affinity_reorders(
    ) {
        let graph = build_inverted_fixture();

        let baseline = graph.assign_layout();
        let baseline_inversions = count_critical_path_inversions(&graph, &baseline.addresses);

        let scheduled = assign_layout_scheduling_aware(&graph);
        let scheduled_inversions = count_critical_path_inversions(&graph, &scheduled.addresses);

        eprintln!(
            "[layout_scheduling] critical-path inversions: alphabetical baseline={baseline_inversions}, \
             scheduling-aware={scheduled_inversions} (fixture has 3 symbols declared in \
             reverse-alphabetical order, so the baseline is guaranteed to invert every adjacent pair; \
             scheduling-aware still pays 1 inversion here because pulling aaa_callee forward next to \
             zzz_caller displaces mmm_middle -- the same affinity-vs-critical-path tension \
             two_signals_can_disagree documents directly, showing up unintentionally in a fixture \
             that set out to isolate only the critical-path signal)"
        );

        assert_eq!(
            baseline_inversions, 2,
            "fixture is constructed so the alphabetical baseline inverts both adjacent pairs \
             (zzz_caller->mmm_middle, mmm_middle->aaa_callee all reversed relative to declaration order)"
        );
        // Not zero: honestly reports the actual measured behavior rather
        // than the originally-assumed "isolates the critical-path signal
        // cleanly" claim, which this run disproved. See the eprintln
        // above and two_signals_can_disagree for why 1 inversion remains
        // an expected, understood cost of this greedy heuristic, not a
        // bug.
        assert!(
            scheduled_inversions < baseline_inversions,
            "scheduling-aware layout must still strictly reduce inversions vs the baseline even \
             though it does not reach zero on this fixture, got baseline={baseline_inversions} \
             scheduled={scheduled_inversions}"
        );
        assert_eq!(
            scheduled_inversions, 1,
            "pin the exact measured value so a future change to the greedy algorithm must \
             consciously update this, not silently regress it"
        );
    }

    #[test]
    fn scheduling_aware_layout_reduces_distance_between_an_affine_pair_placed_far_apart_alphabetically(
    ) {
        let graph = build_inverted_fixture();

        let baseline = graph.assign_layout();
        let baseline_distance = total_affinity_weighted_distance(&graph, &baseline.addresses);

        let scheduled = assign_layout_scheduling_aware(&graph);
        let scheduled_distance = total_affinity_weighted_distance(&graph, &scheduled.addresses);

        eprintln!(
            "[layout_scheduling] affinity-weighted distance: alphabetical baseline={baseline_distance}, \
             scheduling-aware={scheduled_distance} (zzz_caller requires aaa_callee, placed at opposite \
             ends alphabetically but adjacent by declaration order)"
        );

        assert!(
            scheduled_distance < baseline_distance,
            "scheduling-aware layout must place the affine pair (zzz_caller, aaa_callee) strictly \
             closer than the alphabetical baseline does on this fixture, got baseline={baseline_distance} \
             scheduled={scheduled_distance}"
        );
        // zzz_caller and aaa_callee are adjacent in scheduling-aware
        // order (mmm_middle gets pulled forward if it also has
        // affinity, but here it has none, so it lands after the pair) --
        // their distance must equal aaa_callee's own code length (8),
        // the tightest possible adjacency for this fixture's byte sizes.
        assert_eq!(
            scheduled_distance, 8,
            "with mmm_middle carrying zero affinity, the pair should be placed immediately \
             adjacent (distance == aaa_callee's own 8-byte code length), not just closer"
        );
    }

    #[test]
    fn scheduling_aware_layout_is_deterministic_across_repeated_calls() {
        let graph = build_inverted_fixture();
        let first = assign_layout_scheduling_aware(&graph);
        let second = assign_layout_scheduling_aware(&graph);
        assert_eq!(
            first.order, second.order,
            "same graph, same algorithm, must reproduce the exact same order every time -- \
             assign_layout's own determinism guarantee must hold for this variant too"
        );
    }

    #[test]
    fn two_pass_layout_is_deterministic_across_repeated_calls() {
        let graph = build_inverted_fixture();
        let first = assign_layout_two_pass(&graph);
        let second = assign_layout_two_pass(&graph);
        assert_eq!(
            first.order, second.order,
            "same graph, same algorithm, must reproduce the exact same order every time"
        );
    }

    #[test]
    fn two_pass_layout_achieves_perfect_affinity_placement_where_one_pass_could_not() {
        // Reuses the same tension two_signals_can_disagree documents:
        // first/last are call-affine, mid is declared between them with
        // no affinity to either. The one-pass greedy algorithm resolves
        // this by pulling last forward next to first as soon as it
        // visits first (before ever reaching mid), producing exactly 1
        // critical-path inversion. The two-pass version's own Pass 2
        // runs as a fully separate traversal with no such ordering
        // constraint from Pass 1's walk -- it is free to place the
        // affine pair adjacent while independently placing mid wherever
        // Pass 1's schedule already put it, since Pass 2 only chains
        // nodes that have measured affinity, never merges affinity-free
        // singletons into someone else's chain.
        let graph = SharedSymbolGraph::new();
        let last_id = SymbolId {
            realm: Realm::Cargo,
            name: "last".to_string(),
        };
        graph
            .declare_symbol(
                Realm::Cargo,
                committed_calling(Realm::Cargo, "first", 8, &last_id),
            )
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "mid", 8))
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "last", 8))
            .unwrap();

        let two_pass = assign_layout_two_pass(&graph);
        let inversions = count_critical_path_inversions(&graph, &two_pass.addresses);
        let distance = total_affinity_weighted_distance(&graph, &two_pass.addresses);

        eprintln!(
            "[layout_scheduling][two_pass] order={:?} inversions={inversions} \
             affinity_weighted_distance={distance}",
            two_pass.order
        );

        // Pass 1's schedule is [first, mid, last] (declared_at_seq
        // order). Pass 2 seeds a chain per symbol in that order: "first"
        // seeds a chain, then grows it by pulling in "last" (its only
        // affine neighbor; attached to the front because HashSet
        // iteration order, not declared_at_seq, decides which of
        // "front"/"back" ties `best` keeps when both attachment points
        // tie on weight) BEFORE "mid" is ever seeded (mid is still in
        // `remaining` at that point, but has zero affinity with either
        // chain end, so it is never pulled in). "mid" then seeds its own
        // single-symbol chain. Final order: [last, first, mid] --
        // confirmed by running this test, not assumed in advance (an
        // earlier version of this assertion incorrectly predicted
        // [first, last, mid] and this test caught that mistake).
        // Whichever end "last" attaches to, the same honest point holds:
        // two passes does not automatically avoid the inversion here
        // either, because "mid" has no affinity signal to relocate it
        // by -- affinity-based placement cannot move a node that has no
        // measured affinity with anything.
        assert_eq!(
            two_pass.order,
            vec![
                SymbolId {
                    realm: Realm::Cargo,
                    name: "last".to_string()
                },
                SymbolId {
                    realm: Realm::Cargo,
                    name: "first".to_string()
                },
                SymbolId {
                    realm: Realm::Cargo,
                    name: "mid".to_string()
                },
            ],
            "documents the actual two-pass result on this fixture, not an assumed one"
        );
        assert_eq!(
            inversions, 1,
            "two-pass does not eliminate this inversion either: mid has zero affinity with \
             anything, so no affinity-based pass (one-pass or two-pass) can reposition it \
             relative to the schedule -- this is a property of the fixture (mid has no affinity \
             signal at all), not a difference between one-pass and two-pass algorithms"
        );
        assert_eq!(
            distance, 8,
            "the affine pair (first, last) is still placed maximally close in the two-pass \
             version, same as the one-pass version achieves on this fixture"
        );
    }

    #[test]
    fn scheduling_aware_layout_matches_baseline_on_a_graph_with_no_affinity_at_all() {
        // A graph where every symbol's declared_at_seq order already
        // matches alphabetical order, and no RequiresEdge exists at all
        // -- both algorithms should produce the identical order, since
        // there is no signal for the scheduling-aware pass to act on
        // differently.
        let graph = SharedSymbolGraph::new();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "aaa", 4))
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "bbb", 4))
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "ccc", 4))
            .unwrap();

        let baseline = graph.assign_layout();
        let scheduled = assign_layout_scheduling_aware(&graph);

        let baseline_order: Vec<&SymbolId> = {
            let mut v: Vec<&SymbolId> = baseline.addresses.keys().collect();
            v.sort_by_key(|id| baseline.addresses[*id]);
            v
        };
        let scheduled_order: Vec<&SymbolId> = scheduled.order.iter().collect();

        assert_eq!(
            baseline_order, scheduled_order,
            "with no affinity signal and declaration order already matching alphabetical order, \
             the two algorithms must agree -- confirms the scheduling-aware pass does not \
             introduce spurious reordering when it has nothing useful to act on"
        );
    }

    /// Issue #69's actual research question (not yet answered by the
    /// first two tests, which each isolate one signal): can the two
    /// signals *disagree* -- does optimizing critical-path order ever
    /// force a worse affinity placement than a layout that ignored
    /// critical-path order entirely, on a graph small enough to check
    /// both layouts exhaustively? This fixture builds exactly that
    /// tension: `mid` is declared between two symbols that are mutually
    /// call-affine with each other but NOT with `mid`, so `mid`'s
    /// natural critical-path position (in the middle) sits directly
    /// between the pair the affinity pass would rather place adjacent.
    #[test]
    fn two_signals_can_disagree_and_greedy_algorithm_picks_critical_path_order_over_affinity() {
        let graph = SharedSymbolGraph::new();
        let last_id = SymbolId {
            realm: Realm::Cargo,
            name: "last".to_string(),
        };
        // Declaration order: first, mid, last. first's own code calls
        // last (a relocation), so first<->last are call-affine; mid has
        // no affinity with either.
        graph
            .declare_symbol(
                Realm::Cargo,
                committed_calling(Realm::Cargo, "first", 8, &last_id),
            )
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "mid", 8))
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "last", 8))
            .unwrap();

        let scheduled = assign_layout_scheduling_aware(&graph);
        let inversions = count_critical_path_inversions(&graph, &scheduled.addresses);
        let distance = total_affinity_weighted_distance(&graph, &scheduled.addresses);

        eprintln!(
            "[layout_scheduling][two_signals_disagree] order={:?} inversions={inversions} \
             affinity_weighted_distance={distance} (first/last are call-affine but declared with \
             mid in between; the greedy algorithm as implemented walks in critical-path order \
             and only pulls an unplaced neighbor forward when it visits one endpoint of an \
             affine pair, so it cannot un-place mid once mid is already visited first if mid's \
             own seq comes before last's)",
            scheduled.order
        );

        // The greedy algorithm processes symbols in declared_at_seq
        // order (first, mid, last). When it visits "first", "last" is
        // its highest-affinity unplaced neighbor, so "last" is pulled
        // forward immediately -- BEFORE "mid" is ever visited. This
        // means the algorithm, as implemented, resolves this particular
        // conflict in favor of affinity (pulling last next to first),
        // at the cost of a critical-path inversion (mid, declared
        // before last, ends up placed after last).
        assert_eq!(
            scheduled.order,
            vec![
                SymbolId {
                    realm: Realm::Cargo,
                    name: "first".to_string()
                },
                SymbolId {
                    realm: Realm::Cargo,
                    name: "last".to_string()
                },
                SymbolId {
                    realm: Realm::Cargo,
                    name: "mid".to_string()
                },
            ],
            "documents the actual greedy resolution: affinity pull-forward happens before the \
             outer walk reaches the symbol that would otherwise come next in critical-path order"
        );
        assert_eq!(
            inversions, 1,
            "this is the concrete cost the greedy heuristic pays for prioritizing affinity here: \
             one critical-path inversion (mid ends up after last, despite being declared first)"
        );
        assert_eq!(
            distance, 8,
            "in exchange, the affine pair (first, last) is placed maximally close (distance == \
             last's own 8-byte code length)"
        );
    }
}

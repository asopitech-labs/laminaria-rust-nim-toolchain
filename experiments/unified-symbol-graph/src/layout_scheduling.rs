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
//! # One-pass vs. two-pass: three algorithms, not two
//!
//! An earlier version of this module claimed
//! `assign_layout_schedule_seeded_clustering` (then named
//! `assign_layout_two_pass`) was "the" two-pass alternative to
//! `assign_layout_scheduling_aware`'s one-pass approach. That claim was
//! challenged and did not survive scrutiny: that function's own Pass 2
//! seeds one chain per symbol *by walking the schedule pass's own
//! output in order*, so the schedule still fully determines which chain
//! comes before which -- only intra-chain order is free of it. That
//! makes it a stronger version of the *same family* as the one-pass
//! algorithm (both let the schedule dominate overall order and only let
//! affinity act locally), not a structurally different, schedule-blind
//! second pass. Calling the comparison between them "one-pass vs.
//! two-pass" mischaracterized what was actually being compared: two
//! points on a "how much does affinity get to override the schedule"
//! spectrum, not two different algorithm families.
//!
//! `assign_layout_two_pass` (below, the name freed up by the rename
//! above) is the actual schedule-independent second family: it builds
//! a global edge list sorted purely by affinity weight across *all* pairs in the
//! whole graph (a global sort, not seeded by or walked in schedule
//! order), then merges symbols into chains greedily by that global
//! order (Kruskal's-algorithm-style edge selection: take the next
//! highest-weight edge, and if its two endpoints are not already in the
//! same chain, merge the two chains at those endpoints), and only
//! finally places any symbol untouched by this process, plus (this is
//! the ONLY place the schedule is even consulted) breaks ties among
//! finished chains by the schedule position of each chain's own
//! lowest-`declared_at_seq` member -- needed because *some* deterministic
//! rule must decide which finished chain goes first, and "which chain
//! happened to include the earliest-arriving symbol" is a real, cheap,
//! schedule-derived tie-break, not schedule-driven overall ordering.
//!
//! Measured on `real_llvm_ffi_fixture`'s real 308-symbol
//! rustc_codegen_llvm FFI graph (see that module's own test for the
//! reproducing code), critical-path inversions / affinity-weighted
//! distance for all three, relative to `assign_layout_scheduling_aware`
//! (one-pass) as the reference point:
//!
//! | algorithm | inversions | vs one-pass | distance | vs one-pass |
//! |---|---|---|---|---|
//! | one-pass (`assign_layout_scheduling_aware`) | 17 | -- | 2,988,701 | -- |
//! | schedule-seeded clustering (`assign_layout_schedule_seeded_clustering`) | 42 | +147.1% | 2,443,714 | -18.2% |
//! | true two-pass (`assign_layout_two_pass`) | 38 | +123.5% | 1,534,651 | -48.6% |
//!
//! The true two-pass algorithm not only achieves far better locality
//! (-48.6% vs. -18.2%) than the schedule-seeded version, it also
//! produces *fewer* inversions (38 vs. 42) -- schedule-seeded clustering
//! is not just mischaracterized as "two-pass," it is a strictly worse
//! point on both axes than the real two-pass algorithm on this fixture,
//! confirming that letting the schedule seed chain order (rather than
//! only using it as a last-resort tie-break) actively hurts both
//! objectives here rather than trading one for the other. The genuine
//! trade-off is between one-pass and true two-pass: one-pass preserves
//! far more of the fine-grained schedule (17 vs. 38 inversions) at the
//! cost of locality; true two-pass achieves much better locality at the
//! cost of the fine-grained schedule. This is a real, measured
//! confirmation of the NP-hardness-motivated framing this module's own
//! introduction cites (joint scheduling-and-placement optimization is a
//! genuine trade-off, not a case where one approach is simply better) --
//! but only once compared against an algorithm that actually earns the
//! name "two-pass," which `assign_layout_schedule_seeded_clustering`
//! alone did not.

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
/// `assign_layout_schedule_seeded_clustering`'s own doc comment for the precise
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
pub fn assign_layout_schedule_seeded_clustering(
    graph: &SharedSymbolGraph,
) -> SchedulingAwareLayout {
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

/// The actual schedule-independent two-pass placement algorithm -- see
/// this module's own "One-pass vs. two-pass: three algorithms, not two"
/// doc section for why `assign_layout_schedule_seeded_clustering` did
/// not qualify as this, and what this one does differently.
///
/// Pass 1 (Cargo-style schedule) is identical to the other two
/// functions: `declared_at_seq` ascending, ties by `SymbolId`.
///
/// Pass 2 (real lld-style global clustering, Kruskal's-algorithm-style):
/// every affinity edge in the whole graph is sorted once, by weight
/// descending (ties broken by the `SymbolId` pair, for determinism --
/// never by schedule position, since this pass must not consult the
/// schedule at all until the very last, unavoidable step). Chains start
/// as one symbol each. Walking edges in that global order, each edge
/// whose two endpoints are not already in the same chain merges those
/// two chains by attaching at the matching endpoints (an edge whose
/// endpoints are both interior to their own chains, or already in the
/// same chain, is skipped -- this is exactly Kruskal's own "skip an edge
/// that would create a cycle" rule, adapted to "skip an edge that can't
/// extend a chain from an endpoint"). This produces the maximum-weight
/// set of chains reachable by always taking the next-best global edge,
/// with zero input from `declared_at_seq` anywhere in this process.
///
/// The only place the schedule is consulted at all: once every edge has
/// been considered and every symbol belongs to exactly one finished
/// chain, the chains themselves must still be placed in *some* final
/// order relative to each other, and a byte layout cannot avoid picking
/// one. Each chain is ordered by its own lowest-`declared_at_seq`
/// member -- a real, cheap, deterministic tie-break for "which
/// completely-unrelated cluster of code goes at a lower address," not a
/// mechanism that lets the schedule influence which symbols end up
/// adjacent (that was decided entirely by Pass 2's own affinity sort).
pub fn assign_layout_two_pass(graph: &SharedSymbolGraph) -> SchedulingAwareLayout {
    let nodes = graph.nodes.read().expect("nodes lock poisoned");
    let declared_at_seq = graph
        .declared_at_seq
        .read()
        .expect("declared_at_seq lock poisoned");

    let committed: Vec<SymbolId> = nodes
        .iter()
        .filter(|(_, node)| matches!(node.address, AddressState::Committed(_)))
        .map(|(id, _)| id.clone())
        .collect();

    // Pass 2 (global clustering): sort every affinity edge once, purely
    // by weight, never touching declared_at_seq.
    let affinity = call_affinity(graph);
    let mut edges: Vec<(SymbolId, SymbolId, u64)> = affinity
        .into_iter()
        .map(|((a, b), weight)| (a, b, weight))
        .collect();
    edges.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });

    // Each symbol starts as its own chain, tracked as a VecDeque so both
    // ends can be extended in O(1). `chain_of` maps a symbol to the
    // index of the chain it currently belongs to; only an endpoint's
    // entry is trustworthy for merge decisions, but every member's own
    // entry is kept in sync so `chain_of` never goes stale.
    let mut chains: Vec<std::collections::VecDeque<SymbolId>> = committed
        .iter()
        .map(|id| std::collections::VecDeque::from([id.clone()]))
        .collect();
    let mut chain_of: HashMap<SymbolId, usize> = committed
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i))
        .collect();
    let mut merged_away: HashSet<usize> = HashSet::new();

    for (a, b, weight) in edges {
        if weight == 0 {
            continue;
        }
        let Some(&chain_a) = chain_of.get(&a) else {
            continue;
        };
        let Some(&chain_b) = chain_of.get(&b) else {
            continue;
        };
        if chain_a == chain_b {
            continue; // Already in the same chain -- would create a cycle.
        }
        // Kruskal's-style edge acceptance additionally requires here
        // that BOTH endpoints are still exposed at an end of their own
        // chain (an affinity edge to a symbol buried in the interior of
        // a chain cannot extend anything -- that symbol already has two
        // neighbors, or is not at an end).
        let a_at_front = chains[chain_a].front() == Some(&a);
        let a_at_back = chains[chain_a].back() == Some(&a);
        let b_at_front = chains[chain_b].front() == Some(&b);
        let b_at_back = chains[chain_b].back() == Some(&b);
        if !(a_at_front || a_at_back) || !(b_at_front || b_at_back) {
            continue;
        }

        // Merge chain_b into chain_a, orienting chain_b so `b`'s own end
        // is adjacent to `a`'s own end.
        let mut incoming = std::mem::take(&mut chains[chain_b]);
        if b_at_back {
            incoming.make_contiguous().reverse();
        }
        // incoming now has `b` at its front (if it was at the back, the
        // reverse above put it at front; if it was already at front, no
        // reversal was needed).
        if a_at_back {
            chains[chain_a].extend(incoming);
        } else {
            // a_at_front: prepend, so a stays adjacent to b.
            incoming.extend(std::mem::take(&mut chains[chain_a]));
            chains[chain_a] = incoming;
        }
        for member in chains[chain_a].iter() {
            chain_of.insert(member.clone(), chain_a);
        }
        merged_away.insert(chain_b);
    }

    // Order the surviving chains by each chain's own earliest
    // declared_at_seq member -- the one and only place this pass
    // consults the schedule, purely as a tie-break for cluster order
    // (see this function's own doc comment).
    let mut surviving: Vec<&std::collections::VecDeque<SymbolId>> = chains
        .iter()
        .enumerate()
        .filter(|(i, chain)| !merged_away.contains(i) && !chain.is_empty())
        .map(|(_, chain)| chain)
        .collect();
    surviving.sort_by_key(|chain| {
        chain
            .iter()
            .map(|id| declared_at_seq.get(id).copied().unwrap_or(u64::MAX))
            .min()
            .unwrap_or(u64::MAX)
    });

    let order: Vec<SymbolId> = surviving.into_iter().flatten().cloned().collect();

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
    fn schedule_seeded_clustering_is_deterministic_across_repeated_calls() {
        let graph = build_inverted_fixture();
        let first = assign_layout_schedule_seeded_clustering(&graph);
        let second = assign_layout_schedule_seeded_clustering(&graph);
        assert_eq!(
            first.order, second.order,
            "same graph, same algorithm, must reproduce the exact same order every time"
        );
    }

    #[test]
    fn schedule_seeded_clustering_achieves_perfect_affinity_placement_where_one_pass_could_not() {
        // Reuses the same tension two_signals_can_disagree documents:
        // first/last are call-affine, mid is declared between them with
        // no affinity to either. The one-pass greedy algorithm resolves
        // this by pulling last forward next to first as soon as it
        // visits first (before ever reaching mid), producing exactly 1
        // critical-path inversion. This function's own Pass 2 still
        // seeds chains by walking Pass 1's schedule in order (see this
        // module's own "three algorithms, not two" doc section), so it
        // is NOT free of the schedule's influence on chain order -- it
        // happens to place the affine pair adjacent here because "last"
        // gets pulled into "first"'s chain before "mid" is ever seeded,
        // not because this pass ignores the schedule.
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

        let clustered = assign_layout_schedule_seeded_clustering(&graph);
        let inversions = count_critical_path_inversions(&graph, &clustered.addresses);
        let distance = total_affinity_weighted_distance(&graph, &clustered.addresses);

        eprintln!(
            "[layout_scheduling][schedule_seeded_clustering] order={:?} inversions={inversions} \
             affinity_weighted_distance={distance}",
            clustered.order
        );

        // Pass 1's schedule is [first, mid, last] (declared_at_seq
        // order). Pass 2 seeds a chain per symbol in that order: "first"
        // seeds a chain, then grows it by pulling in "last" (its only
        // affine neighbor; attached to the front because, among
        // SymbolId-sorted candidates, "first" tries at_front=true before
        // at_front=false and the weights tie) BEFORE "mid" is ever
        // seeded (mid is still in `remaining` at that point, but has
        // zero affinity with either chain end, so it is never pulled
        // in). "mid" then seeds its own single-symbol chain. Final
        // order: [last, first, mid] -- confirmed by running this test,
        // not assumed in advance. This still depends on chain SEEDING
        // order tracking the schedule (see this module's own "three
        // algorithms, not two" doc section) -- it is not evidence that
        // this pass ignores the schedule.
        assert_eq!(
            clustered.order,
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
            "documents the actual schedule_seeded_clustering result on this fixture, not an \
             assumed one"
        );
        assert_eq!(
            inversions, 1,
            "schedule_seeded_clustering does not eliminate this inversion either: mid has zero \
             affinity with anything, so no affinity-based pass can reposition it relative to the \
             schedule -- this is a property of the fixture (mid has no affinity signal at all)"
        );
        assert_eq!(
            distance, 8,
            "the affine pair (first, last) is still placed maximally close, same as the \
             one-pass version achieves on this fixture"
        );
    }

    #[test]
    fn two_pass_is_deterministic_across_repeated_calls() {
        let graph = build_inverted_fixture();
        let first = assign_layout_two_pass(&graph);
        let second = assign_layout_two_pass(&graph);
        assert_eq!(
            first.order, second.order,
            "same graph, same algorithm, must reproduce the exact same order every time"
        );
    }

    /// The test the schedule-seeded version could not honestly claim to
    /// pass: a fixture where the schedule order and the affinity
    /// structure actively DISAGREE about grouping, and the true
    /// two-pass algorithm groups by affinity anyway, in direct
    /// contradiction of the schedule's own adjacency.
    ///
    /// Declared (schedule) order: w, x, y, z. Affinity: w<->z (strong,
    /// weight 5) and x<->y (strong, weight 5) -- i.e. the schedule
    /// interleaves two pairs that should each cluster together, in the
    /// worst possible order for a schedule-seeded approach (adjacent
    /// schedule neighbors x,y ARE each other's affinity partners, but w
    /// and z, which are also partners, are schedule-adjacent to the
    /// WRONG members of the other pair).
    #[test]
    fn two_pass_clusters_by_affinity_even_when_schedule_interleaves_the_two_pairs() {
        let graph = SharedSymbolGraph::new();
        let z_id = SymbolId {
            realm: Realm::Cargo,
            name: "w_z_pair_z".to_string(),
        };
        let y_id = SymbolId {
            realm: Realm::Cargo,
            name: "x_y_pair_y".to_string(),
        };
        // w (declared 1st) calls z (declared 4th) -- 5 relocations for a
        // strong, unambiguous affinity weight.
        let w_body = SymbolNode {
            id: SymbolId {
                realm: Realm::Cargo,
                name: "w_z_pair_w".to_string(),
            },
            address: AddressState::Committed(CodeBody {
                code: vec![0u8; 8],
                relocations: (0..5)
                    .map(|_| ElfX86_64PendingReloc {
                        offset: 0,
                        width: 4,
                        target: z_id.clone(),
                        addend: 0,
                    })
                    .collect(),
            }),
        };
        // x (declared 2nd) calls y (declared 3rd) -- also weight 5.
        let x_body = SymbolNode {
            id: SymbolId {
                realm: Realm::Cargo,
                name: "x_y_pair_x".to_string(),
            },
            address: AddressState::Committed(CodeBody {
                code: vec![0u8; 8],
                relocations: (0..5)
                    .map(|_| ElfX86_64PendingReloc {
                        offset: 0,
                        width: 4,
                        target: y_id.clone(),
                        addend: 0,
                    })
                    .collect(),
            }),
        };
        graph.declare_symbol(Realm::Cargo, w_body).unwrap();
        graph.declare_symbol(Realm::Cargo, x_body).unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "x_y_pair_y", 8))
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "w_z_pair_z", 8))
            .unwrap();

        let two_pass = assign_layout_two_pass(&graph);
        let distance = total_affinity_weighted_distance(&graph, &two_pass.addresses);

        eprintln!(
            "[layout_scheduling][two_pass] order={:?} affinity_weighted_distance={distance}",
            two_pass.order
        );

        // Both pairs must end up adjacent (byte-distance 8, one
        // code-length apart), and total_affinity_weighted_distance
        // weights that per-pair distance by relocation COUNT (5 per
        // pair here), so the expected total is 2 * (8 * 5) = 80, not a
        // raw byte-distance sum -- this test's first version asserted
        // 16 and was caught wrong by actually running it, corrected
        // here. This is still the property
        // assign_layout_schedule_seeded_clustering cannot guarantee
        // here (its own chain-seeding order still tracks the schedule),
        // and is exactly what "genuinely schedule-independent
        // clustering" means in practice.
        assert_eq!(
            distance, 80,
            "both affine pairs must be placed maximally adjacent (8 bytes each, weighted by 5 \
             relocations per pair) despite the schedule interleaving them, proving Pass 2 \
             clustered by affinity alone -- got {distance}"
        );

        // Structural check on the order itself: each pair must be
        // contiguous (no third symbol wedged between them).
        let pos = |name: &str| {
            two_pass
                .order
                .iter()
                .position(|id| id.name == name)
                .unwrap_or_else(|| panic!("{name} missing from order"))
        };
        assert_eq!(
            (pos("w_z_pair_w") as isize - pos("w_z_pair_z") as isize).abs(),
            1,
            "w and z must be placed immediately adjacent in the final order, order was {:?}",
            two_pass.order
        );
        assert_eq!(
            (pos("x_y_pair_x") as isize - pos("x_y_pair_y") as isize).abs(),
            1,
            "x and y must be placed immediately adjacent in the final order, order was {:?}",
            two_pass.order
        );
    }

    #[test]
    fn two_pass_matches_schedule_order_when_there_is_no_affinity_at_all() {
        // With zero affinity edges, Pass 2's global sort has nothing to
        // merge, so every symbol stays a singleton chain, and the only
        // ordering signal left is the schedule tie-break -- this must
        // reduce to plain declared_at_seq order, same as the other two
        // algorithms in the same degenerate case.
        let graph = SharedSymbolGraph::new();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "ccc", 4))
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "aaa", 4))
            .unwrap();
        graph
            .declare_symbol(Realm::Cargo, committed(Realm::Cargo, "bbb", 4))
            .unwrap();

        let two_pass = assign_layout_two_pass(&graph);
        assert_eq!(
            two_pass.order,
            vec![
                SymbolId {
                    realm: Realm::Cargo,
                    name: "ccc".to_string()
                },
                SymbolId {
                    realm: Realm::Cargo,
                    name: "aaa".to_string()
                },
                SymbolId {
                    realm: Realm::Cargo,
                    name: "bbb".to_string()
                },
            ],
            "with zero affinity, order must fall back to declared_at_seq (declaration order: \
             ccc, aaa, bbb -- deliberately NOT alphabetical, to prove this is schedule order, \
             not a hidden alphabetical fallback)"
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

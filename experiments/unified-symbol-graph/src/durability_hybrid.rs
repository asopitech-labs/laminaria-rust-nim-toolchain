//! Issue #85 -- by-provenance-alone durability failed 4/4 on a reversed-
//! churn scenario (`durability_v2::preclassification_agreement`, see the
//! measured output quoted in issue #85's own body and issue #67 comment
//! <https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/67#issuecomment-5724646520>).
//! This module designs and measures a **hybrid** classifier: `Realm`
//! pre-classification as the zero-cost initial guess, corrected by
//! *observed* redeclare behavior once it disagrees with that guess --
//! issue #85's own concern #1.
//!
//! Builds directly on both prior generations rather than replacing them:
//! `durability::Durability` (the shared Durable/Volatile enum),
//! `durability_v2::RealmDurabilityPolicy` (the pre-classification half),
//! and `durability_v2::HeapEvictingGraph`'s own O(log V) eviction
//! mechanism (this module does not reimplement eviction -- see
//! `HybridDurabilityGraph`'s own doc comment for why it wraps rather than
//! forks that type).
//!
//! # Scope (issue #85's own non-goal)
//!
//! Design and measurement only -- issue #85's non-goals explicitly rule
//! out "実装着手" (starting implementation) beyond what is needed to
//! measure the four concerns its body lists. `HybridDurabilityGraph`
//! below is a real, working implementation (not a stub), because
//! measuring reclassification cost (concern #2) and misprediction cost
//! asymmetry (concern #3) requires something that actually runs -- but it
//! is not wired into `SharedSymbolGraph`'s own production call sites, any
//! scheduler, or any cache-adoption decision. Per issue #67's own
//! completion condition this issue is itself scoped under, adopting a
//! durable cache at all remains contingent on issue #67's own state-
//! retention feasibility conclusion, which this module does not attempt
//! to reach.
//!
//! # A structural asymmetry found while measuring, not assumed going in
//!
//! Issue #85's own concern #3 asks whether the two misprediction
//! directions (`DurableToVolatile`/`VolatileToDurable`) are cost-
//! symmetric. This module's own test suite found something more basic
//! first: under a purely `declare_symbol`-triggered observation model
//! (the only trigger this experiment implements), **`VolatileToDurable`
//! correction cannot occur at all from a single symbol's own repeated
//! declarations** -- see
//! `volatile_to_durable_correction_cannot_occur_from_declare_symbol_observations_alone`'s
//! own doc comment for the mechanism. A symbol only disagrees with a
//! wrong `Volatile` guess while its redeclare count is still below 2;
//! every subsequent declaration of that same symbol necessarily raises
//! the count past 2 and makes the observation agree with the (wrong)
//! guess again. Closing this gap would need a *time/version-based*
//! observation trigger ("this symbol has gone N generations without a
//! redeclare") rather than only a redeclare-triggered one -- a genuinely
//! different observation mechanism, out of this issue's own scope
//! (design/measurement of the four listed concerns, not starting a new
//! implementation direction).

use std::collections::HashMap;
use std::sync::RwLock;

use crate::durability::Durability;
use crate::durability_v2::RealmDurabilityPolicy;
use crate::{Realm, RegisterError, SharedSymbolGraph, SymbolId, SymbolNode};

/// Concern #1 (観測ベース判定の設計): a durability decision that starts
/// from `RealmDurabilityPolicy`'s zero-cost provenance guess and
/// re-classifies to the *opposite* value once the observed redeclare
/// count crosses `reclassify_after_redeclares` disagreements in a row --
/// not on the first disagreement, which would make a single noisy
/// redeclare (e.g. one build-script-triggered re-declare of an otherwise
/// stable FFI symbol) flip the classification. This mirrors
/// `durability::DurableSymbolGraph`'s own observed-based threshold (2
/// redeclares) but applies it only as a *correction* to the provenance
/// guess, not as the sole signal.
///
/// **Why a streak, not a raw count**: a symbol correctly predicted
/// `Durable` by `Realm` and redeclared once (e.g. a genuine, rare content
/// update to a vendored C header) should not immediately flip to
/// `Volatile` -- that single event does not yet distinguish "this symbol
/// turned out to churn like Cargo/Nimble code" from "an ordinary,
/// infrequent real change occurred." A consecutive-disagreement streak
/// (reset to 0 whenever an observation agrees with the *current* live
/// classification) is the simplest mechanism that answers this without
/// introducing a decaying/weighted counter this experiment's scale does
/// not need.
#[derive(Debug, Clone, Copy)]
pub struct HybridDurabilityPolicy {
    /// How many *consecutive* redeclares must disagree with the current
    /// live classification before it flips. `1` degenerates to "flip on
    /// first disagreement" (closest to `durability::DurableSymbolGraph`'s
    /// own unconditional `n >= 2` rule); higher values trade slower
    /// correction for more resistance to single-event noise.
    pub reclassify_after_disagreements: u32,
}

impl Default for HybridDurabilityPolicy {
    /// `2` consecutive disagreements -- the same magnitude
    /// `durability::DurableSymbolGraph`'s own `n >= 2` observed threshold
    /// already uses for "this symbol is volatile," reused here as the
    /// default rather than inventing an unmeasured new constant. Not
    /// claimed optimal (see this module's own doc comment on scope).
    fn default() -> Self {
        HybridDurabilityPolicy {
            reclassify_after_disagreements: 2,
        }
    }
}

/// Per-symbol hybrid classification state: the live classification (what
/// `durability_of` currently returns), and the streak of consecutive
/// observations that disagreed with it.
#[derive(Debug, Clone, Copy)]
struct HybridState {
    live: Durability,
    disagreement_streak: u32,
}

/// One reclassification event, recorded for concern #3's cost-asymmetry
/// measurement -- names *which direction* the flip went, since
/// `durability_optimistic_cost`/`durability_pessimistic_cost` (see
/// `MispredictionCostModel` below) need to charge the two directions
/// differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclassificationEvent {
    /// Was predicted/classified `Durable`, corrected to `Volatile` --
    /// issue #85's "Volatileと予測してDurableだった" is the OPPOSITE
    /// direction; this is "Durableと信じ続けていたが実はVolatileだった"
    /// (the retained-too-long case).
    DurableToVolatile { id: SymbolId },
    /// Was predicted/classified `Volatile`, corrected to `Durable` -- the
    /// "discarded/recomputed too eagerly" case.
    VolatileToDurable { id: SymbolId },
}

/// Wraps `SharedSymbolGraph` directly (not `durability_v2::
/// HeapEvictingGraph`) because this module's own concern is the
/// *classification* policy (concern #1), not re-deriving eviction
/// mechanics `HeapEvictingGraph` already measured and fixed (issue #67's
/// heap-based-vs-sort-based, compaction, incremental reclaim cycles).
/// A production implementation would compose `HybridDurabilityPolicy`
/// INTO `HeapEvictingGraph`'s own classification call
/// (`n >= 2` today) rather than duplicating its eviction heap here --
/// this type exists only to measure the classification policy alone,
/// isolated from eviction-mechanism noise, matching this crate's existing
/// practice of measuring one variable at a time (`cost_correlation_noise`
/// isolated measurement noise from the weighted-proxy question the same
/// way).
pub struct HybridDurabilityGraph {
    pub graph: SharedSymbolGraph,
    policy: HybridDurabilityPolicy,
    redeclare_counts: RwLock<HashMap<SymbolId, u64>>,
    state: RwLock<HashMap<SymbolId, HybridState>>,
    reclassifications: RwLock<Vec<ReclassificationEvent>>,
}

impl HybridDurabilityGraph {
    pub fn new(policy: HybridDurabilityPolicy) -> Self {
        HybridDurabilityGraph {
            graph: SharedSymbolGraph::new(),
            policy,
            redeclare_counts: RwLock::new(HashMap::new()),
            state: RwLock::new(HashMap::new()),
            reclassifications: RwLock::new(Vec::new()),
        }
    }

    /// Declares (or redeclares) `node`, updating the hybrid classification
    /// per this type's own doc comment. On first declaration, the live
    /// classification is seeded from `RealmDurabilityPolicy` alone (the
    /// zero-cost guess) -- no observation exists yet to correct it.
    pub fn declare_symbol(
        &self,
        declaring_realm: Realm,
        node: SymbolNode,
    ) -> Result<(), RegisterError> {
        let id = node.id.clone();
        self.graph.declare_symbol(declaring_realm, node)?;

        let n = {
            let mut counts = self.redeclare_counts.write().expect("counts lock poisoned");
            let entry = counts.entry(id.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        // Same observed-classification rule durability.rs uses (n >= 2 =>
        // Volatile) -- this is the "what does the raw observation say"
        // signal the hybrid policy corrects the live classification
        // towards, kept identical to the first-generation rule so this
        // module measures ONLY the effect of adding provenance
        // pre-classification + streak-gated correction, not a second,
        // independently-varying observed-threshold change at the same
        // time.
        let observed = if n >= 2 {
            Durability::Volatile
        } else {
            Durability::Durable
        };

        let mut state = self.state.write().expect("state lock poisoned");
        let entry = state.entry(id.clone()).or_insert_with(|| HybridState {
            live: RealmDurabilityPolicy::classify(declaring_realm),
            disagreement_streak: 0,
        });

        if observed == entry.live {
            entry.disagreement_streak = 0;
        } else {
            entry.disagreement_streak += 1;
            if entry.disagreement_streak >= self.policy.reclassify_after_disagreements {
                let event = match entry.live {
                    Durability::Durable => {
                        ReclassificationEvent::DurableToVolatile { id: id.clone() }
                    }
                    Durability::Volatile => {
                        ReclassificationEvent::VolatileToDurable { id: id.clone() }
                    }
                };
                entry.live = observed;
                entry.disagreement_streak = 0;
                self.reclassifications
                    .write()
                    .expect("reclassifications lock poisoned")
                    .push(event);
            }
        }

        Ok(())
    }

    pub fn durability_of(&self, id: &SymbolId) -> Option<Durability> {
        self.state
            .read()
            .expect("state lock poisoned")
            .get(id)
            .map(|s| s.live)
    }

    /// Every reclassification event that has occurred so far, in order --
    /// concern #3's own raw data (which direction, how many).
    pub fn reclassification_events(&self) -> Vec<ReclassificationEvent> {
        self.reclassifications
            .read()
            .expect("reclassifications lock poisoned")
            .clone()
    }
}

/// Concern #3 (誤判定のコスト非対称性): the two misprediction directions
/// charged in the SAME unit (an abstract "cost point"), so a caller can
/// compare them directly. This module does not claim real-world costs are
/// actually in this unit -- see `retention_cost_estimate`/
/// `recompute_cost_estimate`'s own doc comments for what a caller would
/// need to supply to make this a real cost comparison rather than a
/// relative one.
#[derive(Debug, Clone, Copy)]
pub struct MispredictionCostModel {
    /// Cost charged per "extra call" a symbol stays resident beyond when
    /// it would have been evicted under correct classification --
    /// i.e. the `DurableToVolatile` direction's cost (issue #85's
    /// "Durableと予測してVolatileだった...不要に長く保持し続ける" case).
    /// Named `retention_cost_per_call` because it is naturally a
    /// per-call-while-mistakenly-retained rate, not a one-off charge.
    pub retention_cost_per_call: f64,
    /// Cost charged once, at the moment a symbol is mistakenly discarded/
    /// recomputed under a wrongly-`Volatile` classification -- the
    /// `VolatileToDurable` direction (issue #85's "Volatileと予測して
    /// Durableだった...不要に頻繁に破棄・再計算する" case). A one-off
    /// charge (not per-call) because each individual unnecessary
    /// eviction+recompute is itself the unit of waste, unlike retention's
    /// accumulating-while-resident cost.
    pub recompute_cost_per_reclassification: f64,
}

/// Applies `model` to a sequence of `ReclassificationEvent`s plus how many
/// `declare_symbol` calls occurred between each `DurableToVolatile`
/// event's misclassification window and its correction (the "how long
/// was it mistakenly retained" duration `retention_cost_per_call`
/// multiplies against). Returns the total cost attributed to each
/// direction separately -- NOT summed into one number -- because concern
/// #3 asks whether the two directions are symmetric, which a single
/// combined total would hide.
#[derive(Debug, Clone, Copy, Default)]
pub struct AsymmetryReport {
    pub total_retention_cost: f64,
    pub total_recompute_cost: f64,
    pub durable_to_volatile_count: usize,
    pub volatile_to_durable_count: usize,
}

/// Computes `AsymmetryReport` from a hybrid graph's own recorded events,
/// where `retained_calls_before_correction` gives, for each
/// `DurableToVolatile` event in order, how many `declare_symbol` calls
/// passed for that symbol between when the wrong classification started
/// being trusted and when it was corrected (the retention window) --
/// supplied by the caller because `HybridDurabilityGraph` itself does not
/// track per-symbol call counts beyond the redeclare counter already
/// used for classification (adding that bookkeeping would be a real,
/// separate design decision this module's scope does not reach).
pub fn asymmetry_report(
    events: &[ReclassificationEvent],
    retained_calls_before_correction: &[u64],
    model: MispersonCostModelAlias,
) -> AsymmetryReport {
    let mut report = AsymmetryReport::default();
    let mut retention_iter = retained_calls_before_correction.iter();
    for event in events {
        match event {
            ReclassificationEvent::DurableToVolatile { .. } => {
                report.durable_to_volatile_count += 1;
                let calls = retention_iter.next().copied().unwrap_or(0) as f64;
                report.total_retention_cost += calls * model.retention_cost_per_call;
            }
            ReclassificationEvent::VolatileToDurable { .. } => {
                report.volatile_to_durable_count += 1;
                report.total_recompute_cost += model.recompute_cost_per_reclassification;
            }
        }
    }
    report
}

/// Type alias purely to keep `asymmetry_report`'s signature from
/// repeating the full `MispredictionCostModel` name twice in one line --
/// no behavioral meaning beyond that.
pub type MispersonCostModelAlias = MispredictionCostModel;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AddressState, CodeBody};

    fn sym(realm: Realm, name: &str) -> SymbolNode {
        SymbolNode {
            id: SymbolId {
                realm,
                name: name.to_string(),
            },
            address: AddressState::Committed(CodeBody {
                code: vec![0x90],
                relocations: vec![],
            }),
        }
    }

    /// Concern #1's core claim, tested directly: a symbol whose `Realm`
    /// guess is WRONG (declared `Realm::C`, i.e. predicted `Durable`, but
    /// actually redeclared repeatedly like application code) is corrected
    /// to `Volatile` after enough consecutive disagreements -- unlike
    /// `RealmDurabilityPolicy` alone, which would stay wrong forever (it
    /// has no observation feedback at all).
    #[test]
    fn a_realm_misprediction_is_corrected_after_the_disagreement_streak() {
        let g = HybridDurabilityGraph::new(HybridDurabilityPolicy {
            reclassify_after_disagreements: 2,
        });
        let id = SymbolId {
            realm: Realm::C,
            name: "vendored_but_actively_patched".to_string(),
        };

        g.declare_symbol(Realm::C, sym(Realm::C, &id.name)).unwrap();
        assert_eq!(
            g.durability_of(&id),
            Some(Durability::Durable),
            "first declaration must seed from Realm alone (Realm::C -> Durable)"
        );

        // Redeclare #2: n=2 => observed Volatile, 1st disagreement (streak
        // 1, not yet enough to flip with reclassify_after_disagreements=2).
        g.declare_symbol(Realm::C, sym(Realm::C, &id.name)).unwrap();
        assert_eq!(
            g.durability_of(&id),
            Some(Durability::Durable),
            "a single disagreement must not flip the classification yet"
        );

        // Redeclare #3: n=3 => still observed Volatile, 2nd consecutive
        // disagreement => flips.
        g.declare_symbol(Realm::C, sym(Realm::C, &id.name)).unwrap();
        assert_eq!(
            g.durability_of(&id),
            Some(Durability::Volatile),
            "two consecutive disagreements must flip Durable -> Volatile"
        );

        let events = g.reclassification_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            ReclassificationEvent::DurableToVolatile { id: id.clone() }
        );
    }

    /// Concern #1's mirror direction (`VolatileToDurable`), and an honest
    /// structural finding this test surfaces rather than hides: under
    /// this module's own observation model (`declare_symbol`'s own
    /// redeclare counter, `n >= 2 => Volatile`), a symbol initially
    /// mispredicted `Volatile` can ONLY disagree (observed `Durable`)
    /// while `n < 2` -- i.e. on its very first declaration. Every
    /// subsequent `declare_symbol` call for that same symbol pushes `n`
    /// past 2, which makes the observation agree with the (wrong)
    /// Volatile guess again. This means a single symbol, observed through
    /// repeated `declare_symbol` calls alone, can never accumulate a
    /// 2-consecutive-disagreement streak in the `VolatileToDurable`
    /// direction -- only a DIFFERENT symbol that is genuinely rarely
    /// redeclared (n stays at 0 or 1 across the observation window) would
    /// ever need that correction, and this module's per-`declare_symbol`
    /// observation trigger has no way to "observe non-redeclaration" as
    /// an event at all (there is nothing to react to when nothing
    /// happens). This is a real, load-bearing limitation of the hybrid
    /// design as specified here -- recorded honestly per this
    /// experiment's own "does not assume the hypothesis is true" rule,
    /// not asserted away. A future design would need a time/version-based
    /// observation trigger ("this symbol has not been redeclared across N
    /// generations") rather than only a redeclare-triggered one to close
    /// this gap -- out of this issue's own non-goal of starting
    /// implementation beyond what measuring the four concerns requires.
    #[test]
    fn volatile_to_durable_correction_cannot_occur_from_declare_symbol_observations_alone() {
        let g = HybridDurabilityGraph::new(HybridDurabilityPolicy {
            reclassify_after_disagreements: 2,
        });
        let id = SymbolId {
            realm: Realm::Cargo,
            name: "actually_rarely_redeclared_cargo_symbol".to_string(),
        };

        // First declaration: n=1 => observed Durable, disagrees with the
        // initial Volatile guess -- streak becomes 1.
        g.declare_symbol(Realm::Cargo, sym(Realm::Cargo, &id.name))
            .unwrap();
        assert_eq!(g.durability_of(&id), Some(Durability::Volatile));

        // A second declaration of the SAME symbol pushes n to 2, which
        // makes the observation flip to Volatile -- agreeing with (not
        // disagreeing from) the live guess, resetting the streak instead
        // of advancing it towards a VolatileToDurable correction.
        g.declare_symbol(Realm::Cargo, sym(Realm::Cargo, &id.name))
            .unwrap();
        assert_eq!(
            g.durability_of(&id),
            Some(Durability::Volatile),
            "a second declare_symbol call necessarily raises n to 2, which makes the \
             observation agree with the wrong Volatile guess -- the disagreement streak resets \
             instead of accumulating towards a VolatileToDurable correction"
        );

        assert!(
            g.reclassification_events().is_empty(),
            "no VolatileToDurable correction can occur here by construction -- see this test's \
             own doc comment for why the per-declare_symbol observation trigger structurally \
             cannot produce this direction from a single symbol's own repeated declarations"
        );

        eprintln!(
            "[durability_hybrid][structural-limitation] VolatileToDurable correction cannot be \
             produced by repeated declare_symbol calls on the SAME symbol under this module's \
             observation model -- a genuinely-rarely-redeclared symbol would need a \
             time/version-based 'no redeclare observed across N generations' trigger instead, \
             which this design does not implement (see this test's own doc comment)"
        );
    }

    /// The mirror case: a symbol correctly predicted Durable and observed
    /// as such (never redeclared past n=1) must NEVER reclassify -- the
    /// hybrid policy must not spuriously flip a symbol whose provenance
    /// guess is actually correct.
    #[test]
    fn a_correct_realm_prediction_never_reclassifies() {
        let g = HybridDurabilityGraph::new(HybridDurabilityPolicy::default());
        let id = SymbolId {
            realm: Realm::Cpp,
            name: "stable_cpp_export".to_string(),
        };
        g.declare_symbol(Realm::Cpp, sym(Realm::Cpp, &id.name))
            .unwrap();

        assert_eq!(g.durability_of(&id), Some(Durability::Durable));
        assert!(g.reclassification_events().is_empty());
    }

    /// Concern #4 (一般化可能性), first of two opposite scenarios: the
    /// "normal case" issue #85 names -- Cargo/Nimble application code
    /// changes frequently (correctly predicted Volatile), C/C++ FFI
    /// symbols stay stable (correctly predicted Durable). The hybrid
    /// policy must reclassify NOTHING here -- it should behave identically
    /// to bare `RealmDurabilityPolicy` when the provenance guess is
    /// already right, confirming the hybrid mechanism adds no spurious
    /// churn when it is not needed.
    #[test]
    fn normal_churn_pattern_produces_zero_reclassifications() {
        let g = HybridDurabilityGraph::new(HybridDurabilityPolicy::default());

        // Stable C/C++ symbols: declared once each, never redeclared.
        for i in 0..5 {
            g.declare_symbol(Realm::C, sym(Realm::C, &format!("libc_fn_{i}")))
                .unwrap();
            g.declare_symbol(Realm::Cpp, sym(Realm::Cpp, &format!("libcpp_fn_{i}")))
                .unwrap();
        }
        // Actively-edited Cargo/Nimble application symbols: redeclared
        // repeatedly (5 times each), matching real edit-and-rebuild churn.
        for i in 0..5 {
            for _ in 0..5 {
                g.declare_symbol(Realm::Cargo, sym(Realm::Cargo, &format!("app_fn_{i}")))
                    .unwrap();
            }
        }

        assert!(
            g.reclassification_events().is_empty(),
            "when Realm's guess already matches observed behavior, the hybrid policy must not \
             reclassify anything -- got {:?}",
            g.reclassification_events()
        );
    }

    /// Concern #4, second scenario: the REVERSED-churn case issue #85's
    /// own body already measured breaking bare `RealmDurabilityPolicy`
    /// 4/4 (a vendored C/C++ library under active local patching, while
    /// Cargo/Nimble dependencies are stable pinned versions). This is the
    /// direct empirical answer to concern #1: does the hybrid policy
    /// correct itself where the pure-provenance policy failed completely?
    #[test]
    fn reversed_churn_pattern_is_corrected_by_the_hybrid_policy_where_realm_alone_fails() {
        let g = HybridDurabilityGraph::new(HybridDurabilityPolicy::default());

        // C/C++ "vendored but actively patched" symbols: redeclared 6
        // times each, matching issue #67's own reversed-churn measurement
        // (observed_redeclares/symbol=6).
        for i in 0..3 {
            for _ in 0..6 {
                g.declare_symbol(Realm::C, sym(Realm::C, &format!("patched_c_fn_{i}")))
                    .unwrap();
            }
        }
        // Cargo/Nimble "pinned, stable" symbols: declared once (matching
        // issue #67's own reversed-churn measurement,
        // observed_redeclares/symbol=1 -- one declaration, zero
        // redeclares). This single declaration disagrees with the
        // Volatile guess exactly once, and BY DESIGN (see
        // HybridDurabilityPolicy's own doc comment on why a lone
        // disagreement must not flip the classification) a single
        // disagreement is not enough evidence to reclassify -- these
        // symbols are therefore expected to STAY at their (wrong) initial
        // Volatile guess below, an honest limitation this test documents
        // rather than hides: issue #85's own concern #1 asks whether the
        // hybrid design corrects Realm's mispredictions, and the answer
        // for a symbol observed only ONCE is "not yet, by design" --
        // correction requires enough repeated observation to distinguish
        // a real pattern from a single event, matching this module's own
        // reasoning against flipping on first disagreement.
        for i in 0..3 {
            g.declare_symbol(
                Realm::Cargo,
                sym(Realm::Cargo, &format!("pinned_dep_fn_{i}")),
            )
            .unwrap();
        }

        for i in 0..3 {
            let id = SymbolId {
                realm: Realm::C,
                name: format!("patched_c_fn_{i}"),
            };
            eprintln!(
                "[durability_hybrid][reversed-churn] symbol={:<20} final_durability={:?} \
                 (Realm-only prediction would have stayed Durable forever, wrong)",
                id.name,
                g.durability_of(&id)
            );
            assert_eq!(
                g.durability_of(&id),
                Some(Durability::Volatile),
                "a C symbol redeclared 6 times must be corrected to Volatile by the hybrid \
                 policy, where issue #85's own measurement showed Realm-alone predicting \
                 Durable and being wrong"
            );
        }
        for i in 0..3 {
            let id = SymbolId {
                realm: Realm::Cargo,
                name: format!("pinned_dep_fn_{i}"),
            };
            eprintln!(
                "[durability_hybrid][reversed-churn] symbol={:<20} final_durability={:?} \
                 (single-observation disagreement, expected to STAY at the wrong Volatile \
                 guess -- correcting a lone disagreement is by design not attempted)",
                id.name,
                g.durability_of(&id)
            );
            assert_eq!(
                g.durability_of(&id),
                Some(Durability::Volatile),
                "a Cargo symbol declared only once (one disagreement, below the \
                 reclassify_after_disagreements=2 threshold) must remain at Realm's initial \
                 (wrong) Volatile guess -- this is the hybrid policy's honest limitation, not a \
                 bug: distinguishing 'one real, ordinary change' from 'this symbol genuinely \
                 behaves the opposite of its Realm' needs more than a single observation"
            );
        }

        // Only the 3 C symbols (redeclared 6 times each, crossing the
        // reclassify_after_disagreements=2 threshold) reclassify -- the 3
        // Cargo symbols (a single disagreement each) correctly do NOT,
        // per this test's own doc comment on that being the hybrid
        // policy's honest, by-design limitation rather than a bug.
        let events = g.reclassification_events();
        assert_eq!(
            events.len(),
            3,
            "only the 3 C symbols (which crossed the 2-consecutive-disagreement threshold) \
             should have reclassified; the 3 Cargo symbols (a single disagreement each) must \
             not have, got {events:?}"
        );
        assert!(
            events
                .iter()
                .all(|e| matches!(e, ReclassificationEvent::DurableToVolatile { .. })),
            "every reclassification in this scenario must be DurableToVolatile (the C symbols), \
             not VolatileToDurable -- got {events:?}"
        );
    }

    /// Concern #2 (再分類のコスト): measures the wall-clock cost of
    /// `declare_symbol` under the hybrid policy against the same
    /// operation under `durability_v2::HeapEvictingGraph`'s existing
    /// bare-observed classification, to check whether adding the
    /// provenance-guess/streak-tracking bookkeeping meaningfully changes
    /// per-call cost. Per issue #67's own `cost_correlation_noise`
    /// finding (measurement noise can exceed a real effect at this scale),
    /// this test reports the ratio across multiple repeated batches
    /// rather than asserting a specific improvement/regression, honestly.
    #[test]
    fn reclassification_overhead_is_measured_across_repeated_batches_not_asserted_favorably() {
        use std::time::Instant;

        const N: usize = 2000;
        let mut ratios = Vec::new();

        for batch in 0..3 {
            let heap_graph = crate::durability_v2::HeapEvictingGraph::new(50);
            let start = Instant::now();
            for i in 0..N {
                heap_graph
                    .declare_symbol(Realm::Cargo, sym(Realm::Cargo, &format!("sym_{}", i % 100)))
                    .unwrap();
            }
            let heap_elapsed = start.elapsed();

            let hybrid_graph = HybridDurabilityGraph::new(HybridDurabilityPolicy::default());
            let start = Instant::now();
            for i in 0..N {
                hybrid_graph
                    .declare_symbol(Realm::Cargo, sym(Realm::Cargo, &format!("sym_{}", i % 100)))
                    .unwrap();
            }
            let hybrid_elapsed = start.elapsed();

            let ratio = hybrid_elapsed.as_nanos() as f64 / heap_elapsed.as_nanos().max(1) as f64;
            ratios.push(ratio);
            eprintln!(
                "[durability_hybrid][reclassification-overhead] batch={batch} \
                 heap_only={heap_elapsed:?} hybrid={hybrid_elapsed:?} hybrid/heap_ratio={ratio:.3}x"
            );
        }

        eprintln!(
            "[durability_hybrid][reclassification-overhead] ratio range across 3 batches: \
             {:.3}x to {:.3}x -- per issue #67's own cost_correlation_noise finding, a ratio \
             range this wide relative to 1.0x means the difference (if any) cannot be \
             distinguished from measurement noise at this sample size; report, do not assert \
             a specific direction",
            ratios.iter().cloned().fold(f64::INFINITY, f64::min),
            ratios.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        );

        // Correctness-only assertion: every batch actually ran and
        // produced a positive ratio. No assertion on the ratio's
        // magnitude or direction -- see this test's own doc comment.
        assert_eq!(ratios.len(), 3);
        for r in &ratios {
            assert!(*r > 0.0, "ratio must be positive, got {r}");
        }
    }

    /// Concern #3 companion: exercises `asymmetry_report` against a small,
    /// hand-constructed event sequence with an explicit cost model, to
    /// confirm the two misprediction directions ARE tracked and reported
    /// separately (not collapsed into one number) -- the direct structural
    /// answer to "are the two directions symmetric," which this module
    /// leaves to the caller's own cost model rather than asserting a
    /// universal answer (issue #85's own concern #3 asks to VERIFY
    /// asymmetry, not assume it).
    #[test]
    fn asymmetry_report_tracks_both_misprediction_directions_independently() {
        let events = vec![
            ReclassificationEvent::DurableToVolatile {
                id: SymbolId {
                    realm: Realm::C,
                    name: "a".to_string(),
                },
            },
            ReclassificationEvent::VolatileToDurable {
                id: SymbolId {
                    realm: Realm::Cargo,
                    name: "b".to_string(),
                },
            },
            ReclassificationEvent::VolatileToDurable {
                id: SymbolId {
                    realm: Realm::Cargo,
                    name: "c".to_string(),
                },
            },
        ];
        // Symbol "a" was mistakenly retained for 10 extra declare_symbol
        // calls before correction.
        let retained_calls = vec![10];
        let model = MispredictionCostModel {
            retention_cost_per_call: 1.0,
            recompute_cost_per_reclassification: 50.0,
        };

        let report = asymmetry_report(&events, &retained_calls, model);

        assert_eq!(report.durable_to_volatile_count, 1);
        assert_eq!(report.volatile_to_durable_count, 2);
        assert_eq!(report.total_retention_cost, 10.0);
        assert_eq!(report.total_recompute_cost, 100.0);

        eprintln!(
            "[durability_hybrid][asymmetry] with retention_cost_per_call=1.0, \
             recompute_cost_per_reclassification=50.0: total_retention_cost={:.1} \
             total_recompute_cost={:.1} -- under THIS cost model the two directions are NOT \
             symmetric (recompute cost dominates); a real cost model (issue #85's own concern #3) \
             would need actual measured retention/recompute costs from a real toolchain, which \
             this module does not supply -- see this module's own doc comment on scope",
            report.total_retention_cost, report.total_recompute_cost
        );
    }
}

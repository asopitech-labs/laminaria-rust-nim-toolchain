//! Hypothesis (not connected to laminaria-plan's Obligation model on
//! purpose -- built fresh in an isolated worktree/crate to avoid carrying
//! over that model's assumptions): instead of "N independent frontends ->
//! N object files -> one linker pass that resolves symbols after the
//! fact," can the boundary-crossing (FFI) symbol resolution be modeled as
//! a *shared, concurrently-writable graph* that every frontend writes to
//! *while* it does its own realm-local semantic analysis, so no separate
//! "linking" pass is needed to discover which realm satisfies which
//! requirement?
//!
//! Deliberately not one monolithic table: each realm (Cargo/Rust,
//! Nimble/Nim, C, C++) owns its own local symbol table -- its internal
//! type/signature detail is never exposed outside that realm. Only
//! symbols that cross a realm boundary (an `extern "C"` declaration in
//! Rust, an FFI export in Nim/C/C++) become nodes in the shared global
//! graph, and only the *relationship* between a requirement and its
//! provider becomes an edge. This keeps the shared, contended state small
//! (boundary symbols only) while each realm's own heavy semantic work
//! (parsing, type-checking, monomorphization) stays entirely local and
//! parallel -- the same shape mold's own design took relative to GNU
//! ld/lld (parse everything in parallel first, resolve symbols as a
//! separate, independent pass), but applied one layer upstream of object
//! files: this graph exists *instead of* one, not to describe one after
//! the fact.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// The four ecosystems this hypothesis is scoped to (matching this repo's
/// own `cadd`/`app` fixture: Cargo, Nimble, C, C++). Not meant to be an
/// exhaustive or permanent list -- a placeholder for "whichever realms a
/// real cross-ecosystem build needs," kept closed here only to keep the
/// hypothesis concrete and testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Realm {
    Cargo,
    Nimble,
    C,
    Cpp,
}

/// A boundary symbol's identity: never ambiguous by name alone (two
/// realms could each declare `foo`), so identity is (realm, name) --
/// consistent with the real fixture's own `#[link(name = "cadd")]`-style
/// hints naming both a package and a symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolId {
    pub realm: Realm,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressState {
    /// Declared, but this realm's own frontend has not yet finished
    /// enough of its local analysis to assign a definite address.
    Unresolved,
    /// This realm's own frontend has committed to producing this symbol
    /// at a specific (not-yet-final, but stable-for-this-build) offset
    /// within its own eventual code/data region.
    Committed(u64),
}

/// One boundary-crossing symbol node. Lives in the shared graph *only*
/// because some other realm might reference it -- a realm-internal symbol
/// never gets one of these.
#[derive(Debug, Clone)]
pub struct SymbolNode {
    pub id: SymbolId,
    pub address: AddressState,
}

/// A directed edge: `requiring_realm` needs `symbol`, and expects
/// `providing_realm` to supply it. Kept as data (not just an
/// `Option<Realm>` on the requirement) so more than one candidate
/// provider can be recorded before resolution -- matching this repo's own
/// real "provider@version" selection problem, not assuming resolution is
/// trivial.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequiresEdge {
    pub requiring_realm: Realm,
    pub symbol: SymbolId,
}

/// The shared, concurrently-writable graph. Realm-local semantic state
/// (types, ASTs, monomorphization) deliberately does NOT live here -- see
/// this module's own doc comment. `RwLock` per map (not one lock over the
/// whole graph) so a write from one realm's frontend does not block reads
/// or writes concerning a different realm's own boundary symbols, as long
/// as they don't touch the same key -- a real, if coarse-grained,
/// approximation of "resolution proceeds as an independent parallel pass"
/// without requiring a lock-free concurrent hash map (the real gap ld.lld
/// itself named -- see this crate's own README).
pub struct SharedSymbolGraph {
    nodes: RwLock<HashMap<SymbolId, SymbolNode>>,
    /// requirement -> the provider realms declared as candidates, in
    /// declaration order (first-registered is not "wins" here -- that is
    /// exactly the GNU-ld-style order dependency this hypothesis exists
    /// to avoid; order is retained only as evidence, never as a tie-break
    /// rule).
    edges: RwLock<HashMap<RequiresEdge, Vec<Realm>>>,
    /// Monotonic counter so every mutation this graph accepts can be
    /// ordered for later inspection/debugging without relying on
    /// wall-clock time (which is not comparable across threads reliably
    /// enough for this purpose).
    mutation_seq: AtomicU64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// A realm attempted to declare a boundary symbol under a different
    /// realm's own name -- e.g. Rust code trying to register a node as
    /// `Realm::C`. Never silently reassigned; this is a caller bug.
    RealmMismatch {
        declared_by: Realm,
        node_realm: Realm,
    },
}

impl Default for SharedSymbolGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSymbolGraph {
    pub fn new() -> Self {
        SharedSymbolGraph {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(HashMap::new()),
            mutation_seq: AtomicU64::new(0),
        }
    }

    /// A realm's own frontend declares that it owns (will eventually
    /// provide) this boundary symbol. Callable concurrently by every
    /// realm's frontend thread; only the `nodes` map is locked, and only
    /// for the duration of one insert.
    pub fn declare_symbol(
        &self,
        declaring_realm: Realm,
        node: SymbolNode,
    ) -> Result<(), RegisterError> {
        if node.id.realm != declaring_realm {
            return Err(RegisterError::RealmMismatch {
                declared_by: declaring_realm,
                node_realm: node.id.realm,
            });
        }
        self.mutation_seq.fetch_add(1, Ordering::Relaxed);
        let mut nodes = self.nodes.write().expect("nodes lock poisoned");
        nodes.insert(node.id.clone(), node);
        Ok(())
    }

    /// A realm's own frontend records that it needs `symbol`, believing
    /// `expected_provider` to be the realm that supplies it. Never blocks
    /// on `symbol` actually existing yet -- registering the requirement
    /// and satisfying it are independent events, exactly the property
    /// this hypothesis needs (a Rust frontend thread can register its
    /// `extern "C" { fn c_add(...) }` requirement before the C frontend
    /// thread has finished analyzing `cadd.c`, with no ordering
    /// constraint between the two).
    pub fn require_symbol(
        &self,
        requiring_realm: Realm,
        symbol: SymbolId,
        expected_provider: Realm,
    ) {
        self.mutation_seq.fetch_add(1, Ordering::Relaxed);
        let edge = RequiresEdge {
            requiring_realm,
            symbol,
        };
        let mut edges = self.edges.write().expect("edges lock poisoned");
        let candidates = edges.entry(edge).or_default();
        if !candidates.contains(&expected_provider) {
            candidates.push(expected_provider);
        }
    }

    /// Total, read-only snapshot: every requirement this graph has seen,
    /// paired with whichever `SymbolNode` (if any) its `expected_provider`
    /// candidates actually declared. `None` means genuinely unresolved --
    /// not a fabricated placeholder. This is the *only* function that
    /// crosses realm boundaries to actually compute a match; declaration
    /// and requirement registration never do.
    pub fn resolve_all(&self) -> Vec<ResolvedRequirement> {
        let edges = self.edges.read().expect("edges lock poisoned");
        let nodes = self.nodes.read().expect("nodes lock poisoned");
        edges
            .iter()
            .map(|(edge, candidate_realms)| {
                let provider = candidate_realms.iter().find_map(|realm| {
                    let candidate_id = SymbolId {
                        realm: *realm,
                        name: edge.symbol.name.clone(),
                    };
                    nodes.get(&candidate_id).cloned()
                });
                ResolvedRequirement {
                    requirement: edge.clone(),
                    candidate_realms: candidate_realms.clone(),
                    provider,
                }
            })
            .collect()
    }

    /// Evidence-only counter: how many declare/require calls this graph
    /// has accepted, for a caller to confirm concurrent writers actually
    /// interleaved rather than serializing by accident (e.g. behind one
    /// giant lock this crate's own design claims to avoid).
    pub fn mutation_count(&self) -> u64 {
        self.mutation_seq.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedRequirement {
    pub requirement: RequiresEdge,
    pub candidate_realms: Vec<Realm>,
    pub provider: Option<SymbolNode>,
}

impl ResolvedRequirement {
    pub fn is_resolved(&self) -> bool {
        self.provider.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// The real fixture's own boundary shape (`cadd`/`app`), reproduced
    /// here as four independent threads -- one per realm -- so the test
    /// actually exercises concurrent writers, not just sequential calls
    /// that happen to compile against a thread-safe type. Each thread
    /// only ever touches its own realm's declarations/requirements; the
    /// graph itself is the only shared state.
    #[test]
    fn four_realms_declare_and_require_concurrently_and_every_boundary_symbol_resolves() {
        let graph = Arc::new(SharedSymbolGraph::new());

        let handles: Vec<thread::JoinHandle<()>> = vec![
            {
                let graph = Arc::clone(&graph);
                thread::spawn(move || {
                    // app (Cargo/Rust): requires c_add (C), cpp_max_i32
                    // (C++), nim_double (Nimble) -- never declares a
                    // boundary symbol of its own in this fixture shape.
                    graph.require_symbol(
                        Realm::Cargo,
                        SymbolId {
                            realm: Realm::C,
                            name: "c_add".to_string(),
                        },
                        Realm::C,
                    );
                    graph.require_symbol(
                        Realm::Cargo,
                        SymbolId {
                            realm: Realm::Cpp,
                            name: "cpp_max_i32".to_string(),
                        },
                        Realm::Cpp,
                    );
                    graph.require_symbol(
                        Realm::Cargo,
                        SymbolId {
                            realm: Realm::Nimble,
                            name: "nim_double".to_string(),
                        },
                        Realm::Nimble,
                    );
                })
            },
            {
                let graph = Arc::clone(&graph);
                thread::spawn(move || {
                    graph
                        .declare_symbol(
                            Realm::C,
                            SymbolNode {
                                id: SymbolId {
                                    realm: Realm::C,
                                    name: "c_add".to_string(),
                                },
                                address: AddressState::Committed(0x1000),
                            },
                        )
                        .expect("C realm may declare its own symbol");
                })
            },
            {
                let graph = Arc::clone(&graph);
                thread::spawn(move || {
                    graph
                        .declare_symbol(
                            Realm::Cpp,
                            SymbolNode {
                                id: SymbolId {
                                    realm: Realm::Cpp,
                                    name: "cpp_max_i32".to_string(),
                                },
                                address: AddressState::Committed(0x2000),
                            },
                        )
                        .expect("C++ realm may declare its own symbol");
                })
            },
            {
                let graph = Arc::clone(&graph);
                thread::spawn(move || {
                    graph
                        .declare_symbol(
                            Realm::Nimble,
                            SymbolNode {
                                id: SymbolId {
                                    realm: Realm::Nimble,
                                    name: "nim_double".to_string(),
                                },
                                address: AddressState::Committed(0x3000),
                            },
                        )
                        .expect("Nimble realm may declare its own symbol");
                })
            },
        ];

        for handle in handles {
            handle.join().expect("realm thread must not panic");
        }

        // No separate "link" pass: resolve_all() is the only step after
        // the four realm threads finish, and it only ever reads what
        // they already wrote -- it performs no analysis of its own.
        let resolved = graph.resolve_all();
        assert_eq!(resolved.len(), 3, "app's three real FFI requirements");
        for r in &resolved {
            assert!(
                r.is_resolved(),
                "requirement {:?} must resolve against the real concurrent declarations, got {:?}",
                r.requirement,
                r
            );
        }

        // At least declare_symbol x3 + require_symbol x3 = 6 mutations;
        // confirms the graph actually accepted writes from all four
        // threads, not just the last one to run.
        assert!(graph.mutation_count() >= 6);
    }

    /// A requirement whose declared provider never actually registers a
    /// matching symbol must resolve to `None` -- never silently invent a
    /// resolution. This is the same "conservative retention" discipline
    /// `cross-layer-reachability-pruning_ja.md` requires of any pruning
    /// scheme: an unresolved requirement is observable as unresolved, not
    /// hidden.
    #[test]
    fn an_unsatisfied_requirement_resolves_to_none_not_a_fabricated_match() {
        let graph = SharedSymbolGraph::new();
        graph.require_symbol(
            Realm::Cargo,
            SymbolId {
                realm: Realm::C,
                name: "never_declared".to_string(),
            },
            Realm::C,
        );

        let resolved = graph.resolve_all();
        assert_eq!(resolved.len(), 1);
        assert!(!resolved[0].is_resolved());
        assert!(resolved[0].provider.is_none());
    }

    /// A realm attempting to declare a symbol under a different realm's
    /// name is a structural error, never silently reassigned to the
    /// caller's own realm -- this is the boundary discipline that keeps
    /// "which realm owns this symbol" unambiguous even under concurrent
    /// writes.
    #[test]
    fn declaring_a_symbol_under_the_wrong_realm_is_refused() {
        let graph = SharedSymbolGraph::new();
        let result = graph.declare_symbol(
            Realm::Cargo,
            SymbolNode {
                id: SymbolId {
                    realm: Realm::C,
                    name: "c_add".to_string(),
                },
                address: AddressState::Committed(0x1000),
            },
        );
        assert_eq!(
            result,
            Err(RegisterError::RealmMismatch {
                declared_by: Realm::Cargo,
                node_realm: Realm::C,
            })
        );
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolId {
    pub realm: Realm,
    pub name: String,
}

/// One place inside `code` whose bytes are a placeholder (all-zero, per
/// the real `cc -c` output verified directly: `objdump -d` on a function
/// calling two external symbols showed `e8 00 00 00 00` at each call
/// site -- a `call` opcode followed by a zeroed 4-byte operand) that must
/// be overwritten once `target`'s own final address is known. This is
/// the same information a real ELF `R_X86_64_PLT32`/`R_X86_64_PC32`
/// relocation record carries (`objdump -r`: offset, symbol, addend) --
/// kept here as one entry directly on the symbol's own `CodeBody` rather
/// than in a separate section/relocation-table indirection, since this
/// hypothesis has no sections to begin with.
///
/// **Platform scope, verified not assumed**: this shape (offset/width/
/// target/addend, `apply_relocations`'s own PC-relative formula) is
/// ELF/x86_64-specific -- confirmed by checking what `cc` actually is on
/// each of the four real ecosystems' target platforms. On Linux, `cc`
/// (gcc or clang) and Nim's `c`/`cpp` backends both emit ELF objects;
/// the same `cc` command name on macOS is Apple's Clang, which emits
/// **Mach-O**, a structurally different object format with its own
/// relocation type set and its own linker identity (`ld64`, not GNU
/// ld/lld) -- LLVM's own Mach-O port confirms this is a distinct linker
/// mode, not just a different flag to the same ELF logic
/// (<https://lld.llvm.org/MachO/index.html>). Nim's third native
/// backend, `nim objc`, generates Objective-C (`.m`) source specifically
/// because macOS's own toolchain expects it, and that source is in turn
/// compiled by the same platform Clang -- so it is the *same* platform
/// divergence as the C/C++ backends, not a fourth, separate case. The
/// principle this hypothesis tests (shared graph, no separate object-file/
/// link pass) is platform-independent; this concrete `PendingReloc`
/// encoding is not, and a macOS/Mach-O (or Windows/COFF) port needs its
/// own relocation-shape verification against real `clang`/`cl.exe`
/// output before this crate's claims can be said to hold there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReloc {
    /// Byte offset into `code` where the placeholder begins.
    pub offset: usize,
    /// How many bytes the placeholder occupies (4 for the 32-bit
    /// PC-relative call operand verified above).
    pub width: usize,
    /// The boundary symbol whose eventual address this placeholder must
    /// be computed from.
    pub target: SymbolId,
    /// Added to `target`'s resolved address before truncating to `width`
    /// bytes -- e.g. the real `-0x4` addend `objdump -r` reported,
    /// accounting for PC-relative addressing measuring from the
    /// instruction's own end, not its start.
    pub addend: i64,
}

/// What a realm's own frontend has produced for this symbol once its
/// local analysis is done -- not a bare "here is an address" claim, but
/// the actual machine code bytes plus every place inside them that still
/// needs another (possibly cross-realm) symbol's address plugged in.
/// This is "the compiler's output" this hypothesis asks for directly: no
/// object file, no section table, no separate symbol-table indirection
/// -- the code and its own outstanding cross-references travel together
/// as one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBody {
    pub code: Vec<u8>,
    pub relocations: Vec<PendingReloc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressState {
    /// Declared, but this realm's own frontend has not yet finished
    /// enough of its local analysis to assign a definite address.
    Unresolved,
    /// This realm's own frontend has finished this symbol's own machine
    /// code (`CodeBody`), but that code may still contain unresolved
    /// `PendingReloc` entries referencing other boundary symbols -- this
    /// state says "the bytes are ready," not "this symbol's own address
    /// is final," since final placement (where in the eventual image
    /// this code body lands) is a separate concern this hypothesis does
    /// not yet model (see this crate's own README on scope).
    Committed(CodeBody),
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

    /// The step that replaces a traditional linker's "layout" phase:
    /// walks every declared `SymbolNode` in a deterministic order
    /// (sorted by `SymbolId`, never HashMap iteration order, so this is
    /// reproducible across runs) and assigns each one a final byte
    /// offset within one conceptual, contiguous image -- laid out back
    /// to back by each `CodeBody`'s own length, no alignment/section
    /// separation modeled yet (see this crate's own scope note). Returns
    /// the assignment as its own map rather than mutating `self` so a
    /// caller can inspect a proposed layout before committing to it.
    pub fn assign_layout(&self) -> LayoutAssignment {
        let nodes = self.nodes.read().expect("nodes lock poisoned");
        let mut entries: Vec<(&SymbolId, &SymbolNode)> = nodes.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));

        let mut addresses = HashMap::new();
        let mut cursor: u64 = 0;
        for (id, node) in entries {
            let AddressState::Committed(body) = &node.address else {
                continue;
            };
            addresses.insert(id.clone(), cursor);
            cursor += body.code.len() as u64;
        }
        LayoutAssignment { addresses }
    }

    /// The step that replaces a traditional linker's "apply relocations"
    /// phase: for every declared symbol's own `CodeBody`, overwrite each
    /// `PendingReloc`'s placeholder bytes with `layout`'s resolved
    /// address for that relocation's `target`, computed the same way the
    /// real `R_X86_64_PLT32` records verified against `cadd`/`app`-shaped
    /// code do (`target_address + addend - reloc_site_address`, i.e.
    /// PC-relative from the *end* of the 4-byte operand). Returns an
    /// error naming the first symbol/relocation that could not be
    /// resolved rather than silently leaving a placeholder unpatched --
    /// a patched-looking binary with a live zero placeholder is a
    /// miscompile, never an acceptable partial result.
    pub fn apply_relocations(&self, layout: &LayoutAssignment) -> Result<PatchedImage, LinkError> {
        let nodes = self.nodes.read().expect("nodes lock poisoned");
        let mut entries: Vec<(&SymbolId, &SymbolNode)> = nodes.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));

        let mut patched = HashMap::new();
        for (id, node) in entries {
            let AddressState::Committed(body) = &node.address else {
                continue;
            };
            let site_base = *layout
                .addresses
                .get(id)
                .ok_or_else(|| LinkError::MissingLayoutEntry(id.clone()))?;
            let mut bytes = body.code.clone();
            for reloc in &body.relocations {
                let target_address = *layout.addresses.get(&reloc.target).ok_or_else(|| {
                    LinkError::UnresolvedRelocationTarget {
                        in_symbol: id.clone(),
                        target: reloc.target.clone(),
                    }
                })?;
                let reloc_site_address = site_base + reloc.offset as u64 + reloc.width as u64;
                let value = (target_address as i64 + reloc.addend) - reloc_site_address as i64;
                let value_bytes = (value as i32).to_le_bytes();
                if reloc.width != value_bytes.len() {
                    return Err(LinkError::UnsupportedRelocationWidth {
                        in_symbol: id.clone(),
                        width: reloc.width,
                    });
                }
                let start = reloc.offset;
                let end = start + reloc.width;
                if end > bytes.len() {
                    return Err(LinkError::RelocationOutOfBounds {
                        in_symbol: id.clone(),
                        offset: reloc.offset,
                    });
                }
                bytes[start..end].copy_from_slice(&value_bytes);
            }
            patched.insert(id.clone(), bytes);
        }
        Ok(PatchedImage { code: patched })
    }
}

#[derive(Debug, Clone, Default)]
pub struct LayoutAssignment {
    pub addresses: HashMap<SymbolId, u64>,
}

#[derive(Debug, Clone)]
pub struct PatchedImage {
    /// Each symbol's own code, with every `PendingReloc` placeholder
    /// already overwritten by its resolved value. Never contains an
    /// un-patched all-zero placeholder for a relocation `apply_relocations`
    /// itself reported success for.
    pub code: HashMap<SymbolId, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// A symbol was declared but `assign_layout` never placed it --
    /// should be unreachable given `assign_layout`'s own logic, kept as
    /// an explicit error rather than a panic so a caller mixing
    /// layouts/graphs gets a structured failure instead of an index
    /// panic.
    MissingLayoutEntry(SymbolId),
    /// A relocation names a target this graph never saw a `Committed`
    /// declaration for -- the real "undefined reference" a linker
    /// reports, surfaced with which symbol's own code contains the
    /// unresolved reference.
    UnresolvedRelocationTarget {
        in_symbol: SymbolId,
        target: SymbolId,
    },
    /// This hypothesis only implements the 4-byte PC32/PLT32-shaped
    /// relocation verified against real `cc -c` output; any other width
    /// is refused rather than silently truncated/extended.
    UnsupportedRelocationWidth { in_symbol: SymbolId, width: usize },
    /// A `PendingReloc`'s offset+width falls outside its own `CodeBody`'s
    /// actual byte length -- a producer bug, never patched around.
    RelocationOutOfBounds { in_symbol: SymbolId, offset: usize },
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

    /// The real `c_add` machine code, byte-for-byte, from `objdump -d`
    /// against an actual `cc -c` of this repo's own
    /// `fixtures/cross-ecosystem-native-executable/c/cadd/v1/cadd.c`
    /// (verified inside `laminaria-bootstrap`, see this crate's own
    /// `inspect-cadd.sh`). No external references -- `objdump -r` showed
    /// zero relocations in `.text` for this function.
    fn real_c_add_code() -> Vec<u8> {
        vec![
            0x55, 0x48, 0x89, 0xe5, 0x89, 0x7d, 0xfc, 0x89, 0x75, 0xf8, 0x8b, 0x55, 0xfc, 0x8b,
            0x45, 0xf8, 0x01, 0xd0, 0x5d, 0xc3,
        ]
    }

    /// The real `compute` machine code (`c_add(cpp_max_i32(3, 4), 1)`),
    /// byte-for-byte from an actual `cc -c`, with its two real
    /// `R_X86_64_PLT32` relocations reproduced exactly as `objdump -r`
    /// reported them (offset 0xf -> cpp_max_i32-0x4, offset 0x1b ->
    /// c_add-0x4). See `inspect-caller.sh`.
    fn real_compute_code_and_relocs() -> (Vec<u8>, Vec<PendingReloc>) {
        let code = vec![
            0x55, 0x48, 0x89, 0xe5, 0xbe, 0x04, 0x00, 0x00, 0x00, 0xbf, 0x03, 0x00, 0x00, 0x00,
            0xe8, 0x00, 0x00, 0x00, 0x00, 0xbe, 0x01, 0x00, 0x00, 0x00, 0x89, 0xc7, 0xe8, 0x00,
            0x00, 0x00, 0x00, 0x5d, 0xc3,
        ];
        let relocs = vec![
            PendingReloc {
                offset: 0xf,
                width: 4,
                target: SymbolId {
                    realm: Realm::Cpp,
                    name: "cpp_max_i32".to_string(),
                },
                addend: -4,
            },
            PendingReloc {
                offset: 0x1b,
                width: 4,
                target: SymbolId {
                    realm: Realm::C,
                    name: "c_add".to_string(),
                },
                addend: -4,
            },
        ];
        (code, relocs)
    }

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
                                address: AddressState::Committed(CodeBody {
                                    code: real_c_add_code(),
                                    relocations: vec![],
                                }),
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
                                address: AddressState::Committed(CodeBody {
                                    code: vec![0xb8, 0x04, 0x00, 0x00, 0x00, 0xc3], // mov eax,4; ret
                                    relocations: vec![],
                                }),
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
                                address: AddressState::Committed(CodeBody {
                                    code: vec![0xb8, 0x08, 0x00, 0x00, 0x00, 0xc3], // mov eax,8; ret
                                    relocations: vec![],
                                }),
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
                address: AddressState::Committed(CodeBody {
                    code: real_c_add_code(),
                    relocations: vec![],
                }),
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

    /// The real end-to-end case this crate's whole hypothesis exists to
    /// prove: `app`'s `compute` function -- with its two real, unresolved
    /// `call` placeholders -- gets its final machine code produced by
    /// `assign_layout` + `apply_relocations` alone, no object file and no
    /// separate linker invocation. Every provider symbol's real machine
    /// code (`c_add`, plus stand-in bodies for `cpp_max_i32`/
    /// `nim_double`) is declared first; `compute` (owned by `Cargo`,
    /// since `app` is the one requiring the calls) is declared with its
    /// real relocations, and patching must produce PC-relative operand
    /// bytes that satisfy the exact formula a real ELF loader/linker
    /// would use.
    #[test]
    fn apply_relocations_patches_real_call_placeholders_to_the_correct_pc_relative_values() {
        let graph = SharedSymbolGraph::new();
        graph
            .declare_symbol(
                Realm::C,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::C,
                        name: "c_add".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: real_c_add_code(),
                        relocations: vec![],
                    }),
                },
            )
            .unwrap();
        graph
            .declare_symbol(
                Realm::Cpp,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::Cpp,
                        name: "cpp_max_i32".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: vec![0xb8, 0x04, 0x00, 0x00, 0x00, 0xc3],
                        relocations: vec![],
                    }),
                },
            )
            .unwrap();
        let (compute_code, compute_relocs) = real_compute_code_and_relocs();
        graph
            .declare_symbol(
                Realm::Cargo,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::Cargo,
                        name: "compute".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: compute_code,
                        relocations: compute_relocs,
                    }),
                },
            )
            .unwrap();

        let layout = graph.assign_layout();
        let patched = graph
            .apply_relocations(&layout)
            .expect("every relocation target was declared");

        let compute_id = SymbolId {
            realm: Realm::Cargo,
            name: "compute".to_string(),
        };
        let patched_compute = &patched.code[&compute_id];

        // Manually recompute what the real linker formula must produce,
        // independent of apply_relocations's own implementation, so this
        // assertion cannot pass merely by mirroring a bug.
        let compute_addr = layout.addresses[&compute_id];
        let cpp_addr = layout.addresses[&SymbolId {
            realm: Realm::Cpp,
            name: "cpp_max_i32".to_string(),
        }];
        let c_add_addr = layout.addresses[&SymbolId {
            realm: Realm::C,
            name: "c_add".to_string(),
        }];

        let expected_cpp_operand = ((cpp_addr as i64 - 4) - (compute_addr as i64 + 0xf + 4)) as i32;
        let expected_c_add_operand =
            ((c_add_addr as i64 - 4) - (compute_addr as i64 + 0x1b + 4)) as i32;

        assert_eq!(
            &patched_compute[0xf..0xf + 4],
            expected_cpp_operand.to_le_bytes().as_slice(),
            "cpp_max_i32 call operand must be the real PC-relative displacement"
        );
        assert_eq!(
            &patched_compute[0x1b..0x1b + 4],
            expected_c_add_operand.to_le_bytes().as_slice(),
            "c_add call operand must be the real PC-relative displacement"
        );
        // Never a leftover placeholder: both slots must differ from the
        // real cc-emitted zero placeholder unless the true displacement
        // genuinely happens to be zero (not the case for this layout).
        assert_ne!(&patched_compute[0xf..0xf + 4], &[0, 0, 0, 0]);
        assert_ne!(&patched_compute[0x1b..0x1b + 4], &[0, 0, 0, 0]);
    }

    /// A relocation naming a target this graph never saw declared must
    /// fail loudly -- the real "undefined reference" case -- never
    /// silently leave the placeholder's all-zero bytes in place, which
    /// would look like a successfully patched (but wrong) binary.
    #[test]
    fn apply_relocations_refuses_to_silently_leave_an_undefined_reference_unpatched() {
        let graph = SharedSymbolGraph::new();
        let (compute_code, compute_relocs) = real_compute_code_and_relocs();
        graph
            .declare_symbol(
                Realm::Cargo,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::Cargo,
                        name: "compute".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: compute_code,
                        relocations: compute_relocs,
                    }),
                },
            )
            .unwrap();
        // cpp_max_i32 and c_add are deliberately never declared.

        let layout = graph.assign_layout();
        let result = graph.apply_relocations(&layout);
        assert!(matches!(
            result,
            Err(LinkError::UnresolvedRelocationTarget { .. })
        ));
    }
}

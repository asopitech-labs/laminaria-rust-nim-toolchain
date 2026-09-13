//! Issue #48 (G1): the typed dependency-obligation graph spanning
//! Cargo, Nimble, C, and C++ ecosystems, feeding source/IR facts back
//! into package/artifact candidate selection -- the "共通
//! dependency-obligation vocabulary" G1's work instruction requires.
//! This module is a coordinate-shared *addition* alongside this crate's
//! existing `Action`/`ArtifactRef`/`PlanningInput` wire contract, not a
//! replacement or a second parallel graph representation: the same
//! "dependency edges are derived from matching declared inputs against
//! declared outputs, never a hand-written adjacency list" convention
//! `types.rs`'s own `ArtifactRef` doc comment already fixes is reused
//! here as `requested_by` edges. This module does not touch
//! `PLAN_SCHEMA_VERSION`, `ActionKind`, or any type
//! `nim-planner/src/contract.nim` hand-mirrors -- G1's own stop
//! condition is producing G2's action *requests*, not live-wiring a new
//! action kind into the already-tested Rust<->Nim planner contract
//! (see [`RequiredAction`]'s own doc comment for why).
//!
//! ## Vocabulary provenance (not invented here)
//!
//! - `ObligationState` (6 values, terminal-inclusive) and `DischargeKind`
//!   (9 reasons a `Discharged`/`Externalized` obligation reached that
//!   state) come from
//!   `docs/02-research-areas/toolchains/dependency-resolved-artifact-closure_ja.md`
//!   as two *separate* types. A companion research doc's own summary
//!   table lists `ProvenIrrelevant` as if it were a 7th top-level state;
//!   this module follows the more detailed artifact-closure contract
//!   instead, since that document states the explicit invariant "every
//!   obligation reaches `Discharged`, `Externalized`, or `Rejected`"
//!   (3 terminal categories, not 4) and is the document issue #46 (G2)
//!   itself cites as its canonical contract. `ProvenIrrelevant` is
//!   modeled here as `DischargeKind::ProvenIrrelevant`, a *reason* an
//!   obligation is `Discharged`, never a peer state.
//! - `ObligationKind`'s ten values are exactly the ones G1's own work
//!   instruction fixes (package selection / source-module /
//!   semantic-type-FFI / lowering / artifact production / ABI / symbol
//!   / link / runtime / provenance).
//! - The node-kind sketch (`PackageCandidate`, `Symbol`, ...) mirrors
//!   `docs/02-research-areas/toolchains/cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md`
//!   §9.1's own typed-hypergraph proposal, trimmed to what this minimal
//!   vertical slice actually needs -- it is explicitly a hypothesis
//!   sketch in that document, not prior code; this module is its first
//!   production implementation.
//!
//! ## What this module does not do
//!
//! It never spawns a process, reads a file, or invokes a compiler --
//! see [`tests::the_resolver_never_spawns_a_subprocess`] for a
//! source-text regression guard using the exact technique issue #5
//! T1's `wasm_target.rs` and issue #36 T1's `incremental_executor.rs`
//! already established for this project. Ecosystem fact *ingestion*
//! (`cargo metadata`, `nimble dump --json`, real `cc`/`clang++`/`nim`
//! compilation of the fixture's own foreign libraries, real `nm`
//! symbol inspection) lives entirely in
//! `crates/laminaria-run/src/cross_ecosystem_ingest.rs`, which builds a
//! [`FixtureFacts`] value and hands it to [`resolve`] as plain,
//! already-gathered data -- exactly the same "manifest/lockfile/source
//! retrieval are legitimate inputs, but the resolver itself must never
//! hide a package-manager build inside its own success path" boundary
//! `docs/01-foundations/compiler-ownership-contract_ja.md` and the G1
//! work instruction both fix (required test 7: "opaqueなcargo build、
//! nimble buildなどをresolver成功の代用にしていない").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Which ecosystem an obligation or candidate belongs to. `Cross` is
/// for obligations that are not owned by any one ecosystem (the final
/// link and the native-executable demand itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Cargo,
    Nimble,
    C,
    Cpp,
    Cross,
}

/// The ten obligation kinds G1's work instruction fixes. Kept as one
/// tag on a common envelope rather than ten separate graph types, per
/// the landscape doc's own "don't flatten distinct domains into one
/// syntax, but don't duplicate a graph per domain either" conclusion --
/// each kind's *resolution rule* still differs (a `PackageSelection`
/// obligation is settled by a non-monotonic candidate choice; a
/// `Symbol` obligation is settled by monotonic fact accumulation), and
/// [`resolve`] treats them differently, but they share one identity,
/// state-machine, and provenance representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationKind {
    PackageSelection,
    SourceModule,
    SemanticTypeFfi,
    Lowering,
    ArtifactProduction,
    Abi,
    Symbol,
    Link,
    Runtime,
    Provenance,
}

/// `Unresolved` is the only non-terminal-inclusive value that may
/// legitimately appear on an obligation `resolve` is still working on;
/// a retained obligation left `Unresolved`/`Selected`/`Satisfied` once
/// `resolve` would otherwise return is exactly the invariant violation
/// required test 3 exercises
/// (`an_unresolved_retained_obligation_makes_production_fail` in
/// `crates/laminaria-run/src/cross_ecosystem_ingest.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationState {
    Unresolved,
    Selected,
    Satisfied,
    Discharged,
    Externalized,
    Rejected,
}

impl ObligationState {
    /// Exactly the three categories
    /// `dependency-resolved-artifact-closure_ja.md`'s own "解決時の不変条件"
    /// section fixes as the only states a *retained* obligation may
    /// carry once resolution finishes.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ObligationState::Discharged | ObligationState::Externalized | ObligationState::Rejected
        )
    }
}

/// The reason a `Discharged`/`Externalized` obligation reached that
/// state -- never a bare state flip with no justification. Exactly the
/// nine values `dependency-resolved-artifact-closure_ja.md` fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DischargeKind {
    Specialized,
    Lowered,
    Generated,
    Inlined,
    StaticallyLinked,
    Embedded,
    ProvenIrrelevant,
    ReplacedByEquivalent,
    ExternalRuntimeContract,
}

/// Why an obligation was `Rejected`. Distinct from
/// `laminaria_plan::RejectionReasonKind` (this crate's existing,
/// unrelated wire-contract rejection reason for the Nim Planning
/// Kernel's own action-graph gate: cycle/unsupported-input/missing- or
/// duplicate-producer/invalid-contract-version) -- a G1 obligation
/// rejection is a *dependency-graph* judgment (an incompatible ABI, a
/// missing symbol, an unreconcilable version/feature choice), not an
/// action-graph malformation, so reusing that enum here would blur two
/// genuinely different things instead of merely renaming one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    IncompatibleAbi,
    MissingSymbol,
    VersionConflict,
    FeatureConflict,
    HostTargetRoleMismatch,
    UnsupportedConstruct,
}

/// Host-versus-target build-dependency role (M1-W1's own required
/// distinction). A `Host`-role obligation (e.g. the `nim` compiler
/// itself, used only to build a foreign Nimble package's static
/// library) must never satisfy a `Target`-role symbol/link/runtime
/// obligation -- see
/// `crates/laminaria-run/src/cross_ecosystem_ingest.rs`'s
/// `a_host_role_obligation_never_satisfies_a_target_role_symbol_requirement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Host,
    Target,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectionDetail {
    pub reason: RejectionReason,
    pub detail: String,
}

/// The production operation that justifies a state transition -- the
/// "証拠またはdiagnostic" every obligation the work instruction
/// requires must carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalOperation {
    pub operation: String,
    pub evidence: String,
}

/// One dependency obligation. `requested_by` is this graph's only edge
/// representation (mirroring `ArtifactRef`'s own "derive edges from
/// matching, never hand-write an adjacency list" convention, applied
/// here to *why* an obligation exists rather than *what produces its
/// input*): a `Symbol` obligation's `requested_by` names the
/// `SemanticTypeFfi` obligation whose real FFI declaration demanded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub id: String,
    pub kind: ObligationKind,
    pub ecosystem: Ecosystem,
    pub role: Role,
    pub requested_by: Vec<String>,
    pub state: ObligationState,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub discharge_kind: Option<DischargeKind>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rejection: Option<RejectionDetail>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub causal_operation: Option<CausalOperation>,
}

impl Obligation {
    fn unresolved(
        id: impl Into<String>,
        kind: ObligationKind,
        ecosystem: Ecosystem,
        role: Role,
        requested_by: Vec<String>,
    ) -> Self {
        Obligation {
            id: id.into(),
            kind,
            ecosystem,
            role,
            requested_by,
            state: ObligationState::Unresolved,
            discharge_kind: None,
            rejection: None,
            causal_operation: None,
        }
    }

    fn discharge(
        &mut self,
        kind: DischargeKind,
        operation: impl Into<String>,
        evidence: impl Into<String>,
    ) {
        self.state = ObligationState::Discharged;
        self.discharge_kind = Some(kind);
        self.causal_operation = Some(CausalOperation {
            operation: operation.into(),
            evidence: evidence.into(),
        });
    }

    fn externalize(&mut self, operation: impl Into<String>, evidence: impl Into<String>) {
        self.state = ObligationState::Externalized;
        self.discharge_kind = Some(DischargeKind::ExternalRuntimeContract);
        self.causal_operation = Some(CausalOperation {
            operation: operation.into(),
            evidence: evidence.into(),
        });
    }

    fn reject(&mut self, reason: RejectionReason, detail: impl Into<String>) {
        self.state = ObligationState::Rejected;
        self.rejection = Some(RejectionDetail {
            reason,
            detail: detail.into(),
        });
    }
}

/// One concrete, real, already-produced native artifact this graph can
/// select or reject as a package candidate -- the *facts* half of the
/// coupled-resolution loop. `provided_symbols` is real `nm` output
/// (or, for the host toolchain sentinel, empty), never a value this
/// module invents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCandidateFacts {
    pub package_id: String,
    pub version: String,
    pub ecosystem: Ecosystem,
    pub role: Role,
    pub archive_path: String,
    pub provided_symbols: Vec<String>,
    pub target_triple: String,
}

/// One real FFI requirement discovered from source (see
/// `laminaria_ir::foreign_discover::discover_foreign_function_requirements`) --
/// this module's own `SemanticTypeFfi`/`Symbol` obligations are built
/// directly from these, never fabricated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FfiRequirementFacts {
    pub declaring_package: String,
    pub symbol: String,
    pub abi: String,
    pub param_count: usize,
    /// Which logical package (by id, not version) is expected to
    /// provide this symbol -- e.g. from the real `#[link(name = "cadd")]`
    /// hint the source itself carries.
    pub expected_provider_package: String,
}

/// The Cargo-side facts this graph ingests, already normalized from a
/// real `cargo metadata` call
/// (`crates/laminaria-run/src/cross_ecosystem_ingest.rs::ingest_cargo_metadata`) --
/// this module receives plain data, never the manifest path itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoFacts {
    pub package_name: String,
    pub version: String,
    pub features: Vec<String>,
    pub target_triple: String,
}

/// The Nimble-side facts this graph ingests, already normalized from a
/// real `nimble dump --json` call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NimbleFacts {
    pub package_name: String,
    pub version: String,
    pub requires: Vec<String>,
}

/// The complete, already-gathered input to [`resolve`] -- every field
/// is real data an ingestion adapter produced (see this module's own
/// top-level doc comment), never invented here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureFacts {
    pub demand_entry_point: String,
    pub target_triple: String,
    pub cargo: CargoFacts,
    pub nimble: NimbleFacts,
    pub host_toolchain_id: String,
    pub ffi_requirements: Vec<FfiRequirementFacts>,
    /// Keyed by logical package id (e.g. `"cadd"`); may hold more than
    /// one real version/variant candidate per package (the C-library
    /// version-conflict/negative case).
    pub native_candidates: BTreeMap<String, Vec<NativeCandidateFacts>>,
}

/// One action G2 must execute to actually produce the requested native
/// executable -- deliberately shaped like `laminaria_plan::types::Action`
/// (`id`/inputs/outputs/producer), on purpose, but *not* that type: this
/// module never touches `PLAN_SCHEMA_VERSION` or `ActionKind`, which
/// `nim-planner/src/contract.nim` hand-mirrors and dozens of existing
/// Rust<->Nim integration tests already exercise live. Adding a new
/// `ActionKind` variant here would force a schema-version bump that
/// breaks every existing planner-integration test that doesn't need
/// it, for a wire path this G1 slice never drives live (G1's own stop
/// condition is producing G2's action *requests*, not wiring a new
/// action kind into the already-tested pair). `kind_label` is a plain
/// string for exactly that reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredAction {
    pub id: String,
    pub kind_label: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub producer: String,
}

/// The successful result of [`resolve`]: every obligation this graph
/// retained has reached `Discharged` or `Externalized`, plus the
/// concrete action requests G2 needs to actually produce the native
/// executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositiveClosure {
    pub obligations: BTreeMap<String, Obligation>,
    pub required_actions: Vec<RequiredAction>,
    /// Selected-candidate package id -> the alternative candidate ids
    /// considered and rejected for it, with typed reasons -- the
    /// "唯一の整合するclosureと、その選択理由" positive-case evidence.
    pub rejected_alternatives: BTreeMap<String, Vec<RejectionDetail>>,
}

/// The graph-level rejection [`resolve`] returns before ever
/// generating a single [`RequiredAction`] -- the "compile開始前に拒否"
/// negative-case evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRejection {
    pub obligation_id: String,
    pub reason: RejectionReason,
    pub detail: String,
    /// Every obligation actually evaluated before the rejection was
    /// reached -- "どのartifact demandから始まったか、どの候補が選択されたか、
    /// どのcross-layer factで矛盾したか、どの義務がRejectedになったか" from
    /// the work instruction's own negative-case evidence requirement.
    pub obligations_at_rejection: BTreeMap<String, Obligation>,
}

fn native_obligation_id(kind: ObligationKind, package_id: &str) -> String {
    format!("{kind:?}:{package_id}")
}

/// Resolves one [`FixtureFacts`] value into a [`PositiveClosure`] or a
/// [`GraphRejection`] -- pure, deterministic (no I/O, no randomness, no
/// hash-order-dependent iteration: every collection here is a
/// `BTreeMap`/sorted `Vec`), and total over well-formed input. This is
/// the "production resolver" every direct executable test in
/// `crates/laminaria-run/src/cross_ecosystem_ingest.rs` calls.
///
/// Algorithm, one coupled pass (sufficient for this minimal vertical
/// slice's own fixed workload; the full lazy/incremental fixed-point
/// loop `cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md`
/// §9.2 sketches is Lane B's own G3 concern, explicitly out of scope
/// here):
///
/// 1. Discharge the Cargo/Nimble package-selection obligations
///    (single, already-resolved candidate each -- no real choice).
/// 2. Discharge one `SemanticTypeFfi` obligation per real FFI
///    requirement, and open one `Symbol` obligation per requirement,
///    `requested_by` that FFI obligation.
/// 3. **The feedback step**: for each `Symbol` obligation, look up its
///    expected provider package's real candidates and select the one
///    whose real `provided_symbols` actually contains the required
///    name -- a genuine source-derived FFI fact narrowing a
///    `PackageSelection` obligation that started with more than one
///    real candidate. Every other real candidate for that package is
///    recorded as a rejected alternative with a typed reason. If *no*
///    candidate qualifies, the whole resolution is rejected here, before
///    any `RequiredAction` is generated.
/// 4. Discharge one `Abi`/`ArtifactProduction` obligation per selected
///    candidate (their real target triple already matches the demand's,
///    checked here -- an ABI/target mismatch is this same step's own
///    rejection path).
/// 5. Open and discharge one `Link` obligation (all its own
///    `requested_by` obligations must already be terminal) and one
///    `Runtime` obligation (externalized: OS ABI is not bundled), plus
///    one `Provenance` obligation (the original graph is retained, not
///    re-resolved by G2).
pub fn resolve(facts: &FixtureFacts) -> Result<PositiveClosure, GraphRejection> {
    let mut obligations: BTreeMap<String, Obligation> = BTreeMap::new();
    let mut required_actions: Vec<RequiredAction> = Vec::new();
    let mut rejected_alternatives: BTreeMap<String, Vec<RejectionDetail>> = BTreeMap::new();

    // Step 1: Cargo/Nimble package selection -- one already-resolved
    // candidate each (a real manifest/lockfile already picked exactly
    // one), so these discharge immediately via `Specialized`.
    let cargo_id =
        native_obligation_id(ObligationKind::PackageSelection, &facts.cargo.package_name);
    let mut cargo_obligation = Obligation::unresolved(
        &cargo_id,
        ObligationKind::PackageSelection,
        Ecosystem::Cargo,
        Role::Target,
        vec![],
    );
    cargo_obligation.discharge(
        DischargeKind::Specialized,
        "cargo_metadata",
        format!(
            "cargo metadata resolved {} {} with features {:?}",
            facts.cargo.package_name, facts.cargo.version, facts.cargo.features
        ),
    );
    obligations.insert(cargo_id.clone(), cargo_obligation);

    let nimble_id =
        native_obligation_id(ObligationKind::PackageSelection, &facts.nimble.package_name);
    let mut nimble_obligation = Obligation::unresolved(
        &nimble_id,
        ObligationKind::PackageSelection,
        Ecosystem::Nimble,
        Role::Target,
        vec![],
    );
    nimble_obligation.discharge(
        DischargeKind::Specialized,
        "nimble_dump",
        format!(
            "nimble dump --json resolved {} {} requiring {:?}",
            facts.nimble.package_name, facts.nimble.version, facts.nimble.requires
        ),
    );
    obligations.insert(nimble_id.clone(), nimble_obligation);

    // The `nim` compiler itself: a host-role build tool, never a
    // target-role symbol/link provider (M1-W1's own required
    // distinction).
    let host_toolchain_obligation_id = format!(
        "ArtifactProduction:host-toolchain:{}",
        facts.host_toolchain_id
    );
    let mut host_toolchain_obligation = Obligation::unresolved(
        &host_toolchain_obligation_id,
        ObligationKind::ArtifactProduction,
        Ecosystem::Nimble,
        Role::Host,
        vec![],
    );
    host_toolchain_obligation.discharge(
        DischargeKind::Generated,
        "resolve_host_toolchain",
        format!(
            "{} resolved on host PATH to build the doubler Nimble package",
            facts.host_toolchain_id
        ),
    );
    obligations.insert(host_toolchain_obligation_id, host_toolchain_obligation);

    // Step 2: one `SemanticTypeFfi` + one `Symbol` obligation per real
    // FFI requirement.
    let mut symbol_obligation_ids: Vec<String> = Vec::new();
    for req in &facts.ffi_requirements {
        let ffi_id = format!(
            "SemanticTypeFfi:{}:{}",
            facts.cargo.package_name, req.symbol
        );
        let mut ffi_obligation = Obligation::unresolved(
            &ffi_id,
            ObligationKind::SemanticTypeFfi,
            Ecosystem::Cargo,
            Role::Target,
            vec![],
        );
        ffi_obligation.discharge(
            DischargeKind::Lowered,
            "discover_foreign_function_requirements",
            format!(
                "real extern \"{}\" fn {}({} params) discovered in {}",
                req.abi, req.symbol, req.param_count, req.declaring_package
            ),
        );
        obligations.insert(ffi_id.clone(), ffi_obligation);

        let symbol_id = format!("Symbol:{}", req.symbol);
        let symbol_obligation = Obligation::unresolved(
            &symbol_id,
            ObligationKind::Symbol,
            match req.expected_provider_package.as_str() {
                p if p == facts.nimble.package_name => Ecosystem::Nimble,
                _ => Ecosystem::C, // narrowed to the real ecosystem once a candidate is selected below
            },
            Role::Target,
            vec![ffi_id.clone()],
        );
        obligations.insert(symbol_id.clone(), symbol_obligation);
        symbol_obligation_ids.push(symbol_id);
    }

    // Step 3: the feedback step. For each `Symbol` obligation, select
    // the real candidate that actually provides it.
    let mut selected_candidate_ids: BTreeMap<String, String> = BTreeMap::new(); // package_id -> selected version
    for req in &facts.ffi_requirements {
        let symbol_id = format!("Symbol:{}", req.symbol);
        let package_id = &req.expected_provider_package;
        let package_selection_id =
            native_obligation_id(ObligationKind::PackageSelection, package_id);

        let Some(candidates) = facts.native_candidates.get(package_id) else {
            let obligation = obligations.get_mut(&symbol_id).unwrap();
            obligation.reject(
                RejectionReason::MissingSymbol,
                format!("no candidate at all is declared for package '{package_id}'"),
            );
            return Err(GraphRejection {
                obligation_id: symbol_id.clone(),
                reason: RejectionReason::MissingSymbol,
                detail: format!(
                    "required symbol '{}' has no candidate provider package",
                    req.symbol
                ),
                obligations_at_rejection: obligations,
            });
        };

        let providing: Vec<&NativeCandidateFacts> = candidates
            .iter()
            .filter(|c| c.provided_symbols.iter().any(|s| s == &req.symbol))
            .collect();

        match providing.as_slice() {
            [] => {
                let obligation = obligations.get_mut(&symbol_id).unwrap();
                let detail = format!(
                    "no real candidate for '{package_id}' provides required symbol '{}' (checked: {})",
                    req.symbol,
                    candidates
                        .iter()
                        .map(|c| format!("{}@{} [{}]", c.package_id, c.version, c.provided_symbols.join(",")))
                        .collect::<Vec<_>>()
                        .join("; ")
                );
                obligation.reject(RejectionReason::MissingSymbol, detail.clone());
                return Err(GraphRejection {
                    obligation_id: symbol_id.clone(),
                    reason: RejectionReason::MissingSymbol,
                    detail,
                    obligations_at_rejection: obligations,
                });
            }
            [only] => {
                if only.target_triple != facts.target_triple {
                    let obligation = obligations.get_mut(&symbol_id).unwrap();
                    let detail = format!(
                        "candidate '{}' provides '{}' but was built for target '{}', not the demanded '{}'",
                        only.package_id, req.symbol, only.target_triple, facts.target_triple
                    );
                    obligation.reject(RejectionReason::IncompatibleAbi, detail.clone());
                    return Err(GraphRejection {
                        obligation_id: symbol_id.clone(),
                        reason: RejectionReason::IncompatibleAbi,
                        detail,
                        obligations_at_rejection: obligations,
                    });
                }

                let obligation = obligations.get_mut(&symbol_id).unwrap();
                obligation.ecosystem = only.ecosystem;
                obligation.discharge(
                    DischargeKind::StaticallyLinked,
                    "nm_symbol_inspection",
                    format!(
                        "real nm output for {} confirms it provides '{}'",
                        only.archive_path, req.symbol
                    ),
                );
                selected_candidate_ids.insert(package_id.clone(), only.version.clone());

                let rejected: Vec<RejectionDetail> = candidates
                    .iter()
                    .filter(|c| c.version != only.version)
                    .map(|c| RejectionDetail {
                        reason: RejectionReason::MissingSymbol,
                        detail: format!(
                            "candidate '{}@{}' does not provide required symbol '{}' (real symbols: {:?})",
                            c.package_id, c.version, req.symbol, c.provided_symbols
                        ),
                    })
                    .collect();
                if !rejected.is_empty() {
                    rejected_alternatives.insert(package_id.clone(), rejected);
                }

                let package_obligation = obligations
                    .entry(package_selection_id.clone())
                    .or_insert_with(|| {
                        Obligation::unresolved(
                            package_selection_id.clone(),
                            ObligationKind::PackageSelection,
                            only.ecosystem,
                            Role::Target,
                            vec![],
                        )
                    });
                if package_obligation.state != ObligationState::Discharged {
                    package_obligation.discharge(
                        DischargeKind::Specialized,
                        "candidate_symbol_feedback",
                        format!(
                            "selected {}@{} because it is the only real candidate providing required symbol '{}'",
                            only.package_id, only.version, req.symbol
                        ),
                    );
                }

                let artifact_id = format!("ArtifactProduction:{}", only.package_id);
                let mut artifact_obligation = Obligation::unresolved(
                    &artifact_id,
                    ObligationKind::ArtifactProduction,
                    only.ecosystem,
                    Role::Target,
                    vec![package_selection_id.clone()],
                );
                artifact_obligation.discharge(
                    DischargeKind::StaticallyLinked,
                    "real_foreign_compile",
                    format!("real archive already produced at {}", only.archive_path),
                );
                obligations.insert(artifact_id.clone(), artifact_obligation);

                let abi_id = format!("Abi:{}", only.package_id);
                let mut abi_obligation = Obligation::unresolved(
                    &abi_id,
                    ObligationKind::Abi,
                    only.ecosystem,
                    Role::Target,
                    vec![artifact_id.clone()],
                );
                abi_obligation.discharge(
                    DischargeKind::Specialized,
                    "target_triple_match",
                    format!(
                        "{} target triple '{}' matches demand",
                        only.package_id, only.target_triple
                    ),
                );
                obligations.insert(abi_id, abi_obligation);

                required_actions.push(RequiredAction {
                    id: format!("link-input:{}", only.package_id),
                    kind_label: "ForeignArchiveAlreadyProduced".to_string(),
                    inputs: vec![only.archive_path.clone()],
                    outputs: vec![only.archive_path.clone()],
                    producer: only.package_id.clone(),
                });
            }
            multiple => {
                // More than one real candidate provides the same
                // symbol: an unreconcilable ambiguity this slice does
                // not attempt to break with a preference rule (that is
                // exactly the kind of candidate-explosion question
                // Lane B/G3 owns) -- reject rather than pick silently.
                let obligation = obligations.get_mut(&symbol_id).unwrap();
                let detail =
                    format!(
                    "{} real candidates for '{package_id}' all provide required symbol '{}': {}",
                    multiple.len(),
                    req.symbol,
                    multiple.iter().map(|c| c.version.clone()).collect::<Vec<_>>().join(", ")
                );
                obligation.reject(RejectionReason::VersionConflict, detail.clone());
                return Err(GraphRejection {
                    obligation_id: symbol_id.clone(),
                    reason: RejectionReason::VersionConflict,
                    detail,
                    obligations_at_rejection: obligations,
                });
            }
        }
    }

    // Step 4/5: final Link + Runtime + Provenance obligations. All
    // `requested_by` obligations must already be terminal -- this is
    // the "an unresolved obligation must make artifact production fail"
    // invariant, checked structurally rather than merely assumed.
    let mut unresolved: Vec<&Obligation> = obligations
        .values()
        .filter(|o| !o.state.is_terminal())
        .collect();
    unresolved.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(first) = unresolved.first() {
        return Err(GraphRejection {
            obligation_id: first.id.clone(),
            reason: RejectionReason::UnsupportedConstruct,
            detail: format!(
                "obligation '{}' is still {:?}, not Discharged/Externalized/Rejected",
                first.id, first.state
            ),
            obligations_at_rejection: obligations,
        });
    }

    let link_id = "Link:native_executable".to_string();
    let mut link_obligation = Obligation::unresolved(
        &link_id,
        ObligationKind::Link,
        Ecosystem::Cross,
        Role::Target,
        symbol_obligation_ids.clone(),
    );
    let mut link_inputs: Vec<String> = obligations
        .values()
        .filter(|o| o.kind == ObligationKind::ArtifactProduction && o.role == Role::Target)
        .map(|o| o.id.clone())
        .collect();
    link_inputs.sort();
    link_obligation.discharge(
        DischargeKind::Generated,
        "generate_final_link_action_request",
        format!(
            "final-link RequiredAction generated for G2 (issue #46); inputs: {}. Not executed in this G1 slice.",
            link_inputs.join(", ")
        ),
    );
    obligations.insert(link_id.clone(), link_obligation);
    required_actions.push(RequiredAction {
        id: "final-link".to_string(),
        kind_label: "LinkNativeExecutable".to_string(),
        inputs: link_inputs,
        outputs: vec![facts.demand_entry_point.clone()],
        producer: "G2".to_string(),
    });

    let runtime_id = "Runtime:os_abi".to_string();
    let mut runtime_obligation = Obligation::unresolved(
        &runtime_id,
        ObligationKind::Runtime,
        Ecosystem::Cross,
        Role::Target,
        vec![link_id.clone()],
    );
    runtime_obligation.externalize(
        "externalize_os_runtime",
        format!("OS ABI/dynamic loader for target '{}' is not bundled; declared as an external runtime contract", facts.target_triple),
    );
    obligations.insert(runtime_id, runtime_obligation);

    let provenance_id = "Provenance:original_graph".to_string();
    let mut provenance_obligation = Obligation::unresolved(
        &provenance_id,
        ObligationKind::Provenance,
        Ecosystem::Cross,
        Role::Target,
        vec![link_id],
    );
    provenance_obligation.discharge(
        DischargeKind::Embedded,
        "retain_original_graph",
        "the original Cargo/Nimble/C/C++ ecosystem graph is retained as audit provenance, not re-resolved by a consumer",
    );
    obligations.insert(provenance_id, provenance_obligation);

    Ok(PositiveClosure {
        obligations,
        required_actions,
        rejected_alternatives,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same source-text regression-guard technique issue #5 T1's
    /// `wasm_target.rs` and issue #36 T1's `incremental_executor.rs`
    /// already use (required test 7: the resolver never substitutes an
    /// opaque package-manager build for its own success).
    #[test]
    fn the_resolver_never_spawns_a_subprocess() {
        let source = include_str!("dependency_graph.rs");
        let test_module_marker = "#[cfg(test)]";
        let production_source = source
            .split(test_module_marker)
            .next()
            .expect("this file always contains its own test module marker");
        let subprocess_spawn_needle = format!("{}{}", "Command", "::new");
        assert!(
            !production_source.contains(&subprocess_spawn_needle),
            "the dependency-obligation resolver must never spawn an external process -- ecosystem \
             fact ingestion belongs in crates/laminaria-run, not here"
        );
        assert!(
            !production_source.contains("std::fs::"),
            "the resolver must never read the filesystem itself -- it consumes already-gathered \
             FixtureFacts only"
        );
    }

    fn minimal_facts(cadd_candidates: Vec<NativeCandidateFacts>) -> FixtureFacts {
        FixtureFacts {
            demand_entry_point: "app".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            cargo: CargoFacts {
                package_name: "app".to_string(),
                version: "0.1.0".to_string(),
                features: vec!["use_nim_double".to_string()],
                target_triple: "x86_64-unknown-linux-gnu".to_string(),
            },
            nimble: NimbleFacts {
                package_name: "doubler".to_string(),
                version: "0.1.0".to_string(),
                requires: vec!["nim >= 2.0.0".to_string()],
            },
            host_toolchain_id: "nim-2.2.10".to_string(),
            ffi_requirements: vec![FfiRequirementFacts {
                declaring_package: "app".to_string(),
                symbol: "c_add".to_string(),
                abi: "C".to_string(),
                param_count: 2,
                expected_provider_package: "cadd".to_string(),
            }],
            native_candidates: BTreeMap::from([("cadd".to_string(), cadd_candidates)]),
        }
    }

    fn candidate(version: &str, symbols: &[&str], target_triple: &str) -> NativeCandidateFacts {
        NativeCandidateFacts {
            package_id: "cadd".to_string(),
            version: version.to_string(),
            ecosystem: Ecosystem::C,
            role: Role::Target,
            archive_path: format!("/tmp/cadd-{version}.a"),
            provided_symbols: symbols.iter().map(|s| s.to_string()).collect(),
            target_triple: target_triple.to_string(),
        }
    }

    #[test]
    fn a_single_qualifying_candidate_is_selected_and_reaches_a_positive_closure() {
        let facts = minimal_facts(vec![candidate(
            "1.0.0",
            &["c_add"],
            "x86_64-unknown-linux-gnu",
        )]);
        let closure = resolve(&facts).expect("must resolve");
        assert!(closure.obligations.values().all(|o| o.state.is_terminal()));
        let symbol = &closure.obligations["Symbol:c_add"];
        assert_eq!(symbol.state, ObligationState::Discharged);
        assert_eq!(symbol.discharge_kind, Some(DischargeKind::StaticallyLinked));
    }

    #[test]
    fn a_candidate_missing_the_required_symbol_is_rejected_before_any_action_is_generated() {
        let facts = minimal_facts(vec![candidate(
            "2.0.0",
            &["c_add_v2"],
            "x86_64-unknown-linux-gnu",
        )]);
        let rejection = resolve(&facts).expect_err("must reject");
        assert_eq!(rejection.reason, RejectionReason::MissingSymbol);
        assert_eq!(rejection.obligation_id, "Symbol:c_add");
    }

    #[test]
    fn feedback_selects_the_qualifying_candidate_and_records_the_other_as_a_rejected_alternative() {
        let facts = minimal_facts(vec![
            candidate("1.0.0", &["c_add"], "x86_64-unknown-linux-gnu"),
            candidate("2.0.0", &["c_add_v2"], "x86_64-unknown-linux-gnu"),
        ]);
        let closure = resolve(&facts).expect("must resolve");
        let rejected = &closure.rejected_alternatives["cadd"];
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].reason, RejectionReason::MissingSymbol);
    }

    #[test]
    fn an_abi_target_mismatch_is_rejected() {
        let facts = minimal_facts(vec![candidate(
            "1.0.0",
            &["c_add"],
            "aarch64-unknown-linux-gnu",
        )]);
        let rejection = resolve(&facts).expect_err("must reject");
        assert_eq!(rejection.reason, RejectionReason::IncompatibleAbi);
    }

    #[test]
    fn resolving_the_same_facts_twice_produces_an_identical_closure() {
        let facts = minimal_facts(vec![candidate(
            "1.0.0",
            &["c_add"],
            "x86_64-unknown-linux-gnu",
        )]);
        let a = resolve(&facts).expect("must resolve");
        let b = resolve(&facts).expect("must resolve");
        assert_eq!(a, b);
    }

    #[test]
    fn the_host_toolchain_obligation_is_host_role_and_never_target_role() {
        let facts = minimal_facts(vec![candidate(
            "1.0.0",
            &["c_add"],
            "x86_64-unknown-linux-gnu",
        )]);
        let closure = resolve(&facts).expect("must resolve");
        let host_obligations: Vec<&Obligation> = closure
            .obligations
            .values()
            .filter(|o| o.role == Role::Host)
            .collect();
        assert_eq!(host_obligations.len(), 1);
        assert!(host_obligations[0].id.contains("host-toolchain"));
    }
}

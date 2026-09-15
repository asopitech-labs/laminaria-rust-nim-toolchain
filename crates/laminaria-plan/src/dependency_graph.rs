//! Issue #48 (G1): the typed dependency-obligation graph spanning
//! Cargo, Nimble, C, and C++ ecosystems, feeding source-derived
//! symbol/ABI facts back into package/candidate selection.
//!
//! ## G1's exact boundary (issue #48's authoritative recovery spec)
//!
//! G1 is a **read, normalize, resolve, and plan** stage. [`resolve`]
//! never builds anything: it consumes an already-gathered
//! [`DependencyResolutionInput`] (real `cargo metadata`/`nimble dump
//! --json` facts, plus symbols *declared* in real C/C++ headers and
//! Nim `exportc` pragmas -- never symbols observed by compiling a
//! candidate and inspecting the result with `nm`) and produces either
//! a [`PositiveClosure`] -- a fully connected obligation graph plus the
//! exact typed [`RequiredAction`] chain issue #46 (G2) must execute to
//! actually produce the demanded native executable -- or a
//! [`GraphRejection`] identifying exactly which obligation could not be
//! satisfied and why, before a single [`RequiredAction`] is ever
//! generated.
//!
//! A successful G1 result never marks a production artifact, link,
//! runtime, or provenance obligation `Discharged`/`Externalized`:
//! those remain `Satisfied`, each carrying the [`RequiredAction`] id
//! that names how G2 will discharge it later. Generating that action
//! request is not discharge evidence -- see
//! [`ObligationState`]'s own doc comment.
//!
//! This module never spawns a process or reads a file -- ecosystem
//! fact *ingestion* (`cargo metadata`, `nimble dump --json`, and the
//! pure-text C-header/Nim-`exportc` readers) lives entirely in
//! `crates/laminaria-run/src/cross_ecosystem_ingest.rs` and
//! `crates/laminaria-ir`, which hand this module plain, already-read
//! data.
//!
//! No type or branch here is named for, or special-cases, this
//! project's own fixture (`app`/`doubler`/`cadd`/`cppmax`): a package
//! is treated as a "provider" purely because some
//! [`FfiRequirementFacts::expected_provider_package`] names it, and as
//! the "root/consumer" package otherwise -- a structural distinction,
//! not a name check.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Which ecosystem an obligation, source, or candidate belongs to.
/// `Cross` is for obligations owned by no single ecosystem (the final
/// link, runtime preflight, provenance publication, and the top-level
/// native-executable demand itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Cargo,
    Nimble,
    C,
    Cpp,
    Cross,
}

/// The obligation kinds the positive graph must contain and connect
/// (issue #48's own "Required obligation graph" list, items 1-13,
/// grouped onto one kind each where the work instruction itself groups
/// them -- e.g. items 3-6, the four package selections, all carry
/// `PackageSelection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationKind {
    /// Item 1: the root native-executable demand.
    NativeExecutableDemand,
    /// Items 3-6: one obligation per chosen (or rejected) package
    /// candidate, keyed by `package@version`.
    PackageSelection,
    /// Item 7: one obligation per Rust/Nim/C/C-header/C++/C++-header
    /// input actually read.
    SourceModule,
    /// Item 8: one obligation per real FFI symbol requirement
    /// discovered from source (`c_add`, `nim_double`, `cpp_max_i32`).
    SemanticFfi,
    /// Item 9: one obligation per Rust/Nim package LAMINARIA itself
    /// must lower (never C/C++, which are foreign/opaque toolchains
    /// here).
    Lowering,
    /// Item 10: one obligation per foreign boundary, checking the
    /// selected provider's ABI/target triple against the demand's.
    AbiTarget,
    /// Item 11: one obligation per declared artifact output (Rust
    /// object, Nim archive, C object+archive, C++ adapter
    /// object+archive, final executable).
    ArtifactProduction,
    /// Item 12 (symbol half): one obligation per required symbol,
    /// declaring that a selected candidate's declared export actually
    /// matches it.
    Symbol,
    /// Item 12 (link-order half): the declared ordering of every
    /// retained artifact-production output at final-link time.
    LinkOrder,
    /// Item 2: the final-link obligation itself.
    FinalLink,
    /// Item 13 (runtime half): the runtime-contract preflight.
    Runtime,
    /// Item 13 (provenance half): provenance publication for the
    /// exact produced executable identity.
    Provenance,
}

/// `Unresolved` is the only state that may legitimately still be on an
/// obligation while [`resolve`] is working; any obligation `resolve`
/// would otherwise return still `Unresolved` is exactly the "an
/// unresolved reachable obligation prevents plan publication"
/// invariant `unresolved_reachable_obligation_prevents_plan_publication`
/// exercises.
///
/// `Discharged` and `Externalized` are reserved for issue #46 (G2),
/// which actually executes a [`RequiredAction`] and can then transition
/// the obligation it names. **G1 itself never constructs an `Obligation`
/// in either state** -- see
/// `g1_never_discharges_a_production_action_obligation`.
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
    /// The three states a G1 result may actually leave an obligation
    /// in: a chosen candidate is `Selected`, a verified fact/plan-only
    /// obligation is `Satisfied`, an incompatible alternative is
    /// `Rejected`. `Unresolved` reaching here is the defect required
    /// test 7 exercises; `Discharged`/`Externalized` reaching here is
    /// the defect required test `g1_never_discharges_a_production_action_obligation`
    /// exercises.
    pub fn is_g1_terminal(self) -> bool {
        matches!(
            self,
            ObligationState::Selected | ObligationState::Satisfied | ObligationState::Rejected
        )
    }

    pub fn is_rejected(self) -> bool {
        matches!(self, ObligationState::Rejected)
    }

    /// Whether an obligation currently in this state is eligible for
    /// issue #46 (G2)'s own lifecycle transition
    /// ([`PositiveClosure::discharge_obligation`]/
    /// [`PositiveClosure::externalize_obligation`]): a chosen candidate
    /// (`Selected`) or a verified fact/plan-only obligation
    /// (`Satisfied`) may still be discharged or externalized once real
    /// production evidence exists. `Unresolved` (a G1 defect),
    /// `Rejected` (a permanently closed alternative), and an
    /// already-`Discharged`/`Externalized` obligation (no
    /// re-discharge) are not.
    pub fn is_g2_dischargeable(self) -> bool {
        matches!(self, ObligationState::Selected | ObligationState::Satisfied)
    }
}

/// The reason a `Discharged`/`Externalized` obligation reached that
/// state. Reserved vocabulary for issue #46 (G2) -- see
/// [`ObligationState`]'s own doc comment for why G1 never constructs
/// one of these; kept here (rather than deferred entirely to G2's own
/// crate) so both stages share one vocabulary instead of two
/// independently-invented ones. `ProvenIrrelevant` in particular is
/// explicitly unused by G1 (issue #48's own fixed decision).
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

/// Why an obligation was `Rejected`.
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

/// Host-versus-target build-dependency role. A `Host`-role candidate
/// (a build tool, never a target-role artifact provider) must never
/// satisfy a `Target`-role symbol/artifact obligation -- see
/// `host_tool_actions_cannot_satisfy_target_artifact_obligations`.
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

/// One dependency obligation. `depends_on` is this graph's only edge
/// representation: `A.depends_on = [B]` means "A cannot be satisfied
/// until B is satisfied" (issue #48's own fixed edge semantics). A
/// disconnected *retained* obligation (one [`resolve`] did not reject)
/// that the demand cannot reach by repeatedly following `depends_on` is
/// an error -- see `native_demand_reaches_every_retained_obligation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub id: String,
    pub kind: ObligationKind,
    pub ecosystem: Ecosystem,
    pub role: Role,
    pub depends_on: Vec<String>,
    pub state: ObligationState,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub discharge_kind: Option<DischargeKind>,
    /// The [`RequiredAction`] id that will discharge this obligation in
    /// G2 -- present exactly on the artifact-production, final-link,
    /// runtime, and provenance obligations issue #48 requires to stay
    /// `Satisfied` (never `Discharged`) in G1's own output.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub required_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rejection: Option<RejectionDetail>,
    /// The production fact or reasoning that justifies this
    /// obligation's current state -- never a bare state flip with no
    /// justification.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub evidence: Option<String>,
}

impl Obligation {
    fn unresolved(
        id: impl Into<String>,
        kind: ObligationKind,
        ecosystem: Ecosystem,
        role: Role,
        depends_on: Vec<String>,
    ) -> Self {
        Obligation {
            id: id.into(),
            kind,
            ecosystem,
            role,
            depends_on,
            state: ObligationState::Unresolved,
            discharge_kind: None,
            required_action: None,
            rejection: None,
            evidence: None,
        }
    }

    fn select(&mut self, evidence: impl Into<String>) {
        self.state = ObligationState::Selected;
        self.evidence = Some(evidence.into());
    }

    fn satisfy(&mut self, evidence: impl Into<String>) {
        self.state = ObligationState::Satisfied;
        self.evidence = Some(evidence.into());
    }

    fn satisfy_with_action(&mut self, action_id: impl Into<String>, evidence: impl Into<String>) {
        self.state = ObligationState::Satisfied;
        self.required_action = Some(action_id.into());
        self.evidence = Some(evidence.into());
    }

    fn reject(&mut self, reason: RejectionReason, detail: impl Into<String>) {
        self.state = ObligationState::Rejected;
        self.rejection = Some(RejectionDetail {
            reason,
            detail: detail.into(),
        });
    }

    /// Issue #46 (G2)'s own shared lifecycle transition: real
    /// production evidence, tied to the exact [`RequiredAction`] that
    /// produced it, moves this obligation to `Discharged`. `pub(crate)`
    /// only -- outside callers must go through
    /// [`PositiveClosure::discharge_obligation`], which validates the
    /// action actually names this obligation and is structurally
    /// allowed to discharge its kind before ever calling this.
    pub(crate) fn discharge(
        &mut self,
        action_id: impl Into<String>,
        kind: DischargeKind,
        evidence: impl Into<String>,
    ) {
        self.state = ObligationState::Discharged;
        self.required_action = Some(action_id.into());
        self.discharge_kind = Some(kind);
        self.evidence = Some(evidence.into());
    }

    /// Issue #46 (G2)'s own shared lifecycle transition for an
    /// obligation satisfied by something outside LAMINARIA's own
    /// executed action chain (an OS/library/driver runtime contract G2
    /// verifies but never itself produces) -- `pub(crate)` only, see
    /// [`discharge`](Self::discharge)'s own doc comment for why.
    pub(crate) fn externalize(&mut self, kind: DischargeKind, evidence: impl Into<String>) {
        self.state = ObligationState::Externalized;
        self.discharge_kind = Some(kind);
        self.evidence = Some(evidence.into());
    }
}

/// One real source/module/header LAMINARIA's own ingestion actually
/// read -- never invented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceModuleFacts {
    /// A stable, repo-relative path identity (never a temp-directory
    /// path) -- e.g. `"app/src/main.rs"`.
    pub id: String,
    pub ecosystem: Ecosystem,
    pub package_id: String,
}

/// One real FFI symbol a source *imports* (requires from elsewhere) --
/// e.g. discovered by `laminaria_ir::foreign_discover` scanning real
/// Rust `extern "C"` blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FfiRequirementFacts {
    pub declaring_source: String,
    pub symbol: String,
    pub abi: String,
    pub param_count: usize,
    /// The real declared return type text (e.g. `"i32"`, `"()"`) --
    /// part of the real signature a candidate's declared export must
    /// match, not merely its symbol name.
    pub return_type: String,
    /// Which logical package is expected to provide this symbol (e.g.
    /// from a real `#[link(name = "cadd")]` hint).
    pub expected_provider_package: String,
}

/// One real FFI symbol a source *declares as exported* -- discovered
/// from a real C/C++ header prototype
/// (`laminaria_ir::c_header_discover`) or a real Nim `{.exportc.}`
/// pragma (`laminaria_ir::nim_export_discover`). Never derived from
/// compiling the candidate and inspecting the result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FfiExportFacts {
    pub declaring_source: String,
    pub symbol: String,
    pub abi: String,
    pub param_count: usize,
    /// The real declared return type text -- see
    /// [`FfiRequirementFacts::return_type`].
    pub return_type: String,
}

/// Canonicalizes a small, explicit set of primitive C-ABI type
/// spellings that differ only by which language's reader produced them
/// (Rust's `i32`, C's `int`, Nim's `cint` are the exact same 32-bit C
/// ABI integer; Rust's `()`, C's `void`, and Nim's absent return type
/// are the exact same "returns nothing"). Deliberately narrow: this is
/// not a general Rust/C/Nim type-system unification, only the handful
/// of primitive spellings this fixture's own real cross-language
/// boundary actually uses. An unrecognized spelling is returned
/// unchanged (compared as exact text), so an unknown type can never be
/// silently treated as equivalent to something it wasn't verified
/// against.
fn canonical_c_abi_type(raw: &str) -> &str {
    match raw.trim() {
        "i32" | "int" | "cint" | "c_int" => "i32",
        "()" | "void" | "" => "void",
        "i8" | "int8_t" | "cchar" | "char" => "i8",
        "i64" | "long long" | "clonglong" | "int64_t" => "i64",
        "f32" | "float" | "cfloat" => "f32",
        "f64" | "double" | "cdouble" => "f64",
        other => other,
    }
}

/// Whether a real declared export actually satisfies a real required
/// import -- symbol name alone is not enough: arity, return type, and
/// ABI must all match, so a same-named provider with an incompatible
/// signature is never silently accepted (issue #48's own G1 follow-up,
/// Checkpoint A/B: "同名symbolでも引数、戻り値、ABIが異なるproviderは拒否される").
/// Return-type comparison goes through [`canonical_c_abi_type`] since
/// the required side and the declared side are read by different
/// language-specific readers that spell the same ABI type differently.
fn export_satisfies_requirement(export: &FfiExportFacts, req: &FfiRequirementFacts) -> bool {
    export.symbol == req.symbol
        && export.param_count == req.param_count
        && canonical_c_abi_type(&export.return_type) == canonical_c_abi_type(&req.return_type)
        && export.abi == req.abi
}

/// One real package candidate: sources and declared outputs only --
/// **never** an already-produced archive path or `nm` output. Matching
/// a candidate against a required symbol means checking
/// `declared_exports`, not inspecting a compiled artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageCandidateFacts {
    pub ecosystem: Ecosystem,
    pub package_id: String,
    pub version: String,
    pub role: Role,
    pub target_triple: String,
    /// [`SourceModuleFacts::id`] values this candidate's sources
    /// belong to.
    pub sources: Vec<String>,
    pub declared_exports: Vec<FfiExportFacts>,
    /// Generic declared constraints (a real Cargo feature, a real
    /// Nimble `requires` line, ...), carried as evidence text only --
    /// never branched on by package name here.
    pub declared_constraints: Vec<String>,
}

/// One real requirement that LAMINARIA itself must lower a package's
/// own source (Rust or Nim only -- C/C++ are foreign/opaque toolchains
/// this graph never lowers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoweringRequirementFacts {
    pub package_id: String,
    pub source_id: String,
    pub description: String,
}

/// The ABI/target-triple constraint at one real foreign boundary
/// (one per required FFI symbol).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiConstraintFacts {
    pub boundary_symbol: String,
    pub abi: String,
    pub target_triple: String,
}

/// Which kind of artifact a [`ArtifactOutputFacts`] entry declares.
/// Maps 1:1 onto [`RequiredActionKind`]'s own compile/archive/link
/// vocabulary, but stays a *declared output identity* here, never an
/// action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactOutputKind {
    RustObject,
    NimStaticLibrary,
    CObject,
    CStaticArchive,
    CppAdapterObject,
    CppStaticArchive,
    NativeExecutable,
}

/// One artifact output a real manifest/source graph declares LAMINARIA
/// will eventually need to produce -- an identity and a kind, never an
/// already-produced path.
///
/// Issue #65 (B-H3, carried-forward item 1): `source_ids` names which
/// [`SourceModuleFacts::id`] entries this one output is compiled from.
/// Empty means "every source belonging to `package_id`" (a whole-package
/// unity-build output, this type's only shape before issue #65 -- every
/// existing caller that never set this field keeps that exact behavior,
/// verified by `find_outputs_falls_back_to_whole_package_when_source_ids_is_empty`).
/// A non-empty `source_ids` declares a *sub-candidate* output: one
/// [`ArtifactOutputKind::CObject`]/[`ArtifactOutputKind::CppAdapterObject`]
/// entry per independently-compilable translation-unit member (e.g. one
/// of `bcm.c`'s 98 independently-compilable unity-build members, issue
/// #64) rather than one output covering the whole package's sources at
/// once. This field only ever names *already-known* source ids an
/// ingestion adapter declares as separately compilable -- it never
/// triggers LAMINARIA to parse or split C/C++ source itself (this
/// project's own non-goal: no LAMINARIA-owned C compiler/body parser).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactOutputFacts {
    pub id: String,
    pub package_id: String,
    pub kind: ArtifactOutputKind,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

/// One real runtime contract the produced executable will need at
/// execution time (e.g. the OS ABI/dynamic loader for the target
/// triple) -- never bundled by G1/G2, only preflighted and published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRequirementFacts {
    pub target_triple: String,
    pub description: String,
}

/// The complete, already-gathered input to [`resolve`]. Every field is
/// real data an ingestion adapter produced
/// (`crates/laminaria-run/src/cross_ecosystem_ingest.rs`) -- collections
/// throughout, never one hard-coded Cargo record and one hard-coded
/// Nimble record (issue #48's own required production input model).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyResolutionInput {
    pub demand_entry_point: String,
    pub target_triple: String,
    pub host_toolchain_id: String,
    pub sources: Vec<SourceModuleFacts>,
    pub package_candidates: Vec<PackageCandidateFacts>,
    pub ffi_requirements: Vec<FfiRequirementFacts>,
    pub lowering_requirements: Vec<LoweringRequirementFacts>,
    pub abi_constraints: Vec<AbiConstraintFacts>,
    pub declared_outputs: Vec<ArtifactOutputFacts>,
    pub runtime_requirements: Vec<RuntimeRequirementFacts>,
}

/// The eight operations G1 may ever require of G2 (issue #48's own
/// fixed, exhaustive list). Deliberately not an extension of
/// `laminaria_plan::types::ActionKind` -- see this module's own
/// top-of-file doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredActionKind {
    CompileRustObject,
    CompileNimStaticLibrary,
    CompileCObject,
    CompileCppAdapterObject,
    ArchiveStaticLibrary,
    LinkNativeExecutable,
    PreflightRuntimeContract,
    PublishProvenance,
}

/// One action G2 must execute to actually produce the requested native
/// executable. Every field issue #48 requires is present: a stable
/// identity, its [`RequiredActionKind`], target/role, a
/// producer/toolchain requirement, input/output identities, the
/// obligation identities it will discharge, and its own dependencies on
/// earlier action identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredAction {
    pub id: String,
    pub kind: RequiredActionKind,
    pub target_triple: String,
    pub role: Role,
    pub toolchain: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub discharges: Vec<String>,
    pub depends_on: Vec<String>,
}

/// The successful result of [`resolve`]: every retained obligation has
/// reached a G1-terminal state (`Selected`/`Satisfied`/`Rejected` --
/// never `Discharged`/`Externalized`), reachable from the native
/// executable demand, plus the complete typed action chain G2 needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositiveClosure {
    pub obligations: BTreeMap<String, Obligation>,
    pub required_actions: Vec<RequiredAction>,
    /// Selected package id -> the alternative candidates considered and
    /// rejected for it, with typed reasons.
    pub rejected_alternatives: BTreeMap<String, Vec<RejectionDetail>>,
}

/// Which [`ObligationKind`]s a given [`RequiredActionKind`] is actually
/// allowed to discharge -- a structural constraint, not a per-fixture
/// rule (e.g. a compile action can never legitimately discharge a
/// `Runtime` obligation). The canonical home for this mapping: both
/// `plan_integrity`'s structural closure verification and this
/// module's own [`PositiveClosure::discharge_obligation`] enforce it,
/// so it is defined exactly once.
pub(crate) fn discharge_kind_allowed(
    action_kind: RequiredActionKind,
    obligation_kind: ObligationKind,
) -> bool {
    use ObligationKind::*;
    use RequiredActionKind::*;
    match action_kind {
        CompileRustObject | CompileNimStaticLibrary | CompileCObject | CompileCppAdapterObject => {
            matches!(obligation_kind, ArtifactProduction)
        }
        ArchiveStaticLibrary => matches!(obligation_kind, ArtifactProduction),
        LinkNativeExecutable => {
            matches!(obligation_kind, FinalLink | LinkOrder | ArtifactProduction)
        }
        PreflightRuntimeContract => matches!(obligation_kind, Runtime),
        PublishProvenance => matches!(obligation_kind, Provenance),
    }
}

/// Why [`PositiveClosure::discharge_obligation`] or
/// [`PositiveClosure::externalize_obligation`] refused a transition --
/// never a bare `Err(())`, always the exact obligation/action identity
/// and the concrete rule it violates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleViolation {
    UnknownObligation(String),
    UnknownAction(String),
    ActionDoesNotDischargeThisObligation {
        action_id: String,
        obligation_id: String,
    },
    ActionKindCannotDischargeObligationKind {
        action_kind: RequiredActionKind,
        obligation_kind: ObligationKind,
    },
    ObligationNotDischargeable {
        obligation_id: String,
        state: ObligationState,
    },
}

impl std::fmt::Display for LifecycleViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleViolation::UnknownObligation(id) => {
                write!(f, "unknown obligation '{id}'")
            }
            LifecycleViolation::UnknownAction(id) => {
                write!(f, "unknown required action '{id}'")
            }
            LifecycleViolation::ActionDoesNotDischargeThisObligation {
                action_id,
                obligation_id,
            } => write!(
                f,
                "action '{action_id}' does not name obligation '{obligation_id}' in its own discharges list"
            ),
            LifecycleViolation::ActionKindCannotDischargeObligationKind {
                action_kind,
                obligation_kind,
            } => write!(
                f,
                "action kind {action_kind:?} cannot discharge obligation kind {obligation_kind:?}"
            ),
            LifecycleViolation::ObligationNotDischargeable { obligation_id, state } => write!(
                f,
                "obligation '{obligation_id}' is in state {state:?}, not Selected/Satisfied -- not eligible for discharge/externalization"
            ),
        }
    }
}

impl std::error::Error for LifecycleViolation {}

impl PositiveClosure {
    /// Issue #46 (G2)'s own shared obligation-lifecycle transition:
    /// moves a `Selected`/`Satisfied` obligation to `Discharged`,
    /// justified by a real, already-included [`RequiredAction`] that
    /// names this exact obligation in its own `discharges` list.
    /// Refuses structurally (never silently) when: the obligation or
    /// action id is unknown, the action does not actually discharge
    /// this obligation, the action's kind is not one
    /// [`discharge_kind_allowed`] permits for this obligation's kind,
    /// or the obligation is not currently `Selected`/`Satisfied`
    /// (already `Discharged`/`Externalized`, still `Unresolved`, or
    /// `Rejected`). Generating a `RequiredAction` is not itself
    /// discharge evidence -- the caller must supply real evidence from
    /// having actually executed that action.
    pub fn discharge_obligation(
        &mut self,
        obligation_id: &str,
        action_id: &str,
        kind: DischargeKind,
        evidence: impl Into<String>,
    ) -> Result<(), LifecycleViolation> {
        let obligation_kind = self
            .obligations
            .get(obligation_id)
            .ok_or_else(|| LifecycleViolation::UnknownObligation(obligation_id.to_string()))?
            .kind;
        let action = self
            .required_actions
            .iter()
            .find(|a| a.id == action_id)
            .ok_or_else(|| LifecycleViolation::UnknownAction(action_id.to_string()))?;
        if !action.discharges.iter().any(|d| d == obligation_id) {
            return Err(LifecycleViolation::ActionDoesNotDischargeThisObligation {
                action_id: action_id.to_string(),
                obligation_id: obligation_id.to_string(),
            });
        }
        if !discharge_kind_allowed(action.kind, obligation_kind) {
            return Err(
                LifecycleViolation::ActionKindCannotDischargeObligationKind {
                    action_kind: action.kind,
                    obligation_kind,
                },
            );
        }
        let obligation = self
            .obligations
            .get_mut(obligation_id)
            .expect("presence already checked above");
        if !obligation.state.is_g2_dischargeable() {
            return Err(LifecycleViolation::ObligationNotDischargeable {
                obligation_id: obligation_id.to_string(),
                state: obligation.state,
            });
        }
        obligation.discharge(action_id, kind, evidence);
        Ok(())
    }

    /// Issue #46 (G2)'s own shared obligation-lifecycle transition for
    /// an obligation satisfied by something outside LAMINARIA's own
    /// executed action chain (an OS/library/driver runtime contract G2
    /// verifies but never itself produces) -- always
    /// `DischargeKind::ExternalRuntimeContract`, and never tied to a
    /// `RequiredAction` (there may be none: G2 does not execute an
    /// action to make the platform's own dynamic loader exist). Same
    /// `Selected`/`Satisfied`-only precondition as
    /// [`discharge_obligation`](Self::discharge_obligation).
    pub fn externalize_obligation(
        &mut self,
        obligation_id: &str,
        evidence: impl Into<String>,
    ) -> Result<(), LifecycleViolation> {
        let obligation = self
            .obligations
            .get_mut(obligation_id)
            .ok_or_else(|| LifecycleViolation::UnknownObligation(obligation_id.to_string()))?;
        if !obligation.state.is_g2_dischargeable() {
            return Err(LifecycleViolation::ObligationNotDischargeable {
                obligation_id: obligation_id.to_string(),
                state: obligation.state,
            });
        }
        obligation.externalize(DischargeKind::ExternalRuntimeContract, evidence);
        Ok(())
    }
}

/// The graph-level rejection [`resolve`] returns before ever generating
/// a single [`RequiredAction`] -- structurally incapable of carrying a
/// plan (this type has no `required_actions` field at all).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRejection {
    pub obligation_id: String,
    pub reason: RejectionReason,
    pub detail: String,
    pub demand_entry_point: String,
    /// Every candidate identity actually considered for the rejected
    /// obligation (e.g. `"cadd@2.0.0 (source: c/cadd/v2/cadd.h)"`).
    pub considered_candidates: Vec<String>,
    /// Every obligation actually evaluated before the rejection was
    /// reached.
    pub obligations_at_rejection: BTreeMap<String, Obligation>,
}

fn package_selection_id(package_id: &str, version: &str) -> String {
    format!("PackageSelection:{package_id}@{version}")
}

fn source_module_id(source_id: &str) -> String {
    format!("SourceModule:{source_id}")
}

fn find_source<'a>(
    input: &'a DependencyResolutionInput,
    source_id: &str,
) -> Option<&'a SourceModuleFacts> {
    input.sources.iter().find(|s| s.id == source_id)
}

/// Every declared output for `(package_id, kind)`, in `declared_outputs`
/// order. Issue #65 (B-H3): more than one entry means the package
/// declares sub-candidate (translation-unit-granularity) outputs of this
/// kind rather than one whole-package output -- see
/// [`ArtifactOutputFacts::source_ids`]'s own doc comment.
fn find_outputs<'a>(
    input: &'a DependencyResolutionInput,
    package_id: &str,
    kind: ArtifactOutputKind,
) -> Vec<&'a ArtifactOutputFacts> {
    input
        .declared_outputs
        .iter()
        .filter(|o| o.package_id == package_id && o.kind == kind)
        .collect()
}

/// The single-output convenience wrapper every pre-issue-#65 call site
/// used, kept so a `(package_id, kind)` pair that still only ever
/// declares at most one output (every kind except sub-candidate-capable
/// C/C++ object outputs) does not need to change at all.
fn find_output<'a>(
    input: &'a DependencyResolutionInput,
    package_id: &str,
    kind: ArtifactOutputKind,
) -> Option<&'a ArtifactOutputFacts> {
    find_outputs(input, package_id, kind).into_iter().next()
}

/// Maps a provider candidate's ecosystem onto the one
/// [`ArtifactOutputKind`] a foreign provider in that ecosystem
/// declares -- a structural mapping, never a package-name branch.
fn provider_artifact_kind(ecosystem: Ecosystem) -> ArtifactOutputKind {
    match ecosystem {
        Ecosystem::Nimble => ArtifactOutputKind::NimStaticLibrary,
        Ecosystem::C => ArtifactOutputKind::CStaticArchive,
        Ecosystem::Cpp => ArtifactOutputKind::CppStaticArchive,
        Ecosystem::Cargo | Ecosystem::Cross => {
            unreachable!("a foreign symbol provider is never Cargo/Cross-ecosystem")
        }
    }
}

/// Registers every [`SourceModuleFacts`] entry as a `Satisfied`
/// `SourceModule` obligation -- a leaf in the graph (no `depends_on`).
fn register_source_obligations(
    input: &DependencyResolutionInput,
    obligations: &mut BTreeMap<String, Obligation>,
) {
    for source in &input.sources {
        let id = source_module_id(&source.id);
        let mut obligation = Obligation::unresolved(
            &id,
            ObligationKind::SourceModule,
            source.ecosystem,
            Role::Target,
            vec![],
        );
        obligation.satisfy(format!("read {}", source.id));
        obligations.insert(id, obligation);
    }
}

/// Registers every [`LoweringRequirementFacts`] entry as a `Satisfied`
/// `Lowering` obligation depending on its own source.
fn register_lowering_obligations(
    input: &DependencyResolutionInput,
    obligations: &mut BTreeMap<String, Obligation>,
) {
    for lowering in &input.lowering_requirements {
        let source = find_source(input, &lowering.source_id);
        let ecosystem = source.map(|s| s.ecosystem).unwrap_or(Ecosystem::Cross);
        let id = format!("Lowering:{}", lowering.package_id);
        let mut obligation = Obligation::unresolved(
            &id,
            ObligationKind::Lowering,
            ecosystem,
            Role::Target,
            vec![source_module_id(&lowering.source_id)],
        );
        obligation.satisfy(lowering.description.clone());
        obligations.insert(id, obligation);
    }
}

/// Trivially resolves every "root" package candidate -- one whose
/// package id never appears as an [`FfiRequirementFacts::expected_provider_package`]
/// (i.e. it is not itself feedback-selected; it is the consumer whose
/// own demand starts the graph). A single real candidate is already
/// the only choice a real manifest/lockfile made; more than one with no
/// feedback signal to break the tie is an unresolvable ambiguity.
// `GraphRejection` is deliberately rich (it carries the full
// obligations-at-rejection map so a negative-case caller never needs a
// second query to explain a rejection) -- boxing it would only move the
// allocation, not remove it, so it is accepted here rather than
// papered over.
#[allow(clippy::result_large_err)]
fn resolve_root_packages(
    input: &DependencyResolutionInput,
    provider_package_ids: &std::collections::BTreeSet<&str>,
    obligations: &mut BTreeMap<String, Obligation>,
) -> Result<(), GraphRejection> {
    let mut by_package: BTreeMap<&str, Vec<&PackageCandidateFacts>> = BTreeMap::new();
    for candidate in &input.package_candidates {
        if !provider_package_ids.contains(candidate.package_id.as_str()) {
            by_package
                .entry(&candidate.package_id)
                .or_default()
                .push(candidate);
        }
    }
    for (package_id, candidates) in by_package {
        let sources: Vec<String> = candidates
            .iter()
            .flat_map(|c| c.sources.iter().map(|s| source_module_id(s)))
            .collect();
        match candidates.as_slice() {
            [only] => {
                let id = package_selection_id(package_id, &only.version);
                let mut obligation = Obligation::unresolved(
                    &id,
                    ObligationKind::PackageSelection,
                    only.ecosystem,
                    only.role,
                    sources,
                );
                obligation.select(format!(
                    "the only real candidate for '{package_id}' (declared constraints: {:?})",
                    only.declared_constraints
                ));
                obligations.insert(id, obligation);
            }
            multiple => {
                let id = package_selection_id(package_id, "ambiguous");
                let mut obligation = Obligation::unresolved(
                    &id,
                    ObligationKind::PackageSelection,
                    multiple[0].ecosystem,
                    multiple[0].role,
                    sources,
                );
                let detail = format!(
                    "{} real candidates for root package '{package_id}' with no feedback signal to select between them",
                    multiple.len()
                );
                obligation.reject(RejectionReason::VersionConflict, detail.clone());
                obligations.insert(id.clone(), obligation);
                return Err(GraphRejection {
                    obligation_id: id,
                    reason: RejectionReason::VersionConflict,
                    detail,
                    demand_entry_point: input.demand_entry_point.clone(),
                    considered_candidates: multiple
                        .iter()
                        .map(|c| format!("{}@{}", c.package_id, c.version))
                        .collect(),
                    obligations_at_rejection: obligations.clone(),
                });
            }
        }
    }
    Ok(())
}

/// The feedback step: for one real FFI requirement, selects the real
/// candidate whose *declared* export actually matches, rejects every
/// other real candidate for that package as an alternative, and
/// registers the `SemanticFfi`/`AbiTarget`/`Symbol`/`ArtifactProduction`
/// obligations the selection implies.
#[allow(clippy::too_many_arguments, clippy::result_large_err)]
fn resolve_provider_for_requirement(
    input: &DependencyResolutionInput,
    req: &FfiRequirementFacts,
    obligations: &mut BTreeMap<String, Obligation>,
    rejected_alternatives: &mut BTreeMap<String, Vec<RejectionDetail>>,
    provider_artifact_obligations: &mut BTreeMap<String, String>,
) -> Result<(), GraphRejection> {
    let package_id = &req.expected_provider_package;
    // The declared ABI/target-triple constraint for this exact foreign
    // boundary, if the ingestion adapter recorded one; falling back to
    // the demand's own target triple keeps this total over minimal
    // hand-constructed test inputs that don't bother populating
    // `abi_constraints`.
    let expected_target_triple = input
        .abi_constraints
        .iter()
        .find(|c| c.boundary_symbol == req.symbol)
        .map(|c| c.target_triple.as_str())
        .unwrap_or(input.target_triple.as_str());
    let ffi_id = format!("SemanticFfi:{}", req.symbol);
    let mut ffi_obligation = Obligation::unresolved(
        &ffi_id,
        ObligationKind::SemanticFfi,
        find_source(input, &req.declaring_source)
            .map(|s| s.ecosystem)
            .unwrap_or(Ecosystem::Cross),
        Role::Target,
        vec![source_module_id(&req.declaring_source)],
    );
    ffi_obligation.satisfy(format!(
        "required by {}: extern \"{}\" fn {}({} params)",
        req.declaring_source, req.abi, req.symbol, req.param_count
    ));
    obligations.insert(ffi_id.clone(), ffi_obligation);

    let candidates: Vec<&PackageCandidateFacts> = input
        .package_candidates
        .iter()
        .filter(|c| c.package_id == *package_id)
        .collect();

    let considered_candidates = || -> Vec<String> {
        candidates
            .iter()
            .flat_map(|c| {
                c.declared_exports.iter().map(move |e| {
                    format!(
                        "{}@{} declares '{}' (params={}, returns={:?}, abi={}, source: {})",
                        c.package_id,
                        c.version,
                        e.symbol,
                        e.param_count,
                        e.return_type,
                        e.abi,
                        e.declaring_source
                    )
                })
            })
            .collect()
    };

    if candidates.is_empty() {
        let symbol_id = format!("Symbol:{}", req.symbol);
        let mut symbol_obligation = Obligation::unresolved(
            &symbol_id,
            ObligationKind::Symbol,
            Ecosystem::Cross,
            Role::Target,
            vec![ffi_id],
        );
        let detail = format!("no candidate at all is declared for package '{package_id}'");
        symbol_obligation.reject(RejectionReason::MissingSymbol, detail.clone());
        obligations.insert(symbol_id.clone(), symbol_obligation);
        return Err(GraphRejection {
            obligation_id: symbol_id,
            reason: RejectionReason::MissingSymbol,
            detail,
            demand_entry_point: input.demand_entry_point.clone(),
            considered_candidates: vec![],
            obligations_at_rejection: obligations.clone(),
        });
    }

    let target_matching: Vec<&PackageCandidateFacts> = candidates
        .iter()
        .filter(|c| {
            c.role == Role::Target
                && c.declared_exports
                    .iter()
                    .any(|e| export_satisfies_requirement(e, req))
        })
        .copied()
        .collect();
    let host_matching: Vec<&PackageCandidateFacts> = candidates
        .iter()
        .filter(|c| {
            c.role == Role::Host
                && c.declared_exports
                    .iter()
                    .any(|e| export_satisfies_requirement(e, req))
        })
        .copied()
        .collect();

    if target_matching.is_empty() && !host_matching.is_empty() {
        let symbol_id = format!("Symbol:{}", req.symbol);
        let mut symbol_obligation = Obligation::unresolved(
            &symbol_id,
            ObligationKind::Symbol,
            Ecosystem::Cross,
            Role::Target,
            vec![ffi_id],
        );
        let detail = format!(
            "candidate(s) {} declare required symbol '{}' but only in a Host-role capacity, which can never satisfy a Target-role symbol obligation",
            host_matching
                .iter()
                .map(|c| format!("{}@{}", c.package_id, c.version))
                .collect::<Vec<_>>()
                .join(", "),
            req.symbol
        );
        symbol_obligation.reject(RejectionReason::HostTargetRoleMismatch, detail.clone());
        obligations.insert(symbol_id.clone(), symbol_obligation);
        return Err(GraphRejection {
            obligation_id: symbol_id,
            reason: RejectionReason::HostTargetRoleMismatch,
            detail,
            demand_entry_point: input.demand_entry_point.clone(),
            considered_candidates: considered_candidates(),
            obligations_at_rejection: obligations.clone(),
        });
    }

    match target_matching.as_slice() {
        [] => {
            let symbol_id = format!("Symbol:{}", req.symbol);
            // Distinguish "no candidate declares this symbol name at
            // all" from "a candidate declares the right name but an
            // incompatible signature" -- same-named-but-mismatched is
            // exactly the "同名symbolでも引数、戻り値、ABIが異なるproviderは
            //拒否される" case the G1 follow-up requires be diagnosed,
            // not conflated with a bare missing-symbol report.
            let same_name_mismatches: Vec<String> = candidates
                .iter()
                .flat_map(|c| {
                    c.declared_exports
                        .iter()
                        .filter(|e| e.symbol == req.symbol)
                        .map(move |e| {
                            format!(
                                "{}@{} declares '{}' with params={} (required {}), returns={:?} (required {:?}), abi={} (required {})",
                                c.package_id, c.version, e.symbol, e.param_count, req.param_count,
                                e.return_type, req.return_type, e.abi, req.abi
                            )
                        })
                })
                .collect();
            let declared: Vec<String> = candidates
                .iter()
                .flat_map(|c| c.declared_exports.iter().map(|e| e.symbol.clone()))
                .collect();
            let (reason, detail) = if same_name_mismatches.is_empty() {
                (
                    RejectionReason::MissingSymbol,
                    format!(
                        "required symbol '{}' has no matching declared export among '{package_id}' candidates (declared instead: {declared:?})",
                        req.symbol
                    ),
                )
            } else {
                (
                    RejectionReason::IncompatibleAbi,
                    format!(
                        "required symbol '{}' (params={}, returns={:?}, abi={}) has no candidate declaring a matching signature: {}",
                        req.symbol, req.param_count, req.return_type, req.abi,
                        same_name_mismatches.join("; ")
                    ),
                )
            };
            let mut symbol_obligation = Obligation::unresolved(
                &symbol_id,
                ObligationKind::Symbol,
                Ecosystem::Cross,
                Role::Target,
                vec![ffi_id],
            );
            symbol_obligation.reject(reason, detail.clone());
            obligations.insert(symbol_id.clone(), symbol_obligation);
            Err(GraphRejection {
                obligation_id: symbol_id,
                reason,
                detail,
                demand_entry_point: input.demand_entry_point.clone(),
                considered_candidates: considered_candidates(),
                obligations_at_rejection: obligations.clone(),
            })
        }
        [selected] => {
            if selected.target_triple != expected_target_triple {
                let symbol_id = format!("Symbol:{}", req.symbol);
                let mut symbol_obligation = Obligation::unresolved(
                    &symbol_id,
                    ObligationKind::Symbol,
                    Ecosystem::Cross,
                    Role::Target,
                    vec![ffi_id],
                );
                let detail = format!(
                    "candidate '{}@{}' declares '{}' but targets '{}', not the demanded '{}'",
                    selected.package_id,
                    selected.version,
                    req.symbol,
                    selected.target_triple,
                    expected_target_triple
                );
                symbol_obligation.reject(RejectionReason::IncompatibleAbi, detail.clone());
                obligations.insert(symbol_id.clone(), symbol_obligation);
                return Err(GraphRejection {
                    obligation_id: symbol_id,
                    reason: RejectionReason::IncompatibleAbi,
                    detail,
                    demand_entry_point: input.demand_entry_point.clone(),
                    considered_candidates: considered_candidates(),
                    obligations_at_rejection: obligations.clone(),
                });
            }

            let selection_id = package_selection_id(&selected.package_id, &selected.version);
            let selection_sources: Vec<String> = selected
                .sources
                .iter()
                .map(|s| source_module_id(s))
                .collect();
            let mut selection_obligation = Obligation::unresolved(
                &selection_id,
                ObligationKind::PackageSelection,
                selected.ecosystem,
                selected.role,
                selection_sources,
            );
            selection_obligation.select(format!(
                "selected because it is the only real candidate declaring required symbol '{}'",
                req.symbol
            ));
            obligations.insert(selection_id.clone(), selection_obligation);

            for other in &candidates {
                if other.version == selected.version {
                    continue;
                }
                let other_id = package_selection_id(&other.package_id, &other.version);
                let other_sources: Vec<String> =
                    other.sources.iter().map(|s| source_module_id(s)).collect();
                let mut other_obligation = Obligation::unresolved(
                    &other_id,
                    ObligationKind::PackageSelection,
                    other.ecosystem,
                    other.role,
                    other_sources,
                );
                let same_name_export = other
                    .declared_exports
                    .iter()
                    .find(|e| e.symbol == req.symbol);
                let detail = match same_name_export {
                    Some(e) => format!(
                        "'{}@{}' declares '{}' but with an incompatible signature (params={}, returns={:?}, abi={} vs required params={}, returns={:?}, abi={})",
                        other.package_id, other.version, req.symbol,
                        e.param_count, e.return_type, e.abi,
                        req.param_count, req.return_type, req.abi
                    ),
                    None => format!(
                        "'{}@{}' does not declare required symbol '{}' (declares: {:?})",
                        other.package_id,
                        other.version,
                        req.symbol,
                        other
                            .declared_exports
                            .iter()
                            .map(|e| &e.symbol)
                            .collect::<Vec<_>>()
                    ),
                };
                other_obligation.reject(RejectionReason::MissingSymbol, detail.clone());
                obligations.insert(other_id, other_obligation);
                rejected_alternatives
                    .entry(package_id.clone())
                    .or_default()
                    .push(RejectionDetail {
                        reason: RejectionReason::MissingSymbol,
                        detail,
                    });
            }

            let abi_id = format!("AbiTarget:{}", req.symbol);
            let mut abi_obligation = Obligation::unresolved(
                &abi_id,
                ObligationKind::AbiTarget,
                selected.ecosystem,
                Role::Target,
                vec![ffi_id, selection_id.clone()],
            );
            abi_obligation.satisfy(format!(
                "'{}@{}' target triple '{}' matches the declared boundary constraint '{}'",
                selected.package_id,
                selected.version,
                selected.target_triple,
                expected_target_triple
            ));
            obligations.insert(abi_id.clone(), abi_obligation);

            let symbol_id = format!("Symbol:{}", req.symbol);
            let mut symbol_obligation = Obligation::unresolved(
                &symbol_id,
                ObligationKind::Symbol,
                selected.ecosystem,
                Role::Target,
                vec![abi_id],
            );
            symbol_obligation.satisfy(format!(
                "'{}@{}' declares required symbol '{}'",
                selected.package_id, selected.version, req.symbol
            ));
            obligations.insert(symbol_id, symbol_obligation);

            let artifact_kind = provider_artifact_kind(selected.ecosystem);
            if let Some(output) = find_output(input, &selected.package_id, artifact_kind) {
                provider_artifact_obligations
                    .entry(selected.package_id.clone())
                    .or_insert_with(|| {
                        // A Nimble provider is still real Nim source
                        // LAMINARIA itself lowers (item 9); a C/C++
                        // provider is a foreign/opaque toolchain with no
                        // `Lowering` obligation of its own.
                        let lowering_id = format!("Lowering:{}", selected.package_id);
                        let mut depends_on = vec![selection_id];
                        if obligations.contains_key(&lowering_id) {
                            depends_on.push(lowering_id);
                        }
                        let artifact_id = format!("ArtifactProduction:{}", output.id);
                        let mut artifact_obligation = Obligation::unresolved(
                            &artifact_id,
                            ObligationKind::ArtifactProduction,
                            selected.ecosystem,
                            Role::Target,
                            depends_on,
                        );
                        // `required_action` is filled in once the action
                        // chain for this candidate is built, below.
                        artifact_obligation.satisfy(format!(
                            "declared output '{}' for selected candidate '{}@{}'",
                            output.id, selected.package_id, selected.version
                        ));
                        obligations.insert(artifact_id.clone(), artifact_obligation);
                        artifact_id
                    });
            }

            Ok(())
        }
        multiple => {
            let symbol_id = format!("Symbol:{}", req.symbol);
            let mut symbol_obligation = Obligation::unresolved(
                &symbol_id,
                ObligationKind::Symbol,
                Ecosystem::Cross,
                Role::Target,
                vec![ffi_id],
            );
            let detail = format!(
                "{} real candidates for '{package_id}' all declare required symbol '{}': {}",
                multiple.len(),
                req.symbol,
                multiple
                    .iter()
                    .map(|c| c.version.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            symbol_obligation.reject(RejectionReason::VersionConflict, detail.clone());
            obligations.insert(symbol_id.clone(), symbol_obligation);
            Err(GraphRejection {
                obligation_id: symbol_id,
                reason: RejectionReason::VersionConflict,
                detail,
                demand_entry_point: input.demand_entry_point.clone(),
                considered_candidates: considered_candidates(),
                obligations_at_rejection: obligations.clone(),
            })
        }
    }
}

/// Issue #47 (G3, Lane B) E0-vs-E1 comparison: how many
/// [`PackageCandidateFacts`] entries a [`resolve`] call actually
/// considered -- `resolve` itself has no such counter (its algorithm is
/// unconditional iteration, so "considered" and "input length" are
/// always the same number for it), so this is computed once, here,
/// purely from the input `resolve` was actually given. `demand_driven`
/// candidates already excludes unreachable entries before `resolve`
/// ever sees them (see [`resolve_demand_driven`]); comparing this
/// field's value between an E0 and an E1 run is exactly the
/// "expanded/pruned node count" issue #47's direct-acceptance criteria
/// require recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpansionStats {
    pub package_candidates_considered: usize,
    pub package_candidates_total: usize,
}

/// The set of package ids, and the set of source ids belonging to them,
/// [`resolve_demand_driven`] treats as reachable from
/// `input.demand_entry_point`: the demand's own package (and its
/// sources), plus every [`FfiRequirementFacts::expected_provider_package`]
/// declared by a requirement whose `declaring_source` is itself
/// reachable (and that provider's own sources). A single fixed-point
/// pass suffices for this fixture's own graph shape (every FFI
/// requirement's `declaring_source` is the demand package itself, never
/// a transitive provider-of-a-provider); returning both sets (rather
/// than recomputing `reachable_source_ids` a second time from
/// `reachable_package_ids` at each of `resolve_demand_driven`'s several
/// call sites) keeps the two collections guaranteed consistent with each
/// other by construction, not just by convention.
fn reachable_package_and_source_ids(
    input: &DependencyResolutionInput,
) -> (
    std::collections::BTreeSet<&str>,
    std::collections::BTreeSet<&str>,
) {
    let mut reachable_packages: std::collections::BTreeSet<&str> =
        std::collections::BTreeSet::from([input.demand_entry_point.as_str()]);
    let mut reachable_sources: std::collections::BTreeSet<&str> = input
        .sources
        .iter()
        .filter(|s| reachable_packages.contains(s.package_id.as_str()))
        .map(|s| s.id.as_str())
        .collect();
    for req in &input.ffi_requirements {
        if reachable_sources.contains(req.declaring_source.as_str())
            && reachable_packages.insert(req.expected_provider_package.as_str())
        {
            reachable_sources.extend(
                input
                    .sources
                    .iter()
                    .filter(|s| s.package_id == req.expected_provider_package)
                    .map(|s| s.id.as_str()),
            );
        }
    }
    (reachable_packages, reachable_sources)
}

/// Issue #47 (G3, Lane B) E1: demand-driven candidate expansion. Filters
/// `input.package_candidates` (and the `sources`/`lowering_requirements`
/// entries that only exist to describe an excluded candidate) down to
/// exactly the packages [`reachable_package_ids`] reaches from
/// `input.demand_entry_point`, *before* calling the same, unmodified
/// [`resolve`] E0 uses -- this is the whole difference: E0 always hands
/// `resolve` the complete candidate pool and lets its own unconditional
/// iteration walk every entry; E1 pulls backward from the demand first
/// and only ever hands `resolve` the reachable subset. Producing exactly
/// the same [`PositiveClosure`] (byte-for-byte, once `demand_entry_point`
/// is the only entry point and no candidate outside the reachable set was
/// ever going to be selected anyway) is E1's own correctness requirement
/// -- it must prune only candidates `resolve` would itself have rejected
/// or ignored, never one it would have selected.
#[allow(clippy::result_large_err)]
pub fn resolve_demand_driven(
    input: &DependencyResolutionInput,
) -> Result<(PositiveClosure, ExpansionStats), GraphRejection> {
    let (reachable, reachable_source_ids) = reachable_package_and_source_ids(input);
    let package_candidates_total = input.package_candidates.len();

    let filtered = DependencyResolutionInput {
        demand_entry_point: input.demand_entry_point.clone(),
        target_triple: input.target_triple.clone(),
        host_toolchain_id: input.host_toolchain_id.clone(),
        sources: input
            .sources
            .iter()
            .filter(|s| reachable.contains(s.package_id.as_str()))
            .cloned()
            .collect(),
        package_candidates: input
            .package_candidates
            .iter()
            .filter(|c| reachable.contains(c.package_id.as_str()))
            .cloned()
            .collect(),
        // Only a requirement whose *declaring* source is itself
        // reachable is kept -- this is the same reachability test
        // `reachable_package_and_source_ids` itself used to decide
        // whether to admit the requirement's own
        // `expected_provider_package` into `reachable`, so this filter
        // and that set are always consistent with each other by
        // construction.
        ffi_requirements: input
            .ffi_requirements
            .iter()
            .filter(|r| reachable_source_ids.contains(r.declaring_source.as_str()))
            .cloned()
            .collect(),
        lowering_requirements: input
            .lowering_requirements
            .iter()
            .filter(|l| reachable.contains(l.package_id.as_str()))
            .cloned()
            .collect(),
        abi_constraints: input.abi_constraints.clone(),
        declared_outputs: input
            .declared_outputs
            .iter()
            .filter(|o| reachable.contains(o.package_id.as_str()))
            .cloned()
            .collect(),
        runtime_requirements: input.runtime_requirements.clone(),
    };
    let package_candidates_considered = filtered.package_candidates.len();

    let closure = resolve(&filtered)?;
    Ok((
        closure,
        ExpansionStats {
            package_candidates_considered,
            package_candidates_total,
        },
    ))
}

/// Resolves one [`DependencyResolutionInput`] into a [`PositiveClosure`]
/// or a [`GraphRejection`] -- pure, deterministic (no I/O, no
/// randomness; every collection is a `BTreeMap`/sorted `Vec`), and
/// total over well-formed input.
///
/// Algorithm:
///
/// 1. Register a `Satisfied` `SourceModule` obligation per real source.
/// 2. Register a `Satisfied` `Lowering` obligation per real Rust/Nim
///    lowering requirement.
/// 3. Trivially `Select` every root (non-provider) package's single
///    real candidate.
/// 4. **The feedback step**: for each real FFI requirement, select the
///    real provider candidate whose *declared* export (from a header or
///    an `exportc` pragma, never a compiled artifact) actually matches,
///    reject every other real candidate for that package, and register
///    the `SemanticFfi`/`AbiTarget`/`Symbol`/`ArtifactProduction`
///    obligations that selection implies. Any requirement that cannot
///    be satisfied this way rejects the whole resolution here, before a
///    single [`RequiredAction`] exists.
/// 5. Register the root package's own `ArtifactProduction` obligation
///    (its own object) and the final executable's.
/// 6. Build the full [`RequiredAction`] chain and attach each
///    artifact-production/final-link/runtime/provenance obligation's
///    `required_action` reference.
/// 7. Register `LinkOrder`, `FinalLink`, `Runtime`, `Provenance`, and
///    the root `NativeExecutableDemand` obligations, all `Satisfied`.
/// 8. A final structural check: every obligation this function itself
///    inserted must have reached a G1-terminal state -- an
///    `Unresolved` survivor here is this function's own bug, not a
///    caller error.
#[allow(clippy::result_large_err)]
pub fn resolve(input: &DependencyResolutionInput) -> Result<PositiveClosure, GraphRejection> {
    let mut obligations: BTreeMap<String, Obligation> = BTreeMap::new();
    let mut rejected_alternatives: BTreeMap<String, Vec<RejectionDetail>> = BTreeMap::new();
    let mut provider_artifact_obligations: BTreeMap<String, String> = BTreeMap::new();

    register_source_obligations(input, &mut obligations);
    register_lowering_obligations(input, &mut obligations);

    let provider_package_ids: std::collections::BTreeSet<&str> = input
        .ffi_requirements
        .iter()
        .map(|r| r.expected_provider_package.as_str())
        .collect();
    resolve_root_packages(input, &provider_package_ids, &mut obligations)?;

    for req in &input.ffi_requirements {
        resolve_provider_for_requirement(
            input,
            req,
            &mut obligations,
            &mut rejected_alternatives,
            &mut provider_artifact_obligations,
        )?;
    }

    // The root package's own artifact production (e.g. the Rust
    // object) -- one per root `PackageSelection` this resolution
    // actually selected.
    let mut root_artifact_ids: Vec<String> = Vec::new();
    let _ = &provider_artifact_obligations; // dedup guard only; not read further
    let root_selections: Vec<(String, String, Ecosystem)> = obligations
        .values()
        .filter(|o| {
            o.kind == ObligationKind::PackageSelection && o.state == ObligationState::Selected
        })
        .filter_map(|o| {
            let rest = o.id.strip_prefix("PackageSelection:")?;
            let (package_id, version) = rest.split_once('@')?;
            if provider_package_ids.contains(package_id) {
                return None;
            }
            Some((package_id.to_string(), version.to_string(), o.ecosystem))
        })
        .collect();
    for (package_id, _version, ecosystem) in &root_selections {
        let root_kind = match ecosystem {
            Ecosystem::Cargo => ArtifactOutputKind::RustObject,
            Ecosystem::Nimble => ArtifactOutputKind::NimStaticLibrary,
            other => unreachable!("a root package is only ever Cargo or Nimble, got {other:?}"),
        };
        if let Some(output) = find_output(input, package_id, root_kind) {
            let artifact_id = format!("ArtifactProduction:{}", output.id);
            let selection_id = obligations
                .keys()
                .find(|id| id.starts_with(&format!("PackageSelection:{package_id}@")))
                .cloned()
                .unwrap_or_default();
            let lowering_id = format!("Lowering:{package_id}");
            let mut depends_on = vec![selection_id];
            if obligations.contains_key(&lowering_id) {
                depends_on.push(lowering_id);
            }
            let mut artifact_obligation = Obligation::unresolved(
                &artifact_id,
                ObligationKind::ArtifactProduction,
                *ecosystem,
                Role::Target,
                depends_on,
            );
            artifact_obligation.satisfy(format!(
                "declared output '{}' for root package '{package_id}'",
                output.id
            ));
            obligations.insert(artifact_id.clone(), artifact_obligation);
            root_artifact_ids.push(artifact_id);
        }
    }

    // Every retained ArtifactProduction obligation (root + provider),
    // in deterministic (sorted) order -- this is also the declared
    // link order.
    let mut all_artifact_ids: Vec<String> = obligations
        .values()
        .filter(|o| {
            o.kind == ObligationKind::ArtifactProduction && o.state != ObligationState::Rejected
        })
        .map(|o| o.id.clone())
        .collect();
    all_artifact_ids.sort();

    // The final executable's own declared output.
    let final_executable = input
        .declared_outputs
        .iter()
        .find(|o| o.kind == ArtifactOutputKind::NativeExecutable)
        .ok_or_else(|| GraphRejection {
            obligation_id: "ArtifactProduction:final_executable".to_string(),
            reason: RejectionReason::UnsupportedConstruct,
            detail: "no NativeExecutable output is declared for this input".to_string(),
            demand_entry_point: input.demand_entry_point.clone(),
            considered_candidates: vec![],
            obligations_at_rejection: obligations.clone(),
        })?;

    // --- Build the exact G2 action chain ---------------------------------
    let mut required_actions: Vec<RequiredAction> = Vec::new();

    // One unified pass over every *selected* candidate (root or
    // provider alike -- Cargo/Nimble need exactly one lowering-then-
    // compile action, C/C++ need compile-then-archive), branching only
    // on ecosystem, never on package identity.
    for candidate in &input.package_candidates {
        let selection_id = package_selection_id(&candidate.package_id, &candidate.version);
        let Some(selection) = obligations.get(&selection_id) else {
            continue;
        };
        if selection.state != ObligationState::Selected {
            continue;
        }
        let source_ids: Vec<String> = candidate
            .sources
            .iter()
            .map(|s| source_module_id(s))
            .collect();
        match candidate.ecosystem {
            Ecosystem::Cargo => {
                let package_id = &candidate.package_id;
                let Some(lowering_source) = input
                    .lowering_requirements
                    .iter()
                    .find(|l| &l.package_id == package_id)
                else {
                    continue;
                };
                let action_id = format!("compile-rust-object:{package_id}");
                let output = find_output(input, package_id, ArtifactOutputKind::RustObject);
                required_actions.push(RequiredAction {
                    id: action_id.clone(),
                    kind: RequiredActionKind::CompileRustObject,
                    target_triple: input.target_triple.clone(),
                    role: Role::Target,
                    toolchain: "rustc".to_string(),
                    inputs: vec![source_module_id(&lowering_source.source_id)],
                    outputs: output.map(|o| vec![o.id.clone()]).unwrap_or_default(),
                    discharges: output
                        .map(|o| vec![format!("ArtifactProduction:{}", o.id)])
                        .unwrap_or_default(),
                    depends_on: vec![],
                });
                if let Some(output) = output {
                    if let Some(o) =
                        obligations.get_mut(&format!("ArtifactProduction:{}", output.id))
                    {
                        o.required_action = Some(action_id);
                    }
                }
            }
            Ecosystem::Nimble => {
                let package_id = &candidate.package_id;
                let Some(lowering_source) = input
                    .lowering_requirements
                    .iter()
                    .find(|l| &l.package_id == package_id)
                else {
                    continue;
                };
                let action_id = format!("compile-nim-static-library:{package_id}");
                let output = find_output(input, package_id, ArtifactOutputKind::NimStaticLibrary);
                required_actions.push(RequiredAction {
                    id: action_id.clone(),
                    kind: RequiredActionKind::CompileNimStaticLibrary,
                    target_triple: input.target_triple.clone(),
                    role: Role::Target,
                    toolchain: format!("{} (host-role toolchain)", input.host_toolchain_id),
                    inputs: vec![source_module_id(&lowering_source.source_id)],
                    outputs: output.map(|o| vec![o.id.clone()]).unwrap_or_default(),
                    discharges: output
                        .map(|o| vec![format!("ArtifactProduction:{}", o.id)])
                        .unwrap_or_default(),
                    depends_on: vec![],
                });
                if let Some(output) = output {
                    if let Some(o) =
                        obligations.get_mut(&format!("ArtifactProduction:{}", output.id))
                    {
                        o.required_action = Some(action_id);
                    }
                }
            }
            Ecosystem::C => {
                let object_outputs =
                    find_outputs(input, &candidate.package_id, ArtifactOutputKind::CObject);
                let archive_output = find_output(
                    input,
                    &candidate.package_id,
                    ArtifactOutputKind::CStaticArchive,
                );
                let is_sub_candidate = object_outputs.len() > 1
                    && object_outputs.iter().all(|o| !o.source_ids.is_empty());

                let compile_ids: Vec<String> = if is_sub_candidate {
                    // Issue #65 (B-H3): one CompileCObject action per
                    // declared sub-candidate (translation-unit-granularity)
                    // object output, each discharging its own real source's
                    // `SourceModule` obligation -- the granularity below
                    // "one candidate = one compile action" issue #47 left
                    // unimplemented. Never a LAMINARIA-owned split of a
                    // single source file: `object.source_ids` are
                    // already-declared, already-known source ids from the
                    // ingestion adapter, not something this function
                    // derives from C source text.
                    object_outputs
                        .iter()
                        .map(|object_output| {
                            let compile_id = format!(
                                "compile-c-object:{}@{}:{}",
                                candidate.package_id, candidate.version, object_output.id
                            );
                            let discharges: Vec<String> = object_output
                                .source_ids
                                .iter()
                                .map(|source_id| source_module_id(source_id))
                                .collect();
                            required_actions.push(RequiredAction {
                                id: compile_id.clone(),
                                kind: RequiredActionKind::CompileCObject,
                                target_triple: candidate.target_triple.clone(),
                                role: Role::Target,
                                toolchain: "cc".to_string(),
                                inputs: object_output
                                    .source_ids
                                    .iter()
                                    .map(|source_id| source_module_id(source_id))
                                    .collect(),
                                outputs: vec![object_output.id.clone()],
                                discharges: discharges.clone(),
                                depends_on: vec![],
                            });
                            for source_module_obligation_id in &discharges {
                                if let Some(o) = obligations.get_mut(source_module_obligation_id) {
                                    o.required_action = Some(compile_id.clone());
                                }
                            }
                            compile_id
                        })
                        .collect()
                } else {
                    let object_output = object_outputs.first().copied();
                    let compile_id = format!(
                        "compile-c-object:{}@{}",
                        candidate.package_id, candidate.version
                    );
                    required_actions.push(RequiredAction {
                        id: compile_id.clone(),
                        kind: RequiredActionKind::CompileCObject,
                        target_triple: candidate.target_triple.clone(),
                        role: Role::Target,
                        toolchain: "cc".to_string(),
                        inputs: source_ids,
                        outputs: object_output
                            .map(|o| vec![o.id.clone()])
                            .unwrap_or_default(),
                        discharges: vec![],
                        depends_on: vec![],
                    });
                    vec![compile_id]
                };

                let archive_id = format!(
                    "archive-static-library:{}@{}",
                    candidate.package_id, candidate.version
                );
                required_actions.push(RequiredAction {
                    id: archive_id.clone(),
                    kind: RequiredActionKind::ArchiveStaticLibrary,
                    target_triple: candidate.target_triple.clone(),
                    role: Role::Target,
                    toolchain: "ar".to_string(),
                    inputs: object_outputs.iter().map(|o| o.id.clone()).collect(),
                    outputs: archive_output
                        .map(|o| vec![o.id.clone()])
                        .unwrap_or_default(),
                    discharges: archive_output
                        .map(|o| vec![format!("ArtifactProduction:{}", o.id)])
                        .unwrap_or_default(),
                    depends_on: compile_ids,
                });
                if let Some(output) = archive_output {
                    if let Some(o) =
                        obligations.get_mut(&format!("ArtifactProduction:{}", output.id))
                    {
                        o.required_action = Some(archive_id);
                    }
                }
            }
            Ecosystem::Cpp => {
                let object_output = find_output(
                    input,
                    &candidate.package_id,
                    ArtifactOutputKind::CppAdapterObject,
                );
                let archive_output = find_output(
                    input,
                    &candidate.package_id,
                    ArtifactOutputKind::CppStaticArchive,
                );
                let compile_id = format!(
                    "compile-cpp-adapter-object:{}@{}",
                    candidate.package_id, candidate.version
                );
                required_actions.push(RequiredAction {
                    id: compile_id.clone(),
                    kind: RequiredActionKind::CompileCppAdapterObject,
                    target_triple: candidate.target_triple.clone(),
                    role: Role::Target,
                    toolchain: "c++".to_string(),
                    inputs: source_ids,
                    outputs: object_output
                        .map(|o| vec![o.id.clone()])
                        .unwrap_or_default(),
                    discharges: vec![],
                    depends_on: vec![],
                });
                let archive_id = format!(
                    "archive-static-library:{}@{}",
                    candidate.package_id, candidate.version
                );
                required_actions.push(RequiredAction {
                    id: archive_id.clone(),
                    kind: RequiredActionKind::ArchiveStaticLibrary,
                    target_triple: candidate.target_triple.clone(),
                    role: Role::Target,
                    toolchain: "ar".to_string(),
                    inputs: object_output
                        .map(|o| vec![o.id.clone()])
                        .unwrap_or_default(),
                    outputs: archive_output
                        .map(|o| vec![o.id.clone()])
                        .unwrap_or_default(),
                    discharges: archive_output
                        .map(|o| vec![format!("ArtifactProduction:{}", o.id)])
                        .unwrap_or_default(),
                    depends_on: vec![compile_id],
                });
                if let Some(output) = archive_output {
                    if let Some(o) =
                        obligations.get_mut(&format!("ArtifactProduction:{}", output.id))
                    {
                        o.required_action = Some(archive_id);
                    }
                }
            }
            Ecosystem::Cross => {}
        }
    }

    // Structural check before proceeding: every retained obligation
    // built so far must already be terminal for G1 -- an `Unresolved`
    // survivor here means the graph is not actually connected/decided
    // yet, which must never reach a `RequiredAction`.
    let mut unresolved: Vec<&Obligation> = obligations
        .values()
        .filter(|o| o.state == ObligationState::Unresolved)
        .collect();
    unresolved.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(first) = unresolved.first() {
        return Err(GraphRejection {
            obligation_id: first.id.clone(),
            reason: RejectionReason::UnsupportedConstruct,
            detail: format!(
                "obligation '{}' is still Unresolved, not Selected/Satisfied/Rejected",
                first.id
            ),
            demand_entry_point: input.demand_entry_point.clone(),
            considered_candidates: vec![],
            obligations_at_rejection: obligations.clone(),
        });
    }

    let link_id = "link-native-executable".to_string();
    let mut link_inputs: Vec<String> = Vec::new();
    for id in &all_artifact_ids {
        if let Some(output_id) = id.strip_prefix("ArtifactProduction:") {
            if let Some(output) = input.declared_outputs.iter().find(|o| o.id == output_id) {
                link_inputs.push(output.id.clone());
            }
        }
    }
    let link_action_depends_on: Vec<String> = required_actions
        .iter()
        .filter(|a| {
            matches!(
                a.kind,
                RequiredActionKind::CompileRustObject | RequiredActionKind::CompileNimStaticLibrary
            ) || (a.kind == RequiredActionKind::ArchiveStaticLibrary)
        })
        .map(|a| a.id.clone())
        .collect();
    required_actions.push(RequiredAction {
        id: link_id.clone(),
        kind: RequiredActionKind::LinkNativeExecutable,
        target_triple: input.target_triple.clone(),
        role: Role::Target,
        toolchain: "system linker".to_string(),
        inputs: link_inputs,
        outputs: vec![final_executable.id.clone()],
        discharges: vec![
            "FinalLink:native_executable".to_string(),
            "LinkOrder:native_executable".to_string(),
            format!("ArtifactProduction:{}", final_executable.id),
        ],
        depends_on: link_action_depends_on,
    });

    let runtime_action_id = "preflight-runtime-contract".to_string();
    required_actions.push(RequiredAction {
        id: runtime_action_id.clone(),
        kind: RequiredActionKind::PreflightRuntimeContract,
        target_triple: input.target_triple.clone(),
        role: Role::Target,
        toolchain: "runtime environment inspection".to_string(),
        inputs: vec![final_executable.id.clone()],
        outputs: vec![],
        discharges: vec!["Runtime:native_executable".to_string()],
        depends_on: vec![link_id.clone()],
    });

    let provenance_action_id = "publish-provenance".to_string();
    required_actions.push(RequiredAction {
        id: provenance_action_id.clone(),
        kind: RequiredActionKind::PublishProvenance,
        target_triple: input.target_triple.clone(),
        role: Role::Target,
        toolchain: "provenance publisher".to_string(),
        inputs: vec![final_executable.id.clone()],
        outputs: vec![format!("provenance:{}", final_executable.id)],
        discharges: vec!["Provenance:native_executable".to_string()],
        depends_on: vec![runtime_action_id.clone()],
    });

    // --- Register the remaining cross-ecosystem obligations --------------
    let link_order_id = "LinkOrder:native_executable".to_string();
    let mut link_order_obligation = Obligation::unresolved(
        &link_order_id,
        ObligationKind::LinkOrder,
        Ecosystem::Cross,
        Role::Target,
        all_artifact_ids.clone(),
    );
    link_order_obligation.satisfy_with_action(
        link_id.clone(),
        format!("declared link order: {}", all_artifact_ids.join(", ")),
    );
    obligations.insert(link_order_id.clone(), link_order_obligation);

    let symbol_ids: Vec<String> = obligations
        .values()
        .filter(|o| o.kind == ObligationKind::Symbol)
        .map(|o| o.id.clone())
        .collect();

    let final_link_id = "FinalLink:native_executable".to_string();
    let mut final_link_depends_on = all_artifact_ids.clone();
    final_link_depends_on.push(link_order_id.clone());
    final_link_depends_on.extend(symbol_ids);
    let mut final_link_obligation = Obligation::unresolved(
        &final_link_id,
        ObligationKind::FinalLink,
        Ecosystem::Cross,
        Role::Target,
        final_link_depends_on,
    );
    final_link_obligation.satisfy_with_action(
        link_id.clone(),
        format!(
            "final link required for demand '{}'",
            input.demand_entry_point
        ),
    );
    obligations.insert(final_link_id.clone(), final_link_obligation);

    let final_artifact_id = format!("ArtifactProduction:{}", final_executable.id);
    let mut final_artifact_obligation = Obligation::unresolved(
        &final_artifact_id,
        ObligationKind::ArtifactProduction,
        Ecosystem::Cross,
        Role::Target,
        vec![final_link_id.clone()],
    );
    final_artifact_obligation.satisfy_with_action(
        link_id.clone(),
        format!(
            "declared output '{}' produced by the final link",
            final_executable.id
        ),
    );
    obligations.insert(final_artifact_id.clone(), final_artifact_obligation);

    let runtime_id = "Runtime:native_executable".to_string();
    let mut runtime_obligation = Obligation::unresolved(
        &runtime_id,
        ObligationKind::Runtime,
        Ecosystem::Cross,
        Role::Target,
        vec![final_artifact_id.clone()],
    );
    let runtime_note = input
        .runtime_requirements
        .iter()
        .map(|r| r.description.clone())
        .collect::<Vec<_>>()
        .join("; ");
    runtime_obligation.satisfy_with_action(
        runtime_action_id,
        format!("runtime contract(s) to preflight: {runtime_note}"),
    );
    obligations.insert(runtime_id.clone(), runtime_obligation);

    let provenance_id = "Provenance:native_executable".to_string();
    let mut provenance_obligation = Obligation::unresolved(
        &provenance_id,
        ObligationKind::Provenance,
        Ecosystem::Cross,
        Role::Target,
        vec![runtime_id.clone()],
    );
    provenance_obligation.satisfy_with_action(
        provenance_action_id,
        "the original ecosystem graph is retained as audit provenance, not re-resolved by a consumer",
    );
    obligations.insert(provenance_id.clone(), provenance_obligation);

    let demand_id = format!("NativeExecutableDemand:{}", input.demand_entry_point);
    let mut demand_obligation = Obligation::unresolved(
        &demand_id,
        ObligationKind::NativeExecutableDemand,
        Ecosystem::Cross,
        Role::Target,
        vec![provenance_id],
    );
    demand_obligation.satisfy(format!(
        "native executable demand for '{}'",
        input.demand_entry_point
    ));
    obligations.insert(demand_id, demand_obligation);

    debug_assert!(
        obligations.values().all(|o| o.state.is_g1_terminal()),
        "resolve() must never return an obligation left Unresolved/Discharged/Externalized"
    );

    Ok(PositiveClosure {
        obligations,
        required_actions,
        rejected_alternatives,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_input(cadd_candidates: Vec<PackageCandidateFacts>) -> DependencyResolutionInput {
        let mut package_candidates = vec![
            PackageCandidateFacts {
                ecosystem: Ecosystem::Cargo,
                package_id: "app".to_string(),
                version: "0.1.0".to_string(),
                role: Role::Target,
                target_triple: "x86_64-unknown-linux-gnu".to_string(),
                sources: vec!["app/src/main.rs".to_string()],
                declared_exports: vec![],
                declared_constraints: vec!["feature:use_nim_double".to_string()],
            },
            PackageCandidateFacts {
                ecosystem: Ecosystem::Nimble,
                package_id: "doubler".to_string(),
                version: "0.1.0".to_string(),
                role: Role::Target,
                target_triple: "x86_64-unknown-linux-gnu".to_string(),
                sources: vec!["nimble/doubler/src/doubler.nim".to_string()],
                declared_exports: vec![FfiExportFacts {
                    declaring_source: "nimble/doubler/src/doubler.nim".to_string(),
                    symbol: "nim_double".to_string(),
                    abi: "C".to_string(),
                    param_count: 1,
                    return_type: "cint".to_string(),
                }],
                declared_constraints: vec!["requires:nim >= 2.0.0".to_string()],
            },
        ];
        package_candidates.extend(cadd_candidates);

        DependencyResolutionInput {
            demand_entry_point: "app".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            host_toolchain_id: "nim-2.2.10".to_string(),
            sources: vec![
                SourceModuleFacts {
                    id: "app/src/main.rs".to_string(),
                    ecosystem: Ecosystem::Cargo,
                    package_id: "app".to_string(),
                },
                SourceModuleFacts {
                    id: "nimble/doubler/src/doubler.nim".to_string(),
                    ecosystem: Ecosystem::Nimble,
                    package_id: "doubler".to_string(),
                },
                SourceModuleFacts {
                    id: "c/cadd/v1/cadd.c".to_string(),
                    ecosystem: Ecosystem::C,
                    package_id: "cadd".to_string(),
                },
                SourceModuleFacts {
                    id: "c/cadd/v1/cadd.h".to_string(),
                    ecosystem: Ecosystem::C,
                    package_id: "cadd".to_string(),
                },
            ],
            package_candidates,
            ffi_requirements: vec![
                FfiRequirementFacts {
                    declaring_source: "app/src/main.rs".to_string(),
                    symbol: "nim_double".to_string(),
                    abi: "C".to_string(),
                    param_count: 1,
                    return_type: "i32".to_string(),
                    expected_provider_package: "doubler".to_string(),
                },
                FfiRequirementFacts {
                    declaring_source: "app/src/main.rs".to_string(),
                    symbol: "c_add".to_string(),
                    abi: "C".to_string(),
                    param_count: 2,
                    return_type: "i32".to_string(),
                    expected_provider_package: "cadd".to_string(),
                },
            ],
            lowering_requirements: vec![
                LoweringRequirementFacts {
                    package_id: "app".to_string(),
                    source_id: "app/src/main.rs".to_string(),
                    description: "Rust application lowering-feasible for target".to_string(),
                },
                LoweringRequirementFacts {
                    package_id: "doubler".to_string(),
                    source_id: "nimble/doubler/src/doubler.nim".to_string(),
                    description: "Nim package lowering-feasible for target".to_string(),
                },
            ],
            abi_constraints: vec![],
            declared_outputs: vec![
                ArtifactOutputFacts {
                    id: "object:app".to_string(),
                    package_id: "app".to_string(),
                    kind: ArtifactOutputKind::RustObject,
                    source_ids: vec![],
                },
                ArtifactOutputFacts {
                    id: "archive:doubler".to_string(),
                    package_id: "doubler".to_string(),
                    kind: ArtifactOutputKind::NimStaticLibrary,
                    source_ids: vec![],
                },
                ArtifactOutputFacts {
                    id: "object:cadd".to_string(),
                    package_id: "cadd".to_string(),
                    kind: ArtifactOutputKind::CObject,
                    source_ids: vec![],
                },
                ArtifactOutputFacts {
                    id: "archive:cadd".to_string(),
                    package_id: "cadd".to_string(),
                    kind: ArtifactOutputKind::CStaticArchive,
                    source_ids: vec![],
                },
                ArtifactOutputFacts {
                    id: "executable:app".to_string(),
                    package_id: "app".to_string(),
                    kind: ArtifactOutputKind::NativeExecutable,
                    source_ids: vec![],
                },
            ],
            runtime_requirements: vec![RuntimeRequirementFacts {
                target_triple: "x86_64-unknown-linux-gnu".to_string(),
                description: "OS ABI/dynamic loader contract".to_string(),
            }],
        }
    }

    fn cadd_candidate(
        version: &str,
        symbol: &str,
        target_triple: &str,
        role: Role,
    ) -> PackageCandidateFacts {
        PackageCandidateFacts {
            ecosystem: Ecosystem::C,
            package_id: "cadd".to_string(),
            version: version.to_string(),
            role,
            target_triple: target_triple.to_string(),
            sources: vec![
                "c/cadd/v1/cadd.c".to_string(),
                "c/cadd/v1/cadd.h".to_string(),
            ],
            declared_exports: vec![FfiExportFacts {
                declaring_source: "c/cadd/v1/cadd.h".to_string(),
                symbol: symbol.to_string(),
                abi: "C".to_string(),
                param_count: 2,
                return_type: "i32".to_string(),
            }],
            declared_constraints: vec![],
        }
    }

    #[test]
    fn a_single_qualifying_candidate_is_selected_and_reaches_a_positive_closure() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let closure = resolve(&input).expect("must resolve");
        assert!(closure
            .obligations
            .values()
            .all(|o| o.state.is_g1_terminal()));
        let symbol = &closure.obligations["Symbol:c_add"];
        assert_eq!(symbol.state, ObligationState::Satisfied);
    }

    /// Issue #65 (B-H3, carried-forward item 1): a provider candidate
    /// whose real source list is a two-translation-unit unity build
    /// (the `bcm.c`-shape issue #64 measured, scaled down to two members
    /// for a direct executable test) and whose `declared_outputs` names
    /// one `CObject` output per translation unit (`source_ids` non-empty
    /// on each) must produce one `CompileCObject` `RequiredAction` per
    /// TU -- not one action covering both sources -- each discharging
    /// its own real `SourceModule` obligation, with the archive step
    /// depending on every one of them.
    fn two_translation_unit_cadd_candidate() -> (PackageCandidateFacts, Vec<SourceModuleFacts>) {
        let sources = vec![
            SourceModuleFacts {
                id: "c/cadd/v1/member_a.c".to_string(),
                ecosystem: Ecosystem::C,
                package_id: "cadd".to_string(),
            },
            SourceModuleFacts {
                id: "c/cadd/v1/member_b.c".to_string(),
                ecosystem: Ecosystem::C,
                package_id: "cadd".to_string(),
            },
            SourceModuleFacts {
                id: "c/cadd/v1/cadd.h".to_string(),
                ecosystem: Ecosystem::C,
                package_id: "cadd".to_string(),
            },
        ];
        let candidate = PackageCandidateFacts {
            ecosystem: Ecosystem::C,
            package_id: "cadd".to_string(),
            version: "1.0.0".to_string(),
            role: Role::Target,
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            sources: sources.iter().map(|s| s.id.clone()).collect(),
            declared_exports: vec![FfiExportFacts {
                declaring_source: "c/cadd/v1/cadd.h".to_string(),
                symbol: "c_add".to_string(),
                abi: "C".to_string(),
                param_count: 2,
                return_type: "i32".to_string(),
            }],
            declared_constraints: vec![],
        };
        (candidate, sources)
    }

    /// Builds on `minimal_input`'s app/doubler/ffi shape but replaces
    /// `cadd`'s sources/outputs with the two-TU sub-candidate shape --
    /// `minimal_input` itself always wires the single-source `cadd.c`/
    /// `cadd.h` pair into `input.sources`, so this rebuilds the relevant
    /// parts directly rather than trying to graft sub-candidate outputs
    /// onto that fixed shape.
    fn sub_candidate_input() -> DependencyResolutionInput {
        let (cadd_candidate, cadd_sources) = two_translation_unit_cadd_candidate();
        let mut input = minimal_input(vec![cadd_candidate]);
        input.sources.retain(|s| s.package_id != "cadd");
        input.sources.extend(cadd_sources);
        input
            .declared_outputs
            .retain(|o| !(o.package_id == "cadd" && o.kind == ArtifactOutputKind::CObject));
        input.declared_outputs.push(ArtifactOutputFacts {
            id: "object:cadd:member_a".to_string(),
            package_id: "cadd".to_string(),
            kind: ArtifactOutputKind::CObject,
            source_ids: vec!["c/cadd/v1/member_a.c".to_string()],
        });
        input.declared_outputs.push(ArtifactOutputFacts {
            id: "object:cadd:member_b".to_string(),
            package_id: "cadd".to_string(),
            kind: ArtifactOutputKind::CObject,
            source_ids: vec!["c/cadd/v1/member_b.c".to_string()],
        });
        input
    }

    #[test]
    fn a_two_translation_unit_provider_produces_one_compile_action_per_tu() {
        let input = sub_candidate_input();
        let closure = resolve(&input).expect("must resolve");
        assert!(closure
            .obligations
            .values()
            .all(|o| o.state.is_g1_terminal()));

        let compile_actions: Vec<&RequiredAction> = closure
            .required_actions
            .iter()
            .filter(|a| a.kind == RequiredActionKind::CompileCObject)
            .collect();
        assert_eq!(
            compile_actions.len(),
            2,
            "expected one CompileCObject action per translation unit, got {compile_actions:#?}"
        );

        let member_a_action = compile_actions
            .iter()
            .find(|a| a.outputs == vec!["object:cadd:member_a".to_string()])
            .expect("member_a's own compile action must exist");
        assert_eq!(
            member_a_action.inputs,
            vec!["SourceModule:c/cadd/v1/member_a.c".to_string()]
        );
        assert_eq!(
            member_a_action.discharges,
            vec!["SourceModule:c/cadd/v1/member_a.c".to_string()]
        );

        let member_b_action = compile_actions
            .iter()
            .find(|a| a.outputs == vec!["object:cadd:member_b".to_string()])
            .expect("member_b's own compile action must exist");
        assert_eq!(
            member_b_action.inputs,
            vec!["SourceModule:c/cadd/v1/member_b.c".to_string()]
        );

        // Each TU's own SourceModule obligation must now carry the
        // required_action reference naming its own compile action --
        // the sub-candidate discharge target issue #65 item 1 asks for.
        assert_eq!(
            closure.obligations["SourceModule:c/cadd/v1/member_a.c"].required_action,
            Some(member_a_action.id.clone())
        );
        assert_eq!(
            closure.obligations["SourceModule:c/cadd/v1/member_b.c"].required_action,
            Some(member_b_action.id.clone())
        );

        // The archive step must depend on both per-TU compile actions,
        // not just one -- never silently dropping a member.
        let archive_action = closure
            .required_actions
            .iter()
            .find(|a| a.kind == RequiredActionKind::ArchiveStaticLibrary)
            .expect("archive action must exist");
        assert_eq!(
            archive_action
                .depends_on
                .iter()
                .collect::<std::collections::BTreeSet<_>>(),
            [member_a_action.id.clone(), member_b_action.id.clone()]
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
        );
        assert_eq!(
            archive_action
                .inputs
                .iter()
                .collect::<std::collections::BTreeSet<_>>(),
            [
                "object:cadd:member_a".to_string(),
                "object:cadd:member_b".to_string()
            ]
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
        );
    }

    /// Same demand-reachability invariant `the_demand_reaches_every_retained_obligation`
    /// already checks for the whole-package shape, re-run against the
    /// sub-candidate shape: every per-TU `SourceModule` obligation and
    /// its own compile action must still be reachable from the root
    /// demand through `depends_on`, never an orphaned sub-obligation.
    #[test]
    fn sub_candidate_shape_still_reaches_every_retained_obligation_from_the_demand() {
        let input = sub_candidate_input();
        let closure = resolve(&input).expect("must resolve");
        let demand_id = "NativeExecutableDemand:app".to_string();
        let mut reached: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut frontier = vec![demand_id.clone()];
        while let Some(id) = frontier.pop() {
            if !reached.insert(id.clone()) {
                continue;
            }
            let obligation = closure.obligations.get(&id).unwrap_or_else(|| {
                panic!("depends_on names '{id}', which does not exist as a real obligation")
            });
            frontier.extend(obligation.depends_on.iter().cloned());
        }
        for (id, obligation) in &closure.obligations {
            if obligation.state.is_rejected() {
                continue;
            }
            assert!(
                reached.contains(id),
                "retained obligation '{id}' is not reachable from the demand"
            );
        }
    }

    #[test]
    fn a_candidate_missing_the_required_symbol_is_rejected_before_any_action_is_generated() {
        let input = minimal_input(vec![cadd_candidate(
            "2.0.0",
            "c_add_v2",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let rejection = resolve(&input).expect_err("must reject");
        assert_eq!(rejection.reason, RejectionReason::MissingSymbol);
        assert_eq!(rejection.obligation_id, "Symbol:c_add");
        assert!(rejection.detail.contains("c_add"));
    }

    #[test]
    fn a_same_named_candidate_with_a_different_arity_is_rejected_not_silently_accepted() {
        let mut mismatched =
            cadd_candidate("1.0.0", "c_add", "x86_64-unknown-linux-gnu", Role::Target);
        mismatched.declared_exports[0].param_count = 3;
        let input = minimal_input(vec![mismatched]);
        let rejection =
            resolve(&input).expect_err("a same-named but wrong-arity provider must be rejected");
        assert_eq!(rejection.reason, RejectionReason::IncompatibleAbi);
        assert!(rejection.detail.contains("params=3"));
    }

    #[test]
    fn a_same_named_candidate_with_a_different_return_type_is_rejected_not_silently_accepted() {
        let mut mismatched =
            cadd_candidate("1.0.0", "c_add", "x86_64-unknown-linux-gnu", Role::Target);
        mismatched.declared_exports[0].return_type = "void".to_string();
        let input = minimal_input(vec![mismatched]);
        let rejection = resolve(&input)
            .expect_err("a same-named but wrong-return-type provider must be rejected");
        assert_eq!(rejection.reason, RejectionReason::IncompatibleAbi);
    }

    #[test]
    fn feedback_selects_the_qualifying_candidate_and_rejects_the_other_as_an_alternative() {
        let input = minimal_input(vec![
            cadd_candidate("1.0.0", "c_add", "x86_64-unknown-linux-gnu", Role::Target),
            cadd_candidate(
                "2.0.0",
                "c_add_v2",
                "x86_64-unknown-linux-gnu",
                Role::Target,
            ),
        ]);
        let closure = resolve(&input).expect("must resolve");
        let rejected = &closure.rejected_alternatives["cadd"];
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].reason, RejectionReason::MissingSymbol);
        assert_eq!(
            closure.obligations["PackageSelection:cadd@1.0.0"].state,
            ObligationState::Selected
        );
        assert_eq!(
            closure.obligations["PackageSelection:cadd@2.0.0"].state,
            ObligationState::Rejected
        );
    }

    #[test]
    fn an_abi_target_mismatch_is_rejected() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "aarch64-unknown-linux-gnu",
            Role::Target,
        )]);
        let rejection = resolve(&input).expect_err("must reject");
        assert_eq!(rejection.reason, RejectionReason::IncompatibleAbi);
    }

    #[test]
    fn a_host_role_candidate_never_satisfies_a_target_symbol_obligation() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Host,
        )]);
        let rejection = resolve(&input).expect_err("must reject");
        assert_eq!(rejection.reason, RejectionReason::HostTargetRoleMismatch);
    }

    #[test]
    fn resolving_the_same_input_twice_produces_byte_identical_serialized_closures() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let a = resolve(&input).expect("must resolve");
        let b = resolve(&input).expect("must resolve");
        let a_json = serde_json::to_string(&a).unwrap();
        let b_json = serde_json::to_string(&b).unwrap();
        assert_eq!(a_json, b_json);
    }

    #[test]
    fn the_demand_reaches_every_retained_obligation() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let closure = resolve(&input).expect("must resolve");
        let demand_id = "NativeExecutableDemand:app".to_string();
        let mut reached: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut frontier = vec![demand_id.clone()];
        while let Some(id) = frontier.pop() {
            if !reached.insert(id.clone()) {
                continue;
            }
            let obligation = closure.obligations.get(&id).unwrap_or_else(|| {
                panic!("depends_on names '{id}', which does not exist as a real obligation")
            });
            frontier.extend(obligation.depends_on.iter().cloned());
        }
        for (id, obligation) in &closure.obligations {
            if obligation.state.is_rejected() {
                continue;
            }
            assert!(
                reached.contains(id),
                "retained obligation '{id}' is not reachable from the demand"
            );
        }
    }

    /// Issue #46 (G2) Checkpoint 1: the shared obligation-lifecycle
    /// transition, exercised against a real G1 closure. Discharging the
    /// `cadd` archive's `ArtifactProduction` obligation through its own
    /// named `RequiredAction` (`archive-static-library:cadd@1.0.0`,
    /// kind `ArchiveStaticLibrary` -- structurally allowed to discharge
    /// `ArtifactProduction` per `discharge_kind_allowed`) must succeed
    /// and leave the obligation `Discharged` with the exact evidence
    /// and action id supplied.
    #[test]
    fn discharging_an_obligation_through_its_own_named_action_succeeds() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let mut closure = resolve(&input).expect("must resolve");
        let obligation_id = "ArtifactProduction:archive:cadd";
        let action_id = "archive-static-library:cadd@1.0.0";
        assert_eq!(
            closure.obligations[obligation_id].state,
            ObligationState::Satisfied,
            "must start from G1's own terminal state before discharge"
        );

        closure
            .discharge_obligation(
                obligation_id,
                action_id,
                DischargeKind::StaticallyLinked,
                "ar rcs archive:cadd object:cadd -- real cc+ar evidence",
            )
            .expect("a real named action discharging its own obligation must succeed");

        let obligation = &closure.obligations[obligation_id];
        assert_eq!(obligation.state, ObligationState::Discharged);
        assert_eq!(
            obligation.discharge_kind,
            Some(DischargeKind::StaticallyLinked)
        );
        assert_eq!(obligation.required_action.as_deref(), Some(action_id));
        assert_eq!(
            obligation.evidence.as_deref(),
            Some("ar rcs archive:cadd object:cadd -- real cc+ar evidence")
        );
    }

    /// An action may only discharge an obligation it actually names in
    /// its own `discharges` list -- using a real, existing action that
    /// discharges a *different* obligation must be refused, not
    /// silently accepted.
    #[test]
    fn discharging_via_an_action_that_does_not_name_this_obligation_is_refused() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let mut closure = resolve(&input).expect("must resolve");
        let wrong_action_id = closure
            .required_actions
            .iter()
            .find(|a| a.kind == RequiredActionKind::CompileRustObject)
            .expect("must have a compile-rust-object action")
            .id
            .clone();
        let result = closure.discharge_obligation(
            "ArtifactProduction:archive:cadd",
            &wrong_action_id,
            DischargeKind::StaticallyLinked,
            "bogus evidence",
        );
        assert!(matches!(
            result,
            Err(LifecycleViolation::ActionDoesNotDischargeThisObligation { .. })
        ));
        assert_eq!(
            closure.obligations["ArtifactProduction:archive:cadd"].state,
            ObligationState::Satisfied,
            "a refused discharge must never mutate the obligation"
        );
    }

    /// Re-discharging an already-`Discharged` obligation, or
    /// discharging a `Rejected` one, must be refused -- the lifecycle
    /// has no re-entrant or resurrection transition.
    #[test]
    fn discharging_an_already_terminal_obligation_is_refused() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let mut closure = resolve(&input).expect("must resolve");
        let obligation_id = "ArtifactProduction:archive:cadd";
        let action_id = "archive-static-library:cadd@1.0.0";
        closure
            .discharge_obligation(
                obligation_id,
                action_id,
                DischargeKind::StaticallyLinked,
                "first discharge",
            )
            .expect("first discharge must succeed");

        let result = closure.discharge_obligation(
            obligation_id,
            action_id,
            DischargeKind::StaticallyLinked,
            "second discharge attempt",
        );
        assert!(matches!(
            result,
            Err(LifecycleViolation::ObligationNotDischargeable {
                state: ObligationState::Discharged,
                ..
            })
        ));

        let rejected_input = minimal_input(vec![cadd_candidate(
            "2.0.0",
            "c_add_v2",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let rejected_closure = resolve(&rejected_input);
        assert!(
            rejected_closure.is_err(),
            "a lone incompatible candidate must reject at the graph level, not just the obligation"
        );
    }

    /// An action structurally forbidden from discharging a given
    /// obligation kind (a compile action can never discharge `Runtime`
    /// -- `discharge_kind_allowed(CompileCObject, ..)` only ever
    /// permits `ArtifactProduction`) must be refused even if the
    /// caller supplies a fabricated `discharges` entry -- exercised
    /// directly against a mutated closure, mirroring `plan_integrity`'s
    /// own `DischargeKindMismatch` check.
    #[test]
    fn discharging_with_a_structurally_disallowed_action_kind_is_refused() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let mut closure = resolve(&input).expect("must resolve");
        let compile_action_id = closure
            .required_actions
            .iter()
            .find(|a| a.kind == RequiredActionKind::CompileCObject)
            .expect("must have a compile-c-object action")
            .id
            .clone();
        let runtime_id = closure
            .obligations
            .values()
            .find(|o| o.kind == ObligationKind::Runtime)
            .expect("must have a runtime obligation")
            .id
            .clone();
        closure
            .required_actions
            .iter_mut()
            .find(|a| a.id == compile_action_id)
            .unwrap()
            .discharges
            .push(runtime_id.clone());

        let result = closure.discharge_obligation(
            &runtime_id,
            &compile_action_id,
            DischargeKind::Generated,
            "a compile action cannot discharge a runtime obligation",
        );
        assert!(matches!(
            result,
            Err(LifecycleViolation::ActionKindCannotDischargeObligationKind { .. })
        ));
    }

    /// Externalizing a `Runtime` obligation (an OS/dynamic-loader
    /// contract G2 verifies but never produces) succeeds without any
    /// named action and always records `ExternalRuntimeContract`.
    #[test]
    fn externalizing_a_runtime_obligation_succeeds_without_a_named_action() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let mut closure = resolve(&input).expect("must resolve");
        let runtime_id = closure
            .obligations
            .values()
            .find(|o| o.kind == ObligationKind::Runtime)
            .expect("must have a runtime obligation")
            .id
            .clone();
        closure
            .externalize_obligation(
                &runtime_id,
                "verified real dynamic loader present at preflight for x86_64-unknown-linux-gnu",
            )
            .expect("externalizing a Satisfied runtime obligation must succeed");

        let obligation = &closure.obligations[&runtime_id];
        assert_eq!(obligation.state, ObligationState::Externalized);
        assert_eq!(
            obligation.discharge_kind,
            Some(DischargeKind::ExternalRuntimeContract)
        );
    }

    /// An unknown obligation id is refused, not silently ignored.
    #[test]
    fn discharging_an_unknown_obligation_id_is_refused() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let mut closure = resolve(&input).expect("must resolve");
        let result = closure.discharge_obligation(
            "ArtifactProduction:does-not-exist",
            "archive-static-library:cadd@1.0.0",
            DischargeKind::StaticallyLinked,
            "evidence",
        );
        assert!(matches!(
            result,
            Err(LifecycleViolation::UnknownObligation(_))
        ));
    }

    /// Issue #47 (G3, Lane B) E1's own correctness requirement: with no
    /// unreachable candidates present at all, `resolve_demand_driven`
    /// must consider every real candidate (nothing to prune) and produce
    /// the exact same `PositiveClosure` `resolve` (E0) does, byte for
    /// byte -- E1 must never change *what* gets resolved, only how much
    /// of the input it actually walks to get there.
    #[test]
    fn demand_driven_with_no_unreachable_candidates_matches_eager_byte_for_byte() {
        let input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let eager = resolve(&input).expect("E0 must resolve");
        let (demand_driven, stats) =
            resolve_demand_driven(&input).expect("E1 must resolve the same input");

        assert_eq!(
            serde_json::to_string(&eager).unwrap(),
            serde_json::to_string(&demand_driven).unwrap(),
            "E0 and E1 must produce byte-identical closures when nothing is unreachable"
        );
        assert_eq!(stats.package_candidates_total, 3, "app + doubler + cadd");
        assert_eq!(
            stats.package_candidates_considered, 3,
            "no candidate is unreachable in this input, so E1 has nothing to prune"
        );
    }

    /// A package candidate wholly disconnected from `demand_entry_point`
    /// -- it declares no export any real `ffi_requirements` entry names,
    /// so `resolve` (E0) would have ignored it too (its own iteration
    /// over `input.package_candidates` in `resolve_root_packages` would
    /// have registered it as an *extra*, unreached root-package
    /// candidate -- `resolve_root_packages` treats every non-provider
    /// package id as a root, so an unrelated extra package id changes
    /// E0's own obligation count, not just its walk cost). E1 must prune
    /// it entirely before `resolve` ever sees it, and the resulting
    /// closure's *retained* obligations must be identical to the case
    /// with no unreachable candidate at all -- proving the pruned
    /// candidate was never going to be selected by E0 either, only
    /// walked.
    #[test]
    fn demand_driven_prunes_a_candidate_unreachable_from_the_demand() {
        let mut input = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let baseline_stats_total = input.package_candidates.len();

        // A package no `ffi_requirements` entry ever names as
        // `expected_provider_package`, and not `demand_entry_point`
        // itself -- genuinely unreachable from the demand.
        input.package_candidates.push(PackageCandidateFacts {
            ecosystem: Ecosystem::C,
            package_id: "unrelated-unreached-package".to_string(),
            version: "9.0.0".to_string(),
            role: Role::Target,
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            sources: vec!["c/unrelated/unrelated.c".to_string()],
            declared_exports: vec![FfiExportFacts {
                declaring_source: "c/unrelated/unrelated.c".to_string(),
                symbol: "unrelated_fn".to_string(),
                abi: "C".to_string(),
                param_count: 0,
                return_type: "i32".to_string(),
            }],
            declared_constraints: vec![],
        });
        input.sources.push(SourceModuleFacts {
            id: "c/unrelated/unrelated.c".to_string(),
            ecosystem: Ecosystem::C,
            package_id: "unrelated-unreached-package".to_string(),
        });

        let (demand_driven, stats) =
            resolve_demand_driven(&input).expect("E1 must still resolve the reachable subgraph");

        assert_eq!(stats.package_candidates_total, baseline_stats_total + 1);
        assert_eq!(
            stats.package_candidates_considered, baseline_stats_total,
            "the unrelated package must be pruned before resolve() ever sees it"
        );
        assert!(
            !demand_driven
                .obligations
                .keys()
                .any(|id| id.contains("unrelated-unreached-package")),
            "a pruned candidate must never appear in the resulting closure at all"
        );

        // The reachable subgraph's own closure is unaffected by the
        // unreachable candidate's presence in the original input.
        let reachable_only = minimal_input(vec![cadd_candidate(
            "1.0.0",
            "c_add",
            "x86_64-unknown-linux-gnu",
            Role::Target,
        )]);
        let eager_on_reachable_only =
            resolve(&reachable_only).expect("E0 on the reachable-only input must resolve");
        assert_eq!(
            serde_json::to_string(&eager_on_reachable_only).unwrap(),
            serde_json::to_string(&demand_driven).unwrap(),
            "pruning an unreachable candidate must not change the retained closure"
        );
    }
}

//! Issue #27's "B" contract: the versioned descriptor an
//! `ActionKind::LowerSource`/`ValidateIr`/`TransformFunction`/
//! `EvaluateEvidence` action carries, plus the artifact-identity scheme
//! those operations use. This is the FIRST implementation boundary the
//! issue names -- an initial hypothesis chosen from this codebase's own
//! existing APIs and retained state (`laminaria-ir`'s `Program`/
//! `TransformError`, this crate's own `Action`/`ArtifactRef`), not copied
//! from any studied reference compiler's own stage list.
//!
//! `nim-planner/src/contract.nim` mirrors the wire shape by hand, the same
//! way it already mirrors [`crate::types`] -- see that module's own doc
//! comment. The Nim planner only ever sees this descriptor as opaque
//! dependency/resource metadata attached to an `Action`; it never
//! constructs one itself, computes an artifact id, or inspects
//! `laminaria-ir`'s own `Program`/IR payload (issue #27 B: "planner is
//! descriptor/依存metadataを受け取り、compiler IR本体を計算しない").
//!
//! ## What issue #27 explicitly leaves undecided (recorded, not silently
//! chosen here)
//!
//! The exact wire enum names / hash-encoding scheme below are this
//! round's own first fixed choice, covered by the change tests in this
//! module -- but issue #27 §"未確定事項と決める場所" item 1 names the
//! specific bytes as still open to a future contract-version bump.
//! Optimal work granularity (item 2), a real memory-accounting/spill
//! design beyond a bare estimate (item 3), dynamic/recursive dependency
//! discovery (item 4), and target-generation format (item 5) are each
//! named there as later decisions, not implemented or assumed here.

use serde::{Deserialize, Serialize};

use crate::types::{Action, ActionKind, ArtifactRef};

pub const COMPILER_WORK_SCHEMA_VERSION: &str = "0.1.0";

/// Which of `checked_inline`/`anf_insert` (`laminaria-ir::transform`) a
/// `TransformFunction` work item runs -- named after those functions
/// directly rather than invented terminology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformKind {
    Anf,
    Checked,
}

impl TransformKind {
    fn as_str(self) -> &'static str {
        match self {
            TransformKind::Anf => "anf",
            TransformKind::Checked => "checked",
        }
    }
}

/// Where a `LowerSource` work item's input actually came from -- kept
/// distinct from `ArtifactRef::Source` (a build-graph-level path
/// reference) because a compiler-work artifact id needs a content
/// snapshot, not a path: two different snapshots at the same path (an
/// edited file) must never share an identity, and this struct is exactly
/// where that snapshot id is recorded before hashing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceProvenanceRef {
    pub source_file: String,
    /// A content identity for the exact source text lowered -- this
    /// slice does not mandate a specific hash algorithm (a full
    /// content-addressing scheme is issue #7/#12's reuse-decision
    /// territory, not this contract's), only that it changes whenever the
    /// text does; callers may pass a real content hash or another stable
    /// per-edit identity, but never a bare path or mtime.
    pub source_snapshot_id: String,
}

/// `TransformFunction`-only parameters: which transform, which version of
/// it, and which caller/callee pair -- exactly the parameters
/// `laminaria_ir::transform::{anf_insert, checked_inline}` themselves take,
/// so a `TransformFunction` work item is a direct, literal request to run
/// one of those two functions, not a paraphrase of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformParameters {
    pub kind: TransformKind,
    pub transform_version: String,
    pub caller: String,
    pub callee: String,
}

/// Resource this work item asks for before it may run -- an *accounted
/// reservation* the future executor (issue #27's stage C) uses for
/// admission control, explicitly not an OS-enforced hard limit (issue
/// #27's own "未確定事項": "この初期予算はaccounted reservation/admission
/// controlであり、OSの厳密なRSS上限ではない").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequest {
    pub cpu_slots: u32,
    /// An *estimate* of transient memory this work item needs while
    /// running (e.g. `syn`'s own parse-time allocations for
    /// `LowerSource`, or a cloned `Program` for `TransformFunction`) --
    /// not a measured or guaranteed figure. See this module's own
    /// top-level doc comment: precision and a real spill/recompute design
    /// are issue #27 stage C's job, not this descriptor's.
    pub transient_memory_bytes_estimate: u64,
}

impl ResourceRequest {
    /// The smallest meaningful request: one CPU slot, no meaningful
    /// transient memory tracked yet. A placeholder for work items this
    /// round doesn't yet estimate a real figure for, not a claim that
    /// zero memory is actually used.
    pub fn minimal() -> Self {
        ResourceRequest {
            cpu_slots: 1,
            transient_memory_bytes_estimate: 0,
        }
    }
}

/// The versioned descriptor attached to a compiler-work `Action`
/// (`ActionKind::LowerSource`/`ValidateIr`/`TransformFunction`/
/// `EvaluateEvidence`). Every field issue #27's B section names as
/// required is here: a schema version, the operation's own implementation
/// version (separate from `descriptor_schema_version`, so the wire shape
/// and one operation's own logic can each version independently), the
/// semantic input artifact id(s) this work depends on, the
/// requested-function/transform parameters, source provenance, and a
/// resource request. `Action.id` itself doubles as this work's identity
/// (`work_id`) -- see [`lower_source_artifact_id`] and its siblings, which
/// *compute* that id rather than letting a caller pick an arbitrary
/// label, per issue #27's "artifact IDはpath/メモリアドレスやwhole-plan
/// digestだけにしない" requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerWorkDescriptor {
    pub descriptor_schema_version: String,
    pub operation_version: String,
    /// The semantic input(s) this work reads -- for `LowerSource`, empty
    /// (its input is `source_provenance` instead); for `ValidateIr`, the
    /// one candidate-Program artifact id; for `TransformFunction`, the one
    /// validated-Program artifact id; for `EvaluateEvidence`, the one
    /// validated (post-transform or post-lowering) artifact id.
    ///
    /// A review caught that nothing tied this list to `Action.inputs` --
    /// the artifacts an executor would actually wait on for readiness
    /// (Buck2's own `build_action_no_redirect`, `action.inputs()`, waited
    /// on via `ensure_artifact_group_staged` before anything runs). This
    /// field alone is *not* the dependency contract; every id here must
    /// also appear among `Action.inputs`'s own `Declared` artifact ids --
    /// enforced by [`validate_compiler_work_action`], not merely declared
    /// here. See that function's own doc comment.
    pub semantic_input_artifact_ids: Vec<String>,
    /// `LowerSource`-only: which functions to lower (mirrors
    /// `laminaria_ir::rust_frontend::lower_rust_source`/
    /// `nim_frontend::lower_nim_source`'s own `requested_functions`
    /// parameter exactly).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requested_functions: Vec<String>,
    /// `LowerSource`-only: which source language (`"rust"`/`"nim"`) --
    /// required so [`lower_source_artifact_id`] can be *recomputed* from
    /// this descriptor alone (a review caught this was previously
    /// discarded after computing the id once, making the id
    /// unverifiable/tamper-blind for `LowerSource` specifically).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The semantic-contract version relevant to this operation: for
    /// `LowerSource`, the declared-subset version; for `ValidateIr`, the
    /// semantic-contract version being validated against; for
    /// `EvaluateEvidence`, the observation-contract version. Unused by
    /// `TransformFunction` (its own version lives on
    /// [`TransformParameters::transform_version`] instead). Named
    /// generically rather than duplicated per-operation because exactly
    /// one of these ever applies to a given descriptor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<TransformParameters>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_provenance: Option<SourceProvenanceRef>,
    /// `EvaluateEvidence`-only: an identity for the finite test-input
    /// values evaluated against, required (alongside `contract_version`)
    /// to recompute [`evaluate_evidence_artifact_id`] from this descriptor
    /// alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_inputs_digest: Option<String>,
    pub resource_request: ResourceRequest,
    /// Identifies which budget/cancellation scope this work belongs to.
    /// The actual admission-control and cancellation *mechanism* is issue
    /// #27 stage C's job (its own acceptance criteria list cancellation/
    /// resource-exhaustion behavior explicitly) -- this field only
    /// carries the reference a future executor will key that mechanism
    /// on, so the wire contract doesn't have to change again once C
    /// implements it.
    pub budget_token: String,
}

/// A structural (non-cryptographic) 64-bit FNV-1a hash -- deliberately
/// hand-rolled instead of `std::collections::hash_map::DefaultHasher`
/// (whose docs explicitly disclaim algorithm stability across Rust
/// versions) or a new external dependency, matching this workspace's
/// existing minimal-dependency stance and
/// `planning_kernel.computePlanId`'s own "structural, not a
/// cache/security-grade content address" framing, applied on the Rust
/// side instead of Nim's `std/hashes`.
struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01B3;

    fn new() -> Self {
        Fnv1a(Self::OFFSET_BASIS)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    /// Writes `s` followed by a `\0` separator, so `write_str("ab");
    /// write_str("c")` cannot collide with `write_str("a");
    /// write_str("bc")` -- plain concatenation without a separator would.
    fn write_str(&mut self, s: &str) {
        self.write(s.as_bytes());
        self.write(&[0]);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Computes a compiler-work artifact id from an operation name, its
/// implementation version, and its full semantic parameter list --
/// **never** from a path, a memory address, or a whole-plan digest (issue
/// #27's own explicit requirement). Two calls with the same `operation`/
/// `operation_version`/`semantic_parts` always produce the same id;
/// changing any one of them changes the id (see this module's own
/// `artifact_id_changes_when_any_semantic_parameter_changes` test, issue
/// #27's required "変更テスト").
fn compute_artifact_id(
    operation: &str,
    operation_version: &str,
    semantic_parts: &[&str],
) -> String {
    let mut hasher = Fnv1a::new();
    hasher.write_str(operation);
    hasher.write_str(operation_version);
    for part in semantic_parts {
        hasher.write_str(part);
    }
    format!("{:016x}", hasher.finish())
}

/// Artifact id for a `LowerSource` work item.
pub fn lower_source_artifact_id(
    operation_version: &str,
    language: &str,
    source_snapshot_id: &str,
    requested_functions: &[&str],
    subset_version: &str,
) -> String {
    let joined_functions = requested_functions.join(",");
    compute_artifact_id(
        "lower_source",
        operation_version,
        &[
            language,
            source_snapshot_id,
            &joined_functions,
            subset_version,
        ],
    )
}

/// Artifact id for a `ValidateIr` work item.
pub fn validate_ir_artifact_id(
    operation_version: &str,
    program_candidate_id: &str,
    semantic_contract_version: &str,
) -> String {
    compute_artifact_id(
        "validate_ir",
        operation_version,
        &[program_candidate_id, semantic_contract_version],
    )
}

/// Artifact id for a `TransformFunction` work item.
pub fn transform_function_artifact_id(
    operation_version: &str,
    validated_program_id: &str,
    caller: &str,
    callee: &str,
    transform_kind: TransformKind,
    transform_version: &str,
) -> String {
    compute_artifact_id(
        "transform_function",
        operation_version,
        &[
            validated_program_id,
            caller,
            callee,
            transform_kind.as_str(),
            transform_version,
        ],
    )
}

/// Artifact id for an `EvaluateEvidence` work item.
pub fn evaluate_evidence_artifact_id(
    operation_version: &str,
    validated_artifact_id: &str,
    test_inputs_digest: &str,
    observation_contract_version: &str,
) -> String {
    compute_artifact_id(
        "evaluate_evidence",
        operation_version,
        &[
            validated_artifact_id,
            test_inputs_digest,
            observation_contract_version,
        ],
    )
}

/// How one compiler-work action's contract can be malformed --
/// independent of the plan-level concerns `validate::ValidationError`
/// already covers (action-set correspondence, ordering, demand). A review
/// caught that this crate computed artifact ids and declared a
/// `semantic_input_artifact_ids` field, but never actually *checked*
/// either against anything -- see [`validate_compiler_work_action`]'s own
/// doc comment for exactly what each variant closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompilerWorkContractError {
    /// `action.kind` is one of the four compiler-work kinds, but
    /// `action.compiler_work` is `None`.
    MissingDescriptor {
        action_id: String,
    },
    /// `action.kind` is a legacy delegated-build kind, but
    /// `action.compiler_work` is `Some` -- a descriptor attached to a kind
    /// that never asked for one.
    UnexpectedDescriptor {
        action_id: String,
    },
    /// A field this operation kind's artifact-id computation requires is
    /// absent from the descriptor.
    MissingRequiredField {
        action_id: String,
        field: &'static str,
    },
    UnsupportedDescriptorSchemaVersion {
        action_id: String,
        got: String,
    },
    /// Recomputing this work's artifact id from the descriptor's own
    /// fields did not match `Action.id` -- either the descriptor was
    /// mutated after the id was computed, or the id was never actually
    /// derived from the descriptor's own content at all.
    WorkIdMismatch {
        action_id: String,
        recomputed: String,
    },
    /// A `semantic_input_artifact_ids` entry does not appear among
    /// `Action.inputs`'s own declared artifact ids -- this work would read
    /// something no dependency-readiness wait ever covers.
    UndeclaredSemanticInput {
        action_id: String,
        artifact_id: String,
    },
    /// This action's own `outputs` never declares `ArtifactRef::Declared`
    /// with `artifact_id == action.id` -- so `action.id` (already verified
    /// to be the real, content-derived identity by `recompute_work_id`) is
    /// never actually *published* as anything a consumer's dependency
    /// wait, or `semantic_input_artifact_ids` reference, can resolve to.
    /// Without this, a producer's own semantic (content) change -- which
    /// does change its recomputed `action.id` -- would never propagate
    /// into what its output is actually *called* on the wire, so a
    /// downstream consumer's stale reference to the producer's *old*
    /// identity could still silently resolve to this same action's
    /// current output. Requiring the producer's own output artifact id to
    /// *be* its verified identity is what makes a semantic change on one
    /// side of an edge force a real mismatch on the other (Buck2's own
    /// `BuildArtifact` model: an artifact's identity *is* its producing
    /// action's key, `app/buck2_artifact/src/artifact/build_artifact.rs`).
    OutputIdentityNotPublished {
        action_id: String,
    },
}

impl std::fmt::Display for CompilerWorkContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompilerWorkContractError::MissingDescriptor { action_id } => write!(
                f,
                "action {action_id:?} has a compiler-work ActionKind but no compiler_work \
                 descriptor"
            ),
            CompilerWorkContractError::UnexpectedDescriptor { action_id } => write!(
                f,
                "action {action_id:?} has a compiler_work descriptor but a non-compiler-work \
                 ActionKind"
            ),
            CompilerWorkContractError::MissingRequiredField { action_id, field } => write!(
                f,
                "action {action_id:?}'s compiler_work descriptor is missing '{field}', required \
                 to recompute its own artifact id"
            ),
            CompilerWorkContractError::UnsupportedDescriptorSchemaVersion { action_id, got } => {
                write!(
                    f,
                    "action {action_id:?}'s compiler_work.descriptor_schema_version = {got:?}, \
                     expected {COMPILER_WORK_SCHEMA_VERSION:?}"
                )
            }
            CompilerWorkContractError::WorkIdMismatch {
                action_id,
                recomputed,
            } => write!(
                f,
                "action {action_id:?}'s id does not match the artifact id recomputed from its \
                 own compiler_work descriptor ({recomputed:?}) -- the descriptor was likely \
                 mutated after the id was computed, or the id was never derived from it"
            ),
            CompilerWorkContractError::UndeclaredSemanticInput {
                action_id,
                artifact_id,
            } => write!(
                f,
                "action {action_id:?}'s compiler_work.semantic_input_artifact_ids names \
                 {artifact_id:?}, which is not among its own Action.inputs -- this work would \
                 read an artifact no dependency-readiness wait covers"
            ),
            CompilerWorkContractError::OutputIdentityNotPublished { action_id } => write!(
                f,
                "action {action_id:?}'s outputs never declares its own verified id \
                 ({action_id:?}) as a Declared artifact -- a producer's own semantic (content) \
                 change would never propagate into what a consumer can actually resolve its \
                 output to"
            ),
        }
    }
}

impl std::error::Error for CompilerWorkContractError {}

/// Recomputes the artifact id a compiler-work `action` *should* have,
/// purely from its own `descriptor`'s fields -- the same constructor
/// function ([`lower_source_artifact_id`] and its siblings) that ought to
/// have produced `action.id` in the first place. Returns
/// [`CompilerWorkContractError::MissingRequiredField`] if a field this
/// particular operation kind needs is absent, rather than guessing or
/// defaulting it.
fn recompute_work_id(
    kind: ActionKind,
    action_id: &str,
    descriptor: &CompilerWorkDescriptor,
) -> Result<String, CompilerWorkContractError> {
    let missing = |field: &'static str| CompilerWorkContractError::MissingRequiredField {
        action_id: action_id.to_string(),
        field,
    };
    match kind {
        ActionKind::LowerSource => {
            let language = descriptor
                .language
                .as_deref()
                .ok_or_else(|| missing("language"))?;
            let subset_version = descriptor
                .contract_version
                .as_deref()
                .ok_or_else(|| missing("contract_version"))?;
            let source = descriptor
                .source_provenance
                .as_ref()
                .ok_or_else(|| missing("source_provenance"))?;
            let requested: Vec<&str> = descriptor
                .requested_functions
                .iter()
                .map(String::as_str)
                .collect();
            Ok(lower_source_artifact_id(
                &descriptor.operation_version,
                language,
                &source.source_snapshot_id,
                &requested,
                subset_version,
            ))
        }
        ActionKind::ValidateIr => {
            let program_candidate_id = descriptor
                .semantic_input_artifact_ids
                .first()
                .ok_or_else(|| missing("semantic_input_artifact_ids[0]"))?;
            let semantic_contract_version = descriptor
                .contract_version
                .as_deref()
                .ok_or_else(|| missing("contract_version"))?;
            Ok(validate_ir_artifact_id(
                &descriptor.operation_version,
                program_candidate_id,
                semantic_contract_version,
            ))
        }
        ActionKind::TransformFunction => {
            let validated_program_id = descriptor
                .semantic_input_artifact_ids
                .first()
                .ok_or_else(|| missing("semantic_input_artifact_ids[0]"))?;
            let t = descriptor
                .transform
                .as_ref()
                .ok_or_else(|| missing("transform"))?;
            Ok(transform_function_artifact_id(
                &descriptor.operation_version,
                validated_program_id,
                &t.caller,
                &t.callee,
                t.kind,
                &t.transform_version,
            ))
        }
        ActionKind::EvaluateEvidence => {
            let validated_artifact_id = descriptor
                .semantic_input_artifact_ids
                .first()
                .ok_or_else(|| missing("semantic_input_artifact_ids[0]"))?;
            let test_inputs_digest = descriptor
                .test_inputs_digest
                .as_deref()
                .ok_or_else(|| missing("test_inputs_digest"))?;
            let observation_contract_version = descriptor
                .contract_version
                .as_deref()
                .ok_or_else(|| missing("contract_version"))?;
            Ok(evaluate_evidence_artifact_id(
                &descriptor.operation_version,
                validated_artifact_id,
                test_inputs_digest,
                observation_contract_version,
            ))
        }
        ActionKind::NimBuild | ActionKind::CargoBuild | ActionKind::Integrate => {
            unreachable!(
                "recompute_work_id is only ever called after validate_compiler_work_action has \
                 already confirmed action.kind is one of the four compiler-work kinds"
            )
        }
    }
}

/// Checks one action's compiler-work contract in isolation (plan-level
/// concerns -- action-set correspondence, `ordered_actions`, demand --
/// stay `validate::validate`'s job; call this once per action from
/// there). A review of this contract's first slice found it declared the
/// right *fields* but checked none of the following, closed here:
///
/// 1. **Presence per kind**: `action.compiler_work` is `Some` if and only
///    if `action.kind` is one of the four compiler-work kinds -- neither
///    a `LowerSource` action silently missing its descriptor nor a
///    `NimBuild` action carrying an extraneous one.
/// 2. **Schema version**: `descriptor.descriptor_schema_version` must
///    equal this build's own [`COMPILER_WORK_SCHEMA_VERSION`], not
///    merely be present.
/// 3. **Identity, not just declared**: `action.id` must equal
///    [`recompute_work_id`], recomputed from the descriptor's own
///    fields -- a descriptor mutated after its id was computed, or one
///    whose id was never actually derived from it, is caught here rather
///    than trusted by convention (this crate's own doc comments
///    previously only *said* "the id is derived, not chosen," without
///    anything enforcing it).
/// 4. **The producer publishes its own identity**: `action.outputs` must
///    declare `ArtifactRef::Declared` with `artifact_id == action.id` --
///    otherwise a producer's own semantic (content) change, which does
///    change its recomputed `action.id`, would never propagate into what
///    a consumer's dependency wait/`semantic_input_artifact_ids`
///    reference can actually resolve to (see
///    [`CompilerWorkContractError::OutputIdentityNotPublished`]'s own doc
///    comment).
/// 5. **Semantic dependency correspondence**: every entry in
///    `descriptor.semantic_input_artifact_ids` must appear among
///    `action.inputs`'s own declared artifact ids -- ported directly from
///    Buck2's own invariant (`build_action_no_redirect`: `action.inputs()`
///    is exactly what gets staged/waited-on via
///    `ensure_artifact_group_staged` before the action runs at all).
///    Without this check, nothing stopped a work item from naming a
///    semantic input its own declared dependencies never cover.
pub fn validate_compiler_work_action(action: &Action) -> Result<(), CompilerWorkContractError> {
    let is_compiler_work_kind = matches!(
        action.kind,
        ActionKind::LowerSource
            | ActionKind::ValidateIr
            | ActionKind::TransformFunction
            | ActionKind::EvaluateEvidence
    );
    let descriptor = match (&action.compiler_work, is_compiler_work_kind) {
        (None, true) => {
            return Err(CompilerWorkContractError::MissingDescriptor {
                action_id: action.id.clone(),
            })
        }
        (Some(_), false) => {
            return Err(CompilerWorkContractError::UnexpectedDescriptor {
                action_id: action.id.clone(),
            })
        }
        (None, false) => return Ok(()), // an ordinary delegated-build action
        (Some(d), true) => d,
    };

    if descriptor.descriptor_schema_version != COMPILER_WORK_SCHEMA_VERSION {
        return Err(
            CompilerWorkContractError::UnsupportedDescriptorSchemaVersion {
                action_id: action.id.clone(),
                got: descriptor.descriptor_schema_version.clone(),
            },
        );
    }

    let recomputed = recompute_work_id(action.kind, &action.id, descriptor)?;
    if recomputed != action.id {
        return Err(CompilerWorkContractError::WorkIdMismatch {
            action_id: action.id.clone(),
            recomputed,
        });
    }

    // `action.id` is now a verified, content-derived identity -- but that
    // alone doesn't propagate a producer's own semantic change to its
    // consumers unless the producer actually *publishes* that identity as
    // one of its own declared outputs. Without this, a consumer's
    // `semantic_input_artifact_ids`/`inputs` could keep referencing a
    // *stale* id even after the real producer's content (and therefore
    // its real `action.id`) changed, with nothing catching the
    // divergence. See this error variant's own doc comment.
    let publishes_own_identity = action.outputs.iter().any(
        |output| matches!(output, ArtifactRef::Declared { artifact_id } if *artifact_id == action.id),
    );
    if !publishes_own_identity {
        return Err(CompilerWorkContractError::OutputIdentityNotPublished {
            action_id: action.id.clone(),
        });
    }

    let declared_inputs: std::collections::BTreeSet<&str> = action
        .inputs
        .iter()
        .filter_map(|r| match r {
            ArtifactRef::Declared { artifact_id } => Some(artifact_id.as_str()),
            ArtifactRef::Source { .. } => None,
        })
        .collect();
    for semantic_id in &descriptor.semantic_input_artifact_ids {
        if !declared_inputs.contains(semantic_id.as_str()) {
            return Err(CompilerWorkContractError::UndeclaredSemanticInput {
                action_id: action.id.clone(),
                artifact_id: semantic_id.clone(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_produce_the_same_artifact_id() {
        let a = lower_source_artifact_id("0.1.0", "rust", "snap-1", &["f", "g"], "subset-0.1.0");
        let b = lower_source_artifact_id("0.1.0", "rust", "snap-1", &["f", "g"], "subset-0.1.0");
        assert_eq!(a, b);
    }

    /// Issue #27's own required "変更テスト": every semantic parameter
    /// this id is derived from must actually change the id when it
    /// changes -- not just the ones convenient to test.
    #[test]
    fn artifact_id_changes_when_any_semantic_parameter_changes() {
        let base = lower_source_artifact_id("0.1.0", "rust", "snap-1", &["f", "g"], "subset-0.1.0");

        let different_snapshot =
            lower_source_artifact_id("0.1.0", "rust", "snap-2", &["f", "g"], "subset-0.1.0");
        assert_ne!(
            base, different_snapshot,
            "changing the source snapshot must change the id"
        );

        let different_language =
            lower_source_artifact_id("0.1.0", "nim", "snap-1", &["f", "g"], "subset-0.1.0");
        assert_ne!(
            base, different_language,
            "changing the language must change the id"
        );

        let different_functions =
            lower_source_artifact_id("0.1.0", "rust", "snap-1", &["f"], "subset-0.1.0");
        assert_ne!(
            base, different_functions,
            "changing the requested-function set must change the id"
        );

        let different_subset =
            lower_source_artifact_id("0.1.0", "rust", "snap-1", &["f", "g"], "subset-0.2.0");
        assert_ne!(
            base, different_subset,
            "changing the subset version must change the id"
        );

        let different_op_version =
            lower_source_artifact_id("0.2.0", "rust", "snap-1", &["f", "g"], "subset-0.1.0");
        assert_ne!(
            base, different_op_version,
            "changing the operation's own implementation version must change the id"
        );
    }

    /// A source snapshot edit (different content, same path) must change
    /// the id -- checked at the `SourceProvenanceRef`-consuming level,
    /// not just the raw hash function, matching issue #27's own worked
    /// concern about identity that survives an edit.
    #[test]
    fn editing_source_text_changes_the_lower_source_artifact_id_even_at_the_same_path() {
        let before =
            lower_source_artifact_id("0.1.0", "rust", "hash-of-v1-text", &["f"], "subset-0.1.0");
        let after =
            lower_source_artifact_id("0.1.0", "rust", "hash-of-v2-text", &["f"], "subset-0.1.0");
        assert_ne!(before, after);
    }

    #[test]
    fn different_operations_with_otherwise_identical_parts_do_not_collide() {
        let lower = compute_artifact_id("lower_source", "0.1.0", &["a", "b"]);
        let validate = compute_artifact_id("validate_ir", "0.1.0", &["a", "b"]);
        assert_ne!(
            lower, validate,
            "the operation name must namespace the id, not just its own parameters"
        );
    }

    /// The separator in `write_str` must prevent a concatenation collision
    /// between two different part-boundaries that happen to concatenate
    /// to the same string.
    #[test]
    fn semantic_part_boundaries_do_not_collide_under_concatenation() {
        let a = compute_artifact_id("op", "v1", &["ab", "c"]);
        let b = compute_artifact_id("op", "v1", &["a", "bc"]);
        assert_ne!(a, b);
    }

    #[test]
    fn transform_function_artifact_id_distinguishes_anf_from_checked() {
        let anf = transform_function_artifact_id(
            "0.1.0",
            "prog-1",
            "caller",
            "callee",
            TransformKind::Anf,
            "0.1.0",
        );
        let checked = transform_function_artifact_id(
            "0.1.0",
            "prog-1",
            "caller",
            "callee",
            TransformKind::Checked,
            "0.1.0",
        );
        assert_ne!(anf, checked);
    }

    #[test]
    fn compiler_work_descriptor_round_trips_through_the_chosen_wire_shape() {
        let descriptor = CompilerWorkDescriptor {
            descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
            operation_version: "0.1.0".to_string(),
            semantic_input_artifact_ids: vec!["a1".to_string()],
            requested_functions: vec!["f".to_string()],
            language: Some("rust".to_string()),
            contract_version: Some("0.1.0".to_string()),
            transform: Some(TransformParameters {
                kind: TransformKind::Checked,
                transform_version: "0.1.0".to_string(),
                caller: "caller".to_string(),
                callee: "callee".to_string(),
            }),
            source_provenance: Some(SourceProvenanceRef {
                source_file: "src/f.rs".to_string(),
                source_snapshot_id: "hash-1".to_string(),
            }),
            test_inputs_digest: Some("digest-1".to_string()),
            resource_request: ResourceRequest::minimal(),
            budget_token: "budget-1".to_string(),
        };
        let json = serde_json::to_value(&descriptor).unwrap();
        assert_eq!(
            json["transform"]["kind"], "checked",
            "TransformKind must serialize snake_case, matching every other enum in this contract"
        );
        let round_tripped: CompilerWorkDescriptor = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, descriptor);
    }

    /// An `Action` with no compiler-work descriptor (every existing
    /// delegated-build `ActionKind`) must serialize with no
    /// `compiler_work` key at all, not `"compiler_work": null` --
    /// preserving byte-for-byte compatibility with the exact JSON
    /// `laminaria-planner` already emits for those actions today, so this
    /// extension cannot silently change existing wire output.
    #[test]
    fn an_action_with_no_compiler_work_omits_the_field_entirely() {
        let action = crate::types::Action {
            id: "a".to_string(),
            kind: crate::types::ActionKind::NimBuild,
            command_identity: "nim c".to_string(),
            inputs: vec![],
            outputs: vec![],
            compiler_work: None,
        };
        let json = serde_json::to_value(&action).unwrap();
        assert!(
            json.get("compiler_work").is_none(),
            "expected no compiler_work key at all, got {json}"
        );
    }

    /// A review of this contract's first slice found it declared fields
    /// and computed ids, but validated neither against the other -- these
    /// tests close that gap, one failure mode at a time.
    mod validate_compiler_work_action_tests {
        use super::*;
        use crate::types::{Action, ActionKind, ArtifactRef};

        /// A well-formed `TransformFunction` action whose `id` is the
        /// *actual* recomputed artifact id -- every other test in this
        /// module starts from this and mutates exactly one thing.
        fn valid_transform_action() -> Action {
            let validated_program_id = "prog-1".to_string();
            let descriptor = CompilerWorkDescriptor {
                descriptor_schema_version: COMPILER_WORK_SCHEMA_VERSION.to_string(),
                operation_version: "0.1.0".to_string(),
                semantic_input_artifact_ids: vec![validated_program_id.clone()],
                requested_functions: vec![],
                language: None,
                contract_version: None,
                transform: Some(TransformParameters {
                    kind: TransformKind::Checked,
                    transform_version: "0.1.0".to_string(),
                    caller: "caller".to_string(),
                    callee: "callee".to_string(),
                }),
                source_provenance: None,
                test_inputs_digest: None,
                resource_request: ResourceRequest::minimal(),
                budget_token: "budget-1".to_string(),
            };
            let id = transform_function_artifact_id(
                "0.1.0",
                &validated_program_id,
                "caller",
                "callee",
                TransformKind::Checked,
                "0.1.0",
            );
            Action {
                outputs: vec![ArtifactRef::declared(&id)],
                id,
                kind: ActionKind::TransformFunction,
                command_identity: "transform_function".to_string(),
                inputs: vec![ArtifactRef::declared(&validated_program_id)],
                compiler_work: Some(descriptor),
            }
        }

        /// 正常往復: a well-formed action, built the same way any real
        /// constructor would, passes outright.
        #[test]
        fn a_well_formed_compiler_work_action_validates() {
            assert_eq!(
                validate_compiler_work_action(&valid_transform_action()),
                Ok(())
            );
        }

        /// 欠落 (missing), direction 1: a compiler-work `ActionKind` with
        /// no descriptor at all.
        #[test]
        fn a_compiler_work_kind_with_no_descriptor_is_rejected() {
            let mut action = valid_transform_action();
            action.compiler_work = None;
            assert_eq!(
                validate_compiler_work_action(&action),
                Err(CompilerWorkContractError::MissingDescriptor {
                    action_id: action.id.clone()
                })
            );
        }

        /// 欠落, direction 2: a legacy delegated-build `ActionKind`
        /// carrying an extraneous descriptor.
        #[test]
        fn a_legacy_action_kind_with_a_descriptor_is_rejected() {
            let mut action = valid_transform_action();
            action.kind = ActionKind::NimBuild;
            assert_eq!(
                validate_compiler_work_action(&action),
                Err(CompilerWorkContractError::UnexpectedDescriptor {
                    action_id: action.id.clone()
                })
            );
        }

        /// 欠落, direction 3: present, but missing the one field this
        /// specific operation kind's own id computation needs.
        #[test]
        fn a_transform_function_action_missing_its_transform_field_is_rejected() {
            let mut action = valid_transform_action();
            action.compiler_work.as_mut().unwrap().transform = None;
            assert_eq!(
                validate_compiler_work_action(&action),
                Err(CompilerWorkContractError::MissingRequiredField {
                    action_id: action.id.clone(),
                    field: "transform",
                })
            );
        }

        /// A legacy delegated-build action with no descriptor at all
        /// (the overwhelmingly common case) must still validate -- this
        /// function must not accidentally start requiring a descriptor
        /// everywhere.
        #[test]
        fn an_ordinary_delegated_build_action_with_no_descriptor_still_validates() {
            let action = Action {
                id: "a".to_string(),
                kind: ActionKind::NimBuild,
                command_identity: "nim c".to_string(),
                inputs: vec![],
                outputs: vec![],
                compiler_work: None,
            };
            assert_eq!(validate_compiler_work_action(&action), Ok(()));
        }

        /// version不整合: a descriptor whose own schema version doesn't
        /// match this build's `COMPILER_WORK_SCHEMA_VERSION`.
        #[test]
        fn a_mismatched_descriptor_schema_version_is_rejected() {
            let mut action = valid_transform_action();
            action
                .compiler_work
                .as_mut()
                .unwrap()
                .descriptor_schema_version = "9.9.9".to_string();
            assert_eq!(
                validate_compiler_work_action(&action),
                Err(
                    CompilerWorkContractError::UnsupportedDescriptorSchemaVersion {
                        action_id: action.id.clone(),
                        got: "9.9.9".to_string(),
                    }
                )
            );
        }

        /// descriptor改変 (tampered descriptor): the id was computed
        /// honestly, but the descriptor was mutated afterward (here,
        /// `callee`) -- the recomputed id no longer matches `Action.id`,
        /// exactly the "was this id actually derived from this content"
        /// question `WorkIdMismatch` exists to answer.
        #[test]
        fn a_descriptor_mutated_after_its_id_was_computed_is_rejected() {
            let mut action = valid_transform_action();
            action
                .compiler_work
                .as_mut()
                .unwrap()
                .transform
                .as_mut()
                .unwrap()
                .callee = "a-different-callee".to_string();
            match validate_compiler_work_action(&action) {
                Err(CompilerWorkContractError::WorkIdMismatch {
                    action_id,
                    recomputed,
                }) => {
                    assert_eq!(action_id, action.id);
                    assert_ne!(recomputed, action.id, "the whole point: it must differ");
                }
                other => panic!("expected WorkIdMismatch, got {other:?}"),
            }
        }

        /// descriptor改変, the other direction: `Action.id` itself is an
        /// arbitrary label, never actually derived from the descriptor's
        /// real content at all -- the exact hazard this crate's own doc
        /// comments warned about ("the id is derived, not chosen") without
        /// anything having enforced it before this fix.
        #[test]
        fn an_arbitrary_hand_picked_action_id_is_rejected() {
            let mut action = valid_transform_action();
            action.id = "not-actually-derived-from-anything".to_string();
            assert!(matches!(
                validate_compiler_work_action(&action),
                Err(CompilerWorkContractError::WorkIdMismatch { .. })
            ));
        }

        /// 意味依存不一致: `semantic_input_artifact_ids` names an id that
        /// `Action.inputs` never declares -- ported from Buck2's own
        /// `action.inputs()`/`ensure_artifact_group_staged` invariant
        /// (issue #27's reference table): what an operation actually
        /// reads must be covered by what a real executor would wait on
        /// for readiness before running it at all.
        #[test]
        fn a_semantic_input_not_covered_by_declared_action_inputs_is_rejected() {
            let mut action = valid_transform_action();
            action.inputs = vec![]; // no longer declares "prog-1" as an input
            assert_eq!(
                validate_compiler_work_action(&action),
                Err(CompilerWorkContractError::UndeclaredSemanticInput {
                    action_id: action.id.clone(),
                    artifact_id: "prog-1".to_string(),
                })
            );
        }

        /// A review caught this exact gap: a producer's own verified,
        /// content-derived `id` must actually be *published* as one of
        /// its own declared outputs, or a producer's semantic change
        /// (which does change its recomputed id) never propagates into
        /// what a stale downstream reference would resolve to.
        #[test]
        fn a_producer_whose_outputs_never_publish_its_own_verified_id_is_rejected() {
            let mut action = valid_transform_action();
            // A real, unrelated logical name -- not this action's own
            // verified id -- exactly the shape a real bug reproduced.
            action.outputs = vec![ArtifactRef::declared("out-1")];
            assert_eq!(
                validate_compiler_work_action(&action),
                Err(CompilerWorkContractError::OutputIdentityNotPublished {
                    action_id: action.id.clone(),
                })
            );
        }

        /// The positive control: an *additional* output alongside the
        /// action's own verified id is fine -- the requirement is that
        /// the id is published *somewhere* in `outputs`, not that it is
        /// the *only* one.
        #[test]
        fn an_additional_output_alongside_the_published_identity_still_validates() {
            let mut action = valid_transform_action();
            action
                .outputs
                .push(ArtifactRef::declared("a-secondary-output"));
            assert_eq!(validate_compiler_work_action(&action), Ok(()));
        }
    }
}

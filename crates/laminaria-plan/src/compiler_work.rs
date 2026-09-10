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
    pub semantic_input_artifact_ids: Vec<String>,
    /// `LowerSource`-only: which functions to lower (mirrors
    /// `laminaria_ir::rust_frontend::lower_rust_source`/
    /// `nim_frontend::lower_nim_source`'s own `requested_functions`
    /// parameter exactly).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requested_functions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<TransformParameters>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_provenance: Option<SourceProvenanceRef>,
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
}

## Versioned JSON contract for LAMINARIA's Nim Planning Kernel (issue #8):
## `plan(PlanningInput) -> ExecutionPlan`, per `docs/research-foundations.md`
## section 7's `PlanningInput`/`ExecutionPlan` naming.
##
## This module mirrors `crates/laminaria-plan/src/types.rs` field-for-field.
## Every JSON key here is a literal snake_case string chosen to match that
## Rust module's serde output exactly (Rust's default derive already
## lowercases/underscores field names the same way), since this contract
## crosses the Rust<->Nim subprocess boundary as JSON over stdin/stdout
## (`docs/self-build.md`) -- there is no shared schema-generation tool, so
## the two sides are kept in sync by hand and by the shared fixtures used
## in both `nimble test` and `cargo test -p laminaria-plan`.
##
## Dependency edges are deliberately *not* a hand-declared `depends_on`
## list: following Buck2's real model (`BuildArtifact` carries its
## producing `ActionKey`, `app/buck2_artifact/src/artifact/build_artifact.rs`
## in `.reference/buck2`), an `Action` here only declares `inputs`/
## `outputs` as `ArtifactRef`s, and `planning_kernel.plan` derives the
## dependency graph itself by matching inputs against declared outputs.

import std/[json, tables, sequtils, options]

## `PlanSchemaVersion` bumped 0.1.0 -> 0.2.0 for issue #27's B: four new
## `ActionKind` values plus `Action`'s new optional `compilerWork` field
## mirror `crates/laminaria-plan/src/{types,compiler_work}.rs` exactly --
## see that crate's own `PLAN_SCHEMA_VERSION` doc comment for why this is
## bumped even though every existing field/variant is unchanged.
const PlanSchemaVersion* = "0.2.0"
const ProducedBy* = "laminaria-nim-planning-kernel"
const CompilerWorkSchemaVersion* = "0.1.0"

type
  ArtifactRefKind* = enum
    arkSource = "source"
    arkDeclared = "declared"

  ArtifactRef* = object
    case kind*: ArtifactRefKind
    of arkSource:
      path*: string
        ## An external, pre-existing input (e.g. a source directory) --
        ## a leaf with no producing action, the same role a
        ## `SourceArtifact` plays in Buck2/Bazel's own artifact model.
    of arkDeclared:
      artifactId*: string
        ## A logical artifact id that some action in the same
        ## `PlanningInput` must declare as one of its `outputs` --
        ## resolving this is what turns two actions into a dependency
        ## edge (Buck2's `BuildArtifact.key()` lookup, ported to an
        ## upfront, whole-graph resolution instead of a live one).

  ActionKind* = enum
    akNimBuild = "nim_build"
    akCargoBuild = "cargo_build"
    akIntegrate = "integrate"
    ## Issue #27 B's own compiler-work kinds: LAMINARIA's owned pipeline
    ## (`laminaria-ir`'s frontends/transforms), never an existing
    ## compiler/backend invocation -- see
    ## `crates/laminaria-plan/src/compiler_work.rs`'s own doc comment.
    ## This kernel never constructs one of these itself, or inspects the
    ## `compilerWork` descriptor's contents beyond round-tripping it --
    ## dependency edges still come purely from `inputs`/`outputs`
    ## matching, unchanged by which `ActionKind` an action has.
    akLowerSource = "lower_source"
    akValidateIr = "validate_ir"
    akTransformFunction = "transform_function"
    akEvaluateEvidence = "evaluate_evidence"

  TransformKind* = enum
    tkAnf = "anf"
    tkChecked = "checked"

  SourceProvenanceRef* = object
    sourceFile*: string
    sourceSnapshotId*: string

  TransformParameters* = object
    kind*: TransformKind
    transformVersion*: string
    caller*: string
    callee*: string

  ResourceRequest* = object
    cpuSlots*: int
    transientMemoryBytesEstimate*: int64

  CompilerWorkDescriptor* = object
    descriptorSchemaVersion*: string
    operationVersion*: string
    semanticInputArtifactIds*: seq[string]
    requestedFunctions*: seq[string]
    transform*: Option[TransformParameters]
    sourceProvenance*: Option[SourceProvenanceRef]
    resourceRequest*: ResourceRequest
    budgetToken*: string

  Action* = object
    id*: string
    kind*: ActionKind
    commandIdentity*: string
      ## The logical command this action represents (e.g. "cargo build
      ## --workspace --release"), for evidence/display -- not necessarily
      ## the literal argv the Rust executor ends up running.
    inputs*: seq[ArtifactRef]
    outputs*: seq[ArtifactRef]
    compilerWork*: Option[CompilerWorkDescriptor]
      ## Present only for the four compiler-work `ActionKind`s above --
      ## `none` (and omitted from the JSON entirely, never emitted as
      ## `null`) for every existing delegated-build action, matching
      ## `crates/laminaria-plan/src/types.rs`'s own `Action.compiler_work`
      ## exactly.

  PlanningInput* = object
    schemaVersion*: string
    demandedArtifacts*: seq[string]
      ## Artifact ids the caller actually wants produced -- recorded for
      ## evidence today; this first slice's self-build always demands
      ## everything `actions` declares, so demand-driven pruning (issue
      ## #8's variant-explosion scope) is explicitly not implemented yet.
    actions*: seq[Action]

  ExecutionPlan* = object
    schemaVersion*: string
    producedBy*: string
      ## Always `ProducedBy` on a real Nim-produced plan -- Rust's
      ## `laminaria_plan::validate` checks this so a plan that didn't
      ## actually come from this Nim binary can never be silently
      ## accepted as if it had (issue #8: "a Rust replacement planner...
      ## cannot satisfy this slice").
    producerVersion*: string
    planId*: string
      ## A structural (non-cryptographic) hash of the canonicalized
      ## input, for lineage/evidence recording only -- see
      ## `planning_kernel.computePlanId`'s own doc comment for why this
      ## is an honest LAMINARIA-specific addition, not a copied idiom
      ## from any of the four studied reference projects.
    orderedActions*: seq[string]
      ## A deterministic topological order over `actions`' ids.
    actions*: Table[string, Action]

  RejectionReasonKind* = enum
    rrkCycle = "cycle"
    rrkUnsupportedInput = "unsupported_input"
    rrkMissingProducer = "missing_producer"
    rrkDuplicateProducer = "duplicate_producer"
    rrkInvalidContractVersion = "invalid_contract_version"

  PlanRejection* = object
    schemaVersion*: string
    reasonKind*: RejectionReasonKind
    reasonDetail*: string
    cyclePath*: seq[string]
      ## Populated only when `reasonKind == rrkCycle`, rendered the same
      ## way Nx's `findCycle` reports one (`a -> b -> c -> a`,
      ## `packages/nx/src/tasks-runner/task-graph-utils.ts` in
      ## `.reference/nx`) -- empty for every other reason kind.

  PlanOutcome* = object
    case isPlanned*: bool
    of true:
      plan*: ExecutionPlan
    of false:
      rejection*: PlanRejection

# --- Constructors -----------------------------------------------------

proc sourceRef*(path: string): ArtifactRef = ArtifactRef(kind: arkSource, path: path)
proc declaredRef*(artifactId: string): ArtifactRef = ArtifactRef(kind: arkDeclared, artifactId: artifactId)

proc planned*(plan: ExecutionPlan): PlanOutcome = PlanOutcome(isPlanned: true, plan: plan)

proc reject*(kind: RejectionReasonKind, detail: string, cyclePath: seq[string] = @[]): PlanRejection =
  PlanRejection(
    schemaVersion: PlanSchemaVersion,
    reasonKind: kind,
    reasonDetail: detail,
    cyclePath: cyclePath,
  )

proc rejected*(rejection: PlanRejection): PlanOutcome =
  PlanOutcome(isPlanned: false, rejection: rejection)

# --- JSON encoding ------------------------------------------------------

proc toJson*(a: ArtifactRef): JsonNode =
  case a.kind
  of arkSource:
    %*{"kind": "source", "path": a.path}
  of arkDeclared:
    %*{"kind": "declared", "artifact_id": a.artifactId}

proc toJson*(t: TransformParameters): JsonNode =
  %*{
    "kind": $t.kind,
    "transform_version": t.transformVersion,
    "caller": t.caller,
    "callee": t.callee,
  }

proc toJson*(s: SourceProvenanceRef): JsonNode =
  %*{
    "source_file": s.sourceFile,
    "source_snapshot_id": s.sourceSnapshotId,
  }

proc toJson*(r: ResourceRequest): JsonNode =
  %*{
    "cpu_slots": r.cpuSlots,
    "transient_memory_bytes_estimate": r.transientMemoryBytesEstimate,
  }

proc toJson*(d: CompilerWorkDescriptor): JsonNode =
  result = %*{
    "descriptor_schema_version": d.descriptorSchemaVersion,
    "operation_version": d.operationVersion,
    "semantic_input_artifact_ids": d.semanticInputArtifactIds,
    "resource_request": d.resourceRequest.toJson,
    "budget_token": d.budgetToken,
  }
  # `requested_functions` mirrors Rust's own
  # `#[serde(default, skip_serializing_if = "Vec::is_empty")]` -- omitted
  # entirely when empty, not emitted as `[]`.
  if d.requestedFunctions.len > 0:
    result["requested_functions"] = %d.requestedFunctions
  if d.transform.isSome:
    result["transform"] = d.transform.get.toJson
  if d.sourceProvenance.isSome:
    result["source_provenance"] = d.sourceProvenance.get.toJson

proc toJson*(a: Action): JsonNode =
  result = %*{
    "id": a.id,
    "kind": $a.kind,
    "command_identity": a.commandIdentity,
    "inputs": a.inputs.map_it(it.toJson),
    "outputs": a.outputs.map_it(it.toJson),
  }
  # Mirrors Rust's `#[serde(default, skip_serializing_if =
  # "Option::is_none")]` on `Action.compiler_work` -- omitted entirely
  # when absent, so this extension changes no byte of the JSON this
  # kernel already emits for every existing delegated-build action.
  if a.compilerWork.isSome:
    result["compiler_work"] = a.compilerWork.get.toJson

proc toJson*(i: PlanningInput): JsonNode =
  %*{
    "schema_version": i.schemaVersion,
    "demanded_artifacts": i.demandedArtifacts,
    "actions": i.actions.map_it(it.toJson),
  }

proc toJson*(p: ExecutionPlan): JsonNode =
  var actionsNode = newJObject()
  for id, action in p.actions:
    actionsNode[id] = action.toJson
  %*{
    "schema_version": p.schemaVersion,
    "produced_by": p.producedBy,
    "producer_version": p.producerVersion,
    "plan_id": p.planId,
    "ordered_actions": p.orderedActions,
    "actions": actionsNode,
  }

proc toJson*(r: PlanRejection): JsonNode =
  %*{
    "schema_version": r.schemaVersion,
    "reason_kind": $r.reasonKind,
    "reason_detail": r.reasonDetail,
    "cycle_path": r.cyclePath,
  }

proc toJson*(o: PlanOutcome): JsonNode =
  if o.isPlanned:
    %*{"outcome": "planned", "data": o.plan.toJson}
  else:
    %*{"outcome": "rejected", "data": o.rejection.toJson}

# --- JSON decoding --------------------------------------------------------
#
# Deliberately hand-written (not a generic `to(JsonNode, T)` macro): every
# malformed/missing field must become a catchable, specific error so
# `main.nim` can tell "well-formed input this kernel rejects" (exit 0,
# a `PlanRejection`) apart from "input this kernel could not even parse"
# (exit nonzero) -- see that module's doc comment.

type ContractError* = object of ValueError
  ## Raised for any structurally invalid PlanningInput JSON -- caught only
  ## by `main.nim`, never by `plan()` itself (`plan()` receives an
  ## already-decoded `PlanningInput`).

proc expectField(node: JsonNode, key: string): JsonNode =
  if node.kind != JObject or not node.hasKey(key):
    raise newException(ContractError, "missing required field '" & key & "'")
  node[key]

proc getStrField(node: JsonNode, key: string): string =
  let field = node.expectField(key)
  if field.kind != JString:
    raise newException(ContractError, "field '" & key & "' must be a string")
  field.getStr()

proc getStrSeqField(node: JsonNode, key: string): seq[string] =
  let field = node.expectField(key)
  if field.kind != JArray:
    raise newException(ContractError, "field '" & key & "' must be an array")
  result = @[]
  for item in field.elems:
    if item.kind != JString:
      raise newException(ContractError, "field '" & key & "' must be an array of strings")
    result.add(item.getStr())

proc getIntField(node: JsonNode, key: string): BiggestInt =
  let field = node.expectField(key)
  if field.kind != JInt:
    raise newException(ContractError, "field '" & key & "' must be an integer")
  field.getBiggestInt()

proc artifactRefFromJson*(node: JsonNode): ArtifactRef =
  let kind = node.getStrField("kind")
  case kind
  of "source":
    sourceRef(node.getStrField("path"))
  of "declared":
    declaredRef(node.getStrField("artifact_id"))
  else:
    raise newException(ContractError, "unknown ArtifactRef kind '" & kind & "'")

proc transformKindFromJson(s: string): TransformKind =
  case s
  of "anf": tkAnf
  of "checked": tkChecked
  else: raise newException(ContractError, "unknown TransformKind '" & s & "'")

proc transformParametersFromJson(node: JsonNode): TransformParameters =
  TransformParameters(
    kind: transformKindFromJson(node.getStrField("kind")),
    transformVersion: node.getStrField("transform_version"),
    caller: node.getStrField("caller"),
    callee: node.getStrField("callee"),
  )

proc sourceProvenanceRefFromJson(node: JsonNode): SourceProvenanceRef =
  SourceProvenanceRef(
    sourceFile: node.getStrField("source_file"),
    sourceSnapshotId: node.getStrField("source_snapshot_id"),
  )

proc resourceRequestFromJson(node: JsonNode): ResourceRequest =
  ResourceRequest(
    cpuSlots: node.getIntField("cpu_slots").int,
    transientMemoryBytesEstimate: node.getIntField("transient_memory_bytes_estimate").int64,
  )

proc compilerWorkDescriptorFromJson(node: JsonNode): CompilerWorkDescriptor =
  result = CompilerWorkDescriptor(
    descriptorSchemaVersion: node.getStrField("descriptor_schema_version"),
    operationVersion: node.getStrField("operation_version"),
    semanticInputArtifactIds: node.getStrSeqField("semantic_input_artifact_ids"),
    resourceRequest: node.expectField("resource_request").resourceRequestFromJson,
    budgetToken: node.getStrField("budget_token"),
  )
  # `requested_functions` mirrors Rust's own `#[serde(default, ...)]` --
  # absent means empty, not an error.
  if node.hasKey("requested_functions"):
    result.requestedFunctions = node.getStrSeqField("requested_functions")
  else:
    result.requestedFunctions = @[]
  if node.hasKey("transform"):
    result.transform = some(node["transform"].transformParametersFromJson)
  if node.hasKey("source_provenance"):
    result.sourceProvenance = some(node["source_provenance"].sourceProvenanceRefFromJson)

proc actionFromJson*(node: JsonNode): Action =
  let kindStr = node.getStrField("kind")
  let kind =
    case kindStr
    of "nim_build": akNimBuild
    of "cargo_build": akCargoBuild
    of "integrate": akIntegrate
    of "lower_source": akLowerSource
    of "validate_ir": akValidateIr
    of "transform_function": akTransformFunction
    of "evaluate_evidence": akEvaluateEvidence
    else: raise newException(ContractError, "unknown Action kind '" & kindStr & "'")
  let inputsNode = node.expectField("inputs")
  let outputsNode = node.expectField("outputs")
  if inputsNode.kind != JArray or outputsNode.kind != JArray:
    raise newException(ContractError, "'inputs'/'outputs' must be arrays")
  result = Action(
    id: node.getStrField("id"),
    kind: kind,
    commandIdentity: node.getStrField("command_identity"),
    inputs: inputsNode.elems.map_it(it.artifactRefFromJson),
    outputs: outputsNode.elems.map_it(it.artifactRefFromJson),
  )
  if node.hasKey("compiler_work"):
    result.compilerWork = some(node["compiler_work"].compilerWorkDescriptorFromJson)

proc planningInputFromJson*(node: JsonNode): PlanningInput =
  let actionsNode = node.expectField("actions")
  if actionsNode.kind != JArray:
    raise newException(ContractError, "'actions' must be an array")
  PlanningInput(
    schemaVersion: node.getStrField("schema_version"),
    demandedArtifacts: node.getStrSeqField("demanded_artifacts"),
    actions: actionsNode.elems.map_it(it.actionFromJson),
  )

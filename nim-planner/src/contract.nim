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

import std/[json, tables, sequtils]

const PlanSchemaVersion* = "0.1.0"
const ProducedBy* = "laminaria-nim-planning-kernel"

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

  Action* = object
    id*: string
    kind*: ActionKind
    commandIdentity*: string
      ## The logical command this action represents (e.g. "cargo build
      ## --workspace --release"), for evidence/display -- not necessarily
      ## the literal argv the Rust executor ends up running.
    inputs*: seq[ArtifactRef]
    outputs*: seq[ArtifactRef]

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

proc toJson*(a: Action): JsonNode =
  %*{
    "id": a.id,
    "kind": $a.kind,
    "command_identity": a.commandIdentity,
    "inputs": a.inputs.map_it(it.toJson),
    "outputs": a.outputs.map_it(it.toJson),
  }

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

proc artifactRefFromJson*(node: JsonNode): ArtifactRef =
  let kind = node.getStrField("kind")
  case kind
  of "source":
    sourceRef(node.getStrField("path"))
  of "declared":
    declaredRef(node.getStrField("artifact_id"))
  else:
    raise newException(ContractError, "unknown ArtifactRef kind '" & kind & "'")

proc actionFromJson*(node: JsonNode): Action =
  let kindStr = node.getStrField("kind")
  let kind =
    case kindStr
    of "nim_build": akNimBuild
    of "cargo_build": akCargoBuild
    of "integrate": akIntegrate
    else: raise newException(ContractError, "unknown Action kind '" & kindStr & "'")
  let inputsNode = node.expectField("inputs")
  let outputsNode = node.expectField("outputs")
  if inputsNode.kind != JArray or outputsNode.kind != JArray:
    raise newException(ContractError, "'inputs'/'outputs' must be arrays")
  Action(
    id: node.getStrField("id"),
    kind: kind,
    commandIdentity: node.getStrField("command_identity"),
    inputs: inputsNode.elems.map_it(it.artifactRefFromJson),
    outputs: outputsNode.elems.map_it(it.artifactRefFromJson),
  )

proc planningInputFromJson*(node: JsonNode): PlanningInput =
  let actionsNode = node.expectField("actions")
  if actionsNode.kind != JArray:
    raise newException(ContractError, "'actions' must be an array")
  PlanningInput(
    schemaVersion: node.getStrField("schema_version"),
    demandedArtifacts: node.getStrSeqField("demanded_artifacts"),
    actions: actionsNode.elems.map_it(it.actionFromJson),
  )

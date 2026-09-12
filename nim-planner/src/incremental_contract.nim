## Issue #36 T0's accepted session-scoped incremental-planning wire
## protocol (`docs/design/issue-36-t0-incremental-contract.md`, commit
## `9d730ec`), mirroring `crates/laminaria-plan/src/incremental.rs`
## field-for-field by hand -- the same convention `contract.nim` already
## uses for `crates/laminaria-plan/src/types.rs` (see that module's own
## doc comment).
##
## JSON shape matches the Rust side exactly: every command/response/
## event is one flat JSON object with an internal `kind` tag, envelope
## fields present alongside the tag on every variant. See `incremental.rs`'s
## own doc comment for why this shape (not a separate flatten) was chosen.

import std/[json, sequtils, options]
import ./contract

const IncrementalProtocolSchemaVersion* = "0.1.0"
const IncrementalEventSchemaVersion* = "0.1.0"

type
  DemandReference* = object
    artifactId*: string
    requestedBy*: string

  Supersession* = object
    oldActionId*: string
    newActionId*: string

  PlanningEventKind* = enum
    pekDependencyDiscovered = "dependency_discovered"
    pekProducerCompleted = "producer_completed"
    pekProducerFailed = "producer_failed"
    pekDemandRequested = "demand_requested"
    pekDemandCancelled = "demand_cancelled"

  PlanningEvent* = object
    eventId*: string
    sequenceNumber*: uint64
    planningGeneration*: uint64
    emittedAtUnixNs*: uint64
    case kind*: PlanningEventKind
    of pekDependencyDiscovered:
      discoveringActionId*: string
      newActions*: seq[Action]
      supersessions*: seq[Supersession]
      newDemands*: seq[DemandReference]
    of pekProducerCompleted, pekProducerFailed:
      artifactId*: string
      producedByActionId*: string
      failureReason*: string
        ## Only meaningful (and only ever encoded) for `pekProducerFailed`
        ## -- present here too (rather than a separate branch) purely so
        ## `artifactId`/`producedByActionId` can be shared between the
        ## two variants, per Nim's same-name/same-type field-sharing
        ## rule for case objects.
    of pekDemandRequested, pekDemandCancelled:
      demandArtifactId*: string
      requestedBy*: string

  ActionState* = enum
    asDiscovered = "discovered"
    asBlockedDependency = "blocked_dependency"
    asReady = "ready"
    asCompleted = "completed"
    asFailed = "failed"
    asCancelled = "cancelled"
    ## Deliberately no `running` variant -- T0 §6.1: that is a Rust-side
    ## execution-status overlay Nim never tracks or reports.

  IncrementalDiagnosticReason* = enum
    idrStaleGeneration = "stale_generation"
    idrCancelledResult = "cancelled_result"
    idrDependencyFailed = "dependency_failed"

  ActionStateChange* = object
    actionId*: string
    fromState*: Option[ActionState]
    toState*: Option[ActionState]
      ## `none` means this action was retired by a `Supersession`
      ## (`retiredBecauseSupersededBy` is then always `some`).
    retiredBecauseSupersededBy*: Option[string]
    newAction*: Option[Action]

  IncrementalPlannerCommandKind* = enum
    ipckStartSession = "start_session"
    ipckApplyDelta = "apply_delta"
    ipckCloseSession = "close_session"

  IncrementalPlannerCommand* = object
    schemaVersion*: string
    sessionId*: string
    commandIndex*: uint64
    case kind*: IncrementalPlannerCommandKind
    of ipckStartSession:
      initialGraph*: PlanningInput
      initialDemands*: seq[DemandReference]
    of ipckApplyDelta:
      event*: PlanningEvent
    of ipckCloseSession:
      discard

  IncrementalPlannerResponseKind* = enum
    iprkPlanDelta = "plan_delta"
    iprkRejected = "rejected"
    iprkSessionClosed = "session_closed"

  IncrementalPlannerResponse* = object
    schemaVersion*: string
    sessionId*: string
    inReplyToCommandIndex*: uint64
    case kind*: IncrementalPlannerResponseKind
    of iprkPlanDelta:
      planningGeneration*: uint64
      changedActions*: seq[ActionStateChange]
      diagnostic*: Option[IncrementalDiagnosticReason]
    of iprkRejected:
      reasonKind*: RejectionReasonKind
      reasonDetail*: string
      cyclePath*: seq[string]
    of iprkSessionClosed:
      discard

# --- JSON encoding ----------------------------------------------------

proc toJson*(d: DemandReference): JsonNode =
  %*{"artifact_id": d.artifactId, "requested_by": d.requestedBy}

proc toJson*(s: Supersession): JsonNode =
  %*{"old_action_id": s.oldActionId, "new_action_id": s.newActionId}

proc toJson*(e: PlanningEvent): JsonNode =
  result = %*{
    "event_id": e.eventId,
    "sequence_number": e.sequenceNumber,
    "planning_generation": e.planningGeneration,
    "emitted_at_unix_ns": e.emittedAtUnixNs,
    "kind": $e.kind,
  }
  case e.kind
  of pekDependencyDiscovered:
    result["discovering_action_id"] = %e.discoveringActionId
    result["new_actions"] = %e.newActions.map_it(it.toJson)
    result["supersessions"] = %e.supersessions.map_it(it.toJson)
    result["new_demands"] = %e.newDemands.map_it(it.toJson)
  of pekProducerCompleted:
    result["artifact_id"] = %e.artifactId
    result["produced_by_action_id"] = %e.producedByActionId
  of pekProducerFailed:
    result["artifact_id"] = %e.artifactId
    result["produced_by_action_id"] = %e.producedByActionId
    result["failure_reason"] = %e.failureReason
  of pekDemandRequested, pekDemandCancelled:
    result["artifact_id"] = %e.demandArtifactId
    result["requested_by"] = %e.requestedBy

proc toJson*(a: ActionState): JsonNode = %($a)

proc toJson*(c: ActionStateChange): JsonNode =
  result = %*{"action_id": c.actionId}
  if c.fromState.isSome:
    result["from_state"] = c.fromState.get.toJson
  if c.toState.isSome:
    result["to_state"] = c.toState.get.toJson
  if c.retiredBecauseSupersededBy.isSome:
    result["retired_because_superseded_by"] = %c.retiredBecauseSupersededBy.get
  if c.newAction.isSome:
    result["new_action"] = c.newAction.get.toJson

proc toJson*(cmd: IncrementalPlannerCommand): JsonNode =
  result = %*{
    "schema_version": cmd.schemaVersion,
    "session_id": cmd.sessionId,
    "command_index": cmd.commandIndex,
    "kind": $cmd.kind,
  }
  case cmd.kind
  of ipckStartSession:
    result["initial_graph"] = cmd.initialGraph.toJson
    result["initial_demands"] = %cmd.initialDemands.map_it(it.toJson)
  of ipckApplyDelta:
    result["event"] = cmd.event.toJson
  of ipckCloseSession:
    discard

proc toJson*(resp: IncrementalPlannerResponse): JsonNode =
  result = %*{
    "schema_version": resp.schemaVersion,
    "session_id": resp.sessionId,
    "in_reply_to_command_index": resp.inReplyToCommandIndex,
    "kind": $resp.kind,
  }
  case resp.kind
  of iprkPlanDelta:
    result["planning_generation"] = %resp.planningGeneration
    result["changed_actions"] = %resp.changedActions.map_it(it.toJson)
    if resp.diagnostic.isSome:
      result["diagnostic"] = %($resp.diagnostic.get)
  of iprkRejected:
    result["reason_kind"] = %($resp.reasonKind)
    result["reason_detail"] = %resp.reasonDetail
    result["cycle_path"] = %resp.cyclePath
  of iprkSessionClosed:
    discard

# --- JSON decoding ------------------------------------------------------

proc demandReferenceFromJson*(node: JsonNode): DemandReference =
  DemandReference(
    artifactId: node.getStrField("artifact_id"),
    requestedBy: node.getStrField("requested_by"),
  )

proc supersessionFromJson*(node: JsonNode): Supersession =
  Supersession(
    oldActionId: node.getStrField("old_action_id"),
    newActionId: node.getStrField("new_action_id"),
  )

proc getU64Field(node: JsonNode, key: string): uint64 =
  node.getIntField(key).uint64

proc planningEventKindFromJson(s: string): PlanningEventKind =
  case s
  of "dependency_discovered": pekDependencyDiscovered
  of "producer_completed": pekProducerCompleted
  of "producer_failed": pekProducerFailed
  of "demand_requested": pekDemandRequested
  of "demand_cancelled": pekDemandCancelled
  else: raise newException(ContractError, "unknown PlanningEventKind '" & s & "'")

proc planningEventFromJson*(node: JsonNode): PlanningEvent =
  let kind = planningEventKindFromJson(node.getStrField("kind"))
  case kind
  of pekDependencyDiscovered:
    let newActionsNode = node.expectField("new_actions")
    let supersessionsNode = node.expectField("supersessions")
    let newDemandsNode = node.expectField("new_demands")
    result = PlanningEvent(
      eventId: node.getStrField("event_id"),
      sequenceNumber: node.getU64Field("sequence_number"),
      planningGeneration: node.getU64Field("planning_generation"),
      emittedAtUnixNs: node.getU64Field("emitted_at_unix_ns"),
      kind: pekDependencyDiscovered,
      discoveringActionId: node.getStrField("discovering_action_id"),
      newActions: newActionsNode.elems.map_it(it.actionFromJson),
      supersessions: supersessionsNode.elems.map_it(it.supersessionFromJson),
      newDemands: newDemandsNode.elems.map_it(it.demandReferenceFromJson),
    )
  of pekProducerCompleted:
    result = PlanningEvent(
      eventId: node.getStrField("event_id"),
      sequenceNumber: node.getU64Field("sequence_number"),
      planningGeneration: node.getU64Field("planning_generation"),
      emittedAtUnixNs: node.getU64Field("emitted_at_unix_ns"),
      kind: pekProducerCompleted,
      artifactId: node.getStrField("artifact_id"),
      producedByActionId: node.getStrField("produced_by_action_id"),
      failureReason: "",
    )
  of pekProducerFailed:
    result = PlanningEvent(
      eventId: node.getStrField("event_id"),
      sequenceNumber: node.getU64Field("sequence_number"),
      planningGeneration: node.getU64Field("planning_generation"),
      emittedAtUnixNs: node.getU64Field("emitted_at_unix_ns"),
      kind: pekProducerFailed,
      artifactId: node.getStrField("artifact_id"),
      producedByActionId: node.getStrField("produced_by_action_id"),
      failureReason: node.getStrField("failure_reason"),
    )
  of pekDemandRequested:
    result = PlanningEvent(
      eventId: node.getStrField("event_id"),
      sequenceNumber: node.getU64Field("sequence_number"),
      planningGeneration: node.getU64Field("planning_generation"),
      emittedAtUnixNs: node.getU64Field("emitted_at_unix_ns"),
      kind: pekDemandRequested,
      demandArtifactId: node.getStrField("artifact_id"),
      requestedBy: node.getStrField("requested_by"),
    )
  of pekDemandCancelled:
    result = PlanningEvent(
      eventId: node.getStrField("event_id"),
      sequenceNumber: node.getU64Field("sequence_number"),
      planningGeneration: node.getU64Field("planning_generation"),
      emittedAtUnixNs: node.getU64Field("emitted_at_unix_ns"),
      kind: pekDemandCancelled,
      demandArtifactId: node.getStrField("artifact_id"),
      requestedBy: node.getStrField("requested_by"),
    )

proc incrementalPlannerCommandFromJson*(node: JsonNode): IncrementalPlannerCommand =
  let kindStr = node.getStrField("kind")
  let schemaVersion = node.getStrField("schema_version")
  let sessionId = node.getStrField("session_id")
  let commandIndex = node.getU64Field("command_index")
  case kindStr
  of "start_session":
    let initialDemandsNode = node.expectField("initial_demands")
    IncrementalPlannerCommand(
      schemaVersion: schemaVersion,
      sessionId: sessionId,
      commandIndex: commandIndex,
      kind: ipckStartSession,
      initialGraph: node.expectField("initial_graph").planningInputFromJson,
      initialDemands: initialDemandsNode.elems.map_it(it.demandReferenceFromJson),
    )
  of "apply_delta":
    IncrementalPlannerCommand(
      schemaVersion: schemaVersion,
      sessionId: sessionId,
      commandIndex: commandIndex,
      kind: ipckApplyDelta,
      event: node.expectField("event").planningEventFromJson,
    )
  of "close_session":
    IncrementalPlannerCommand(
      schemaVersion: schemaVersion,
      sessionId: sessionId,
      commandIndex: commandIndex,
      kind: ipckCloseSession,
    )
  else:
    raise newException(ContractError, "unknown IncrementalPlannerCommand kind '" & kindStr & "'")

# --- Constructors for responses (mirrors contract.nim's own convention) --

proc planDeltaResponse*(
    sessionId: string,
    inReplyTo: uint64,
    planningGeneration: uint64,
    changedActions: seq[ActionStateChange],
    diagnostic: Option[IncrementalDiagnosticReason] = none(IncrementalDiagnosticReason),
): IncrementalPlannerResponse =
  IncrementalPlannerResponse(
    schemaVersion: IncrementalProtocolSchemaVersion,
    sessionId: sessionId,
    inReplyToCommandIndex: inReplyTo,
    kind: iprkPlanDelta,
    planningGeneration: planningGeneration,
    changedActions: changedActions,
    diagnostic: diagnostic,
  )

proc rejectedResponse*(
    sessionId: string,
    inReplyTo: uint64,
    reasonKind: RejectionReasonKind,
    reasonDetail: string,
    cyclePath: seq[string] = @[],
): IncrementalPlannerResponse =
  IncrementalPlannerResponse(
    schemaVersion: IncrementalProtocolSchemaVersion,
    sessionId: sessionId,
    inReplyToCommandIndex: inReplyTo,
    kind: iprkRejected,
    reasonKind: reasonKind,
    reasonDetail: reasonDetail,
    cyclePath: cyclePath,
  )

proc sessionClosedResponse*(sessionId: string, inReplyTo: uint64): IncrementalPlannerResponse =
  IncrementalPlannerResponse(
    schemaVersion: IncrementalProtocolSchemaVersion,
    sessionId: sessionId,
    inReplyToCommandIndex: inReplyTo,
    kind: iprkSessionClosed,
  )

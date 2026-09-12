## Issue #36 T0's session-scoped incremental planning state machine
## (`docs/design/issue-36-t0-incremental-contract.md`, commit `9d730ec`):
## `IncrementalSession` owns the accumulated graph, demand closure, cycle
## detection, and ready frontier (T0 §2 principle 4) across a whole
## `StartSession`..`CloseSession` session. Reuses `planning_kernel.findCycle`
## (exported for this reuse) -- the same cycle-detection algorithm the
## existing one-shot `plan()` already relies on, not a reimplementation.
##
## `running` is deliberately not a state this module tracks (T0 §6.1):
## dependency closure/cycle/ready-frontier computation never depends on
## whether an action is *currently executing*, only on whether its
## producers have *completed*. The Rust runtime overlays `running` on
## top of whichever action it just received as `ready`.

import std/[tables, sets, options, sequtils, strutils]
import ./contract
import ./planning_kernel
import ./incremental_contract

type
  ActionRecord = object
    action: Action
    state: ActionState
    referenceCount: int
    retiredBy: Option[string] ## `some(newActionId)` once superseded.

  IncrementalSession* = ref object
    sessionId*: string
    planningGeneration: uint64
    records: Table[string, ActionRecord]
      ## Keyed by `Action.id`. Retired (superseded) records are *kept*,
      ## not deleted -- a later event referencing a retired id must still
      ## resolve to something so it can be diagnosed (T0 §3.4's
      ## supersession note: a stale reference to a retired id is
      ## `stale_generation`, not "unknown action").
    producerOf: Table[string, string] ## artifact_id -> action_id, active producers only.
    knownEventIds: HashSet[string]
    closed*: bool
    lastCommandIndex: int64
      ## The most recently *accepted* (envelope-valid) `command_index`
      ## -- `StartSession` itself is always 0 (`checkEnvelope*` enforces
      ## this before a session is even created). Used to enforce strict
      ## `command_index` monotonic contiguity (`checkEnvelope*` again),
      ## a review-caught gap: the wire contract's own identity fields
      ## (schema/session/command-index) were never actually checked by
      ## this kernel or the binary wrapping it.

proc newIncrementalSession*(sessionId: string): IncrementalSession =
  IncrementalSession(
    sessionId: sessionId,
    planningGeneration: 0,
    records: initTable[string, ActionRecord](),
    producerOf: initTable[string, string](),
    knownEventIds: initHashSet[string](),
    closed: false,
    lastCommandIndex: 0,
  )

proc lastCommandIndex*(session: IncrementalSession): int64 = session.lastCommandIndex

proc `lastCommandIndex=`*(session: IncrementalSession, value: int64) =
  session.lastCommandIndex = value

type EnvelopeViolation* = object
  reasonKind*: RejectionReasonKind
  detail*: string

## Checks the wire contract's own fixed identity invariants (T0 §3.1)
## *before* a command is ever dispatched to `startSession`/`applyDelta`/
## `closeSession` -- none of these were previously checked anywhere,
## which a review caught directly by sending real, well-formed-JSON but
## identity-violating commands to the real binary and observing they
## were silently accepted as normal:
##
## 1. `schema_version` must equal `IncrementalProtocolSchemaVersion`
##    exactly (`rrkInvalidContractVersion`, matching the existing
##    one-shot planner's own schema-gate precedent, `planning_kernel.
##    decodePlanningInputOrReject`).
## 2. The *first* command of a session must be `StartSession` at
##    `command_index == 0` -- `session` is `nil` exactly when no
##    `StartSession` has yet been accepted.
## 3. Once a session exists, every subsequent command's `session_id`
##    must equal the one `StartSession` established
##    (`session.sessionId`) exactly.
## 4. `StartSession` is never accepted twice for the same session.
## 5. `command_index` must be strictly sequential: exactly
##    `session.lastCommandIndex + 1`, never skipped or repeated.
##
## Returns `none` when `cmd` may proceed; never mutates `session` itself
## (the caller updates `lastCommandIndex` only after successfully
## dispatching `cmd`, so a rejected-at-the-envelope command never
## advances it -- the caller is expected to resend the correct index).
proc checkEnvelope*(session: IncrementalSession, cmd: IncrementalPlannerCommand): Option[EnvelopeViolation] =
  if cmd.schemaVersion != IncrementalProtocolSchemaVersion:
    return some(EnvelopeViolation(
      reasonKind: rrkInvalidContractVersion,
      detail: "expected schema_version '" & IncrementalProtocolSchemaVersion & "', got '" &
        cmd.schemaVersion & "'",
    ))

  if session.isNil:
    if cmd.kind != ipckStartSession:
      return some(EnvelopeViolation(
        reasonKind: rrkUnsupportedInput,
        detail: "the first command of a session must be StartSession, got '" & $cmd.kind & "'",
      ))
    if cmd.commandIndex != 0'u64:
      return some(EnvelopeViolation(
        reasonKind: rrkUnsupportedInput,
        detail: "StartSession must be command_index 0, got " & $cmd.commandIndex,
      ))
    return none(EnvelopeViolation)

  if cmd.sessionId != session.sessionId:
    return some(EnvelopeViolation(
      reasonKind: rrkUnsupportedInput,
      detail: "session_id mismatch: this session is '" & session.sessionId & "', got '" &
        cmd.sessionId & "'",
    ))
  if cmd.kind == ipckStartSession:
    return some(EnvelopeViolation(
      reasonKind: rrkUnsupportedInput,
      detail: "StartSession received twice for the same session",
    ))
  let expected = session.lastCommandIndex + 1
  if cmd.commandIndex.int64 != expected:
    return some(EnvelopeViolation(
      reasonKind: rrkUnsupportedInput,
      detail: "command_index must be sequential: expected " & $expected & ", got " &
        $cmd.commandIndex,
    ))
  none(EnvelopeViolation)

proc declaredInputIds(action: Action): seq[string] =
  for inp in action.inputs:
    if inp.kind == arkDeclared:
      result.add(inp.artifactId)

proc isReadyAgainst(session: IncrementalSession, action: Action): bool =
  for artifactId in declaredInputIds(action):
    let producerId = session.producerOf.getOrDefault(artifactId, "")
    if producerId == "" or session.records[producerId].state != asCompleted:
      return false
  true

type GraphError = object
  reasonKind: RejectionReasonKind
  detail: string
  cyclePath: seq[string]

## Validates that inserting `newActions` (T0 §1.3/§1.8: never mutating an
## existing `Action`, only ever adding brand-new ones) into the session's
## *active* graph stays well-formed -- no artifact produced twice, no
## unresolved `Declared` input, no dependency cycle. Returns the merged
## producer index on success so the caller doesn't have to recompute it,
## or a `GraphError` on the first violation found. Never mutates
## `session` itself -- validate-then-commit, so a rejected delta leaves
## the live session completely untouched (T0 §3.3).
proc validateAgainstGraph(
    session: IncrementalSession, newActions: seq[Action]
): tuple[producerOf: Table[string, string], error: Option[GraphError]] =
  var candidateProducerOf = session.producerOf
  for na in newActions:
    for output in na.outputs:
      if output.kind == arkDeclared:
        if candidateProducerOf.hasKey(output.artifactId):
          return (
            candidateProducerOf,
            some(GraphError(
              reasonKind: rrkDuplicateProducer,
              detail: "artifact '" & output.artifactId & "' would be declared as an output of " &
                "both '" & candidateProducerOf[output.artifactId] & "' and '" & na.id & "'",
            )),
          )
        candidateProducerOf[output.artifactId] = na.id

  var actionsById = initTable[string, Action]()
  for id, rec in session.records:
    if rec.retiredBy.isNone:
      actionsById[id] = rec.action
  for na in newActions:
    actionsById[na.id] = na

  for na in newActions:
    for inp in na.inputs:
      if inp.kind == arkDeclared and not candidateProducerOf.hasKey(inp.artifactId):
        return (
          candidateProducerOf,
          some(GraphError(
            reasonKind: rrkMissingProducer,
            detail: "action '" & na.id & "' requires artifact '" & inp.artifactId &
              "' which no action in this graph declares as an output",
          )),
        )

  var deps = initTable[string, seq[string]]()
  for id, action in actionsById:
    var actionDeps: seq[string] = @[]
    for inp in action.inputs:
      if inp.kind == arkDeclared:
        let producerId = candidateProducerOf[inp.artifactId]
        if producerId notin actionDeps:
          actionDeps.add(producerId)
    deps[id] = actionDeps

  var allIds: seq[string] = @[]
  for id in actionsById.keys:
    allIds.add(id)
  let cyclePath = findCycle(allIds, deps)
  if cyclePath.len > 0:
    return (
      candidateProducerOf,
      some(GraphError(
        reasonKind: rrkCycle,
        detail: "cyclic dependency detected: " & cyclePath.join(" -> "),
        cyclePath: cyclePath,
      )),
    )

  (candidateProducerOf, none(GraphError))

proc computeInitialState(session: IncrementalSession, action: Action): ActionState =
  if isReadyAgainst(session, action): asReady else: asBlockedDependency

## T0 §3.1: `StartSession` sends exactly one, already-complete
## `PlanningInput` -- this session's first commit. Rejects the same way
## `plan()` would (missing/duplicate producer, cycle) rather than
## silently accepting an already-inconsistent starting graph, but this
## should never actually fire for a well-formed `StartSession` (T0's own
## fix#5: every case's `start_session` must be valid standing alone).
proc startSession*(
    session: IncrementalSession,
    commandIndex: uint64,
    initialGraph: PlanningInput,
    initialDemands: seq[DemandReference],
): IncrementalPlannerResponse =
  let (producerOf, error) = validateAgainstGraph(session, initialGraph.actions)
  if error.isSome:
    let e = error.get
    return rejectedResponse(session.sessionId, commandIndex, e.reasonKind, e.detail, e.cyclePath)

  session.producerOf = producerOf
  for artifactId in initialGraph.demandedArtifacts:
    if not producerOf.hasKey(artifactId):
      return rejectedResponse(
        session.sessionId, commandIndex, rrkMissingProducer,
        "demanded artifact '" & artifactId & "' has no declared producer",
      )

  for action in initialGraph.actions:
    session.records[action.id] = ActionRecord(
      action: action,
      state: session.computeInitialState(action),
      referenceCount: 0,
      retiredBy: none(string),
    )

  if initialDemands.len > 0:
    for d in initialDemands:
      if session.records.hasKey(d.artifactId):
        session.records[d.artifactId].referenceCount.inc()
  else:
    for artifactId in initialGraph.demandedArtifacts:
      if session.records.hasKey(artifactId):
        session.records[artifactId].referenceCount.inc()

  var changed: seq[ActionStateChange] = @[]
  for action in initialGraph.actions:
    changed.add(ActionStateChange(
      actionId: action.id,
      fromState: none(ActionState),
      toState: some(session.records[action.id].state),
      retiredBecauseSupersededBy: none(string),
      newAction: some(action),
    ))
  planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, changed)

## Recomputes every currently-`blockedDependency` action's readiness
## against the session's *current* completed set, transitioning any that
## are now fully satisfied to `ready` -- called after any producer
## completes and after a discovery commits new actions (a brand-new
## action can itself already be ready, e.g. a `Source`-only leaf, T0
## case1's own `AID_LOWER_ADD`).
proc promoteNewlyReady(session: IncrementalSession, candidateIds: seq[string]): seq[ActionStateChange] =
  for id in candidateIds:
    var rec = session.records[id]
    if rec.retiredBy.isSome:
      continue
    if rec.state == asBlockedDependency and session.isReadyAgainst(rec.action):
      rec.state = asReady
      session.records[id] = rec
      result.add(ActionStateChange(
        actionId: id, fromState: some(asBlockedDependency), toState: some(asReady),
      ))
    elif rec.state == asDiscovered:
      rec.state = session.computeInitialState(rec.action)
      session.records[id] = rec

proc applyDependencyDiscovered(
    session: IncrementalSession,
    commandIndex: uint64,
    event: PlanningEvent,
): IncrementalPlannerResponse =
  if event.newActions.len == 0:
    return rejectedResponse(
      session.sessionId, commandIndex, rrkUnsupportedInput,
      "DependencyDiscovered.new_actions must never be empty",
    )

  let allAlreadyKnown = event.newActions.allIt(session.records.hasKey(it.id))
  if allAlreadyKnown:
    let diag =
      if event.planningGeneration < session.planningGeneration: some(idrStaleGeneration)
      else: none(IncrementalDiagnosticReason)
    return planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, @[], diag)

  for sup in event.supersessions:
    if not session.records.hasKey(sup.oldActionId) or session.records[sup.oldActionId].retiredBy.isSome:
      return rejectedResponse(
        session.sessionId, commandIndex, rrkUnsupportedInput,
        "supersession names a non-existent or already-superseded action: '" & sup.oldActionId & "'",
      )
    if not event.newActions.anyIt(it.id == sup.newActionId):
      return rejectedResponse(
        session.sessionId, commandIndex, rrkUnsupportedInput,
        "supersession new_action_id '" & sup.newActionId & "' is not among this event's new_actions",
      )

  let (producerOf, error) = validateAgainstGraph(session, event.newActions)
  if error.isSome:
    let e = error.get
    return rejectedResponse(session.sessionId, commandIndex, e.reasonKind, e.detail, e.cyclePath)

  session.producerOf = producerOf
  var changed: seq[ActionStateChange] = @[]

  var supersededBy = initTable[string, string]()
  for sup in event.supersessions:
    supersededBy[sup.oldActionId] = sup.newActionId

  for na in event.newActions:
    session.records[na.id] = ActionRecord(
      action: na,
      state: session.computeInitialState(na),
      referenceCount: 0,
      retiredBy: none(string),
    )
    changed.add(ActionStateChange(
      actionId: na.id, fromState: none(ActionState), toState: some(session.records[na.id].state),
      newAction: some(na),
    ))

  for oldId, newId in supersededBy:
    var oldRec = session.records[oldId]
    let lastState = oldRec.state
    # Reference count transfers wholesale to the replacement (T0 §8).
    session.records[newId].referenceCount += oldRec.referenceCount
    oldRec.referenceCount = 0
    oldRec.retiredBy = some(newId)
    session.records[oldId] = oldRec
    changed.add(ActionStateChange(
      actionId: oldId, fromState: some(lastState), toState: none(ActionState),
      retiredBecauseSupersededBy: some(newId),
    ))

  for d in event.newDemands:
    if session.records.hasKey(d.artifactId):
      session.records[d.artifactId].referenceCount.inc()

  session.planningGeneration.inc()
  planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, changed)

proc dependentsOf(session: IncrementalSession, producerActionId: string): seq[string] =
  for id, rec in session.records:
    if rec.retiredBy.isSome:
      continue
    for artifactId in declaredInputIds(rec.action):
      if session.producerOf.getOrDefault(artifactId, "") == producerActionId:
        result.add(id)
        break

## Marks every transitive dependent of `failedActionId` as `failed` too
## (issue #27's own "producer失敗時にはconsumerを開始しない" principle,
## reused here: a poisoned action never reaches `ready`).
proc poisonDependents(session: IncrementalSession, failedActionId: string): seq[ActionStateChange] =
  var queue = session.dependentsOf(failedActionId)
  var seen = initHashSet[string]()
  while queue.len > 0:
    let id = queue.pop()
    if id in seen or session.records[id].retiredBy.isSome:
      continue
    seen.incl(id)
    var rec = session.records[id]
    if rec.state in {asCompleted, asFailed, asCancelled}:
      continue
    let fromState = rec.state
    rec.state = asFailed
    session.records[id] = rec
    result.add(ActionStateChange(actionId: id, fromState: some(fromState), toState: some(asFailed)))
    queue.add(session.dependentsOf(id))

proc applyProducerCompleted(
    session: IncrementalSession, commandIndex: uint64, event: PlanningEvent
): IncrementalPlannerResponse =
  let actionId = event.producedByActionId
  if not session.records.hasKey(actionId):
    return planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, @[])

  var rec = session.records[actionId]
  if rec.retiredBy.isSome:
    return planDeltaResponse(
      session.sessionId, commandIndex, session.planningGeneration, @[], some(idrStaleGeneration)
    )
  if rec.state == asCompleted:
    # Content-idempotent duplicate (T0 §7.3/case13): a second,
    # differently-event_id'd completion notice for an already-completed
    # action changes nothing.
    return planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, @[])
  if rec.state == asCancelled:
    rec.state = asCompleted
    session.records[actionId] = rec
    return planDeltaResponse(
      session.sessionId, commandIndex, session.planningGeneration,
      @[ActionStateChange(actionId: actionId, fromState: some(asCancelled), toState: some(asCompleted))],
      some(idrCancelledResult),
    )

  let fromState = rec.state
  rec.state = asCompleted
  session.records[actionId] = rec
  var changed = @[ActionStateChange(actionId: actionId, fromState: some(fromState), toState: some(asCompleted))]
  changed.add(session.promoteNewlyReady(session.dependentsOf(actionId)))
  planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, changed)

proc applyProducerFailed(
    session: IncrementalSession, commandIndex: uint64, event: PlanningEvent
): IncrementalPlannerResponse =
  let actionId = event.producedByActionId
  if not session.records.hasKey(actionId):
    return planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, @[])
  var rec = session.records[actionId]
  if rec.retiredBy.isSome or rec.state in {asCompleted, asFailed, asCancelled}:
    return planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, @[])

  let fromState = rec.state
  rec.state = asFailed
  session.records[actionId] = rec
  var changed = @[ActionStateChange(actionId: actionId, fromState: some(fromState), toState: some(asFailed))]
  changed.add(session.poisonDependents(actionId))
  planDeltaResponse(
    session.sessionId, commandIndex, session.planningGeneration, changed, some(idrDependencyFailed)
  )

proc applyDemandRequested(
    session: IncrementalSession, commandIndex: uint64, event: PlanningEvent
): IncrementalPlannerResponse =
  if session.records.hasKey(event.demandArtifactId):
    session.records[event.demandArtifactId].referenceCount.inc()
  planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, @[])

proc applyDemandCancelled(
    session: IncrementalSession, commandIndex: uint64, event: PlanningEvent
): IncrementalPlannerResponse =
  let id = event.demandArtifactId
  if not session.records.hasKey(id):
    return planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, @[])
  var rec = session.records[id]
  if rec.referenceCount > 0:
    rec.referenceCount.dec()
  var changed: seq[ActionStateChange] = @[]
  if rec.referenceCount == 0 and rec.retiredBy.isNone and rec.state in {asDiscovered, asBlockedDependency, asReady}:
    let fromState = rec.state
    rec.state = asCancelled
    changed.add(ActionStateChange(actionId: id, fromState: some(fromState), toState: some(asCancelled)))
  session.records[id] = rec
  planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, changed)

proc applyDelta*(
    session: IncrementalSession, commandIndex: uint64, event: PlanningEvent
): IncrementalPlannerResponse =
  if session.knownEventIds.contains(event.eventId):
    # T0 §7.1: a byte-identical resend of an already-processed event is
    # silently, harmlessly ignored -- no diagnostic (a genuine "not yet
    # reflected" staleness gets `stale_generation` instead, via the
    # per-kind handlers below, not this exact-id dedup path).
    return planDeltaResponse(session.sessionId, commandIndex, session.planningGeneration, @[])
  session.knownEventIds.incl(event.eventId)

  case event.kind
  of pekDependencyDiscovered: session.applyDependencyDiscovered(commandIndex, event)
  of pekProducerCompleted: session.applyProducerCompleted(commandIndex, event)
  of pekProducerFailed: session.applyProducerFailed(commandIndex, event)
  of pekDemandRequested: session.applyDemandRequested(commandIndex, event)
  of pekDemandCancelled: session.applyDemandCancelled(commandIndex, event)

proc closeSession*(session: IncrementalSession, commandIndex: uint64): IncrementalPlannerResponse =
  session.closed = true
  sessionClosedResponse(session.sessionId, commandIndex)

# --- Read-only accessors for tests/callers -----------------------------

proc stateOf*(session: IncrementalSession, actionId: string): ActionState =
  session.records[actionId].state

proc referenceCountOf*(session: IncrementalSession, actionId: string): int =
  session.records[actionId].referenceCount

proc isRetired*(session: IncrementalSession, actionId: string): bool =
  session.records[actionId].retiredBy.isSome

proc currentGeneration*(session: IncrementalSession): uint64 =
  session.planningGeneration

proc hasAction*(session: IncrementalSession, actionId: string): bool =
  session.records.hasKey(actionId)

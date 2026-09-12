import std/[unittest, options]
import ../src/contract
import ../src/incremental_contract
import ../src/incremental_kernel

proc planningInput(demanded: seq[string], actions: seq[Action]): PlanningInput =
  PlanningInput(schemaVersion: PlanSchemaVersion, demandedArtifacts: demanded, actions: actions)

proc action(id: string, inputs: seq[ArtifactRef] = @[]): Action =
  Action(
    id: id,
    kind: akLowerSource,
    commandIdentity: "test",
    inputs: inputs,
    outputs: @[declaredRef(id)],
  )

proc discoveredEvent(
    sn: uint64, gen: uint64, discoveringId: string, newActions: seq[Action],
    supersessions: seq[Supersession] = @[], newDemands: seq[DemandReference] = @[],
): PlanningEvent =
  PlanningEvent(
    eventId: "evt-" & $sn, sequenceNumber: sn, planningGeneration: gen, emittedAtUnixNs: sn,
    kind: pekDependencyDiscovered, discoveringActionId: discoveringId,
    newActions: newActions, supersessions: supersessions, newDemands: newDemands,
  )

proc completedEvent(seqId: string, sn: uint64, gen: uint64, actionId: string): PlanningEvent =
  PlanningEvent(
    eventId: seqId, sequenceNumber: sn, planningGeneration: gen, emittedAtUnixNs: sn,
    kind: pekProducerCompleted, artifactId: actionId, producedByActionId: actionId,
  )

proc failedEvent(sn: uint64, gen: uint64, actionId: string): PlanningEvent =
  PlanningEvent(
    eventId: "evt-fail-" & $sn, sequenceNumber: sn, planningGeneration: gen, emittedAtUnixNs: sn,
    kind: pekProducerFailed, artifactId: actionId, producedByActionId: actionId,
    failureReason: "boom",
  )

proc demandRequestedEvent(seqId: string, sn: uint64, artifactId, requestedBy: string): PlanningEvent =
  PlanningEvent(
    eventId: seqId, sequenceNumber: sn, planningGeneration: 0, emittedAtUnixNs: sn,
    kind: pekDemandRequested, demandArtifactId: artifactId, requestedBy: requestedBy,
  )

proc demandCancelledEvent(sn: uint64, artifactId, requestedBy: string): PlanningEvent =
  PlanningEvent(
    eventId: "evt-cancel-" & $sn, sequenceNumber: sn, planningGeneration: 0, emittedAtUnixNs: sn,
    kind: pekDemandCancelled, demandArtifactId: artifactId, requestedBy: requestedBy,
  )

suite "incremental_kernel.startSession":
  test "a Source-only leaf starts ready, a Declared-input consumer starts blocked":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    let validate = action("validate", @[declaredRef("lower")])
    let resp = session.startSession(0, planningInput(@["validate"], @[lower, validate]), @[])
    check resp.kind == iprkPlanDelta
    check session.stateOf("lower") == asReady
    check session.stateOf("validate") == asBlockedDependency

  test "a missing producer is rejected, matching plan()'s own diagnosis":
    let session = newIncrementalSession("s1")
    let consumer = action("c", @[declaredRef("nonexistent")])
    let resp = session.startSession(0, planningInput(@["c"], @[consumer]), @[])
    check resp.kind == iprkRejected
    check resp.reasonKind == rrkMissingProducer

  test "a two-action cycle is rejected with the exact cycle path":
    let session = newIncrementalSession("s1")
    let a = action("a", @[declaredRef("b-out")])
    var aOut = a
    aOut.outputs = @[declaredRef("a-out")]
    var b = action("b", @[declaredRef("a-out")])
    b.outputs = @[declaredRef("b-out")]
    let resp = session.startSession(0, planningInput(@["a-out"], @[aOut, b]), @[])
    check resp.kind == iprkRejected
    check resp.reasonKind == rrkCycle
    check resp.cyclePath == @["a", "b", "a"]

  test "initial_demands drives the reference count, not demanded_artifacts alone":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    discard session.startSession(
      0, planningInput(@["lower"], @[lower]),
      @[DemandReference(artifactId: "lower", requestedBy: "consumer-a"),
        DemandReference(artifactId: "lower", requestedBy: "consumer-b")],
    )
    check session.referenceCountOf("lower") == 2

suite "incremental_kernel.applyDelta -- DependencyDiscovered":
  test "a ready branch is unaffected by an unrelated discovery (T0 case1 shape)":
    let session = newIncrementalSession("s1")
    let lowerF = action("lower-f", @[sourceRef("f.rs")])
    let discover = action("discover-add", @[sourceRef("add.rs")])
    discard session.startSession(0, planningInput(@["lower-f", "discover-add"], @[lowerF, discover]), @[])
    check session.stateOf("lower-f") == asReady

    let lowerAdd = action("lower-add", @[sourceRef("add.rs")])
    let validateAdd = action("validate-add", @[declaredRef("lower-add")])
    let resp = session.applyDelta(1, discoveredEvent(1, 0, "discover-add", @[lowerAdd, validateAdd]))
    check resp.kind == iprkPlanDelta
    check session.stateOf("lower-f") == asReady
    check session.stateOf("lower-add") == asReady
    check session.stateOf("validate-add") == asBlockedDependency
    check session.currentGeneration() == 1

  test "re-asserting an already-known new_actions entry is a harmless no-op (T0 case2)":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    let validate = action("validate", @[declaredRef("lower")])
    discard session.startSession(0, planningInput(@["validate"], @[lower, validate]), @[])
    discard session.applyDelta(1, discoveredEvent(1, 0, "x", @[lower]))
    check session.currentGeneration() == 0
    check session.stateOf("lower") == asReady

  test "DependencyDiscovered with an unresolvable dependency is rejected as missing_producer (T0 case8/11)":
    let session = newIncrementalSession("s1")
    let discover = action("discover", @[sourceRef("f.rs")])
    discard session.startSession(0, planningInput(@["discover"], @[discover]), @[])
    var broken = action("broken", @[declaredRef("nonexistent-producer")])
    let resp = session.applyDelta(1, discoveredEvent(1, 0, "discover", @[broken]))
    check resp.kind == iprkRejected
    check resp.reasonKind == rrkMissingProducer
    check not session.hasAction("broken")

  test "DependencyDiscovered introducing a self-contained cycle is rejected (T0 case10)":
    let session = newIncrementalSession("s1")
    let discover = action("discover", @[sourceRef("f.rs")])
    discard session.startSession(0, planningInput(@["discover"], @[discover]), @[])
    var x = action("x", @[declaredRef("y-out")])
    x.outputs = @[declaredRef("x-out")]
    var y = action("y", @[declaredRef("x-out")])
    y.outputs = @[declaredRef("y-out")]
    let resp = session.applyDelta(1, discoveredEvent(1, 0, "discover", @[x, y]))
    check resp.kind == iprkRejected
    check resp.reasonKind == rrkCycle
    check not session.hasAction("x")

  test "a stale-generation event is only diagnosed after a real generation advance (T0 case6)":
    let session = newIncrementalSession("s1")
    let discover = action("discover", @[sourceRef("f.rs")])
    discard session.startSession(0, planningInput(@["discover"], @[discover]), @[])

    let lowerAdd = action("lower-add", @[sourceRef("add.rs")])
    discard session.applyDelta(1, discoveredEvent(1, 0, "discover", @[lowerAdd]))
    check session.currentGeneration() == 1

    # Same new_actions re-asserted, but now labeled generation 0 -- stale
    # relative to the session's real current generation (1), and the
    # content is already fully reflected.
    let stale = discoveredEvent(2, 0, "discover", @[lowerAdd])
    let resp = session.applyDelta(1, stale)
    check resp.kind == iprkPlanDelta
    check resp.diagnostic == some(idrStaleGeneration)
    check resp.changedActions.len == 0

suite "incremental_kernel.applyDelta -- supersession (T0 case12)":
  test "a replacement action retires the old one and transfers its reference count":
    let session = newIncrementalSession("s1")
    let lowerF = action("lower-f", @[sourceRef("f.rs")])
    discard session.startSession(
      0, planningInput(@["lower-f"], @[lowerF]),
      @[DemandReference(artifactId: "lower-f", requestedBy: "consumer-a")],
    )
    check session.referenceCountOf("lower-f") == 1

    let lowerFV2 = action("lower-f-v2", @[sourceRef("f.rs")])
    let resp = session.applyDelta(
      1,
      discoveredEvent(
        1, 0, "discover", @[lowerFV2],
        supersessions = @[Supersession(oldActionId: "lower-f", newActionId: "lower-f-v2")],
      ),
    )
    check resp.kind == iprkPlanDelta
    check session.isRetired("lower-f")
    check session.referenceCountOf("lower-f") == 0
    check session.referenceCountOf("lower-f-v2") == 1
    check session.stateOf("lower-f-v2") == asReady

    var sawRetirement = false
    for change in resp.changedActions:
      if change.actionId == "lower-f":
        check change.toState.isNone
        check change.retiredBecauseSupersededBy == some("lower-f-v2")
        sawRetirement = true
    check sawRetirement

  test "a supersession naming a non-existent old_action_id is rejected":
    let session = newIncrementalSession("s1")
    let discover = action("discover", @[sourceRef("f.rs")])
    discard session.startSession(0, planningInput(@["discover"], @[discover]), @[])
    let newA = action("new-a", @[sourceRef("f.rs")])
    let resp = session.applyDelta(
      1,
      discoveredEvent(
        1, 0, "discover", @[newA],
        supersessions = @[Supersession(oldActionId: "never-existed", newActionId: "new-a")],
      ),
    )
    check resp.kind == iprkRejected
    check resp.reasonKind == rrkUnsupportedInput

suite "incremental_kernel.applyDelta -- ProducerCompleted/Failed":
  test "a producer completing promotes exactly its own blocked dependents to ready, once":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    let validate = action("validate", @[declaredRef("lower")])
    discard session.startSession(0, planningInput(@["validate"], @[lower, validate]), @[])
    check session.stateOf("validate") == asBlockedDependency

    let resp = session.applyDelta(1, completedEvent("evt-c1", 1, 0, "lower"))
    check session.stateOf("lower") == asCompleted
    check session.stateOf("validate") == asReady
    var transitions = 0
    for change in resp.changedActions:
      if change.actionId == "validate" and change.toState == some(asReady):
        transitions.inc()
    check transitions == 1

  test "a duplicate discovery notification does not cause a double transition (T0 case2's second half)":
    let session = newIncrementalSession("s1")
    let lowerAdd = action("lower-add", @[sourceRef("add.rs")])
    let validateAdd = action("validate-add", @[declaredRef("lower-add")])
    discard session.startSession(0, planningInput(@["validate-add"], @[lowerAdd, validateAdd]), @[])
    discard session.applyDelta(1, discoveredEvent(1, 0, "discover", @[lowerAdd])) # duplicate, no-op
    check session.currentGeneration() == 0
    let resp = session.applyDelta(2, completedEvent("evt-c2", 2, 0, "lower-add"))
    check session.stateOf("validate-add") == asReady
    var transitions = 0
    for change in resp.changedActions:
      if change.actionId == "validate-add": transitions.inc()
    check transitions == 1

  test "a producer failing poisons its transitive dependents without ever reaching ready (T0 case9)":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    let validate = action("validate", @[declaredRef("lower")])
    discard session.startSession(0, planningInput(@["validate"], @[lower, validate]), @[])
    let resp = session.applyDelta(1, failedEvent(1, 0, "lower"))
    check session.stateOf("lower") == asFailed
    check session.stateOf("validate") == asFailed
    check resp.diagnostic == some(idrDependencyFailed)

  test "a completion arriving after cancellation is diagnosed cancelled_result, not published (T0 case7)":
    let session = newIncrementalSession("s1")
    let evidence = action("evidence", @[sourceRef("f.rs")])
    discard session.startSession(
      0, planningInput(@["evidence"], @[evidence]),
      @[DemandReference(artifactId: "evidence", requestedBy: "consumer-a")],
    )
    check session.stateOf("evidence") == asReady
    discard session.applyDelta(1, demandCancelledEvent(1, "evidence", "consumer-a"))
    check session.stateOf("evidence") == asCancelled

    let resp = session.applyDelta(2, completedEvent("evt-late", 2, 0, "evidence"))
    check session.stateOf("evidence") == asCompleted
    check resp.diagnostic == some(idrCancelledResult)

  test "a byte-identical event resend (same event_id) is a pure no-op (T0 case4)":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    discard session.startSession(0, planningInput(@["lower"], @[lower]), @[])
    let e = completedEvent("evt-dup", 1, 0, "lower")
    let first = session.applyDelta(1, e)
    let second = session.applyDelta(2, e)
    check first.kind == iprkPlanDelta
    check second.changedActions.len == 0
    check session.stateOf("lower") == asCompleted

  test "content-identical completions under different event_ids stay idempotent by state, not by id (T0 case13)":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    discard session.startSession(0, planningInput(@["lower"], @[lower]), @[])
    discard session.applyDelta(1, completedEvent("evt-retry-a", 1, 0, "lower"))
    let resp = session.applyDelta(2, completedEvent("evt-retry-b", 2, 0, "lower"))
    check session.stateOf("lower") == asCompleted
    check resp.changedActions.len == 0

suite "incremental_kernel.applyDelta -- demand merge/cancel":
  test "two DemandRequested for the same action merge into one reference count of 2 (T0 case3)":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    # No initial demand at all (unlike the other suites here) -- T0 case3
    # itself starts with an empty demanded_artifacts/initial_demands, so
    # the reference count starts genuinely at 0, not 1 from an implicit
    # anonymous demand.
    discard session.startSession(0, planningInput(@[], @[lower]), @[])
    discard session.applyDelta(1, demandRequestedEvent("evt-d1", 1, "lower", "consumer-a"))
    discard session.applyDelta(2, demandRequestedEvent("evt-d2", 2, "lower", "consumer-c"))
    check session.referenceCountOf("lower") == 2

  test "one consumer's cancellation leaves a shared producer alive (T0 case5)":
    let session = newIncrementalSession("s1")
    let lower = action("lower", @[sourceRef("f.rs")])
    discard session.startSession(
      0, planningInput(@["lower"], @[lower]),
      @[DemandReference(artifactId: "lower", requestedBy: "consumer-a"),
        DemandReference(artifactId: "lower", requestedBy: "consumer-b")],
    )
    discard session.applyDelta(1, demandCancelledEvent(1, "lower", "consumer-a"))
    check session.referenceCountOf("lower") == 1
    check session.stateOf("lower") == asReady

suite "incremental_kernel.checkEnvelope -- wire identity (review-caught gap)":
  proc startCmd(schemaVersion: string, sessionId: string, commandIndex: uint64): IncrementalPlannerCommand =
    IncrementalPlannerCommand(
      schemaVersion: schemaVersion, sessionId: sessionId, commandIndex: commandIndex,
      kind: ipckStartSession, initialGraph: planningInput(@[], @[]), initialDemands: @[],
    )

  proc closeCmd(schemaVersion: string, sessionId: string, commandIndex: uint64): IncrementalPlannerCommand =
    IncrementalPlannerCommand(
      schemaVersion: schemaVersion, sessionId: sessionId, commandIndex: commandIndex,
      kind: ipckCloseSession,
    )

  test "a wrong schema_version is rejected as invalid_contract_version, even before any session exists":
    let violation = checkEnvelope(nil, startCmd("wrong-version", "s1", 0))
    check violation.isSome
    check violation.get.reasonKind == rrkInvalidContractVersion

  test "the first command of a session must be StartSession":
    let violation = checkEnvelope(nil, closeCmd(IncrementalProtocolSchemaVersion, "s1", 0))
    check violation.isSome
    check violation.get.reasonKind == rrkUnsupportedInput

  test "StartSession must be command_index 0":
    let violation = checkEnvelope(nil, startCmd(IncrementalProtocolSchemaVersion, "s1", 7))
    check violation.isSome
    check violation.get.reasonKind == rrkUnsupportedInput

  test "a well-formed StartSession at index 0 is accepted":
    let violation = checkEnvelope(nil, startCmd(IncrementalProtocolSchemaVersion, "s1", 0))
    check violation.isNone

  test "a different session_id on a later command is rejected":
    let session = newIncrementalSession("s1")
    let violation = checkEnvelope(session, closeCmd(IncrementalProtocolSchemaVersion, "s2", 1))
    check violation.isSome
    check violation.get.reasonKind == rrkUnsupportedInput

  test "a second StartSession for the same session is rejected":
    let session = newIncrementalSession("s1")
    let violation = checkEnvelope(session, startCmd(IncrementalProtocolSchemaVersion, "s1", 1))
    check violation.isSome
    check violation.get.reasonKind == rrkUnsupportedInput

  test "a non-sequential command_index (skipping ahead) is rejected":
    let session = newIncrementalSession("s1") # lastCommandIndex starts at 0
    let violation = checkEnvelope(session, closeCmd(IncrementalProtocolSchemaVersion, "s1", 99))
    check violation.isSome
    check violation.get.reasonKind == rrkUnsupportedInput

  test "the correct next sequential command_index is accepted":
    let session = newIncrementalSession("s1")
    let violation = checkEnvelope(session, closeCmd(IncrementalProtocolSchemaVersion, "s1", 1))
    check violation.isNone

  test "checkEnvelope never mutates lastCommandIndex itself":
    let session = newIncrementalSession("s1")
    discard checkEnvelope(session, closeCmd(IncrementalProtocolSchemaVersion, "s1", 1))
    check session.lastCommandIndex() == 0

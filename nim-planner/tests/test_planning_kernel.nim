## Issue #8's own acceptance list, as real tests: stable ordering/
## identity, dependency preservation, cycle rejection, missing/
## unsupported inputs, and invalid contract versions -- run against the
## real `plan()`/`planFromJson()` functions, not against a synthetic
## re-implementation of them.

import std/[unittest, json, tables, strutils, options]
import ../src/contract
import ../src/planning_kernel

proc action(id: string, kind: ActionKind, inputs, outputs: seq[ArtifactRef]): Action =
  Action(id: id, kind: kind, commandIdentity: "cmd:" & id, inputs: inputs, outputs: outputs)

proc input(actions: seq[Action], demandedArtifacts: seq[string] = @[]): PlanningInput =
  PlanningInput(schemaVersion: PlanSchemaVersion, demandedArtifacts: demandedArtifacts, actions: actions)

suite "planning_kernel.plan":
  test "a linear chain plans in dependency order":
    # "integrate" now declares its own output ("done") rather than none,
    # and demand names only it -- issue #27's own demand-selection fix
    # means an action producing nothing can never be part of any demand
    # closure (nothing can ever name it to demand it).
    let i = input(@[
      action("integrate", akIntegrate, @[declaredRef("planner-bin"), declaredRef("host-bin")], @[declaredRef("done")]),
      action("compile-rust-host", akCargoBuild, @[], @[declaredRef("host-bin")]),
      action("compile-nim-planner", akNimBuild, @[], @[declaredRef("planner-bin")]),
    ], demandedArtifacts = @["done"])
    let outcome = plan(i)
    check outcome.isPlanned
    check outcome.plan.orderedActions == @["compile-nim-planner", "compile-rust-host", "integrate"]
    check outcome.plan.producedBy == ProducedBy
    check outcome.plan.actions.len == 3

  test "same input plans identically twice (determinism)":
    let i = input(@[
      action("z", akIntegrate, @[declaredRef("o1"), declaredRef("o2")], @[declaredRef("done")]),
      action("y", akCargoBuild, @[], @[declaredRef("o1")]),
      action("x", akNimBuild, @[], @[declaredRef("o2")]),
    ], demandedArtifacts = @["done"])
    let first = plan(i)
    let second = plan(i)
    check first.isPlanned and second.isPlanned
    check first.plan.toJson == second.plan.toJson
    check first.plan.planId == second.plan.planId

  test "a two-action cycle is rejected with the exact cycle path":
    let i = input(@[
      action("a", akNimBuild, @[declaredRef("out-b")], @[declaredRef("out-a")]),
      action("b", akNimBuild, @[declaredRef("out-a")], @[declaredRef("out-b")]),
    ], demandedArtifacts = @["out-a"])
    let outcome = plan(i)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkCycle
    check outcome.rejection.cyclePath == @["a", "b", "a"]

  test "a self-cycle is rejected":
    let i = input(@[
      action("a", akNimBuild, @[declaredRef("out-a")], @[declaredRef("out-a")]),
    ], demandedArtifacts = @["out-a"])
    let outcome = plan(i)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkCycle

  test "an input naming no producer is a structured missing_producer rejection":
    # "a" now declares an output ("out-a") so it can be demanded directly
    # -- an action producing nothing can never be part of any demand
    # closure.
    let i = input(@[
      action("a", akNimBuild, @[declaredRef("nope")], @[declaredRef("out-a")]),
    ], demandedArtifacts = @["out-a"])
    let outcome = plan(i)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkMissingProducer
    check "nope" in outcome.rejection.reasonDetail

  test "two actions declaring the same output artifact is a structured duplicate_producer rejection":
    # Duplicate-producer detection runs over the *whole* input, before
    # any demand-based pruning -- a structural defect in the graph,
    # independent of what's actually demanded -- so this needs no demand
    # at all to still be caught.
    let i = input(@[
      action("a", akNimBuild, @[], @[declaredRef("shared")]),
      action("b", akNimBuild, @[], @[declaredRef("shared")]),
    ])
    let outcome = plan(i)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkDuplicateProducer

  test "a Source input needs no producer and does not block planning":
    let i = input(@[
      action("a", akNimBuild, @[sourceRef("nim-planner/src")], @[declaredRef("planner-bin")]),
    ], demandedArtifacts = @["planner-bin"])
    let outcome = plan(i)
    check outcome.isPlanned
    check outcome.plan.orderedActions == @["a"]

  test "an action nothing demands is pruned from the plan, not rejected":
    # Issue #27's own demand-selection fix: an action `input.actions`
    # lists but nothing in the demand closure reaches is simply excluded
    # from the plan -- not an error, and its own dangling input ("nope",
    # which no action produces) never even gets inspected, because the
    # closure walk never reaches it.
    let i = input(@[
      action("a", akNimBuild, @[], @[declaredRef("out-a")]),
      action("unreachable", akNimBuild, @[declaredRef("nope")], @[declaredRef("out-unreachable")]),
    ], demandedArtifacts = @["out-a"])
    let outcome = plan(i)
    check outcome.isPlanned
    check outcome.plan.orderedActions == @["a"]
    check outcome.plan.actions.len == 1

suite "planning_kernel.planFromJson (schema-version gate)":
  test "a mismatched schema_version is rejected before other fields are inspected":
    let raw = parseJson("""{"schema_version": "9.9.9"}""")
    let outcome = planFromJson(raw)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkInvalidContractVersion

  test "a missing schema_version is rejected as invalid_contract_version, not a generic parse error":
    let raw = parseJson("""{}""")
    let outcome = planFromJson(raw)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkInvalidContractVersion

  test "a well-formed, current-version input plans successfully through planFromJson":
    # `PlanSchemaVersion` interpolated rather than a hardcoded literal, so
    # a future version bump can't leave this test silently testing a
    # stale, now-rejected version.
    let raw = parseJson("""
      {"schema_version": "$1", "demanded_artifacts": ["out-a"], "actions": [
        {"id": "a", "kind": "nim_build", "command_identity": "x", "inputs": [],
         "outputs": [{"kind": "declared", "artifact_id": "out-a"}]}
      ]}
    """ % [PlanSchemaVersion])
    let outcome = planFromJson(raw)
    check outcome.isPlanned
    check outcome.plan.orderedActions == @["a"]

suite "planning_kernel.contract (issue #27 B: compiler-work descriptor)":
  test "an action with no compiler_work omits the field entirely, round-tripped":
    let a = action("a", akNimBuild, @[], @[])
    let json = a.toJson
    check not json.hasKey("compiler_work")
    let decoded = json.actionFromJson
    check decoded.compilerWork.isNone

  test "a compiler-work action's descriptor round-trips through actionFromJson exactly":
    let descriptor = CompilerWorkDescriptor(
      descriptorSchemaVersion: CompilerWorkSchemaVersion,
      operationVersion: "0.1.0",
      semanticInputArtifactIds: @["prog-1"],
      requestedFunctions: @["f", "g"],
      transform: some(TransformParameters(
        kind: tkChecked,
        transformVersion: "0.1.0",
        caller: "caller",
        callee: "callee",
      )),
      sourceProvenance: some(SourceProvenanceRef(
        sourceFile: "src/f.rs",
        sourceSnapshotId: "hash-1",
      )),
      resourceRequest: ResourceRequest(cpuSlots: 1, transientMemoryBytesEstimate: 4096),
      budgetToken: "budget-1",
    )
    var a = action("t", akTransformFunction, @[], @[declaredRef("out")])
    a.compilerWork = some(descriptor)

    let json = a.toJson
    check json["kind"].getStr == "transform_function"
    check json["compiler_work"]["transform"]["kind"].getStr == "checked"

    let decoded = json.actionFromJson
    check decoded.kind == akTransformFunction
    check decoded.compilerWork.isSome
    check decoded.compilerWork.get == descriptor

  test "language/contract_version/test_inputs round-trip (previously declared but not wired into toJson/fromJson)":
    let descriptor = CompilerWorkDescriptor(
      descriptorSchemaVersion: CompilerWorkSchemaVersion,
      operationVersion: "0.1.0",
      semanticInputArtifactIds: @[],
      requestedFunctions: @["f"],
      language: some("rust"),
      contractVersion: some("0.1.0"),
      sourceProvenance: some(SourceProvenanceRef(
        sourceFile: "src/f.rs",
        sourceSnapshotId: "hash-1",
      )),
      testInputs: @[@[1'i64, 2'i64], @[int64(low(int32)), int64(high(int32))]],
      resourceRequest: ResourceRequest(cpuSlots: 1, transientMemoryBytesEstimate: 4096),
      budgetToken: "budget-1",
    )
    var a = action("l", akLowerSource, @[], @[declaredRef("out")])
    a.compilerWork = some(descriptor)

    let json = a.toJson
    check json["compiler_work"]["language"].getStr == "rust"
    check json["compiler_work"]["contract_version"].getStr == "0.1.0"
    check json["compiler_work"]["test_inputs"].len == 2

    let decoded = json.actionFromJson
    check decoded.compilerWork.get == descriptor

  test "an unknown Action kind is a ContractError, not a silent default":
    let raw = parseJson("""
      {"id": "a", "kind": "not_a_real_kind", "command_identity": "x", "inputs": [], "outputs": []}
    """)
    expect(ContractError):
      discard raw.actionFromJson

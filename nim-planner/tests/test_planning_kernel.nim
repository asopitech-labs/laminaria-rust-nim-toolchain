## Issue #8's own acceptance list, as real tests: stable ordering/
## identity, dependency preservation, cycle rejection, missing/
## unsupported inputs, and invalid contract versions -- run against the
## real `plan()`/`planFromJson()` functions, not against a synthetic
## re-implementation of them.

import std/[unittest, json, tables, strutils]
import ../src/contract
import ../src/planning_kernel

proc action(id: string, kind: ActionKind, inputs, outputs: seq[ArtifactRef]): Action =
  Action(id: id, kind: kind, commandIdentity: "cmd:" & id, inputs: inputs, outputs: outputs)

proc input(actions: seq[Action]): PlanningInput =
  PlanningInput(schemaVersion: PlanSchemaVersion, demandedArtifacts: @[], actions: actions)

suite "planning_kernel.plan":
  test "a linear chain plans in dependency order":
    let i = input(@[
      action("integrate", akIntegrate, @[declaredRef("planner-bin"), declaredRef("host-bin")], @[]),
      action("compile-rust-host", akCargoBuild, @[], @[declaredRef("host-bin")]),
      action("compile-nim-planner", akNimBuild, @[], @[declaredRef("planner-bin")]),
    ])
    let outcome = plan(i)
    check outcome.isPlanned
    check outcome.plan.orderedActions == @["compile-nim-planner", "compile-rust-host", "integrate"]
    check outcome.plan.producedBy == ProducedBy
    check outcome.plan.actions.len == 3

  test "same input plans identically twice (determinism)":
    let i = input(@[
      action("z", akIntegrate, @[declaredRef("o1"), declaredRef("o2")], @[]),
      action("y", akCargoBuild, @[], @[declaredRef("o1")]),
      action("x", akNimBuild, @[], @[declaredRef("o2")]),
    ])
    let first = plan(i)
    let second = plan(i)
    check first.isPlanned and second.isPlanned
    check first.plan.toJson == second.plan.toJson
    check first.plan.planId == second.plan.planId

  test "a two-action cycle is rejected with the exact cycle path":
    let i = input(@[
      action("a", akNimBuild, @[declaredRef("out-b")], @[declaredRef("out-a")]),
      action("b", akNimBuild, @[declaredRef("out-a")], @[declaredRef("out-b")]),
    ])
    let outcome = plan(i)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkCycle
    check outcome.rejection.cyclePath == @["a", "b", "a"]

  test "a self-cycle is rejected":
    let i = input(@[
      action("a", akNimBuild, @[declaredRef("out-a")], @[declaredRef("out-a")]),
    ])
    let outcome = plan(i)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkCycle

  test "an input naming no producer is a structured missing_producer rejection":
    let i = input(@[
      action("a", akNimBuild, @[declaredRef("nope")], @[]),
    ])
    let outcome = plan(i)
    check not outcome.isPlanned
    check outcome.rejection.reasonKind == rrkMissingProducer
    check "nope" in outcome.rejection.reasonDetail

  test "two actions declaring the same output artifact is a structured duplicate_producer rejection":
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
    ])
    let outcome = plan(i)
    check outcome.isPlanned
    check outcome.plan.orderedActions == @["a"]

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
    let raw = parseJson("""
      {"schema_version": "0.1.0", "demanded_artifacts": [], "actions": [
        {"id": "a", "kind": "nim_build", "command_identity": "x", "inputs": [], "outputs": []}
      ]}
    """)
    let outcome = planFromJson(raw)
    check outcome.isPlanned
    check outcome.plan.orderedActions == @["a"]

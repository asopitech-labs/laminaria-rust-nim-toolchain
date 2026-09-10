## Entry point for the `laminaria-planner` binary (issue #8): reads a
## `PlanningInput` JSON document from stdin, calls the deterministic
## planning kernel, and writes the resulting `ExecutionPlan` or
## `PlanRejection` (both wrapped in `PlanOutcome`) as JSON to stdout.
##
## Exit code contract, deliberately narrow: 0 for *both* a successful
## plan and a well-formed rejection (a rejection is a valid, complete
## answer to "can this be planned?" -- issue #8's own acceptance
## criteria ask for cycles/unsupported input/version mismatches to be
## *rejected*, not crashed on). Only a genuinely unparseable stdin
## document (not valid JSON, or valid JSON missing/misshaping a required
## field before even the schema-version gate can run) exits nonzero with
## a plain-text message on stderr -- `crates/laminaria-plan`'s
## `nim_planner_client` treats that as "the planner itself could not be
## invoked," a different failure mode than "the planner rejected this
## input."
##
## No filesystem, network, or environment access here beyond stdin/
## stdout themselves -- everything the kernel needs is already inside
## the parsed `PlanningInput` (issue #8's "no side effects" requirement).

import std/json
import ./contract
import ./planning_kernel

proc main() =
  let raw =
    try:
      stdin.readAll()
    except IOError as e:
      stderr.writeLine("laminaria-planner: failed to read stdin: " & e.msg)
      quit(1)

  let root =
    try:
      parseJson(raw)
    except JsonParsingError as e:
      stderr.writeLine("laminaria-planner: stdin is not valid JSON: " & e.msg)
      quit(1)

  let outcome =
    try:
      planFromJson(root)
    except ContractError as e:
      stderr.writeLine("laminaria-planner: malformed PlanningInput: " & e.msg)
      quit(1)

  stdout.writeLine($outcome.toJson)
  quit(0)

when isMainModule:
  main()

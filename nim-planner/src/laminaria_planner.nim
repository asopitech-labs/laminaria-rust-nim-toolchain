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

import std/[json, monotimes, times, options]
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

  ## Schema-version gate + JSON decode first (same order/behavior
  ## `planFromJson` itself uses), *outside* the timed interval below --
  ## issue #28 D1-a's own measurement review found the previous version
  ## of this file timed the combined `planFromJson` (gate + decode +
  ## `plan`), wider than `docs/design/issue-35-d0-cases.yaml`'s
  ## `M8-many-unrequested-nim-planner` case's own confirmed
  ## `measurement_boundary` ("計測開始はplanning_kernel.plan呼び出し直前、
  ## 終了はExecutionPlan受領直後"). `decodePlanningInputOrReject` is the
  ## same schema-gate/decode step `planFromJson` uses internally, split
  ## out so it can run un-timed here.
  let (maybeInput, maybeRejection) =
    try:
      decodePlanningInputOrReject(root)
    except ContractError as e:
      stderr.writeLine("laminaria-planner: malformed PlanningInput: " & e.msg)
      quit(1)

  let outcome =
    if maybeRejection.isSome:
      maybeRejection.get
    else:
      ## Wall time spent inside `plan` itself only -- excludes stdin
      ## read, JSON parse, the schema gate, and `PlanningInput` decode
      ## above. Reported on stderr as an additive observation only --
      ## the stdout `PlanOutcome` JSON wire contract is byte-for-byte
      ## unchanged, so this changes nothing for any existing caller/test
      ## that only reads stdout.
      let kernelStart = getMonoTime()
      let planned = plan(maybeInput.get)
      let kernelElapsed = getMonoTime() - kernelStart
      stderr.writeLine("laminaria-planner: kernel_nanos=" & $inNanoseconds(kernelElapsed))
      planned

  stdout.writeLine($outcome.toJson)
  quit(0)

when isMainModule:
  main()

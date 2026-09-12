## Entry point for the `laminaria-incremental-planner` binary (issue #36
## T0/T1): a session-scoped process, distinct from `laminaria_planner.nim`'s
## own one-shot `laminaria-planner` binary.
##
## Deliberately a *separate* binary rather than rewriting
## `laminaria_planner.nim`'s own `main()` in place: the accepted T0
## contract's own §2 principle 1 describes `laminaria_planner.nim`'s
## `main()` becoming session-scoped, but every existing caller of the
## one-shot binary (`crates/laminaria-plan/src/nim_planner_client.rs`,
## every self-build/project-build Rust test that plans through it) relies
## on its current read-once/plan-once/quit(0) behavior today. Changing
## that binary's own default behavior would risk regressing all of that
## for a benefit (the new session protocol) those callers never asked
## for. A new binary keeps the existing one-shot contract completely
## untouched while adding the new session-scoped one -- which binary
## file happens to implement §2 principle 1's *process*-level shape is
## an implementation-order choice T1 is free to make (the instructor's
## own T1 task message: "実装順序は作業者に委ねます...方式は指定しません").
##
## Protocol (`incremental_contract.nim`): reads one `IncrementalPlannerCommand`
## JSON object per stdin line, dispatches it to a live `IncrementalSession`,
## writes the resulting `IncrementalPlannerResponse` JSON as one stdout
## line, flushed immediately (so a supervising Rust process reading
## line-by-line never blocks past when a response is actually ready).
## Exits only after processing `CloseSession`, or immediately (nonzero,
## matching `laminaria_planner.nim`'s own exit-code convention) if a
## command line cannot even be parsed -- a well-formed but *rejected*
## delta is a normal, zero-exit response, never a process failure.

import std/[json]
import ./contract
import ./incremental_contract
import ./incremental_kernel

proc main() =
  var session: IncrementalSession = nil

  while true:
    var line: string
    try:
      line = stdin.readLine()
    except EOFError:
      break
    except IOError as e:
      stderr.writeLine("laminaria-incremental-planner: failed to read stdin: " & e.msg)
      quit(1)

    if line.len == 0:
      continue

    let root =
      try:
        parseJson(line)
      except JsonParsingError as e:
        stderr.writeLine("laminaria-incremental-planner: stdin line is not valid JSON: " & e.msg)
        quit(1)

    let cmd =
      try:
        root.incrementalPlannerCommandFromJson
      except ContractError as e:
        stderr.writeLine("laminaria-incremental-planner: malformed command: " & e.msg)
        quit(1)

    if session.isNil:
      if cmd.kind != ipckStartSession:
        stderr.writeLine(
          "laminaria-incremental-planner: the first command of a session must be StartSession, got '" &
            $cmd.kind & "'"
        )
        quit(1)
      session = newIncrementalSession(cmd.sessionId)

    let response =
      case cmd.kind
      of ipckStartSession:
        session.startSession(cmd.commandIndex, cmd.initialGraph, cmd.initialDemands)
      of ipckApplyDelta:
        session.applyDelta(cmd.commandIndex, cmd.event)
      of ipckCloseSession:
        session.closeSession(cmd.commandIndex)

    stdout.writeLine($response.toJson)
    stdout.flushFile()

    if cmd.kind == ipckCloseSession:
      break

  quit(0)

when isMainModule:
  main()

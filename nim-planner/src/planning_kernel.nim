## The deterministic `plan(PlanningInput) -> ExecutionPlan` function
## itself (issue #8). No filesystem, network, process, or environment
## access anywhere in this module -- everything it needs arrives already
## resolved inside `PlanningInput` (issue #8: "the planner performs no
## process launch, environment probing, filesystem or network side
## effects").
##
## Every non-trivial step below is a named port of a real, studied
## reference-project algorithm (`.reference/buck2`, `.reference/bazel`,
## `.reference/pants`, `.reference/nx` -- see the approved plan's
## "Grounded design choices" section and `docs/self-build.md`), not an
## invented one:
## - dependency edges are derived from artifact producer/consumer
##   matching, the way Buck2's `BuildArtifact` carries its producing
##   `ActionKey` (`app/buck2_artifact/src/artifact/build_artifact.rs`);
## - cycle detection is an explicit path-tracking DFS, the same shape as
##   Bazel's `SimpleCycleDetector` (`skyframe/SimpleCycleDetector.java`)
##   and Nx's `findCycle`/`_findCycle`
##   (`packages/nx/src/tasks-runner/task-graph-utils.ts`);
## - the final order is Kahn's algorithm with a lexicographic tie-break,
##   ported from Nx's `walkTaskGraph` (same file), which is what makes
##   "same input -> identical output" a structural guarantee rather than
##   an accident of table/hash iteration order.

import std/[tables, algorithm, strutils, hashes, json, options]
import ./contract

proc findCycle(actionIds: seq[string], deps: Table[string, seq[string]]): seq[string] =
  ## Explicit path-tracking DFS (Bazel `SimpleCycleDetector` / Nx
  ## `findCycle` shape): the moment a node already on the *current* DFS
  ## path is revisited, the cycle is the suffix of `path` starting at
  ## that node's first occurrence, plus the revisited node again to
  ## close the loop -- exactly how Nx renders `a -> b -> c -> a`.
  var visited = initTable[string, bool]()
  var onPath = initTable[string, bool]()
  var path: seq[string] = @[]

  proc visit(id: string): seq[string] =
    if onPath.getOrDefault(id, false):
      let idx = path.find(id)
      return path[idx .. ^1] & @[id]
    if visited.getOrDefault(id, false):
      return @[]
    visited[id] = true
    onPath[id] = true
    path.add(id)
    for dep in deps.getOrDefault(id, @[]):
      let found = visit(dep)
      if found.len > 0:
        return found
    discard path.pop()
    onPath[id] = false
    return @[]

  # Deterministic scan order (sorted ids, not declaration order) so that,
  # among several independent cycles, which one is reported is itself
  # stable across runs -- same determinism concern as the topo-sort tie
  # break below.
  var sortedIds = actionIds
  sortedIds.sort()
  for id in sortedIds:
    if not visited.getOrDefault(id, false):
      let found = visit(id)
      if found.len > 0:
        return found
  return @[]

proc topoSort(actionIds: seq[string], deps: Table[string, seq[string]]): seq[string] =
  ## Kahn's algorithm, ported from Nx's `walkTaskGraph`
  ## (`packages/nx/src/tasks-runner/task-graph-utils.ts`): repeatedly
  ## take the lexicographically-smallest-id action with no unemitted
  ## dependency left, emit it, and decrement its dependents' remaining
  ## count. Assumes the graph is already known acyclic (`findCycle` must
  ## be called first) -- every id is guaranteed to be emitted exactly
  ## once.
  var dependents = initTable[string, seq[string]]()
  var remaining = initTable[string, int]()
  for id in actionIds:
    remaining[id] = deps.getOrDefault(id, @[]).len
    discard dependents.hasKeyOrPut(id, @[])
  for id in actionIds:
    for dep in deps.getOrDefault(id, @[]):
      dependents.mgetOrPut(dep, @[]).add(id)

  var ready: seq[string] = @[]
  for id in actionIds:
    if remaining[id] == 0:
      ready.add(id)
  ready.sort()

  result = @[]
  while ready.len > 0:
    let next = ready[0]
    ready.delete(0)
    result.add(next)
    for dependent in dependents.getOrDefault(next, @[]):
      remaining[dependent] -= 1
      if remaining[dependent] == 0:
        ready.add(dependent)
    ready.sort()

proc computePlanId(input: PlanningInput): string =
  ## A structural (non-cryptographic) hash of the canonicalized input's
  ## JSON, using `std/hashes` -- deliberately *not* claimed as matching
  ## any of the four studied reference projects' own node-identity
  ## scheme, because none of them keep a single whole-graph digest as
  ## their primary identity (Pants: structural `Eq+Hash`/interning, no
  ## `Digest` type in `rule_graph` at all; Bazel: path-based `Artifact`
  ## identity, digest is a separate change-detection value; Buck2: each
  ## *action* gets its own RE `ActionDigest`, no single digest for a
  ## whole `ActionGraph`; Nx: `Task.id` is a plain string, `Task.hash` is
  ## a separate per-task cache value). This field exists only because
  ## issue #6 asks for a recorded "plan ID" for lineage/evidence in
  ## `laminaria-run`'s Run records -- a real, deterministic identity for
  ## exactly that purpose, not a cache/security-grade content address.
  let canonical = $input.toJson
  let h = hash(canonical).int64
  toHex(h, 16).toLowerAscii()

proc plan*(input: PlanningInput): PlanOutcome =
  # Step 2 (schema-version gate is step 1, done in main.nim before this
  # function is ever reached -- see contract.nim's ExecutionPlan doc
  # comment and main.nim's own doc comment for why that ordering matters):
  # build the producer index from every action's declared outputs,
  # following Buck2's `BuildArtifact` model (an output carries its
  # producer's identity) rather than a hand-declared `depends_on` list.
  # Built over *every* action in `input.actions`, before any
  # demand-based pruning below: two actions racing to produce the same
  # artifact is a structural defect in the whole graph, independent of
  # what's actually demanded.
  var producerOf = initTable[string, string]() # artifact_id -> action_id
  var actionsById = initTable[string, Action]()
  for action in input.actions:
    actionsById[action.id] = action
    for output in action.outputs:
      if output.kind == arkDeclared:
        if producerOf.hasKey(output.artifactId):
          return rejected(reject(
            rrkDuplicateProducer,
            "artifact '" & output.artifactId & "' is declared as an output of both '" &
              producerOf[output.artifactId] & "' and '" & action.id & "'",
          ))
        producerOf[output.artifactId] = action.id

  var seenIds = initTable[string, bool]()
  for action in input.actions:
    if seenIds.getOrDefault(action.id, false):
      return rejected(reject(rrkUnsupportedInput, "duplicate action id '" & action.id & "'"))
    seenIds[action.id] = true

  # Step 3: issue #27's own demand-selection fix -- select only the
  # backward dependency closure of `input.demandedArtifacts`, the same
  # way Buck2 only ever runs an artifact's producing action and
  # everything *it* needs (`action.inputs()`, `build_action_no_redirect`).
  # An action `input.actions` lists but nothing demanded (directly or
  # transitively) ever reaches is simply never planned -- not an error,
  # just correctly pruned. A `Declared` input that matches no action's
  # output, discovered while walking this closure, is a structured
  # `missing_producer` rejection naming the exact artifact id, exactly
  # as it always was -- just now only checked for actions the closure
  # actually needs, not every action `input.actions` happens to list.
  var neededIds = initTable[string, bool]()
  var queue: seq[string] = @[]
  for artifactId in input.demandedArtifacts:
    if not producerOf.hasKey(artifactId):
      return rejected(reject(
        rrkMissingProducer,
        "PlanningInput demands artifact '" & artifactId &
          "' which no action declares as an output",
      ))
    let producerId = producerOf[artifactId]
    if not neededIds.getOrDefault(producerId, false):
      neededIds[producerId] = true
      queue.add(producerId)
  while queue.len > 0:
    let id = queue.pop()
    for inp in actionsById[id].inputs:
      if inp.kind == arkDeclared:
        if not producerOf.hasKey(inp.artifactId):
          return rejected(reject(
            rrkMissingProducer,
            "action '" & id & "' requires artifact '" & inp.artifactId &
              "' which no action in this PlanningInput declares as an output",
          ))
        let producerId = producerOf[inp.artifactId]
        if not neededIds.getOrDefault(producerId, false):
          neededIds[producerId] = true
          queue.add(producerId)

  var actionIds: seq[string] = @[]
  for id in neededIds.keys:
    actionIds.add(id)

  # Step 4: resolve every *needed* action's inputs to a producer action
  # id -- the same resolution the closure walk above already performed,
  # recomputed here (over only the needed subset) so `deps` reflects
  # exactly the actions actually being planned.
  var deps = initTable[string, seq[string]]() # action_id -> [action_id it depends on]
  for id in actionIds:
    var actionDeps: seq[string] = @[]
    for inp in actionsById[id].inputs:
      if inp.kind == arkDeclared:
        let producerId = producerOf[inp.artifactId]
        # Deliberately *not* excluding `producerId == action.id`: an
        # action that consumes its own declared output is a genuine
        # (trivial) cycle -- it cannot run before its own output exists
        # -- and must be reported as such by `findCycle` below, not
        # silently treated as "no dependency." Unlike Pants's rule_graph
        # (which allows structural self-edges for recursive `Get`s via
        # its `Reentry` mechanism, `.reference/pants/src/rust/rule_graph`),
        # this flat action graph has no such polymorphic re-entry case.
        if producerId notin actionDeps:
          actionDeps.add(producerId)
    deps[id] = actionDeps

  # Step 5: cycle check, restricted to the needed subgraph -- a cycle
  # entirely among actions nothing demands is irrelevant to this
  # request, the same way Buck2 never cares about a target you didn't
  # ask to build.
  let cyclePath = findCycle(actionIds, deps)
  if cyclePath.len > 0:
    return rejected(reject(
      rrkCycle,
      "cyclic dependency detected: " & cyclePath.join(" -> "),
      cyclePath,
    ))

  # Step 6: deterministic topological order over the needed subgraph.
  let ordered = topoSort(actionIds, deps)

  var neededActionsById = initTable[string, Action]()
  for id in actionIds:
    neededActionsById[id] = actionsById[id]

  planned(ExecutionPlan(
    schemaVersion: PlanSchemaVersion,
    producedBy: ProducedBy,
    producerVersion: PlanSchemaVersion,
    planId: computePlanId(input),
    orderedActions: ordered,
    actions: neededActionsById,
  ))

proc decodePlanningInputOrReject*(root: JsonNode): tuple[input: Option[PlanningInput], rejection: Option[PlanOutcome]] =
  ## Step 1 of `planFromJson`, split out (issue #28 D1-a's own
  ## measurement review): the `schema_version` gate, checked directly on
  ## the raw JSON node *before* any other field is decoded into a
  ## `PlanningInput` -- so a mismatched contract version is reported as
  ## `invalid_contract_version` even when the rest of the document is
  ## otherwise malformed, per issue #8's "invalid contract versions"
  ## rejection requirement and this plan's explicit ordering guarantee --
  ## plus the actual JSON decode. Exported separately from `plan` itself
  ## so a caller that needs to measure `plan`'s own wall time exclusively
  ## (`docs/design/issue-35-d0-cases.yaml`'s `M8-many-unrequested-nim-planner`
  ## case's confirmed `measurement_boundary`) can call this decode step
  ## and the timed `plan` call separately, rather than timing the
  ## combined `planFromJson` (schema gate + decode + plan), which is a
  ## wider interval than what that case actually specifies.
  let versionNode = root{"schema_version"}
  if versionNode.isNil or versionNode.kind != JString or versionNode.getStr() != PlanSchemaVersion:
    let got = if versionNode.isNil: "<missing>" else: $versionNode
    return (none(PlanningInput), some(rejected(reject(
      rrkInvalidContractVersion,
      "expected schema_version '" & PlanSchemaVersion & "', got " & got,
    ))))
  (some(root.planningInputFromJson), none(PlanOutcome))

proc planFromJson*(root: JsonNode): PlanOutcome =
  ## Unchanged behavior/signature -- every existing caller (this
  ## binary's own `main`, `tests/test_planning_kernel.nim`) keeps working
  ## exactly as before. Internally now just the schema gate/decode
  ## (`decodePlanningInputOrReject`) followed by `plan` itself.
  let (input, rejection) = decodePlanningInputOrReject(root)
  if rejection.isSome:
    return rejection.get
  plan(input.get)

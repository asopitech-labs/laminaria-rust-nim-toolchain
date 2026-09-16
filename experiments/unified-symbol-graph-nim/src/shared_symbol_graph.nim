## Issue #68 follow-up: Nim port of `SharedSymbolGraph`, the
## scheduler/planner half of `experiments/unified-symbol-graph`'s Rust
## implementation (`declare_analyzed_symbol`/`require_symbol`'s
## demand-driven Analyzed -> Committed promotion, `assign_layout`,
## `apply_elf_x86_64_relocations`) -- NOT the MIR-text/`target_ir`
## parser/codegen layer, which stays out of this port's scope per the
## user's own explicit framing: this is a check of whether the
## scheduler/planner can be implemented in Nim, not a production
## rewrite of the whole crate.
##
## Per `docs/01-foundations/compiler-ownership-contract_ja.md`'s own
## "C/C++ library再利用をfirst-class requirement" section, this module
## keeps Nim-specific algorithmic code to the minimum needed to express
## the state machine itself, and leans on Nim's standard library
## (`tables`, `locks`, `atomics`), each a thin wrapper over an existing
## C data structure/primitive (a hash table, `pthread_mutex_t`, C11
## atomics) rather than a Nim-original reimplementation.
##
## The Rust original lives in `experiments/unified-symbol-graph/src/lib.rs`
## and is kept as a comparison baseline (not deleted) per this session's
## own explicit instruction.

import std/[tables, locks, hashes, options, algorithm]

type
  Realm* = enum
    rmCargo, rmNimble, rmC, rmCpp

  SymbolId* = object
    realm*: Realm
    name*: string

  ElfX86_64PendingReloc* = object
    offset*: int
    width*: int
    target*: SymbolId
    addend*: int64

  CodeBody* = object
    code*: seq[byte]
    relocations*: seq[ElfX86_64PendingReloc]

  SemanticFacts* = object
    signature*: string
    dependsOn*: seq[SymbolId]
    # Issue #68 H5's own ControlFlowFacts integration is deliberately
    # out of scope for this scheduler/planner-only port -- see this
    # file's own module doc comment on scope.

  AddressStateKind* = enum
    askUnresolved, askAnalyzed, askCommitted

  AddressState* = object
    case kind*: AddressStateKind
    of askUnresolved: discard
    of askAnalyzed: facts*: SemanticFacts
    of askCommitted: body*: CodeBody

  SymbolNode* = object
    id*: SymbolId
    address*: AddressState

  RequiresEdge* = object
    requiringRealm*: Realm
    symbol*: SymbolId

  RegisterErrorKind* = enum
    reRealmMismatch

  RegisterError* = object
    kind*: RegisterErrorKind
    declaredBy*: Realm
    nodeRealm*: Realm

  FinishCodegenFn* = proc(facts: SemanticFacts): CodeBody {.closure, gcsafe.}

  LayoutAssignment* = object
    addresses*: Table[SymbolId, uint64]

  PatchedImage* = object
    code*: Table[SymbolId, seq[byte]]

  LinkErrorKind* = enum
    leMissingLayoutEntry, leUnresolvedRelocationTarget,
    leUnsupportedRelocationWidth, leRelocationOutOfBounds

  LinkError* = object
    kind*: LinkErrorKind
    inSymbol*: SymbolId
    target*: SymbolId
    width*: int
    offset*: int

  LinkException* = object of CatchableError
    err*: LinkError

  ResolvedRequirement* = object
    requirement*: RequiresEdge
    candidateRealms*: seq[Realm]
    provider*: Option[SymbolNode]

  SharedSymbolGraph* = ref object
    lock: Lock
    nodes: Table[SymbolId, SymbolNode]
    edges: Table[RequiresEdge, seq[Realm]]
    mutationSeq: uint64
    pendingCodegen: Table[SymbolId, FinishCodegenFn]

proc hash*(id: SymbolId): Hash =
  ## Required by `Table[SymbolId, _]` -- `tables`'s own hash table
  ## (the C/C++-library-reuse-first approach this port takes) needs
  ## this, mirroring Rust's `#[derive(Hash)]` on `SymbolId`.
  result = hash(id.realm) !& hash(id.name)
  result = !$result

proc `==`*(a, b: SymbolId): bool =
  a.realm == b.realm and a.name == b.name

proc `<`*(a, b: SymbolId): bool =
  ## Total order matching Rust's `#[derive(PartialOrd, Ord)]` (realm
  ## first, then name) -- required by `assign_layout`'s own
  ## deterministic-order guarantee, see that proc's own doc comment.
  if a.realm != b.realm: return a.realm < b.realm
  a.name < b.name

proc `<=`*(a, b: SymbolId): bool = a < b or a == b

proc hash*(edge: RequiresEdge): Hash =
  result = hash(edge.requiringRealm) !& hash(edge.symbol)
  result = !$result

proc `==`*(a, b: RequiresEdge): bool =
  a.requiringRealm == b.requiringRealm and a.symbol == b.symbol

proc newSharedSymbolGraph*(): SharedSymbolGraph =
  ## Matches Rust's own `SharedSymbolGraph::new`. A single `Lock`
  ## (Nim's own thin wrapper over `pthread_mutex_t`) stands in for the
  ## Rust original's three independent `RwLock`s -- a real, honest
  ## reduction in this port's own concurrency granularity, recorded
  ## explicitly as a known difference rather than silently claimed
  ## equivalent (see this file's own doc comment on scope; verifying
  ## whether this coarser locking changes observable scheduling
  ## behavior is exactly the kind of question this port exists to let
  ## a later experiment ask).
  result = SharedSymbolGraph()
  initLock(result.lock)
  result.nodes = initTable[SymbolId, SymbolNode]()
  result.edges = initTable[RequiresEdge, seq[Realm]]()
  result.mutationSeq = 0
  result.pendingCodegen = initTable[SymbolId, FinishCodegenFn]()

proc declareAnalyzedSymbol*(
  g: SharedSymbolGraph,
  declaringRealm: Realm,
  id: SymbolId,
  facts: SemanticFacts,
  finishCodegen: FinishCodegenFn,
): Option[RegisterError] =
  ## Matches Rust's `declare_analyzed_symbol`: registers `id` as
  ## `Analyzed`, and stores the closure that will produce its real
  ## `CodeBody` only when `requireSymbol` (or `forceCommit`) actually
  ## demands it.
  if id.realm != declaringRealm:
    return some(RegisterError(
      kind: reRealmMismatch, declaredBy: declaringRealm, nodeRealm: id.realm))
  withLock g.lock:
    inc g.mutationSeq
    g.nodes[id] = SymbolNode(id: id, address: AddressState(kind: askAnalyzed, facts: facts))
    g.pendingCodegen[id] = finishCodegen
  none[RegisterError]()

proc promoteAnalyzedToCommittedIfNeeded(g: SharedSymbolGraph, id: SymbolId) =
  ## Matches Rust's `promote_analyzed_to_committed_if_needed`: a no-op
  ## unless `id` is currently `Analyzed` and has a registered codegen
  ## closure -- the actual demand-driven Analyzed -> Committed step
  ## this whole port exists to verify survives translation to Nim.
  var factsOpt: Option[SemanticFacts]
  withLock g.lock:
    if id in g.nodes and g.nodes[id].address.kind == askAnalyzed:
      factsOpt = some(g.nodes[id].address.facts)
    else:
      factsOpt = none[SemanticFacts]()
  if factsOpt.isNone: return

  var codegenOpt: Option[FinishCodegenFn]
  withLock g.lock:
    if id in g.pendingCodegen:
      codegenOpt = some(g.pendingCodegen[id])
      g.pendingCodegen.del(id)
    else:
      codegenOpt = none[FinishCodegenFn]()
  if codegenOpt.isNone: return

  let body = codegenOpt.get()(factsOpt.get())
  withLock g.lock:
    inc g.mutationSeq
    # Re-check under the lock: another caller could have raced this
    # promotion (or overwritten the node entirely) between the reads
    # above and this write -- only commit if it is still exactly the
    # Analyzed state promoted from, matching the Rust original's own
    # race-safety comment.
    if id in g.nodes and g.nodes[id].address.kind == askAnalyzed:
      g.nodes[id] = SymbolNode(id: id, address: AddressState(kind: askCommitted, body: body))

proc forceCommit*(g: SharedSymbolGraph, id: SymbolId) =
  promoteAnalyzedToCommittedIfNeeded(g, id)

proc declareSymbol*(
  g: SharedSymbolGraph, declaringRealm: Realm, node: SymbolNode
): Option[RegisterError] =
  if node.id.realm != declaringRealm:
    return some(RegisterError(
      kind: reRealmMismatch, declaredBy: declaringRealm, nodeRealm: node.id.realm))
  withLock g.lock:
    inc g.mutationSeq
    g.nodes[node.id] = node
  none[RegisterError]()

proc requireSymbol*(
  g: SharedSymbolGraph,
  requiringRealm: Realm,
  symbol: SymbolId,
  expectedProvider: Realm,
) =
  ## Matches Rust's `require_symbol`: registering a requirement is the
  ## event that triggers demand-driven promotion, exactly mirroring
  ## Cargo's own pipelining (a downstream crate's full build finally
  ## needing the upstream crate's real object, not just its rmeta).
  withLock g.lock:
    inc g.mutationSeq
  promoteAnalyzedToCommittedIfNeeded(g, symbol)
  let edge = RequiresEdge(requiringRealm: requiringRealm, symbol: symbol)
  withLock g.lock:
    if edge notin g.edges:
      g.edges[edge] = @[]
    if expectedProvider notin g.edges[edge]:
      g.edges[edge].add(expectedProvider)

proc resolveAll*(g: SharedSymbolGraph): seq[ResolvedRequirement] =
  withLock g.lock:
    for edge, candidateRealms in g.edges.pairs:
      var provider = none[SymbolNode]()
      for realm in candidateRealms:
        let candidateId = SymbolId(realm: realm, name: edge.symbol.name)
        if candidateId in g.nodes:
          provider = some(g.nodes[candidateId])
          break
      result.add(ResolvedRequirement(
        requirement: edge, candidateRealms: candidateRealms, provider: provider))

proc mutationCount*(g: SharedSymbolGraph): uint64 =
  withLock g.lock:
    result = g.mutationSeq

proc assignLayout*(g: SharedSymbolGraph): LayoutAssignment =
  ## Matches Rust's `assign_layout`: deterministic (sorted `SymbolId`)
  ## back-to-back placement of every `Committed` symbol's own code --
  ## the same acknowledged non-optimization this port inherits
  ## unchanged (see `lib.rs`'s own module doc comment on
  ## scheduling-blind ordering; this port is not where that gap gets
  ## closed).
  withLock g.lock:
    var ids: seq[SymbolId] = @[]
    for id in g.nodes.keys: ids.add(id)
    # Nim's `sort` needs an explicit comparator since `SymbolId` is not
    # `Ordered` by default -- kept minimal (a direct translation of
    # the `<`/`==` operators above), not a custom sorting algorithm.
    proc cmp(a, b: SymbolId): int =
      if a < b: -1 elif b < a: 1 else: 0
    ids.sort(cmp)

    result = LayoutAssignment(addresses: initTable[SymbolId, uint64]())
    var cursor: uint64 = 0
    for id in ids:
      if g.nodes[id].address.kind != askCommitted: continue
      result.addresses[id] = cursor
      cursor += g.nodes[id].address.body.code.len.uint64

proc applyElfX8664Relocations*(
  g: SharedSymbolGraph, layout: LayoutAssignment
): PatchedImage =
  ## Matches Rust's `apply_elf_x86_64_relocations`. Raises
  ## `LinkException` (carrying a `LinkError`) rather than returning
  ## `Result`, since this is the layer where translating Rust's
  ## `Result<T, E>` idiom directly would fight Nim's own idioms more
  ## than it would help -- the *values* inside `LinkError` are an exact
  ## match for the Rust original's `LinkError` enum, kept as
  ## structured data for a caller to inspect via `getCurrentException()`
  ## rather than collapsed into a bare error string.
  withLock g.lock:
    var ids: seq[SymbolId] = @[]
    for id in g.nodes.keys: ids.add(id)
    proc cmp(a, b: SymbolId): int =
      if a < b: -1 elif b < a: 1 else: 0
    ids.sort(cmp)

    result = PatchedImage(code: initTable[SymbolId, seq[byte]]())
    for id in ids:
      if g.nodes[id].address.kind != askCommitted: continue
      let body = g.nodes[id].address.body
      if id notin layout.addresses:
        raise (ref LinkException)(
          msg: "missing layout entry",
          err: LinkError(kind: leMissingLayoutEntry, inSymbol: id))
      let siteBase = layout.addresses[id]
      var bytes = body.code
      for reloc in body.relocations:
        if reloc.target notin layout.addresses:
          raise (ref LinkException)(
            msg: "unresolved relocation target",
            err: LinkError(kind: leUnresolvedRelocationTarget, inSymbol: id, target: reloc.target))
        let targetAddress = layout.addresses[reloc.target]
        let relocSiteAddress = siteBase + reloc.offset.uint64 + reloc.width.uint64
        let value = (targetAddress.int64 + reloc.addend) - relocSiteAddress.int64
        if reloc.width != 4:
          raise (ref LinkException)(
            msg: "unsupported relocation width",
            err: LinkError(kind: leUnsupportedRelocationWidth, inSymbol: id, width: reloc.width))
        let valueI32 = value.int32
        let valueBytes = [
          byte(valueI32 and 0xFF),
          byte((valueI32 shr 8) and 0xFF),
          byte((valueI32 shr 16) and 0xFF),
          byte((valueI32 shr 24) and 0xFF),
        ]
        let startOff = reloc.offset
        let endOff = startOff + reloc.width
        if endOff > bytes.len:
          raise (ref LinkException)(
            msg: "relocation out of bounds",
            err: LinkError(kind: leRelocationOutOfBounds, inSymbol: id, offset: reloc.offset))
        for i in 0 ..< reloc.width:
          bytes[startOff + i] = valueBytes[i]
      result.code[id] = bytes

proc isResolved*(r: ResolvedRequirement): bool = r.provider.isSome

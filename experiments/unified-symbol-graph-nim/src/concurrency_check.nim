## Issue #68 follow-up: mirrors the Rust original's own
## `four_realms_declare_and_require_concurrently_and_every_boundary_symbol_resolves`
## test -- four real OS threads (one per realm), each touching only its
## own realm's declarations/requirements, with `SharedSymbolGraph`
## itself as the only shared state, confirming the Nim port's `Lock`
## (a `pthread_mutex_t` wrapper, not custom Nim-original synchronization
## code) actually serializes concurrent writers correctly rather than
## merely compiling against a thread-safe-looking type.
##
## Run: `nim c -r --threads:on src/concurrency_check.nim`

import std/options
import shared_symbol_graph

# Nim's `Thread[T]` procs must be top-level (not closures capturing
# outer locals) to be passed across the real OS thread boundary --
# the graph itself is passed in as `T`, matching the Rust original's
# own `Arc::clone(&graph)` per thread.

proc appRealmWork(g: SharedSymbolGraph) {.thread.} =
  g.requireSymbol(rmCargo, SymbolId(realm: rmC, name: "c_add"), rmC)
  g.requireSymbol(rmCargo, SymbolId(realm: rmCpp, name: "cpp_max_i32"), rmCpp)
  g.requireSymbol(rmCargo, SymbolId(realm: rmNimble, name: "nim_double"), rmNimble)

proc cRealmWork(g: SharedSymbolGraph) {.thread.} =
  let err = g.declareSymbol(rmC, SymbolNode(
    id: SymbolId(realm: rmC, name: "c_add"),
    address: AddressState(kind: askCommitted, body: CodeBody(
      code: @[0x55'u8, 0x48'u8, 0x89'u8, 0xe5'u8, 0xc3'u8], relocations: @[]))
  ))
  doAssert err.isNone, "C realm may declare its own symbol"

proc cppRealmWork(g: SharedSymbolGraph) {.thread.} =
  let err = g.declareSymbol(rmCpp, SymbolNode(
    id: SymbolId(realm: rmCpp, name: "cpp_max_i32"),
    address: AddressState(kind: askCommitted, body: CodeBody(
      code: @[0xb8'u8, 0x04'u8, 0x00'u8, 0x00'u8, 0x00'u8, 0xc3'u8], relocations: @[]))
  ))
  doAssert err.isNone, "C++ realm may declare its own symbol"

proc nimbleRealmWork(g: SharedSymbolGraph) {.thread.} =
  let err = g.declareSymbol(rmNimble, SymbolNode(
    id: SymbolId(realm: rmNimble, name: "nim_double"),
    address: AddressState(kind: askCommitted, body: CodeBody(
      code: @[0xb8'u8, 0x08'u8, 0x00'u8, 0x00'u8, 0x00'u8, 0xc3'u8], relocations: @[]))
  ))
  doAssert err.isNone, "Nimble realm may declare its own symbol"

proc main() =
  let g = newSharedSymbolGraph()

  var threads: array[4, Thread[SharedSymbolGraph]]
  createThread(threads[0], appRealmWork, g)
  createThread(threads[1], cRealmWork, g)
  createThread(threads[2], cppRealmWork, g)
  createThread(threads[3], nimbleRealmWork, g)
  joinThreads(threads)

  # No separate "link" pass: resolve_all() is the only step after the
  # four realm threads finish, and it only ever reads what they
  # already wrote.
  let resolved = g.resolveAll()
  doAssert resolved.len == 3, "app's three real FFI requirements, got " & $resolved.len
  for r in resolved:
    doAssert r.isResolved(), "requirement " & $r.requirement & " must resolve against the real concurrent declarations"

  # At least declare_symbol x3 + require_symbol x3 = 6 mutations --
  # confirms the graph actually accepted writes from all four threads,
  # not just the last one to run.
  doAssert g.mutationCount() >= 6'u64, "expected >=6 mutations, got " & $g.mutationCount()

  echo "[concurrency_check] 4 real OS threads (one per realm) declared/required concurrently; ",
       "all 3 boundary requirements resolved; mutation_count=", g.mutationCount(), " -- PASS"

main()

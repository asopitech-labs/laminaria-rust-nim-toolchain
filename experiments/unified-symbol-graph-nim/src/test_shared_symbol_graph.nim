## Issue #68 follow-up: verifies the Nim port of `SharedSymbolGraph`
## reproduces the same demand-driven Analyzed -> Committed promotion,
## layout assignment, and relocation-patching behavior the Rust
## original's own tests (`lib.rs`'s
## `demand_driven_promotion_from_analyzed_to_committed_on_require` and
## the ELF x86_64 relocation tests) established as correct -- run
## against real byte values, not just "it compiles."

import std/[unittest, tables, options]
import shared_symbol_graph

suite "SharedSymbolGraph (Nim port)":
  test "declare_analyzed_symbol keeps a symbol Analyzed until require_symbol demands it":
    let g = newSharedSymbolGraph()
    var codegenRunCount = 0
    let id = SymbolId(realm: rmCargo, name: "diamond")

    let err = g.declareAnalyzedSymbol(
      rmCargo, id,
      SemanticFacts(signature: "(i32) -> i32", dependsOn: @[]),
      proc(facts: SemanticFacts): CodeBody =
        inc codegenRunCount
        CodeBody(code: @[0x90'u8, 0xC3'u8], relocations: @[])
    )
    check err.isNone

    check codegenRunCount == 0
    g.requireSymbol(rmC, id, rmCargo)
    check codegenRunCount == 1

    # A second require_symbol call on an already-Committed symbol must
    # not re-run codegen -- the same demand-driven discipline the Rust
    # original's own test exercises.
    g.requireSymbol(rmCargo, id, rmCargo)
    check codegenRunCount == 1

  test "force_commit triggers codegen without a require_symbol call":
    let g = newSharedSymbolGraph()
    var ran = false
    let id = SymbolId(realm: rmCargo, name: "eager")
    discard g.declareAnalyzedSymbol(
      rmCargo, id, SemanticFacts(signature: "() -> ()", dependsOn: @[]),
      proc(facts: SemanticFacts): CodeBody =
        ran = true
        CodeBody(code: @[0xC3'u8], relocations: @[])
    )
    check not ran
    g.forceCommit(id)
    check ran

  test "declare_symbol rejects a realm mismatch":
    let g = newSharedSymbolGraph()
    let id = SymbolId(realm: rmC, name: "c_add")
    let err = g.declareSymbol(rmCargo, SymbolNode(id: id, address: AddressState(kind: askUnresolved)))
    check err.isSome
    check err.get().kind == reRealmMismatch
    check err.get().declaredBy == rmCargo
    check err.get().nodeRealm == rmC

  test "resolve_all reports unresolved requirements as None, not a fabricated placeholder":
    let g = newSharedSymbolGraph()
    g.requireSymbol(rmCargo, SymbolId(realm: rmC, name: "never_declared"), rmC)
    let resolved = g.resolveAll()
    check resolved.len == 1
    check not resolved[0].isResolved

  test "resolve_all matches a requirement to its declared provider":
    let g = newSharedSymbolGraph()
    let cAddId = SymbolId(realm: rmC, name: "c_add")
    discard g.declareSymbol(rmC, SymbolNode(
      id: cAddId,
      address: AddressState(kind: askCommitted, body: CodeBody(code: @[0x90'u8], relocations: @[]))
    ))
    g.requireSymbol(rmCargo, cAddId, rmC)
    let resolved = g.resolveAll()
    check resolved.len == 1
    check resolved[0].isResolved
    check resolved[0].provider.get().id == cAddId

  test "assign_layout places Committed symbols back-to-back in deterministic SymbolId order":
    let g = newSharedSymbolGraph()
    # Declared out of sorted order deliberately, to confirm assign_layout
    # itself imposes the order (sorted SymbolId), not insertion order.
    discard g.declareSymbol(rmCargo, SymbolNode(
      id: SymbolId(realm: rmCargo, name: "zeta"),
      address: AddressState(kind: askCommitted, body: CodeBody(code: @[0x01'u8, 0x02'u8, 0x03'u8], relocations: @[]))
    ))
    discard g.declareSymbol(rmCargo, SymbolNode(
      id: SymbolId(realm: rmCargo, name: "alpha"),
      address: AddressState(kind: askCommitted, body: CodeBody(code: @[0x04'u8, 0x05'u8], relocations: @[]))
    ))
    # An Analyzed (not Committed) symbol must be skipped by assign_layout,
    # matching the Rust original's own `let AddressState::Committed(body) = ... else { continue }`.
    discard g.declareAnalyzedSymbol(
      rmCargo, SymbolId(realm: rmCargo, name: "beta"),
      SemanticFacts(signature: "() -> ()", dependsOn: @[]),
      proc(facts: SemanticFacts): CodeBody = CodeBody(code: @[], relocations: @[])
    )

    let layout = g.assignLayout()
    check layout.addresses.len == 2
    check layout.addresses[SymbolId(realm: rmCargo, name: "alpha")] == 0'u64
    check layout.addresses[SymbolId(realm: rmCargo, name: "zeta")] == 2'u64  # after alpha's 2 bytes

  test "apply_elf_x86_64_relocations patches to the independently recomputed PC-relative value":
    ## Mirrors the Rust original's own
    ## `call_relocation_patches_to_the_independently_recomputed_pc_relative_value`
    ## test: a caller (offset 0, `call rel32` placeholder at bytes 1..4)
    ## calling a callee, with the correct PC-relative displacement
    ## independently recomputed here (not copied from this port's own
    ## arithmetic) as `target - (site_base + offset + width) + addend`.
    let g = newSharedSymbolGraph()
    let calleeId = SymbolId(realm: rmC, name: "callee")
    let callerId = SymbolId(realm: rmCargo, name: "caller")

    discard g.declareSymbol(rmC, SymbolNode(
      id: calleeId,
      address: AddressState(kind: askCommitted, body: CodeBody(code: @[0xC3'u8], relocations: @[]))
    ))
    # caller: 0xE8 (call opcode) + 4-byte placeholder, then a 3-byte
    # trailer, matching the Rust test's own byte shape.
    discard g.declareSymbol(rmCargo, SymbolNode(
      id: callerId,
      address: AddressState(kind: askCommitted, body: CodeBody(
        code: @[0xE8'u8, 0x00'u8, 0x00'u8, 0x00'u8, 0x00'u8, 0x90'u8, 0x90'u8, 0x90'u8],
        relocations: @[ElfX86_64PendingReloc(offset: 1, width: 4, target: calleeId, addend: 0)]
      ))
    ))

    let layout = g.assignLayout()
    # caller sorts before callee under (realm, name) order (rmCargo < rmC is false;
    # rmCargo=0 < rmC=2 is true in this port's Realm enum order, matching
    # the Rust original's own `Realm` derive order: Cargo, Nimble, C, Cpp).
    let callerBase = layout.addresses[callerId]
    let calleeBase = layout.addresses[calleeId]

    let patched = g.applyElfX8664Relocations(layout)
    let patchedCallerCode = patched.code[callerId]

    let relocSiteAddress = callerBase + 1'u64 + 4'u64
    let expectedValue = (calleeBase.int64 + 0'i64) - relocSiteAddress.int64
    let expectedBytes = [
      byte(expectedValue.int32 and 0xFF),
      byte((expectedValue.int32 shr 8) and 0xFF),
      byte((expectedValue.int32 shr 16) and 0xFF),
      byte((expectedValue.int32 shr 24) and 0xFF),
    ]
    check patchedCallerCode[1] == expectedBytes[0]
    check patchedCallerCode[2] == expectedBytes[1]
    check patchedCallerCode[3] == expectedBytes[2]
    check patchedCallerCode[4] == expectedBytes[3]
    # Never a leftover all-zero placeholder for a relocation this proc
    # reported success for.
    check not (expectedBytes[0] == 0 and expectedBytes[1] == 0 and
               expectedBytes[2] == 0 and expectedBytes[3] == 0)

  test "apply_elf_x86_64_relocations raises on an unresolved relocation target":
    let g = newSharedSymbolGraph()
    let callerId = SymbolId(realm: rmCargo, name: "caller")
    let missingId = SymbolId(realm: rmC, name: "never_declared")
    discard g.declareSymbol(rmCargo, SymbolNode(
      id: callerId,
      address: AddressState(kind: askCommitted, body: CodeBody(
        code: @[0xE8'u8, 0x00'u8, 0x00'u8, 0x00'u8, 0x00'u8],
        relocations: @[ElfX86_64PendingReloc(offset: 1, width: 4, target: missingId, addend: 0)]
      ))
    ))
    let layout = g.assignLayout()
    expect LinkException:
      discard g.applyElfX8664Relocations(layout)

  test "mutation_count increases on every accepted declare/require call":
    let g = newSharedSymbolGraph()
    check g.mutationCount() == 0
    discard g.declareSymbol(rmC, SymbolNode(
      id: SymbolId(realm: rmC, name: "x"), address: AddressState(kind: askUnresolved)))
    check g.mutationCount() == 1
    g.requireSymbol(rmCargo, SymbolId(realm: rmC, name: "x"), rmC)
    check g.mutationCount() == 2

## Issue #48 (G1) fixture: a real Nimble package's own implementation,
## exporting the C-linkage symbol `app/src/main.rs`'s FFI declaration
## expects.
proc nim_double(x: cint): cint {.exportc: "nim_double", cdecl.} =
  x * 2

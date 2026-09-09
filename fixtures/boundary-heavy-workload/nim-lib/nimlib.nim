## `boundary-heavy-workload` fixture — Nim side.
##
## #11's "boundary-heavy workload" Core workload: unlike
## `mixed-rust-nim-executable` (a handful of calls over one array), this
## fixture crosses the Rust/Nim FFI boundary once per loop iteration —
## a large, fixed iteration count, deliberately trivial per-call payload
## and computation — so the boundary-crossing count itself, not data
## volume or per-call work, dominates the workload's cost. That is
## exactly what future call-overhead / boundary-count measurement (#19)
## needs a fixture to exercise.
##
## `nim_fold_step` is an FNV-1a-style hash-fold step using only unsigned
## 32-bit arithmetic, so multiplication overflow wraps by Nim's defined
## unsigned-integer semantics rather than tripping an overflow check.

proc nim_fold_step(acc: cuint, x: cuint): cuint {.exportc, cdecl.} =
  var h = acc xor x
  h = h * 0x0100_0193'u32 # FNV-1a 32-bit prime
  h = h xor (h shr 15)
  h

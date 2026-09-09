## Type/layout matrix: function pointers/callbacks (Layer 3), and the
## reverse-direction Layer 4 question every earlier experiment
## sidesteps -- every call so far has Nim call into Rust. This reverses
## it: Rust calls a Nim-provided function pointer directly. A plain
## function pointer (no captured environment) is exactly the
## "closures/function values" class `docs/rust-nim-native-linking.md`'s
## compatibility matrix lists as usable when nothing is captured --
## verified here, not just declared usable in principle.
##
## Two calls, isolated in separate blocks: the normal case first, then
## the deliberately riskier case -- a Nim exception raised *inside* a
## callback Rust is actively calling, meaning it would have to
## propagate back out through a live Rust stack frame to reach Nim's
## own exception handling. Isolated last so the normal case's result is
## captured regardless of what the risky case does.

proc rust_calls_callback(cb: proc(x: cint): cint {.cdecl.}, x: cint): cint {.importc: "rust_calls_callback", cdecl.}

proc nim_double(x: cint): cint {.cdecl.} =
  x * 2

block normalCallback:
  let result = rust_calls_callback(nim_double, 21)
  echo "rust_calls_callback(nim_double, 21)=", result
  doAssert result == 42, "callback result drifted from the committed reference value"

echo "normal callback case completed"

proc nim_raises(x: cint): cint {.cdecl.} =
  if x > 0:
    raise newException(ValueError, "deliberate Nim exception inside a Rust-invoked callback")
  x

echo "about to call rust_calls_callback(nim_raises, 21) -- observing what happens " &
  "when a Nim exception is raised while a Rust stack frame is live between it and Nim's own handler"
# Deliberately captures the result into `let` rather than `discard`:
# observed directly (see ../NOTES.md) that this changes exactly where
# the compiler-inserted exception check fires relative to this line --
# with `let`, the process reports the unhandled exception immediately
# after this call, and the line below never runs (confirmed reliably
# across repeated runs, not a one-off).
let afterRaise = rust_calls_callback(nim_raises, 21)
echo "UNREACHABLE if the exception was detected at this call site: result=", afterRaise

## Layer 4 focal question, previously entirely untested for this
## project: what actually happens when a Rust function panics while
## called from Nim across the FFI boundary? `../rust-lib/src/lib.rs`
## found, testing Rust calling its own `extern "C" fn` from its own
## test harness, that the panic does not unwind at all -- the process
## aborts immediately ("thread caused non-unwinding panic. aborting.",
## SIGABRT) because Rust treats a plain `extern "C" fn` as a
## "cannot unwind" boundary. This confirms the same holds when the
## caller is Nim, not just Rust's own test harness -- the finding is
## about the ABI boundary itself, not about who is on the other side.

proc rust_panics(trigger: cint): cint {.importc: "rust_panics", cdecl.}

let normal = rust_panics(0)
echo "normal (non-panicking) call result=", normal
doAssert normal == 42, "rust_panics(0) result drifted from the committed reference value"

echo "about to call rust_panics(1) -- expect the process to abort here, not return"
discard rust_panics(1)
echo "UNREACHABLE: if this printed, the panic returned control to Nim instead of " &
  "aborting the process -- a different, real finding from what pure-Rust testing showed"

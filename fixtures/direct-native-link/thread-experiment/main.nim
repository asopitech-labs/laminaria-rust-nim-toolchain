## Layer 4 focal question, previously entirely untested: does calling
## into Rust's `extern "C"` functions work correctly from an OS thread
## that *Nim* created (via `createThread`/pthread), not one Rust's own
## `std::thread::spawn` set up? Rust's stdlib normally registers
## per-thread bookkeeping (thread name, `std::thread::current()`
## metadata, stack-overflow guard page) when it spawns a thread itself;
## a thread created entirely outside Rust's knowledge is "foreign" to
## it. Tested with two calls of increasing risk:
##
## 1. `rust_transform` -- pure arithmetic, touches no thread-local
##    state at all. Expected to work regardless, a baseline sanity
##    check, not the interesting question.
## 2. `rust_vec_growth_probe` -- allocates and reallocates a `Vec`,
##    exercising Rust's global allocator from this foreign thread. The
##    actual stress test: Rust's default allocator (the system
##    allocator on most platforms) is thread-safe and does not require
##    per-thread registration, but that's a claim worth checking
##    directly rather than assuming.

proc rust_transform(x: cint): cint {.importc: "rust_transform", cdecl.}
proc rust_vec_growth_probe(len: cint, outAddrBefore, outAddrAfter: ptr clong, outLenAfter: ptr cint) {.importc: "rust_vec_growth_probe", cdecl.}

proc worker(input: cint) {.thread.} =
  let transformed = rust_transform(input)
  echo "worker thread: rust_transform(", input, ")=", transformed
  doAssert transformed == 43, "rust_transform result drifted when called from a Nim-spawned thread"

  var addrBefore, addrAfter: clong
  var lenAfter: cint
  rust_vec_growth_probe(5, addr addrBefore, addr addrAfter, addr lenAfter)
  echo "worker thread: rust_vec_growth_probe len_after=", lenAfter,
    " (allocator exercised from a Nim-spawned, Rust-foreign OS thread)"
  doAssert lenAfter == 1010, "rust_vec_growth_probe's reported length drifted when called from a Nim-spawned thread"

var t: Thread[cint]
createThread(t, worker, 21.cint)
joinThread(t)
echo "main thread: worker completed successfully -- Rust calls from a Nim-spawned thread work correctly"

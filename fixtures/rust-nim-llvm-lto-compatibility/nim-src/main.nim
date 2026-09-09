## #11's "Rust/Nim 2/Nimony shared LLVM/LTO compatibility workload" —
## Nim side. Declares `rust_add` by raw symbol name, same hand-matched
## contract every fixture in this repo uses — but this time the point
## isn't linking two native objects, it's whether `nlvm`'s own LLVM IR
## for this call can be merged with `../rust-src/lib.rs`'s LLVM IR by
## `llvm-link` *before* native codegen. See ../NOTES.md.

proc rust_add(a, b: cint): cint {.importc: "rust_add", cdecl.}

let result = rust_add(3, 4)
echo "nim_calls_rust_add result=", result
doAssert result == 7, "rust_add result drifted from the committed reference value"

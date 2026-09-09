## `direct-native-link` fixture — Nim side.
##
## Declares `rust_transform` by its raw exported symbol name (see
## `../rust-lib/src/lib.rs`) with no generated C header in between — the
## whole "contract" is this one `importc` line plus the matching
## `#[no_mangle] extern "C"` on the Rust side. This is the reverse
## direction from every other Rust/Nim fixture here: Nim is the final
## linked binary and calls directly into Rust, rather than Rust linking
## a Nim static library.

proc rust_transform(x: cint): cint {.importc: "rust_transform", cdecl.}

const Input: cint = 21
const Expected: cint = 43

let result = rust_transform(Input)
echo "result=", result
doAssert result == Expected, "rust_transform result drifted from the committed reference value"

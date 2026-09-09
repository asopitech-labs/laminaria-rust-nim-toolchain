## Layer 5 required evidence (docs/rust-nim-native-linking.md): a
## workload-matched "conventional C ABI baseline" counterpart to
## ../nim-bin/main.nim. The doc's own framing is:
##
##   C ABI as mandatory architectural boundary
##   versus
##   C ABI/adapters as one possible boundary artifact chosen only where required
##
## This file is the first side of that comparison. The *same*, unmodified
## `../rust-lib` static library is linked here too -- not rebuilt, not
## recompiled differently, not a single line changed -- so any difference
## found is attributable entirely to how the Nim side establishes the
## contract, not to a different Rust artifact. The difference: every
## type and function here is imported straight from `bindings.h`, a
## `cbindgen`-generated header that is the single source of truth for
## the boundary -- the conventional, mandatory-C-ABI-contract shape --
## instead of ../nim-bin/main.nim's independently hand-written `importc`
## declarations with no generated header at all, whose agreement with
## Rust's actual layout is instead verified at runtime (see
## ../nim-bin/main.nim's layoutProbe block).
##
## Three representative classes from the type/layout matrix, matching
## ../nim-bin/main.nim's coverage: a scalar call, a fixed-layout struct
## used by pointer, and an opaque handle. See ../NOTES.md for the
## measured comparison (object/binary size, generated-C diff, build
## step cost) this file exists to produce.

type Point {.importc: "Point", header: "bindings.h", bycopy.} = object
  x, y: cint

type Counter {.importc: "Counter", header: "bindings.h", incompleteStruct.} = object

proc rust_transform(x: cint): cint {.importc: "rust_transform", header: "bindings.h", cdecl.}

proc rust_point_scale_in_place(p: ptr Point, factor: cint) {.importc: "rust_point_scale_in_place", header: "bindings.h", cdecl.}
proc rust_point_layout_probe(outSize, outAlign, outOffsetX, outOffsetY: ptr cint) {.importc: "rust_point_layout_probe", header: "bindings.h", cdecl.}

proc rust_counter_new(start: clong): ptr Counter {.importc: "rust_counter_new", header: "bindings.h", cdecl.}
proc rust_counter_increment(handle: ptr Counter, by: clong) {.importc: "rust_counter_increment", header: "bindings.h", cdecl.}
proc rust_counter_get(handle: ptr Counter): clong {.importc: "rust_counter_get", header: "bindings.h", cdecl.}
proc rust_counter_label_len(handle: ptr Counter): cint {.importc: "rust_counter_label_len", header: "bindings.h", cdecl.}
proc rust_counter_free(handle: ptr Counter) {.importc: "rust_counter_free", header: "bindings.h", cdecl.}

const Input: cint = 21
const Expected: cint = 43

let result = rust_transform(Input)
echo "result=", result
doAssert result == Expected, "rust_transform result drifted from the committed reference value"

block layoutProbe:
  ## Unlike ../nim-bin/main.nim's layoutProbe, `Point` here is *not*
  ## independently declared -- it's imported directly from the same
  ## generated header Rust's own layout came from, so agreement isn't a
  ## finding to verify at runtime, it's definitional. Kept as a
  ## consistency check against the direct route's finding, not as a new
  ## question.
  var rustSize, rustAlign, rustOffsetX, rustOffsetY: cint
  rust_point_layout_probe(addr rustSize, addr rustAlign, addr rustOffsetX, addr rustOffsetY)
  echo "layout (header-imported Point): size=", rustSize, " align=", rustAlign,
    " offset_x=", rustOffsetX, " offset_y=", rustOffsetY

block pointerMutateInPlace:
  var p = Point(x: 3, y: 4)
  rust_point_scale_in_place(addr p, 5)
  echo "scale_in_place: ", p.x, ",", p.y
  doAssert p.x == 15 and p.y == 20,
    "pointer-to-Point mutation drifted from the committed reference value"

block opaqueHandleLifecycle:
  let handle = rust_counter_new(10)
  echo "counter_new(10) -> value=", rust_counter_get(handle), " label_len=", rust_counter_label_len(handle)
  doAssert rust_counter_get(handle) == 10, "fresh Counter handle's value drifted from the committed reference value"
  doAssert rust_counter_label_len(handle) == cint(len("counter@10")),
    "fresh Counter handle's heap-owned label length drifted from the committed reference value"

  rust_counter_increment(handle, 5)
  echo "after increment(5) -> value=", rust_counter_get(handle)
  doAssert rust_counter_get(handle) == 15, "Counter handle's value after increment drifted from the committed reference value"

  rust_counter_increment(handle, -20)
  echo "after increment(-20) -> value=", rust_counter_get(handle)
  doAssert rust_counter_get(handle) == -5, "Counter handle's value after negative increment drifted from the committed reference value"

  rust_counter_free(handle)
  echo "counter handle freed"

echo "all c-abi-baseline experiments passed"

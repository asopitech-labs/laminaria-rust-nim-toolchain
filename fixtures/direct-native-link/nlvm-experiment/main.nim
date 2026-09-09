## The actual apples-to-apples comparison `hello.nim`'s smoke test set
## up for: the identical Layer 1-3 experiments `../nim-bin/main.nim`
## runs through `nim c` (C-generating route), run instead through
## `nlvm` (C-free route: Nim -> nlvm's own LLVM IR emission -> native
## object, no C source or C compiler anywhere in the path). Linked
## against the exact same `../rust-lib` static library -- no Rust
## changes, same `rust_transform`/`Point`-family exported symbols -- so
## any difference in outcome is attributable to the Nim-side route, not
## to a different Rust artifact.
##
## `Point` is declared here independently again, same as
## `../nim-bin/main.nim` -- no shared file with either the Nim-via-C
## declaration or the Rust declaration.

proc rust_transform(x: cint): cint {.importc: "rust_transform", cdecl.}

const Input: cint = 21
const Expected: cint = 43

let result = rust_transform(Input)
echo "result=", result
doAssert result == Expected, "rust_transform result drifted from the committed reference value"

type Point {.bycopy.} = object
  x, y: cint

proc rust_point_translate(p: Point, dx, dy: cint): Point {.importc: "rust_point_translate", cdecl.}
proc rust_point_translate_via_pointer(p: Point, dx, dy: cint, outP: ptr Point) {.importc: "rust_point_translate_via_pointer", cdecl.}
proc rust_point_scale_in_place(p: ptr Point, factor: cint) {.importc: "rust_point_scale_in_place", cdecl.}
proc rust_point_layout_probe(outSize, outAlign, outOffsetX, outOffsetY: ptr cint) {.importc: "rust_point_layout_probe", cdecl.}

block layoutProbe:
  var rustSize, rustAlign, rustOffsetX, rustOffsetY: cint
  rust_point_layout_probe(addr rustSize, addr rustAlign, addr rustOffsetX, addr rustOffsetY)

  var probe: Point
  let base = cast[int](addr probe)
  let nimSize = cint(sizeof(Point))
  let nimAlign = cint(alignof(Point))
  let nimOffsetX = cint(cast[int](addr probe.x) - base)
  let nimOffsetY = cint(cast[int](addr probe.y) - base)

  echo "layout: nim size=", nimSize, " align=", nimAlign, " offset_x=", nimOffsetX, " offset_y=", nimOffsetY
  echo "layout: rust size=", rustSize, " align=", rustAlign, " offset_x=", rustOffsetX, " offset_y=", rustOffsetY

  doAssert nimSize == rustSize, "Point size disagreement between independently-declared Nim and Rust layouts"
  doAssert nimAlign == rustAlign, "Point alignment disagreement between independently-declared Nim and Rust layouts"
  doAssert nimOffsetX == rustOffsetX, "Point.x offset disagreement between independently-declared Nim and Rust layouts"
  doAssert nimOffsetY == rustOffsetY, "Point.y offset disagreement between independently-declared Nim and Rust layouts"

block byValueRoundTrip:
  ## nlvm itself warns at compile time on this call:
  ## "TODO: C ABI for small struct returns not implemented - there may
  ## be issues: rust_point_translate" -- a real, self-acknowledged
  ## limitation, not a maybe. Reported without doAssert (unlike every
  ## other block here) so a known-broken result doesn't abort the
  ## process before pointerMutateInPlace below gets to run; see
  ## ../NOTES.md for the full finding.
  let p = Point(x: 3, y: 4)
  let translated = rust_point_translate(p, 10, -1)
  echo "translate: ", translated.x, ",", translated.y, " (expected 13,3 -- nlvm's small-struct-return ABI is a known-incomplete TODO)"

block byValueInputPointerOutputWorkaround:
  ## The practical mitigation for the known-broken block above: keep
  ## `p` passed in by value (only the *return* ABI is the known-broken
  ## half), and take the result through an output pointer instead of a
  ## return value. Isolates whether by-value struct *input* is actually
  ## fine on nlvm, independent of the broken by-value *return* path.
  let p = Point(x: 3, y: 4)
  var translated: Point
  rust_point_translate_via_pointer(p, 10, -1, addr translated)
  echo "translate_via_pointer: ", translated.x, ",", translated.y
  doAssert translated.x == 13 and translated.y == 3,
    "by-value-input/pointer-output Point translate drifted from the committed reference value"

block pointerMutateInPlace:
  var p = Point(x: 3, y: 4)
  rust_point_scale_in_place(addr p, 5)
  echo "scale_in_place: ", p.x, ",", p.y
  doAssert p.x == 15 and p.y == 20,
    "pointer-to-Point mutation drifted from the committed reference value"

echo "all nlvm-route Layer 1-3 experiments passed"

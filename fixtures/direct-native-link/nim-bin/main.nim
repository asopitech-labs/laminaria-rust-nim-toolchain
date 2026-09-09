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

# --- Issue #4 Layer 2/3 feasibility: fixed-layout struct and pointer
# round-trips, extending past #11's own Layer-1-only scope above. See
# ../NOTES.md for the methodology and results.
#
# `Point` is declared here from scratch — it does not include, generate,
# or read any file shared with ../rust-lib/src/lib.rs's `Point`. Layer 3
# asks whether independently declaring "the same" C-compatible layout on
# both sides is actually safe to rely on; `layoutProbe` below computes
# this side's own sizeof/offsets and compares them against Rust's
# self-reported ones from `rust_point_layout_probe`, so agreement is
# demonstrated at runtime rather than assumed from the linker succeeding.

type Point {.bycopy.} = object
  x, y: cint

proc rust_point_translate(p: Point, dx, dy: cint): Point {.importc: "rust_point_translate", cdecl.}
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
  let p = Point(x: 3, y: 4)
  let translated = rust_point_translate(p, 10, -1)
  echo "translate: ", translated.x, ",", translated.y
  doAssert translated.x == 13 and translated.y == 3,
    "by-value Point round-trip drifted from the committed reference value"

block pointerMutateInPlace:
  var p = Point(x: 3, y: 4)
  rust_point_scale_in_place(addr p, 5)
  echo "scale_in_place: ", p.x, ",", p.y
  doAssert p.x == 15 and p.y == 20,
    "pointer-to-Point mutation drifted from the committed reference value"

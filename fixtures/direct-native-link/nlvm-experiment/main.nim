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
##
## Blocks are ordered safest-first: known-working, then known-broken-
## but-non-fatal (wrong value, doesn't crash), then the one known-fatal
## call is isolated last, so every other result is captured regardless
## of what that last block does. See ../NOTES.md for the full findings.

proc rust_transform(x: cint): cint {.importc: "rust_transform", cdecl.}

const Input: cint = 21
const Expected: cint = 43

let result = rust_transform(Input)
echo "result=", result
doAssert result == Expected, "rust_transform result drifted from the committed reference value"

type Point {.bycopy.} = object
  x, y: cint

proc rust_point_sum(p: Point): cint {.importc: "rust_point_sum", cdecl.}
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

block pointSumSingleArg:
  ## The minimal possible by-value-struct-argument shape: `Point` is the
  ## only parameter, and the return is a plain scalar, not a struct --
  ## meant to isolate whether by-value struct *arguments* work at all
  ## under nlvm, independent of the other failure modes on this
  ## boundary. Result: this is *also* broken, silently -- no crash, but
  ## the wrong value (observed: 3, exactly `p.x` alone, as if `p.y`
  ## never made it across). Reported without doAssert, same reasoning
  ## as byValueRoundTrip below: nlvm's by-value struct handling is
  ## broken across the board (arguments and returns, crashing and
  ## silently-wrong), so this doesn't need to abort the process to make
  ## its point -- and letting it continue is what allows
  ## pointerMutateInPlace below to still run and demonstrate the
  ## pointer-based alternative actually works. See ../NOTES.md.
  let p = Point(x: 3, y: 4)
  let sum = rust_point_sum(p)
  echo "point_sum: ", sum, " (expected 7 -- nlvm's by-value struct argument handling is also broken, silently)"

block byValueRoundTrip:
  ## nlvm itself warns at compile time on this call:
  ## "TODO: C ABI for small struct returns not implemented - there may
  ## be issues: rust_point_translate" -- a real, self-acknowledged
  ## limitation, not a maybe. Reported without doAssert (unlike every
  ## other block here) so a known-broken result doesn't abort the
  ## process before the blocks below get to run; see ../NOTES.md for
  ## the full finding.
  let p = Point(x: 3, y: 4)
  let translated = rust_point_translate(p, 10, -1)
  echo "translate: ", translated.x, ",", translated.y, " (expected 13,3 -- nlvm's small-struct-return ABI is a known-incomplete TODO)"

block pointerMutateInPlace:
  var p = Point(x: 3, y: 4)
  rust_point_scale_in_place(addr p, 5)
  echo "scale_in_place: ", p.x, ",", p.y
  doAssert p.x == 15 and p.y == 20,
    "pointer-to-Point mutation drifted from the committed reference value"

type Status {.size: sizeof(cint).} = enum
  sOk = 0
  sWarning = 1
  sError = 2

proc rust_classify(x: cint): Status {.importc: "rust_classify", cdecl.}
proc rust_status_code(s: Status): cint {.importc: "rust_status_code", cdecl.}

block enumRoundTrip:
  ## A bare-discriminant C-style enum crosses the boundary as a plain
  ## integer, not an aggregate -- the hypothesis is that this sidesteps
  ## the by-value-struct ABI bug class entirely on nlvm, unlike `Point`
  ## above. Asserted (not just reported) because if this is wrong, it's
  ## as fatal a finding for the type/layout matrix as the struct cases
  ## were, and should stop the run rather than be silently swallowed.
  let ok = rust_classify(5)
  let warning = rust_classify(0)
  let error = rust_classify(-1)
  echo "classify: ok=", ok, " warning=", warning, " error=", error
  doAssert ok == sOk and warning == sWarning and error == sError,
    "rust_classify results drifted from the committed reference values"

  let okCode = rust_status_code(sOk)
  let warningCode = rust_status_code(sWarning)
  let errorCode = rust_status_code(sError)
  echo "status_code: ok=", okCode, " warning=", warningCode, " error=", errorCode
  doAssert okCode == 0 and warningCode == 1 and errorCode == 2,
    "rust_status_code results drifted from the committed reference values"

echo "all non-fatal nlvm-route Layer 1-3 experiments completed"

# --- Porting ../nim-bin/main.nim's seq/Vec pointer-resolution and
# GC_ref findings to this route: every one of these calls only ever
# crosses the boundary via scalars and pointers, never a by-value
# struct, so the hypothesis is that they carry over unaffected by the
# by-value-struct ABI bugs above. Verified here, not assumed.

proc rust_sum_via_pointer(data: ptr cint, len: cint): clong {.importc: "rust_sum_via_pointer", cdecl.}
proc rust_double_in_place(data: ptr cint, len: cint) {.importc: "rust_double_in_place", cdecl.}
proc rust_vec_growth_probe(len: cint, outAddrBefore, outAddrAfter: ptr clong, outLenAfter: ptr cint) {.importc: "rust_vec_growth_probe", cdecl.}

block nimSeqPointerIntoRust:
  var buf: seq[cint] = @[10.cint, 20, 30, 40, 50]

  let sum = rust_sum_via_pointer(addr buf[0], cint(buf.len))
  echo "nim seq -> rust sum: ", sum
  doAssert sum == 150, "sum via pointer into a Nim-owned seq drifted from the committed reference value"

  rust_double_in_place(addr buf[0], cint(buf.len))
  echo "nim seq after rust double_in_place: ", buf
  doAssert buf == @[20.cint, 40, 60, 80, 100],
    "in-place mutation via pointer into a Nim-owned seq drifted from the committed reference value"

block nimSeqGrowthAddressObservation:
  var buf: seq[cint] = @[10.cint, 20, 30, 40, 50]
  let addrBefore = cast[int](addr buf[0])
  buf.setLen(buf.len + 1000)
  let addrAfter = cast[int](addr buf[0])
  echo "nim seq buffer address before growth=", addrBefore, " after growth=", addrAfter,
    " changed=", addrBefore != addrAfter, " (allocator-dependent; both outcomes are valid)"
  doAssert buf.len == 1005, "seq length after setLen drifted from the committed reference value"

block rustVecGrowthAddressObservation:
  var addrBefore, addrAfter: clong
  var lenAfter: cint
  rust_vec_growth_probe(5, addr addrBefore, addr addrAfter, addr lenAfter)
  echo "rust vec buffer address before growth=", addrBefore, " after growth=", addrAfter,
    " len_after=", lenAfter, " changed=", addrBefore != addrAfter, " (allocator-dependent; both outcomes are valid)"
  doAssert lenAfter == 1010, "rust_vec_growth_probe's reported post-growth length drifted"

block nimSeqLastReferenceDropDanger:
  var freedAddr: int
  block innerScope:
    var doomed: seq[cint] = @[1.cint, 2, 3, 4, 5]
    freedAddr = cast[int](addr doomed[0])

  var fresh: seq[cint] = @[9.cint, 9, 9, 9, 9]
  let freshAddr = cast[int](addr fresh[0])
  echo "freed seq buffer address=", freedAddr, " new seq buffer address=", freshAddr,
    " reused=", freedAddr == freshAddr, " (suggestive of reuse-after-free risk; never dereferenced)"

type SeqBox = ref object
  data: seq[cint]

block nimRefSeqBoxGcRefKeepsAlive:
  var pinnedAddr: int
  block innerScope:
    var localBox = SeqBox(data: @[111.cint, 222, 333, 444, 555])
    GC_ref(localBox)
    pinnedAddr = cast[int](addr localBox.data[0])

  var fresh: seq[cint] = @[9.cint, 9, 9, 9, 9]
  let freshAddr = cast[int](addr fresh[0])
  echo "GC_ref-pinned SeqBox.data buffer address=", pinnedAddr, " new seq buffer address=", freshAddr,
    " reused=", pinnedAddr == freshAddr

  let stillThere = cast[ptr UncheckedArray[cint]](pinnedAddr)
  echo "data read back through the GC_ref-pinned pointer, after its variable's scope ended: ",
    stillThere[0], ",", stillThere[1], ",", stillThere[2], ",", stillThere[3], ",", stillThere[4]
  doAssert stillThere[0] == 111 and stillThere[1] == 222 and stillThere[2] == 333 and
    stillThere[3] == 444 and stillThere[4] == 555,
    "GC_ref(ref object) did not keep its embedded seq's buffer alive/intact past the " &
    "wrapper's own lexical scope, on the nlvm route"

echo "all seq/Vec pointer-resolution and GC_ref experiments completed on the nlvm route"

block byValueInputPointerOutputWorkaround:
  ## Attempted mitigation for byValueRoundTrip's known-broken return:
  ## keep `p` passed in by value, take the result through an output
  ## pointer instead of a return value. Isolated last, deliberately,
  ## because **this segfaults under nlvm** (SIGSEGV, "Attempt to read
  ## from nil?") -- distinct from both wrong-value results above
  ## (silently-wrong, not a crash): a by-value struct argument
  ## *followed by more parameters* (here, three more: dx, dy, outP)
  ## corrupts something more severely than a lone by-value struct
  ## argument (pointSumSingleArg above, which is also broken, but
  ## silently rather than fatally). Overall picture: nlvm's by-value
  ## struct handling is broken in every shape tested here -- as a lone
  ## argument, as a return value, and worst of all as an argument
  ## followed by more parameters. See ../NOTES.md.
  let p = Point(x: 3, y: 4)
  var translated: Point
  rust_point_translate_via_pointer(p, 10, -1, addr translated)
  echo "translate_via_pointer: ", translated.x, ",", translated.y
  doAssert translated.x == 13 and translated.y == 3,
    "by-value-input/pointer-output Point translate drifted from the committed reference value"

echo "all nlvm-route Layer 1-3 experiments passed"

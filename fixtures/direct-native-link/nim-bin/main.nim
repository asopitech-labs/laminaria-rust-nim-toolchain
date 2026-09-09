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

# --- Issue #4 focal question: pointer resolution into a growable,
# reference-counted buffer, both directions. Every earlier fixture in
# this repo that crosses array data (`mixed-rust-nim-executable`) has
# Rust own a plain `Vec` and merely lend Nim a pointer into it — never
# the harder direction of a pointer into memory Nim's own ORC reference
# counting owns and can grow or free. (Not "a GC can move" — ORC never
# relocates seq/string buffers; growth and last-reference-drop
# deallocation are the two things that actually change an address, and
# this file tests both separately below.) See ../NOTES.md for the full
# discussion of both directions and their results.

proc rust_sum_via_pointer(data: ptr cint, len: cint): clong {.importc: "rust_sum_via_pointer", cdecl.}
proc rust_double_in_place(data: ptr cint, len: cint) {.importc: "rust_double_in_place", cdecl.}
proc rust_vec_growth_probe(len: cint, outAddrBefore, outAddrAfter: ptr clong, outLenAfter: ptr cint) {.importc: "rust_vec_growth_probe", cdecl.}

block nimSeqPointerIntoRust:
  ## Direction 1: Nim owns a genuine `seq` (ORC-managed, growable) --
  ## not a caller-owned fixed buffer -- and hands Rust a raw pointer
  ## into its live buffer. Proves resolution (read) and mutation
  ## (write-through) both work when the pointer originates from Nim's
  ## own managed heap, not just from memory Rust itself allocated.
  var buf: seq[cint] = @[10.cint, 20, 30, 40, 50]

  let sum = rust_sum_via_pointer(addr buf[0], cint(buf.len))
  echo "nim seq -> rust sum: ", sum
  doAssert sum == 150, "sum via pointer into a Nim-owned seq drifted from the committed reference value"

  rust_double_in_place(addr buf[0], cint(buf.len))
  echo "nim seq after rust double_in_place: ", buf
  doAssert buf == @[20.cint, 40, 60, 80, 100],
    "in-place mutation via pointer into a Nim-owned seq drifted from the committed reference value"

block nimSeqGrowthAddressObservation:
  ## The caveat half of direction 1: a pointer into a Nim `seq`'s buffer
  ## is only valid until the `seq` next reallocates -- a documented API
  ## contract (`setLen`/`add` "may reallocate"), not something this
  ## block can prove by observation alone. Whether the address actually
  ## changes on a given run is an allocator implementation detail: a
  ## small allocation early in a process may legitimately be grown
  ## in place if free space happens to follow it (glibc's malloc can do
  ## exactly this -- see rustVecGrowthAddressObservation below, where it
  ## was observed doing so on ubuntu-latest/x86_64/glibc in this
  ## project's own CI, in the Rust-owned direction). So this reports the
  ## observation without asserting a specific outcome, and never
  ## dereferences the stale address either way -- only compares it as a
  ## plain integer.
  var buf: seq[cint] = @[10.cint, 20, 30, 40, 50]
  let addrBefore = cast[int](addr buf[0])
  buf.setLen(buf.len + 1000) # large growth, to make in-place extension least likely
  let addrAfter = cast[int](addr buf[0])
  echo "nim seq buffer address before growth=", addrBefore, " after growth=", addrAfter,
    " changed=", addrBefore != addrAfter, " (allocator-dependent; both outcomes are valid)"
  doAssert buf.len == 1005, "seq length after setLen drifted from the committed reference value"

block rustVecGrowthAddressObservation:
  ## Direction 2, the symmetric reverse: Rust owns a growable `Vec`,
  ## forces its own internal reallocation, and reports both buffer
  ## addresses as plain integers -- Nim never receives, and therefore
  ## never risks dereferencing, a possibly-invalidated pointer.
  ##
  ## Observed result on this project's own CI: on macOS/arm64 the
  ## address reliably changed; on ubuntu-latest/x86_64 (glibc) it did
  ## NOT change for this exact growth (5 -> 1010 elements) -- glibc's
  ## allocator extended the small initial allocation in place. That is
  ## a genuine, useful finding in its own right: **the absence of an
  ## address change is not evidence of safety**, only a report of what
  ## one allocator happened to do for one allocation size on one run.
  ## The documented API contract ("growth may reallocate"), not
  ## observed behavior, is what any caller must design against -- which
  ## is exactly why this block only reports the outcome instead of
  ## asserting one.
  var addrBefore, addrAfter: clong
  var lenAfter: cint
  rust_vec_growth_probe(5, addr addrBefore, addr addrAfter, addr lenAfter)
  echo "rust vec buffer address before growth=", addrBefore, " after growth=", addrAfter,
    " len_after=", lenAfter, " changed=", addrBefore != addrAfter, " (allocator-dependent; both outcomes are valid)"
  doAssert lenAfter == 1010, "rust_vec_growth_probe's reported post-growth length drifted"

# --- The question the two blocks above sidestep entirely: neither one
# ever let the Nim seq's *last reference* actually go away while a
# pointer into its buffer was conceptually "held" by the other side.
# Growth-triggered reallocation is an allocator phenomenon, unrelated to
# Nim's ORC reference counting. ORC's actual GC-ness -- deciding *when*
# to free a buffer -- was never exercised until this block. Unlike a
# tracing/stop-the-world collector, ORC frees deterministically at the
# point the last reference's scope ends (same mental model as Rust's own
# `Drop`), so this is fully reproducible, not a rare GC-pause race.

block nimSeqLastReferenceDropDanger:
  ## Deliberately safe demonstration of the real danger: capture a
  ## `seq` buffer's address, let its one and only reference go out of
  ## scope (ORC's injected destructor frees the buffer synchronously,
  ## right here, not "eventually"), then allocate a fresh seq and check
  ## whether the new allocation reused that exact address. This never
  ## dereferences the freed address -- only compares it as a plain
  ## integer -- so address reuse is observed without committing the
  ## use-after-free it's evidence for.
  var freedAddr: int
  block innerScope:
    var doomed: seq[cint] = @[1.cint, 2, 3, 4, 5]
    freedAddr = cast[int](addr doomed[0])
    # `doomed` goes out of scope here. It has exactly one reference
    # (never copied/aliased), so ORC's refcount hits zero and its
    # `=destroy` runs synchronously at this point -- deterministic,
    # like Rust dropping a `Vec` at end of scope, not a GC pause that
    # might happen at some later, unpredictable time.

  var fresh: seq[cint] = @[9.cint, 9, 9, 9, 9]
  let freshAddr = cast[int](addr fresh[0])
  echo "freed seq buffer address=", freedAddr, " new seq buffer address=", freshAddr,
    " reused=", freedAddr == freshAddr, " (suggestive of reuse-after-free risk; never dereferenced)"
  echo "if Rust had captured and kept using a pointer from the freed seq past its scope, " &
    "this would be a real use-after-free -- distinct from, and more dangerous than, the " &
    "growth-reallocation caveat above, and not exercised by any earlier block in this file"

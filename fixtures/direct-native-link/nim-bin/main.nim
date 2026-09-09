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
  ## Comparison point for ../nlvm-experiment/main.nim, where this
  ## specific shape (a lone by-value struct argument, scalar return) is
  ## the isolating test for a family of nlvm-specific ABI bugs this
  ## route (nim c) never exhibits.
  let p = Point(x: 3, y: 4)
  let sum = rust_point_sum(p)
  echo "point_sum: ", sum, " (expected 7)"
  doAssert sum == 7, "by-value Point single-argument sum drifted from the committed reference value"

block byValueRoundTrip:
  let p = Point(x: 3, y: 4)
  let translated = rust_point_translate(p, 10, -1)
  echo "translate: ", translated.x, ",", translated.y
  doAssert translated.x == 13 and translated.y == 3,
    "by-value Point round-trip drifted from the committed reference value"

block byValueInputPointerOutputWorkaround:
  ## Same call, output via pointer instead of return value -- see
  ## ../nlvm-experiment/main.nim for why this variant exists: nlvm's
  ## small-struct-return ABI is a known-incomplete TODO there, so this
  ## input-by-value/output-by-pointer split is the practical
  ## workaround. Included here on the nim c route too, for direct
  ## comparison -- expected to pass here exactly like the block above.
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

# --- Does a solution exist, and is it anything other than an explicit,
# FFI-style manual ownership protocol? Rust and Nim remain two separate
# compile-time ownership-tracking systems even once linked into one
# binary -- Rust's borrow checker has no visibility into Nim's
# destructor injection, and vice versa -- so there is no "single binary"
# trick that lets one side's scope rules automatically account for the
# other's usage. Nim's own documented answer for exactly this case
# ("lifetime of garbage-collected types... can be extended by calling
# GC_ref and GC_unref") is tested here directly, against the identical
# scenario that failed above, rather than assumed to work from reading
# the docs alone.

## First attempt, left visible because failing to compile *is* the
## finding: `GC_ref(doomed)` where `doomed: seq[cint]` does not compile
## under this Nim/mm combination.
##
##   proc GC_ref*[T](x: ref T) {.magic: "GCref", ...}   <- system/arc.nim,
##                                                          the only overload
##                                                          active under ARC/ORC
##
## `system/gc_interface.nim` *does* declare `GC_ref[T](x: seq[T])` and
## `GC_ref(x: string)` too -- but guarded by
## `when hasAlloc and not defined(js) and not usesDestructors:`, i.e.
## only for the legacy `refc` GC. `--mm:orc` sets `usesDestructors`, so
## that whole block -- including the seq/string overloads -- is not
## even compiled in. **`GC_ref`/`GC_unref` only ever apply to `ref T` on
## this toolchain, never to `seq`/`string` directly, confirmed by
## reading the installed compiler's own source, not assumed from
## documentation or search results (which describe the pre-ORC API).**
## The real mechanism, tested below: wrap the seq in a `ref object` and
## pin *that*.

type SeqBox = ref object
  data: seq[cint]

block nimRefSeqBoxGcRefKeepsAlive:
  var pinnedAddr: int
  block innerScope:
    var localBox = SeqBox(data: @[111.cint, 222, 333, 444, 555])
    GC_ref(localBox) # pins the ref object's cell -- this compiles and is the documented mechanism
    pinnedAddr = cast[int](addr localBox.data[0])
    # `localBox` goes out of scope here -- its own lexical reference is
    # gone, and it was never copied/aliased outside this block. Only
    # the GC_ref pin above can keep its cell (and therefore its `data`
    # seq's payload) alive past this point.

  # Deliberately not calling GC_unref: doing so needs a live handle onto
  # the same cell, and nothing outside `innerScope` has one -- by
  # design, to keep this test isolated to "does GC_ref alone work,"
  # not "can this fixture also reconstruct an unref handle." A real
  # pin/unpin protocol crossing into Rust would need Nim to hand back an
  # explicit token for the release step; this fixture deliberately
  # leaks one small allocation rather than fabricate that design.

  var fresh: seq[cint] = @[9.cint, 9, 9, 9, 9]
  let freshAddr = cast[int](addr fresh[0])
  echo "GC_ref-pinned SeqBox.data buffer address=", pinnedAddr, " new seq buffer address=", freshAddr,
    " reused=", pinnedAddr == freshAddr

  # The definitive test: read back through the captured address. Safe
  # to do now, if and only if GC_ref actually kept the buffer alive.
  let stillThere = cast[ptr UncheckedArray[cint]](pinnedAddr)
  echo "data read back through the GC_ref-pinned pointer, after its variable's scope ended: ",
    stillThere[0], ",", stillThere[1], ",", stillThere[2], ",", stillThere[3], ",", stillThere[4]
  doAssert stillThere[0] == 111 and stillThere[1] == 222 and stillThere[2] == 333 and
    stillThere[3] == 444 and stillThere[4] == 555,
    "GC_ref(ref object) did not keep its embedded seq's buffer alive/intact past the " &
    "wrapper's own lexical scope -- either the documented mechanism doesn't cover this " &
    "case on this Nim version, or this experiment used it incorrectly; either finding " &
    "matters for issue #4"

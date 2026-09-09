## `mixed-rust-nim-executable` fixture — Nim side.
##
## #11's "mixed Rust/Nim native executable" Core workload: unlike
## `rust-nim-c-abi-baseline` (deliberately scalar-only, the minimal
## reference point future direct native-link research (#4) compares
## against), this fixture exercises the everyday FFI pattern of a
## caller-owned array crossing the boundary by pointer + length — both
## read (`nim_array_stats`) and mutated in place
## (`nim_array_scale_evens`). Both languages contribute real algorithmic
## work to the one final binary, rather than Rust merely calling into a
## minimal Nim scalar helper.

proc nimArrayAt(data: ptr cint, idx: int): cint =
  cast[ptr UncheckedArray[cint]](data)[idx]

proc nimArraySet(data: ptr cint, idx: int, value: cint) =
  cast[ptr UncheckedArray[cint]](data)[idx] = value

proc nim_array_stats(data: ptr cint, len: cint, outSum: ptr clong,
                      outMin: ptr cint, outMax: ptr cint,
                      outMeanX1000: ptr clong) {.exportc, cdecl.} =
  if len <= 0:
    outSum[] = 0
    outMin[] = 0
    outMax[] = 0
    outMeanX1000[] = 0
    return
  var s: clong = 0
  var mn = nimArrayAt(data, 0)
  var mx = mn
  for i in 0 ..< len.int:
    let v = nimArrayAt(data, i)
    s += v.clong
    if v < mn: mn = v
    if v > mx: mx = v
  outSum[] = s
  outMin[] = mn
  outMax[] = mx
  outMeanX1000[] = (s * 1000) div len.clong

proc nim_array_scale_evens(data: ptr cint, len: cint, factor: cint) {.exportc, cdecl.} =
  var i = 0
  while i < len.int:
    nimArraySet(data, i, nimArrayAt(data, i) * factor)
    i += 2

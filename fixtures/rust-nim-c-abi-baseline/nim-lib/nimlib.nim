## `rust-nim-c-abi-baseline` fixture — Nim side.
##
## #11's "conventional C ABI baseline" Core workload: the reference point
## future Rust-Nim native-linking research (#4) compares against. Every
## exported function crosses the boundary using only C-primitive
## parameters/return values (`cint`/`clong`) — Nim `seq`/`string`/GC types
## never cross the FFI boundary, which is exactly the constraint a "direct
## native link without a mandatory C ABI" experiment (#4) exists to relax.
##
## Computes the same prime-grid/cluster values as
## `fixtures/rust-heavy-workspace` and `fixtures/nim-heavy-workspace` so
## output is directly comparable across all three fixtures.

proc isPrimeImpl(n: int): bool =
  if n < 2:
    return false
  var i = 2
  while i * i <= n:
    if n mod i == 0:
      return false
    inc i
  true

proc primesUpToImpl(limit: int): seq[int] =
  result = @[]
  for n in 2 .. limit:
    if isPrimeImpl(n):
      result.add(n)

type ClusterImpl = object
  xs, ys: seq[int]

proc clusterFromPrimeGrid(limit: int): ClusterImpl =
  let ps = primesUpToImpl(limit)
  result.xs = @[]
  result.ys = @[]
  for i in 0 ..< ps.len - 1:
    result.xs.add(ps[i])
    result.ys.add(ps[i + 1])

proc perimeterImpl(c: ClusterImpl): int =
  result = 0
  for i in 0 ..< c.xs.len - 1:
    result += abs(c.xs[i] - c.xs[i + 1]) + abs(c.ys[i] - c.ys[i + 1])

# --- C ABI surface: only cint/clong cross the boundary. ---

proc laminaria_is_prime(n: cint): cint {.exportc, cdecl.} =
  if isPrimeImpl(n.int): 1.cint else: 0.cint

proc laminaria_cluster_point_count(limit: cint): cint {.exportc, cdecl.} =
  clusterFromPrimeGrid(limit.int).xs.len.cint

proc laminaria_cluster_perimeter(limit: cint): clong {.exportc, cdecl.} =
  perimeterImpl(clusterFromPrimeGrid(limit.int)).clong

proc laminaria_cluster_centroid_x(limit: cint): clong {.exportc, cdecl.} =
  let c = clusterFromPrimeGrid(limit.int)
  if c.xs.len == 0: return 0
  var s = 0
  for x in c.xs: s += x
  (s div c.xs.len).clong

proc laminaria_cluster_centroid_y(limit: cint): clong {.exportc, cdecl.} =
  let c = clusterFromPrimeGrid(limit.int)
  if c.ys.len == 0: return 0
  var s = 0
  for y in c.ys: s += y
  (s div c.ys.len).clong

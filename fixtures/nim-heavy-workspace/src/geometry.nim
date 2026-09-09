## `nim-heavy-workspace` fixture — geometry module, depends on `primes`.

import primes

type
  Point* = object
    x*, y*: int

proc newPoint*(x, y: int): Point =
  Point(x: x, y: y)

proc manhattanDistance*(a, b: Point): int =
  abs(a.x - b.x) + abs(a.y - b.y)

type
  Cluster* = object
    points*: seq[Point]

proc fromPrimeGrid*(limit: int): Cluster =
  let ps = primesUpTo(limit)
  result.points = @[]
  for i in 0 ..< ps.len - 1:
    result.points.add(newPoint(ps[i], ps[i + 1]))

proc totalPerimeter*(c: Cluster): int =
  result = 0
  for i in 0 ..< c.points.len - 1:
    result += manhattanDistance(c.points[i], c.points[i + 1])

proc centroid*(c: Cluster): Point =
  if c.points.len == 0:
    return newPoint(0, 0)
  var sx, sy = 0
  for p in c.points:
    sx += p.x
    sy += p.y
  newPoint(sx div c.points.len, sy div c.points.len)

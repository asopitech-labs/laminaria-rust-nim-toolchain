## `nim-heavy-workspace` fixture — entry point tying `primes` and
## `geometry` together into one buildable, runnable artifact.

import geometry

proc main() =
  let cluster = fromPrimeGrid(200)
  let perimeter = totalPerimeter(cluster)
  let c = centroid(cluster)

  echo "points=", cluster.points.len
  echo "perimeter=", perimeter
  echo "centroid=(", c.x, ", ", c.y, ")"

main()

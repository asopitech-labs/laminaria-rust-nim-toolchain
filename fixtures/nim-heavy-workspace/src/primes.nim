## `nim-heavy-workspace` fixture — lowest module.
##
## Exists to be *built*, not to demonstrate design: one of #11's "Core
## workloads" reference fixtures for the LAMINARIA measurement spine.

proc isPrime*(n: int): bool =
  if n < 2:
    return false
  var i = 2
  while i * i <= n:
    if n mod i == 0:
      return false
    inc i
  true

proc primesUpTo*(limit: int): seq[int] =
  result = @[]
  for n in 2 .. limit:
    if isPrime(n):
      result.add(n)

when isMainModule:
  echo primesUpTo(20)

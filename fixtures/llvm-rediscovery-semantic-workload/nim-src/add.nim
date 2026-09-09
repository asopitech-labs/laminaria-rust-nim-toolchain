## Semantic contract, see ../CONTRACT.md: given two 32-bit two's-complement
## integers, compute their sum modulo 2^32 (wrapping on overflow), no other
## observable effect.

proc add(a, b: int32): int32 {.exportc, cdecl.} =
  a +% b

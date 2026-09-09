## Semantic contract, see ../CONTRACT.md. Real Nim implementation of the
## workload the candidate representation (../substrate/) is cross-checked
## against. Prints "a,b,use_double,result" for the same fixed set of test
## inputs as ../rust-src/add_or_double.rs, so ../trace.sh can diff them
## byte-for-byte.

proc double(x: int32): int32 =
  x +% x

proc addOrDouble(a, b, useDouble: int32): int32 =
  if useDouble != 0'i32:
    double(a)
  else:
    a +% b

let testInputs = [
  (3'i32, 4'i32, 0'i32),
  (3'i32, 4'i32, 1'i32),
  (int32.high, 1'i32, 0'i32),
  (-5'i32, 10'i32, 1'i32),
]

for (a, b, useDouble) in testInputs:
  echo a, ",", b, ",", useDouble, ",", addOrDouble(a, b, useDouble)

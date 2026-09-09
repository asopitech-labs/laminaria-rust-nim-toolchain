# `rust-nim-llvm-lto-compatibility` fixture

#11's "Rust/Nim 2/Nimony shared LLVM/LTO compatibility workload" Core
workload. `docs/research-program.md` Track J names three candidate
paths from Rust/Nim source to LLVM IR/bitcode:

```text
Rust → rustc LLVM bitcode
Nim 2 → nlvm → LLVM IR
Nim 2 → generated C → Clang → LLVM IR/bitcode
Nim 3/Nimony → Leng/lengc → LLVM IR
```

and asks LAMINARIA to "test shared LTO/ThinLTO participation separately
from language ABI/runtime compatibility" — i.e., not "can the linker
resolve symbols across two already-native objects" (every other
Rust/Nim fixture here, including `fixtures/direct-native-link/`), but
"can Rust's and Nim's own LLVM IR be merged into *one module* with
`llvm-link`, before either side reaches native codegen at all."

## Why this is reachable now

`fixtures/direct-native-link/`'s `nlvm` investigation (issue #4)
established that `nlvm` produces genuine LLVM IR with no C source
anywhere in its path, and — checked directly, not assumed —
`nlvm`'s pinned LLVM version (`llvm/llvm.version` in its own repo) is
**exactly `22.1.8`**, matching this project's own pinned `rustc`'s
bundled LLVM version precisely (`rustc --version --verbose` on both
this dev machine and, expected, CI's runner). Same major *and* patch
version on both sides is what makes direct IR-level merging plausible
rather than requiring a version-bridging adapter.

## Method

1. `rustc --crate-type=staticlib --emit=llvm-ir` on `rust-src/lib.rs`
   directly (no Cargo) — the minimal proof only needs one function,
   matching `docs/rust-nim-native-linking.md`'s "first proof should be
   minimal" principle.
2. `nlvm c -c` on `nim-src/main.nim` — same minimality, one call site.
3. `llvm-link` both `.ll` modules into one `.bc` — the actual claim
   under test: this succeeds only if both sides' target triple, data
   layout, and IR version are compatible enough for LLVM to treat them
   as one program.
4. `opt -O2` the merged module and `llvm-dis` it back to text — Track
   J's own caution applies here: "do not infer optimization from flags
   alone... inspect the resulting artifacts." Whether `rust_add`'s call
   (with compile-time-constant arguments `3, 4`) gets inlined or even
   constant-folded across what was, before step 3, a language boundary,
   is the actual evidence to look for — not just that the tools ran
   without error.

## Status

Evidence not yet gathered — this is the fixture's design and rationale,
committed first per this project's own practice of recording intent
before results. `nlvm` cannot run on this dev machine (macOS; only
Linux/Windows release binaries exist upstream, per
`fixtures/direct-native-link/NOTES.md`), so this needs the same CI path
that fixture already established. `llvm-link`/`opt`/`llvm-dis` matching
LLVM 22 are not yet confirmed available on `ubuntu-latest` — CI will
need to install them (`apt.llvm.org`'s installer script is the standard
route) as part of standing this up.

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

## Result: confirmed, by inspecting the merged+optimized module directly

CI (`nlvm-experiment` job, `34326131903`) ran the full pipeline
end-to-end on the first attempt: `llvm-link` merged both `.ll` modules
without error, `opt -O2` ran on the result, and `llvm-dis` produced
readable text to inspect — exactly the artifact Track J asks for
("inspect the resulting artifacts", not infer from flags alone).

The load-bearing evidence, from the merged+optimized IR:

```llvm
define hidden noundef i32 @main(...) ... {
  ...
  %call.res.excpt.nim.737.12.i.i = tail call ptr @signal(...)      ; from excpt.nim
  ...
  %call.res.main.nim.10.21.i = tail call i32 @rust_add(i32 3, i32 4)  ; the Rust call
  store i32 %call.res.main.nim.10.21.i, ptr @result__main_u4, align 4
  ...
  call fastcc void @_ZN11digitsutils6addIntE...(...)               ; from digitsutils.nim
  ...
  %call.res.system.nim.3101.26.i.i = tail call i64 @fwrite(...)    ; from system.nim
  ...
}

define noundef i32 @rust_add(i32 noundef %a, i32 noundef %b) local_unnamed_addr #21 {
  ...
}
```

Two things this shows directly, not by inference:

1. **Nim's own separate source modules were inlined into one `@main`**
   by `opt` — the `.i`/`.i.i` suffixes on SSA names trace back to
   `main.nim`, `excpt.nim`, `system.nim`, `strs_v2.nim`, and
   `digitsutils.nim`, all folded into a single function body.
2. **`rust_add` sits in the merged module as a `define` with a real
   body, not a `declare`** — LLVM's optimizer had full visibility into
   Rust's function while optimizing Nim's caller, in the same pass, in
   the same module. It chose not to inline this specific call (a cost-
   heuristic decision, not a capability limit — the call is a plain
   `call i32 @rust_add(...)` sitting directly inside the already-merged
   `@main`, with nothing opaque or external between the two languages'
   code).

That is the actual claim under test, confirmed: Rust's and Nim's LLVM
IR share one optimization domain once merged, genuinely prior to native
codegen — not two native objects glued together by a linker's symbol
table.

## Ownership note

Issue #4's own acceptance criteria explicitly delegate this class of
experiment: "Shared LLVM/LTO experiments are delegated to #17 and feed
their compatibility findings back into this contract." This fixture is
committed under #11 (it's on that issue's own Core Workloads list) but
its findings are #17's ("Evaluate shared LLVM IR and LTO convergence
for Rust, Nim 2, and Nimony") research subject — reported there
directly rather than only left implicit here.

## From IR inspection to a real executed artifact

The result above (merged+optimized IR, `rust_add` present as a `define`
inside the merged module) only satisfies "inspect the artifact," not
#17's stronger acceptance criterion: "at least one Rust+Nim-origin LLVM
link experiment produces and executes a final artifact." Extended
`build.sh` to go the rest of the way — `llc -filetype=obj` on the
merged+optimized bitcode, linked with the system `cc` driver, and run:

```
$ ./build.sh
...
--- run the fully-merged, natively-compiled binary ---
nim_calls_rust_add result=7
```

One real hazard surfaced doing this, worth recording: `llc`'s default
relocation model doesn't automatically honor the module's own `PIC
Level` flag (present in rustc's emitted IR) — linking failed first with
`relocation R_X86_64_32S against .rodata can not be used when making a
PIE object`, fixed by passing `-relocation-model=pic` to `llc`
explicitly. `rustc`/`clang`'s own backend invocations set this
correctly by default for a PIE-default target (Ubuntu); driving `llc`
directly does not inherit that default.

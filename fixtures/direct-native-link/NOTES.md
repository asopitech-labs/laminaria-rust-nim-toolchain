# `direct-native-link` fixture

#11's "direct native-link workload" Core workload: the minimal Layer 1
proof from `docs/rust-nim-native-linking.md` — "one Rust-produced object
and one Nim-produced object in the same link, with an intentionally
simple symbol relationship and no generated C header contract." This
fixture is what future direct native-link research (issue #4) builds its
deeper Layer 1-6 experiments on top of; it is not that research itself —
it establishes only that the link is possible and inspects the resulting
symbols, nothing about type/layout compatibility, runtime semantics, or
optimization (#4's Layers 2-5).

## Direction

Every other Rust/Nim fixture in this directory has Rust as the build
orchestrator, linking a Nim static library (`rust-nim-c-abi-baseline`,
`mixed-rust-nim-executable`, `boundary-heavy-workload`). This fixture
reverses it: `nim-bin/main.nim` is the final linked binary, and it links
directly against a Rust-produced static library (`rust-lib`, built
first by `build.sh`). Neither side goes through a generated header —
`rust-lib/src/lib.rs` exports `rust_transform` with
`#[no_mangle] pub extern "C"`, and `nim-bin/main.nim` declares it with
`{.importc: "rust_transform", cdecl.}` naming that exact symbol by hand.

## Evidence

Toolchain (this run, native `aarch64-apple-darwin`, not the Homebrew
`x86_64-apple-darwin` shadow on `PATH` — see `toolchains.lock.toml`):

```
rustc 1.98.1 (48a229cea 2026-09-01), host aarch64-apple-darwin
Nim Compiler Version 2.2.10 [MacOSX: amd64]
```

Object/artifact inventory:

```
$ file rust-lib/target/release/librustlib.a nim-bin/direct_native_link_out
rust-lib/target/release/librustlib.a: current ar archive
nim-bin/direct_native_link_out:       Mach-O 64-bit executable arm64
```

Symbol inspection — `rust_transform` in the Rust-produced object inside
the static library, **defined** (`T`, text/global):

```
$ nm rust-lib/target/release/librustlib.a
rustlib-b666d5ef8633fd5a.rustlib.4e22b0bcaef3357d-cgu.0.rcgu.o:
0000000000000000 T _rust_transform
```

`rust_transform` in Nim's own compiled object, **before** linking against
`rust-lib` — undefined (`U`), i.e. Nim already emits a plain extern
reference by that exact name with no adapter layer in between:

```
$ nm nim-bin/nimcache/@mmain.nim.c.o | grep rust_transform
                 U _rust_transform
```

`rust_transform` in the final linked binary — resolved to a concrete
address, no longer undefined:

```
$ nm nim-bin/direct_native_link_out | grep rust_transform
000000010000d424 T _rust_transform
```

Runtime output, matching the committed reference value in both sources:

```
$ ./build.sh
...
result=43
```

## Layer 2/3: fixed-layout struct and pointer round-trips (issue #4)

The `rust_transform` scalar above only proves Layer 1 — a bare `i32`
never leaves any ambiguity about layout. Layer 3 asks the sharper
question: can a Rust `#[repr(C)]` struct and an independently-declared
Nim `{.bycopy.}` object — written by hand on each side with **no shared
header, no bindgen, no textual copy-paste of one from the other** — be
relied on to agree in memory layout? `rust-lib/src/lib.rs` and
`nim-bin/main.nim` each define their own `Point { x, y: i32/cint }`.

Three experiments, in increasing risk order:

1. **Layout cross-check** (`rust_point_layout_probe`): both sides
   compute their own `sizeof`/`alignof`/field-offset understanding of
   `Point` at runtime and compare them, rather than assuming agreement
   because the link succeeded.
2. **By-value struct round-trip** (`rust_point_translate`): Nim passes a
   `Point` by value, Rust returns a new one by value.
3. **Pointer-to-struct mutation** (`rust_point_scale_in_place`): Rust
   mutates a Nim-allocated `Point` in place through a raw pointer —
   lower-risk than (2) since it's the same pointer-based pattern already
   proven safe by `rust-nim-c-abi-baseline`/`mixed-rust-nim-executable`,
   included here mainly to reuse the same independently-declared type.

Result: **all three passed**, on this toolchain/platform, with the
layout cross-check confirming byte-for-byte agreement, not just
plausible-looking output:

```
$ ./build.sh
...
result=43
layout: nim size=8 align=4 offset_x=0 offset_y=4
layout: rust size=8 align=4 offset_x=0 offset_y=4
translate: 13,3
scale_in_place: 15,20
```

Symbol resolution trail for all four exported functions — undefined in
Nim's own object, defined in Rust's, resolved in the final binary,
exactly like `rust_transform`'s trail above:

```
$ nm rust-lib/target/release/librustlib.a | grep rust_point
0000000000000000 T _rust_point_layout_probe
000000000000001c T _rust_point_scale_in_place
0000000000000030 T _rust_point_translate

$ nm nim-bin/nimcache/@mmain.nim.c.o | grep rust_point
                 U _rust_point_layout_probe
                 U _rust_point_scale_in_place
                 U _rust_point_translate

$ nm nim-bin/direct_native_link_out | grep rust_point
000000010000e0a4 T _rust_point_layout_probe
000000010000e0c0 T _rust_point_scale_in_place
000000010000e0d4 T _rust_point_translate
```

**Why by-value array was deliberately *not* attempted**: a bare
fixed-size array parameter (`[i32; 4]` / `array[4, cint]`) passed by
value has no real precedent to lean on — C itself has no by-value array
calling convention (an array-typed C function parameter always decays to
a pointer), so there is no established "C ABI" for Rust and Nim to
independently converge on here, only whatever each compiler's own
extension of the platform calling convention happens to do for an
aggregate that C could never produce this way. Testing it would either
silently pass by coincidence on one platform/architecture or fail in a
way that's expensive to attribute (compiler bug? platform ABI
divergence? genuine incompatibility?) without first doing the disassembly-level
comparison that risk deserves. Left as explicitly open, not attempted
and not claimed working — passing an array via pointer (as
`mixed-rust-nim-executable` already does) remains the recommended
pattern until a dedicated experiment does that comparison properly.

## Focal question: does a pointer into GC-managed memory resolve, both directions?

Raised explicitly as the priority question after the struct-by-value
results above: by-value passing of scalars and fixed-layout structs is
now treated as settled for this fixture's scope. The sharper, more
consequential question is narrower and different in kind — **not**
"can a `seq`/`Vec` itself cross the boundary as a value" (still no —
see below), but "if one side hands the other a raw pointer *into* its
own GC-managed/growable buffer, does that pointer resolve and work
correctly, in both directions, and what exactly can go wrong?"

This matters because it's the realistic pattern: neither side needs to
understand the other's container type (`seq`'s ORC header, `Vec`'s
capacity/allocator state) — only a plain pointer + length, which is
exactly Layer 1/2's proven vocabulary. The question is purely whether
handing out a pointer into memory a *garbage collector or allocator can
move* is safe, and under what constraint.

### Direction A: Nim owns the `seq`, Rust gets a pointer into it

`nimSeqPointerIntoRust` (`main.nim`): a genuine `var buf: seq[cint]` —
not a caller-owned fixed buffer like every earlier fixture uses — hands
`addr buf[0]` straight to `rust_sum_via_pointer`/`rust_double_in_place`.
**Both read and write-through resolved correctly**, and the mutation is
visible back through Nim's own `buf` afterward, confirming it is
genuinely the same memory, not a copy:

```
$ ./build.sh
...
nim seq -> rust sum: 150
nim seq after rust double_in_place: @[20, 40, 60, 80, 100]
```

### Direction B: Rust owns the `Vec`, Nim would get a pointer into it

The general shape of this direction was already proven safe by
`mixed-rust-nim-executable` (Rust owns a `Vec<i32>`, Nim reads/mutates
it through a pointer within a single call). What that fixture didn't
test is the caveat below, which applies identically in this direction.

### The caveat, proven empirically in both directions: growth invalidates the pointer

A pointer into a `seq`'s or `Vec`'s buffer is only valid **until that
container next reallocates** — exactly the same mtime-vs-content-identity
class of pitfall as `fixtures/STATE-CONTRACTS.md` found in Cargo's
fingerprinting, but here the cost of getting it wrong is memory safety,
not just an extra recompile. Both blocks below only ever compare the
before/after address as a plain integer — **neither dereferences the
stale pointer** — so the caveat is demonstrated without committing the
undefined behavior it's warning about:

```
$ ./build.sh
...
nim seq buffer address before growth=4372713544 after growth=4372721736 changed=true
rust vec buffer address before growth=4373848176 after growth=4373850032 len_after=1010 changed=true
```

`nimSeqGrowthInvalidatesPointer` forces the reallocation with
`buf.setLen(buf.len + 1000)` on the Nim side; `rust_vec_growth_probe`
forces it with `Vec::extend` on the Rust side, and deliberately reports
both addresses as `i64`/`clong` integers rather than pointers, so Nim
never even receives a value of a type it could be tempted to
dereference. Both addresses changed on this run, on both platforms CI
exercises — the reallocation genuinely happened, not just theoretically
could.

**Conclusion for this focal question**: pointer resolution into
GC-managed/growable memory works, symmetrically, in both directions,
*for the duration of one FFI call* — but the moment either side's
container reallocates (Nim `seq` growth, Rust `Vec` growth), any pointer
captured before that point is stale. A future direct-native-link design
that wants to hold such a pointer across multiple calls — rather than
re-deriving it fresh each time, as every experiment here does — needs an
explicit contract for that (e.g. pinning the buffer, or the growable
side notifying the other of reallocation) that does not yet exist and
was not attempted.

## Explicitly not attempted, and why (Layer 3/4 scope)

Per `docs/rust-nim-native-linking.md`'s own required "compatibility
matrix" and non-goals, the following are **not** claimed compatible and
were not attempted here — attempting them without the matching Layer 4
(runtime/failure semantics) groundwork would risk exactly the "unsafe
transmutation as an interoperability design" the doc rules out. Note the
narrower claim above (a pointer *into* a `seq`'s buffer resolves) does
**not** contradict this: Rust never sees or constructs a `seq`/`Vec`
value or its header/refcount/capacity metadata, only a bare
pointer-plus-length into memory it's told is `len` scalars — the
container's own representation never crosses the boundary.

- **Nim `seq`/`string` as values** (not as a source of a pointer):
  variable-length, ORC/ARC-managed (reference counted with a cycle
  collector), heap-allocated with a Nim-runtime-specific header layout
  that differs across Nim versions/GC modes by design. Rust's ownership
  model has no compatible representation to receive one directly as a
  value.
- **Closures/function values**: Nim closures carry an implicit
  environment pointer with Nim-GC-managed capture semantics; Rust
  closures are monomorphized or trait-object-boxed with no compatible
  ABI. Only plain `proc`/`fn` pointers (no captured environment) are
  usable across this boundary.
- **Exceptions/panics**: Nim's exception propagation and Rust's
  panic/unwind mechanism are different unwind implementations; letting
  either cross the boundary uncaught is undefined behavior, not merely
  untested. Every function exported in this fixture is either
  `noexcept`-shaped by construction (pure arithmetic) or would need an
  explicit catch-and-translate adapter at the boundary — not attempted.
- **Holding a resolved pointer across multiple FFI calls**: every
  pointer-resolution experiment above re-derives its pointer immediately
  before use and never holds one across a call boundary where the other
  side could run code — see the growth-invalidation caveat above.

These are Layer 4 concerns (`docs/rust-nim-native-linking.md`'s runtime
and failure semantics layer) and are left as open, explicitly-flagged
gaps for whoever picks up issue #4's Layer 4 work next, not silently
assumed away.

## Non-claims

Per `docs/rust-nim-native-linking.md`'s non-goals: this fixture does not
claim arbitrary Rust/Nim value layouts are compatible, does not invent a
new ABI, and does not test runtime/failure semantics beyond the single
growth-invalidation caveat above (Nim's ARC/ORC GC is exercised only as
the owner of a buffer a pointer is taken from — nothing here triggers
Nim's cycle collector, exception handling, or any GC-managed value
actually crossing into Rust as a value). What's proven is Layer 1
(scalar), a first slice of Layer 3 (one fixed-layout struct, by value
and by pointer, cross-checked for layout agreement), and a first slice
of the Layer 3/4 boundary (pointer resolution into GC-managed memory,
both directions, plus its reallocation caveat) — not the full
compatibility matrix, and not the rest of Layer 4-6. Issue #4's own
research is expected to extend this with more type classes and the
remaining runtime/failure/optimization layers
`docs/rust-nim-native-linking.md` describes.

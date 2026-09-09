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

## Explicitly not attempted, and why (Layer 3/4 scope)

Per `docs/rust-nim-native-linking.md`'s own required "compatibility
matrix" and non-goals, the following classes are **not** claimed
compatible and were not attempted here — attempting them without the
matching Layer 4 (runtime/failure semantics) groundwork would risk
exactly the "unsafe transmutation as an interoperability design" the doc
rules out:

- **Nim `seq`/`string`**: variable-length, ORC/ARC-managed (reference
  counted with a cycle collector), heap-allocated with a
  Nim-runtime-specific layout that differs across Nim versions/GC modes
  by design. Rust's ownership model has no compatible representation to
  receive one directly; every fixture in this directory that crosses an
  array deliberately uses a caller-owned pointer+length instead (see
  `mixed-rust-nim-executable`), never a native `seq`.
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

These are Layer 4 concerns (`docs/rust-nim-native-linking.md`'s runtime
and failure semantics layer) and are left as open, explicitly-flagged
gaps for whoever picks up issue #4's Layer 4 work next, not silently
assumed away.

## Non-claims

Per `docs/rust-nim-native-linking.md`'s non-goals: this fixture does not
claim arbitrary Rust/Nim value layouts are compatible, does not invent a
new ABI, and does not test runtime/failure semantics (Nim's ARC/ORC GC
is never invoked by anything exported here — every function above
touches only `i32`/`cint`-typed data, no Nim-managed memory crosses back
into Rust). What's proven is Layer 1 (scalar) and a first slice of Layer
3 (one fixed-layout struct, by value and by pointer, cross-checked for
layout agreement) — not the full compatibility matrix, and not Layer 4-6
at all. Issue #4's own research is expected to extend this with more
type classes and the runtime/failure/optimization layers
`docs/rust-nim-native-linking.md` describes.

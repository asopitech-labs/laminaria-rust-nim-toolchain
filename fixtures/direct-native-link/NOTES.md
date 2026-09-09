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

## Focal question: does a pointer into a growable/reference-counted buffer resolve, both directions?

Raised explicitly as the priority question after the struct-by-value
results above: by-value passing of scalars and fixed-layout structs is
now treated as settled for this fixture's scope. The sharper, more
consequential question is narrower and different in kind — **not**
"can a `seq`/`Vec` itself cross the boundary as a value" (still no —
see below), but "if one side hands the other a raw pointer *into* its
own growable buffer, does that pointer resolve and work correctly, in
both directions, and what exactly can go wrong?"

**Correction on "GC-managed" wording**: earlier drafts of this section
called this "a pointer into memory a garbage collector can move." That
framing is imprecise and worth correcting explicitly, not quietly.
Nim's `mm:orc` (confirmed active for every build in this fixture — see
the `Hint: mm: orc` line in the evidence below) is **not** a
tracing/stop-the-world/relocating collector for `seq`/`string` payload
buffers at all — Nim has never had one; pointer stability into
seq/string data has always been a language design goal. ORC is
deterministic reference counting, the same mental model as Rust's own
ownership/`Drop`: a buffer's address changes only for two distinct,
unrelated reasons, and this fixture now tests both separately:

1. **Explicit growth** (`setLen`/`add`/`Vec::extend`/`reserve`) is a
   plain allocator reallocation, nothing to do with reference counting
   or collection — tested in the growth-observation blocks below, and
   identical in kind to what `Vec`'s allocator does.
2. **Last-reference-drop deallocation** — ORC's actual "GC-ness":
   freeing a buffer synchronously the moment its reference count hits
   zero (scope exit, in the common case, exactly like Rust dropping a
   `Vec`). This is the one none of the experiments above exercised, and
   is the sharper, more dangerous question — see below.

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

### The caveat: growth *may* invalidate the pointer — and CI proved why "may" is the right word

A pointer into a `seq`'s or `Vec`'s buffer is only valid **until that
container next reallocates** — a documented API contract
(`setLen`/`add`/`extend`/`reserve` all say "may reallocate"), not a
guarantee that every reallocation is observable by comparing addresses.
Both blocks below only ever compare the before/after address as a plain
integer — **neither dereferences the stale pointer** — so the point is
made without committing the undefined behavior it's about:

```
$ ./build.sh   # macOS/arm64
...
nim seq buffer address before growth=4369780808 after growth=4369789000 changed=true
rust vec buffer address before growth=4378747616 after growth=4378748752 len_after=1010 changed=true
```

An earlier version of this fixture hard-asserted `changed == true` for
both, on the (wrong) assumption that a large-enough growth always moves
the buffer. **CI's ubuntu-latest job caught this being false**:

```
rust vec buffer address before growth=94099226362544 after growth=94099226362544 len_after=1010 changed=false
```

On `ubuntu-latest`/`x86_64` with glibc, growing the `Vec` from 5 to 1010
elements did **not** move the buffer — glibc's allocator extended the
small initial allocation in place, because free heap space happened to
follow it early in the process. The Nim-side `seq` growth changed
address on every platform observed so far, but nothing here proves it
always will either.

**This is the actual finding, and it's more useful than "reallocation
always moves the buffer"**: whether an address changes after growth is
an allocator implementation detail, not something a caller can rely on
observing. Code that captured a pointer, grew the container, and then
kept using the old pointer *because the address happened not to
change* would be exhibiting exactly the false sense of safety this
caveat warns about — the bug wouldn't reproduce on every platform, which
is worse than reproducing on all of them. `NOTES.md`'s original claim
("both addresses changed... the reallocation genuinely happened, not
just theoretically could") was itself an overclaim corrected by this
run — left here, struck through in spirit, as its own small case study
in verifying evidence rather than trusting a single platform's run.

**Conclusion for the growth caveat**: pointer resolution into growable
memory works, symmetrically, in both directions, for the duration of one
FFI call — but nothing about *whether the address visibly changes* on
any given reallocation is part of the contract a caller can build on.

### The sharper question: last-reference-drop deallocation (ORC's actual GC-ness)

Both blocks above share a property that understates the real risk:
neither ever let the Nim `seq`'s one and only reference actually go
away while a pointer into its buffer was conceptually "held" elsewhere.
Growth is an allocator phenomenon; **freeing a buffer because its last
reference's scope ended is ORC's actual job**, and it was never
exercised until this block:

```
$ ./build.sh
...
freed seq buffer address=4374057032 new seq buffer address=4374057032 reused=true (suggestive of reuse-after-free risk; never dereferenced)
if Rust had captured and kept using a pointer from the freed seq past its scope, this would be a real use-after-free -- distinct from, and more dangerous than, the growth-reallocation caveat above, and not exercised by any earlier block in this file
```

`nimSeqLastReferenceDropDanger`: allocate a `seq` inside an inner
`block`, capture `addr doomed[0]` as a plain integer, let `doomed` go
out of scope (its single reference drops to zero, so ORC's injected
`=destroy` frees the buffer **synchronously, right there** — not at
some unpredictable later GC pause, since ORC is deterministic reference
counting, not a tracing collector), then allocate a fresh, unrelated
`seq` immediately after and compare its address to the freed one. Never
dereferences the freed address — only compares it as a plain integer —
so the finding is observed without committing the use-after-free it's
evidence for.

**Result: the freed address was reused by the very next allocation on
every repeated run on macOS/arm64 (`aarch64-apple-darwin`)** — a stark,
concrete way to see the real danger: if Rust had captured a pointer from
that `seq` and kept using it past the point where Nim's last reference
dropped, it would not merely risk crashing — it would silently read and
write into what is now a **completely different, unrelated Nim object's
live memory**. That's a worse failure mode than a crash: silent data
corruption in an object that has nothing to do with the one the pointer
was originally taken from.

CI's two platforms diverged on this specific point, which is itself
useful confirmation that "did the address get reused" is exactly as
allocator-dependent as the growth caveat above, not a fixed law:

```
macOS/arm64:            freed=4385329224 new=4385329224 reused=true
ubuntu-latest/x86_64:    freed=140082439680608 new=140082439680672 reused=false
```

On this `ubuntu-latest` run, glibc handed the next allocation a *nearby*
address (64 bytes later) rather than the exact freed one — plausibly the
freed chunk went to a free-list and the next allocation took a
different, adjacent slot. `reused=false` here is not evidence the
danger doesn't exist on that platform: the memory was still freed and
still eligible for reuse by *some* future allocation, on both platforms,
which is the actual claim. Whether the *very next* allocation happens to
land on the exact freed address is exactly the kind of allocator detail
the growth caveat already established isn't something to rely on
observing either way.

**This is the real headline finding for this focal question**, sharper
than the growth caveat: pointer resolution into Nim-owned memory is only
safe for as long as the Nim side guarantees the owning reference stays
alive. Nothing in any experiment in this fixture establishes such a
guarantee across an FFI call boundary — every access here re-derives its
pointer immediately before use, inside the same scope that owns the
`seq`, and never holds one past a call where the other side could run
code. A future direct-native-link design that wants to hand out a
pointer whose validity outlives one call needs an explicit ownership/
lifetime contract (e.g. Nim promising not to drop the reference, or an
explicit "pin" operation, or the boundary requiring reference-counted
ownership on both sides) — none of which exists yet, and this
experiment is exactly why it can't be skipped: the danger is not
hypothetical, it's directly observable with a two-line reproduction.

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
new ABI, and does not test runtime/failure semantics beyond the two
pointer-validity caveats above (growth-triggered reallocation, and
last-reference-drop deallocation — ORC's reference counting is
exercised as the owner of a buffer a pointer is taken from and freed
from, but nothing here triggers Nim's cycle collector — a `seq[cint]` of
scalars can never form a reference cycle — Nim exception handling, or
any `seq`/`string`/closure *value itself* crossing into Rust). What's
proven is Layer 1 (scalar), a first slice of Layer 3 (one fixed-layout
struct, by value and by pointer, cross-checked for layout agreement),
and a first slice of the Layer 3/4 boundary (pointer resolution into a
growable, reference-counted buffer, both directions, plus its two
validity caveats — reallocation and deallocation) — not the full
compatibility matrix, and not the rest of Layer 4-6 (thread/TLS
obligations, exceptions, WASM). Issue #4's own research is expected to
extend this with more type classes and the remaining runtime/failure/
optimization layers `docs/rust-nim-native-linking.md` describes.

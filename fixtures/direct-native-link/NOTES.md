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

## Scope warning, load-bearing for every finding below

Every experiment in `nim-bin/` uses `nim c` — Nim's default backend,
which always generates C source and hands it to a C compiler before a
native object exists. Even with no `.h` file and hand-matched symbol
names (this fixture's actual Layer 1 claim), the resulting object's
calling convention, name-mangling scheme, and aggregate-layout rules are
still fundamentally C's, because a C compiler is what produced them.
That is a real, useful finding on its own — "no generated header" is not
nothing — but it is **not** the same claim as "Rust and Nim participate
in one native link without being constrained by C's ABI at all," which
is `docs/rust-nim-native-linking.md`'s actual framing (a Rust codegen
pipeline and a Nim codegen pipeline sharing one artifact/link model, not
each independently targeting C's calling convention and happening to
agree).

Per this project's own `docs/research-program.md` Track J, there are at
least three distinct Nim-side native-code routes, evaluated separately
by design:

```text
Nim 2 -> generated C -> Clang -> LLVM IR/bitcode   <- nim-bin/, every experiment above
Nim 2 -> nlvm -> LLVM IR                            <- nlvm-experiment/, first attempt below
Nim 3/Nimony -> Leng/lengc -> LLVM IR               <- not attempted; separate, future compiler
```

`nlvm` (https://github.com/arnetheduck/nlvm) bypasses C entirely: Nim
source to nlvm's own LLVM IR emission to native object, no C source or C
compiler anywhere in the path. `nlvm-experiment/` is a first,
genuinely-different-route attempt — see that section below. Nothing in
`nim-bin/` should be read as evidence about the C-free route; it's
evidence about the C-generating route only, useful mainly as the
baseline the C-free route needs to be compared against.

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
alive. Nothing in the experiments above establishes such a guarantee
across an FFI call boundary — every access re-derives its pointer
immediately before use, inside the same scope that owns the `seq`, and
never holds one past a call where the other side could run code.

### Does a solution exist, and is it anything other than an explicit protocol?

Asked directly: Rust and Nim remain two separate compile-time
ownership-tracking systems even once linked into one binary — Rust's
borrow checker has no visibility into Nim's compiler-injected
destructors, and vice versa. There is no "single binary" trick that
makes one side's scope rules automatically account for the other side's
usage; some kind of explicit, manually-operated ownership protocol
across the boundary is the only generally-available answer, the same
way it is for CPython's `Py_INCREF`/`Py_DECREF` or JNI's global
references. That expectation was checked directly against Nim's own
documented mechanism, rather than assumed to hold from precedent.

**First finding: the documented API doesn't mean what the docs (and
initial web research) suggested.** Nim's manual and several search
results describe `GC_ref`/`GC_unref` as accepting `seq[T]`/`string`
directly. Trying that — `GC_ref(doomed)` where `doomed: seq[cint]` —
**fails to compile** under this toolchain. Reading the installed
compiler's own source settles why:

```
$ grep -n -A2 'proc GC_ref' /usr/local/Cellar/nim/2.2.10/nim/lib/system/arc.nim
proc GC_ref*[T](x: ref T) =
  ## New runtime only supports this operation for 'ref T'.
  if x != nil: nimIncRef(cast[pointer](x))
```

`system/gc_interface.nim` *does* declare `seq`/`string` overloads too —
guarded by `when hasAlloc and not defined(js) and not usesDestructors:`.
`--mm:orc` sets `usesDestructors`, so that whole block, seq/string
overloads included, is never even compiled in. **`GC_ref`/`GC_unref`
apply only to `ref T` under ARC/ORC — never to `seq`/`string`
directly.** The search results describing a `seq` overload were
describing the legacy `refc` GC's API, not current default Nim; ground
truth came from the compiler's own source and an actual failed
compilation, not from documentation or search summaries.

**Second finding: wrapped in a `ref object`, the documented mechanism
works, verified by reading data back, not just observing an address.**
`nimRefSeqBoxGcRefKeepsAlive`: wrap the `seq` in `type SeqBox = ref
object; data: seq[cint]`, call `GC_ref(localBox)` before `localBox` goes
out of scope (with no other Nim-level reference to it anywhere), then —
past that scope — read the data back through the raw pointer captured
earlier:

```
$ ./build.sh
...
GC_ref-pinned SeqBox.data buffer address=4304490632 new seq buffer address=4304490600 reused=false
data read back through the GC_ref-pinned pointer, after its variable's scope ended: 111,222,333,444,555
```

Correct, original data (`111,222,333,444,555`), read back through a
pointer captured before the only Nim-level reference went out of scope
— reproduced identically across multiple repeated runs. This is the
positive counterpart to the danger finding above: **the danger is real,
and a real, working solution for it already exists in Nim, provided the
shared data is wrapped in a `ref object` rather than passed as a bare
`seq`/`string`.**

### What's still unsolved

This fixture deliberately never calls the matching `GC_unref` — doing
so needs a live Nim-level handle onto the same cell, and by design
nothing outside `nimRefSeqBoxGcRefKeepsAlive`'s inner scope has one (one
small allocation is deliberately leaked as a result). That gap is
itself the real remaining design question for issue #4: a usable
cross-language pin/unpin protocol needs Nim to hand Rust back an
explicit release token when pinning, and Rust needs to call an exported
Nim proc (not raw `GC_unref`, which isn't itself `exportc`-friendly
across the boundary as used here) to release it — a small, concrete,
buildable next increment, not attempted in this session.

## Layer 4: what happens when Rust panics across the boundary (`panic-experiment/`)

Previously entirely untested — flagged only as "undefined behavior, not
merely untested" below. Checked directly instead of leaving it there.

`rust-lib`'s own unit tests found the first half of this by accident:
an earlier `#[should_panic]` test calling `rust_panics(1)` (a plain
`extern "C" fn` that panics when triggered) didn't get caught by the
test harness's own `catch_unwind` — it aborted the whole test process:

```
thread caused non-unwinding panic. aborting.
```

This happens with **no Nim involved at all** — Rust calling its own
`extern "C" fn` from its own test harness. Reason: a plain
`extern "C" fn` is a "cannot unwind" boundary by Rust's own current
default ABI semantics; a panic that tries to propagate out of one
doesn't unwind, it calls `panic_cannot_unwind` and aborts immediately.

`panic-experiment/main.nim` confirms this holds identically when the
caller is genuinely Nim, on both the `nim c` and `nlvm` routes:

```
$ ./main
normal (non-panicking) call result=42
about to call rust_panics(1) -- expect the process to abort here, not return
...
panicked at src/lib.rs:26:9:
deliberate panic for issue #4 Layer 4 testing
...
panicked at .../core/src/panicking.rs:225:5:
panic in a function that cannot unwind
...
   19:        0x1024d16c8 - _rust_panics
   20:        0x1024d137c - _NimMainModule
   21:        0x1024d1224 - _NimMainInner
   22:        0x1024d1434 - _NimMain
   23:        0x1024d1480 - _main
thread caused non-unwinding panic. aborting.
SIGABRT: Abnormal termination.
```

**Result, and why it's actually good news**: this is not undefined
behavior — it's a clean, deterministic `SIGABRT` (exit `134`), with
Rust's own diagnostic message, and a backtrace that shows the exact
call chain (`_main → _NimMain → ... → _rust_panics → panic_cannot_unwind
→ abort`). Nim's own runtime never gets a chance to run any cleanup —
the process simply ends — but "always aborts, deterministically, with a
diagnosable message" is a real, well-defined contract a caller can plan
around, not the silent-corruption failure mode the doc's non-goals
worry about. The practical implication for anything built on this
fixture's pattern: a Rust function exposed across this boundary must
either be genuinely panic-free (provably, or by construction — pure
arithmetic, no indexing/unwrap/allocation-failure paths) or wrap its
body in `std::panic::catch_unwind` itself and translate the caught
panic into an explicit error return value before it ever reaches the
`extern "C"` boundary — there is no partial-failure/graceful-unwind
option once execution has entered Rust code exported this way.

Not tested: the reverse direction (a Nim exception raised while Rust
code is on the stack, e.g. Rust calling back into a Nim callback that
raises) — no experiment in this fixture has Rust call into Nim yet,
every call here goes Nim → Rust only.

## Layer 4: does calling into Rust from a Nim-spawned thread work? (`thread-experiment/`)

Previously untested. Rust's stdlib normally registers per-thread
bookkeeping (`std::thread::current()` metadata, stack-overflow guard
page) when it spawns a thread itself via `std::thread::spawn`; an OS
thread Nim creates directly (`createThread`/pthread) is "foreign" to
Rust — it never went through that registration. Whether Rust code
still works correctly when called from such a thread is a real
question, not an obviously-yes one, so it was checked with two calls
of increasing risk rather than assumed from "the allocator is
documented as thread-safe":

1. `rust_transform` — pure arithmetic, touches no thread-local state at
   all. Expected to work regardless; a baseline sanity check.
2. `rust_vec_growth_probe` — allocates and reallocates a `Vec`,
   actually exercising Rust's global allocator from this foreign
   thread. The real stress test.

```
$ ./main
worker thread: rust_transform(21)=43
worker thread: rust_vec_growth_probe len_after=1010 (allocator exercised from a Nim-spawned, Rust-foreign OS thread)
main thread: worker completed successfully -- Rust calls from a Nim-spawned thread work correctly
```

**Result: both calls succeed correctly**, on both the `nim c` and
`nlvm` routes, with no special thread-registration step on either
side. Rust's default allocator (the system allocator on the platforms
this project targets) genuinely doesn't require a thread to have gone
through `std::thread::spawn` to use it safely — confirmed directly for
this project's actual boundary shape, not merely cited from Rust's own
allocator documentation.

Not tested: anything that *would* touch Rust's per-thread metadata
directly (e.g. `std::thread::current().name()`, which panics without a
registered thread — a `panic!` here would additionally exercise the
Layer 4 panic-boundary finding above, but wasn't combined with it in
this session), or threads created by *Rust* and called into by Nim
(the reverse direction), or any GC-safety interaction with Nim's own
ORC when Nim-side code (not just Rust's allocator) runs on this
thread.

## Layer 3/4, reverse direction: Rust calling a Nim callback (`callback-experiment/`)

Every experiment above has Nim call into Rust. This reverses it: Rust
calls a Nim-provided function pointer directly (`rust_calls_callback(cb:
extern "C" fn(i32) -> i32, x: i32)`). A plain function pointer with no
captured environment is exactly the "closures/function values" class
`docs/rust-nim-native-linking.md`'s compatibility matrix already lists
as usable when nothing is captured — verified here, not just declared
usable in principle.

**Normal case**: `rust_calls_callback(nim_double, 21)` → `42`, correct,
on both routes.

**The actual focal question**: what happens when the Nim callback
*raises a Nim exception* while a live Rust stack frame sits between it
and Nim's own exception handling? Nim's default exception implementation
(`--exceptions:goto`, Nim 2.x's default) isn't based on stack unwinding
at all — it's deferred flag-checking, compiler-inserted after call
sites — so the interesting question is whether a raw Rust function-
pointer call site (which has no idea it needs to check anything) breaks
that mechanism.

```
$ ./main
rust_calls_callback(nim_double, 21)=42
normal callback case completed
about to call rust_calls_callback(nim_raises, 21) -- observing what happens when a Nim exception is raised while a Rust stack frame is live between it and Nim's own handler
.../callback-experiment/main.nim(42) main
.../callback-experiment/main.nim(31) nim_raises
Error: unhandled exception: deliberate Nim exception inside a Rust-invoked callback [ValueError]
```
Exit code `1`. The line after the raising call (`echo "UNREACHABLE
if..."`) never runs — confirmed reliably across repeated runs, not a
one-off — and the traceback correctly attributes the exception to
`nim_raises` at the exact line it was raised.

**Result: this is safe, in the same spirit as the panic finding above**
— not silent corruption, a clean, deterministic, correctly-attributed
failure report, on both the `nim c` and `nlvm` routes. The Rust stack
frame in between doesn't need to know anything about Nim's exception
convention because Nim's goto-based mechanism was never relying on
stack unwinding through it in the first place; the compiler-inserted
check simply fires at the next point in *Nim-generated* code that looks
at the call's result, which in this fixture is immediately after the
call (assigning `afterRaise`), not delayed.

One earlier version of this experiment used `discard
rust_calls_callback(nim_raises, 21)` instead of capturing the result,
and that version's post-call `echo` line *did* execute once before the
error surfaced — the check still fired correctly and the process still
exited cleanly, but *where* in the following code it fires is
sensitive to what the generated code around the call site looks like,
not a single fixed offset from the call itself. Practical implication:
don't reason about "how many statements after a possibly-raising
callback call it's safe to assume normal control flow" — the safe
assumption is that any statement after such a call may not execute
before the exception is reported, not that a specific one won't.

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
- **Closures with a captured environment**: Nim closures carry an
  implicit environment pointer with Nim-GC-managed capture semantics;
  Rust closures are monomorphized or trait-object-boxed with no
  compatible ABI. Plain `proc`/`fn` pointers with **no** captured
  environment are usable across this boundary and are no longer merely
  claimed so — verified in both directions (`callback-experiment/`
  above; every other experiment already crosses Nim → Rust) with real
  round-trip results.
- **Nim exceptions crossing into Rust**: no longer entirely untested —
  see the dedicated Layer 3/4 "reverse direction" section above for the
  specific case of a Nim callback raising while called from Rust
  (clean, safe: exit code 1, correctly-attributed traceback). Not
  covered by that experiment: a Nim exception raised while Rust code
  deeper on the stack has itself allocated resources needing cleanup
  (Rust has no Drop-running mechanism triggered by Nim's non-unwinding
  exception check, so any Rust-side resource acquired before the
  callback call and only released after it returns normally would leak
  if the exception causes Nim to never return control past that point).
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
obligations, exceptions, WASM). All of it is `nim c`-route evidence
only, per the scope warning above. Issue #4's own research is expected
to extend this with more type classes, the remaining runtime/failure/
optimization layers `docs/rust-nim-native-linking.md` describes, and —
per the scope warning above — the actual C-free route this issue is
ultimately about.

## `nlvm-experiment/`: the first attempt at the actual C-free route

`nim-bin/`'s entire evidence base is `nim c`-route-specific (see the
scope warning at the top of this document). `nlvm-experiment/` is a
first, minimal attempt at issue #4's real subject: Nim reaching a native
artifact through a route that never generates C at all.

No pre-built `nlvm` binary exists for macOS (only Linux and Windows
release assets); building from source on this dev machine was assessed
and set aside for now — `nlvm`'s own build pins and builds its own exact
LLVM revision rather than reusing the host's existing Homebrew LLVM, and
bootstraps its own Nim from C sources, which is a large, failure-prone
time investment relative to the Linux release binary this project's own
CI can already run today. `.github/workflows/ci.yml`'s `nlvm-smoke-test`
job downloads that binary and compiles/runs `hello.nim` — the smallest
possible confirmation that `nlvm` works at all on this project's CI
before attempting the harder direct-link-with-Rust experiment. Isolated
in its own CI job, not the main `rust` matrix, so a failure here
(plausible — this is genuinely exploratory territory) doesn't fail-fast
cancel the fixture suite that already works.

### Result: it works, and the no-C claim is directly verified, not assumed

CI (`nlvm-smoke-test`, ubuntu-latest) actually ran this. `nlvm c
hello.nim` compiled and linked successfully, and the binary ran and
printed the expected output:

```
Hint: ld.lld --hash-style=gnu ... --as-needed /home/runner/.cache/nim/hello_d/hello.o
     -lgcc --as-needed -lgcc_s --no-as-needed -lpthread -lc ...  [Link]
30444 lines; 1.131s; ...; out: .../nlvm-experiment/hello [SuccessX]
hello from nlvm
```

The link line uses `ld.lld` directly against `hello.o` plus the
standard platform C-runtime startup objects (`crt1.o`/`crti.o`/`crtn.o`,
`libc`, `libgcc`, `libpthread`) — the same startup objects *any* native
ELF binary needs on Linux, Rust's own binaries included; their presence
doesn't mean C source was generated for this program. The actual check
for that: `nlvm`'s own per-project build cache
(`~/.cache/nim/hello_d/`, the directory the link line above reads
`hello.o` from) contains **exactly one file**:

```
$ find ~/.cache/nim/hello_d -type f
/home/runner/.cache/nim/hello_d/hello.o
```

One native object file. No `.c`, no intermediate C source anywhere in
the pipeline that produced it — confirmed by inspecting the actual build
cache after a real compile, not inferred from nlvm's README. (An earlier
version of this check scanned nlvm's *entire install tree* instead and
wrongly reported failure — it was matching unrelated `.c` files nlvm
ships for other purposes, Windows mingw COM/ATL interop stubs and Nim
stdlib's `linenoise.c` REPL editor, neither generated by nor related to
compiling `hello.nim`. Scoping the check to the actual per-project cache
fixed the false positive — another small case study in checking the
right thing, not just checking something.)

**This is the first real, verified evidence for issue #4's actual
subject**: Nim reaching a native, linkable object through a route where
no C source or C compiler participates at all, as an alternative to
every other experiment in this directory (`nim-bin/`, all `nim c`-route).

### The apples-to-apples comparison: does `nim c`'s Layer 1-3 evidence hold on the C-free route?

`nlvm-experiment/main.nim` mirrors `nim-bin/main.nim`'s Layer 1-3
experiments exactly, linked against the identical `rust-lib` static
library — same Rust artifact, only the Nim-side route differs. **Result:
every call that keeps `Point` behind a pointer (layout introspection,
in-place mutation) is identical and correct on both `nim c` and `nlvm`,
same as bare scalars.** Symbol resolution and the no-C-generated claim
both hold too (same single-`.o`-file cache contents as `hello.nim`
above; `nm` resolves every symbol correctly regardless of calling-
convention correctness).

Known limitation, noted for completeness rather than as a finding to
act on: passing/returning `Point` **by value** (not behind a pointer) is
broken on this `nlvm` build (`continuous`, commit `a9c3397`) in every
shape tried — nlvm's own compiler self-reports the return case as an
incomplete TODO. This isn't a realistic pattern for hand-written
cross-language FFI code in the first place (by-value aggregate
parameters/returns are rare even in ordinary C-ABI-boundary code), so
it's inherently low priority here, not merely low priority "for this
project's design." The real resolution, if this ever matters, is
LAMINARIA's own tooling automatically wrapping a by-value aggregate into
a pointer-passing call at the boundary — the transparent-upgrade-over-
conventional-C-ABI idea from earlier in this issue's discussion, not a
hand-written-code convention. Not pursued further this session.

### The seq/Vec/GC_ref findings port cleanly to the nlvm route — verified, not assumed

The hypothesis above ("expected to work, since they never pass a struct
by value") was checked directly rather than left as a plausible guess.
Every `seq`/`Vec` pointer-resolution, growth-observation, deallocation-
danger, and `GC_ref` block from `nim-bin/main.nim` was ported verbatim
into `nlvm-experiment/main.nim` and run for real (CI `34322068161`):

```
nim seq -> rust sum: 150
nim seq after rust double_in_place: @[20, 40, 60, 80, 100]
nim seq buffer address before growth=140506370580552 after growth=140506370588744 changed=true
rust vec buffer address before growth=112616112 after growth=112616112 len_after=1010 changed=false
freed seq buffer address=140506370580552 new seq buffer address=140506370580552 reused=true
GC_ref-pinned SeqBox.data buffer address=140506370580616 new seq buffer address=140506370580584 reused=false
data read back through the GC_ref-pinned pointer, after its variable's scope ended: 111,222,333,444,555
```

Every value matches the `nim c` route exactly: correct sum, correct
in-place mutation, correct post-`GC_ref` data readback, and the same
platform-dependent (not route-dependent) growth/reuse-address variance
already established above. **This confirms the general rule cleanly**:
nlvm's ABI gap is specific to by-value aggregate passing; anything
expressed as scalars and pointers — including `GC_ref`'s `ref object`
pinning mechanism itself — carries over identically to the C-free route
with no adaptation needed.

### Not yet attempted

The `nlvm`-via-Docker path on this dev machine's own architecture (arm64
macOS), set aside in favor of CI's already-working Linux path rather
than fixing this machine's broken local Colima/Lima install (a known,
separate, pre-existing environment issue — see `toolchains.lock.toml`
history). Reporting the by-value-struct ABI finding upstream to `nlvm`'s
own issue tracker was considered and deliberately not pursued: this
project's own boundary design was already pointer-only for reasons
independent of `nlvm` (see the Layer 3 note above), so the finding is
confirmatory rather than something this project needs fixed upstream to
proceed.

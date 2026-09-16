//! Hypothesis (not connected to laminaria-plan's Obligation model on
//! purpose -- built fresh in an isolated worktree/crate to avoid carrying
//! over that model's assumptions): instead of "N independent frontends ->
//! N object files -> one linker pass that resolves symbols after the
//! fact," can the boundary-crossing (FFI) symbol resolution be modeled as
//! a *shared, concurrently-writable graph* that every frontend writes to
//! *while* it does its own realm-local semantic analysis, so no separate
//! "linking" pass is needed to discover which realm satisfies which
//! requirement?
//!
//! Deliberately not one monolithic table: each realm (Cargo/Rust,
//! Nimble/Nim, C, C++) owns its own local symbol table -- its internal
//! type/signature detail is never exposed outside that realm. Only
//! symbols that cross a realm boundary (an `extern "C"` declaration in
//! Rust, an FFI export in Nim/C/C++) become nodes in the shared global
//! graph, and only the *relationship* between a requirement and its
//! provider becomes an edge. This keeps the shared, contended state small
//! (boundary symbols only) while each realm's own heavy semantic work
//! (parsing, type-checking, monomorphization) stays entirely local and
//! parallel -- the same shape mold's own design took relative to GNU
//! ld/lld (parse everything in parallel first, resolve symbols as a
//! separate, independent pass), but applied one layer upstream of object
//! files: this graph exists *instead of* one, not to describe one after
//! the fact.
//!
//! ## Scope correction: no cross-platform `ElfX86_64PendingReloc`, ever
//!
//! An earlier version of this crate treated `ElfX86_64PendingReloc`/
//! `apply_relocations` as if they were a platform-agnostic abstraction
//! with an ELF-shaped implementation filled in first and a Windows/COFF
//! variant to follow later. Direct instruction against that: generalizing
//! the *shape* of the code-patching step before more than one real
//! platform's own requirements are known is exactly the mistake this
//! whole hypothesis exists to avoid repeating (GNU ld's own single-pass
//! design became "the" abstraction other linkers inherited, long after
//! the actual constraint that produced it -- 1970s I/O/memory limits --
//! stopped applying). Verified directly why a shared abstraction is
//! premature here, not merely asserted:
//!
//! - x86_64 ELF (Linux) and x86_64 COFF (Windows, via MinGW-w64 cross
//!   compilation) both place a zeroed 4-byte placeholder immediately
//!   after a `call` opcode -- similar only because both targets share
//!   the *same CPU instruction set* (x86_64), not because object formats
//!   converge. Relocation type names still differ
//!   (`R_X86_64_PLT32` vs. `IMAGE_REL_AMD64_REL32`) and so does whether
//!   an addend is explicit or implicit (unconfirmed for COFF -- see
//!   `ElfX86_64PendingReloc`'s own doc comment).
//! - ARM64 Mach-O (macOS on Apple Silicon) is not "ELF/COFF but
//!   different": AArch64's own `bl`/`b` instructions encode a 26-bit
//!   *field inside* a fixed 32-bit instruction word
//!   (`R_AARCH64_CALL26`/`R_AARCH64_JUMP26`), not a standalone
//!   byte-aligned 4-byte placeholder -- `ElfX86_64PendingReloc::width: usize`
//!   (a whole-byte-count rectangle) cannot represent this at all.
//!   Mach-O's own external-call convention additionally goes through a
//!   `__stubs`/`__la_symbol_ptr` indirection (PLT-style) rather than a
//!   direct placeholder in most cases, and modern macOS (12+) replaces
//!   lazy binding with Chained Fixups (`LC_DYLD_CHAINED_FIXUPS`): dyld
//!   walks per-segment pointer chains at process start and rewrites
//!   pointer *table entries*, not instruction bytes -- a structurally
//!   different repair mechanism, not a variant relocation formula.
//! - Universal/fat Mach-O binaries are not even Mach-O objects
//!   themselves -- Apple defines them as a thin archive format wrapping
//!   one complete, independent Mach-O per architecture (PowerPC, x86,
//!   x86_64, ARM64 across macOS's own multiple CPU transitions). There
//!   is no shared "multi-architecture relocation" to model; each
//!   architecture's own Mach-O is produced, resolved, and patched
//!   entirely independently, then archived together as a separate,
//!   later step outside this graph's own scope.
//!
//! The conclusion this crate now follows: **`ElfX86_64PendingReloc` and
//! `apply_elf_x86_64_relocations` are named, scoped, and documented as
//! ELF/x86_64 only** (not "the general case, ELF-flavored for now"). A Windows/COFF
//! or macOS/Mach-O-ARM64 target needs its *own* independently-designed
//! patching type and apply function, built from that platform's own
//! real constraints first -- not retrofitted into this one. Only the
//! *principle* this crate tests (a shared graph replaces a separate
//! object-file/link pass) is meant to generalize; no single Rust type
//! is.
//!
//! **The actual split unit, confirmed against rustc's own target list**
//! (`rustc --print target-list`, not assumed): 330 distinct triples, each
//! `<cpu-arch>-<vendor>-<os>-<abi/env>` (e.g. `aarch64-apple-darwin`,
//! `aarch64-unknown-linux-gnu`, `aarch64-pc-windows-msvc` -- the same
//! `aarch64` CPU architecture paired with three different OS/ABI
//! combinations), 83 distinct CPU-architecture prefixes. This is the
//! same axis this crate's own findings above independently converged on
//! from the object-format/relocation side (CPU instruction set decides
//! relocation *field* shape -- 32-bit-aligned vs. a 26-bit sub-field;
//! OS/ABI decides object *format* and repair mechanism -- ELF static
//! relocations vs. Mach-O Chained Fixups vs. COFF). Any future
//! `PendingReloc`-equivalent type for a new platform should be scoped to
//! one `(cpu-arch, os/abi)` pair, matching rustc's own real granularity,
//! not one dimension alone (an "ARM64 variant" or an "ELF variant" in
//! isolation would each still conflate two independent axes).
//!
//! ## Declared target scope (explicit, not "all 330")
//!
//! This project's own stated support scope is eight of rustc's 330
//! triples -- Linux (x86_64, ARM64), Windows (x86_64, both ABIs), macOS
//! (ARM64 only, no x86_64/Intel), and WASM (all three current variants):
//!
//! - `x86_64-unknown-linux-gnu` -- ELF, verified directly in this crate
//!   (`ElfX86_64PendingReloc`/`apply_elf_x86_64_relocations`).
//! - `aarch64-unknown-linux-gnu` -- ELF, but AArch64 relocation fields
//!   (`R_AARCH64_CALL26`/`JUMP26`), **not yet implemented** -- see this
//!   module's own ARM64 Mach-O finding above for why `width: usize` as a
//!   byte-count rectangle cannot represent a 26-bit sub-field; the same
//!   problem applies here, independent of ELF vs. Mach-O. Literature
//!   gathered (no development possible in this session -- no ARM64 Linux
//!   host or working ARM64 cross-assembler/objdump available here; this
//!   is reading, not measurement, unlike the ELF/x86_64 and COFF findings
//!   above):
//!   - **Primary spec**: ARM's own `abi-aa` repository, "ELF for the
//!     Arm 64-bit Architecture (AArch64)"
//!     (<https://github.com/ARM-software/abi-aa/blob/main/aaelf64/aaelf64.rst>).
//!     `R_AARCH64_CALL26` (`BL`) and `R_AARCH64_JUMP26` (`B`) both
//!     compute `S + A - P` (symbol address + addend - relocation site
//!     address -- the same shape as `ElfX86_64PendingReloc`'s own
//!     formula), then write bits `[27:2]` of that result into the
//!     instruction word's own bits `[27:2]` (bits `[31:28]` are the
//!     opcode, bits `[1:0]` are implicitly zero since AArch64
//!     instructions are 4-byte aligned) -- **not** a standalone
//!     byte-aligned rectangle the way `ElfX86_64PendingReloc::offset`/
//!     `width` assume; any `AArch64PendingReloc`-equivalent type needs a
//!     bit-range (instruction word + bit offset + bit width), not a
//!     byte range.
//!   - **Range limit**: valid only for `-2^27 <= X < 2^27` (26 bits of
//!     word-granular range = ±128 MiB) -- exceeding it is an overflow a
//!     conforming linker must diagnose, unlike `R_X86_64_PLT32`'s full
//!     32-bit range.
//!   - **Range-extension thunks**: LLVM lld review
//!     (<https://reviews.llvm.org/D70637>) describes what a real linker
//!     does when a `CALL26`/`JUMP26` target falls outside that ±128 MiB
//!     window -- it does not fail, it **synthesizes new code**: a small
//!     stub (e.g. `adrp x16, Dest; add x16, x16, #offset; br x16`, or an
//!     absolute-address `ldr x16, [addr]; br x16` form) placed in a
//!     linker-inserted "ThunkSection" within range of the original call
//!     site, and the original `bl`/`b` instruction's own relocation is
//!     retargeted to the thunk instead of the real destination. This is
//!     a materially different shape from `apply_elf_x86_64_relocations`
//!     today: that function only ever overwrites bytes already present
//!     in a `CodeBody`, never emits new code. An AArch64 implementation
//!     needs `apply_elf_x86_64_relocations`'s own layout/patch split to
//!     also support inserting a synthesized `CodeBody` when a target
//!     falls out of range -- a structural change, not just a new
//!     relocation-formula variant.
//!   - **Real hardware for future direct verification**: both Raspberry
//!     Pi 4 (Broadcom BCM2711, quad-core Cortex-A72, up to 1.5GHz) and
//!     Raspberry Pi 5 (Broadcom BCM2712, quad-core Cortex-A76, 2.4GHz)
//!     run in 64-bit (AArch64) mode under a 64-bit OS, making either a
//!     real, obtainable `aarch64-unknown-linux-gnu` host for the direct
//!     `objdump`-based verification this crate's other findings all
//!     depend on -- not yet done in this session, named here as the
//!     concrete next step rather than a cross-compiler-only guess.
//! - `x86_64-pc-windows-gnu` -- COFF via MinGW-w64, placeholder shape
//!   verified directly (`inspect-mingw-coff.sh`); the patch formula
//!   itself (`apply_*_relocations` equivalent) is **not yet
//!   implemented**, and the addend representation for
//!   `IMAGE_REL_AMD64_REL32` still needs PE/COFF spec confirmation (see
//!   `ElfX86_64PendingReloc`'s own doc comment).
//! - `x86_64-pc-windows-msvc` -- COFF via the MSVC ABI specifically;
//!   **not yet verified at all** -- this WSL2 environment's own Windows
//!   side was checked directly and has no `cl.exe`/`link.exe` installed,
//!   so no real `cl`-produced object has been inspected here. MinGW-w64's
//!   own COFF output is evidence for the *object format*, not for
//!   MSVC-specific ABI/name-mangling differences from the GNU-ABI COFF
//!   already measured.
//! - `aarch64-apple-darwin` -- Mach-O/ARM64, the platform this module's
//!   own Chained Fixups/`__stubs` findings above describe; **no real
//!   object has been compiled or inspected** (no macOS host or
//!   Mach-O-capable cross toolchain available in this session) --
//!   everything stated about it above is from Apple/LLVM documentation,
//!   not this crate's own direct measurement, unlike the ELF and COFF
//!   findings.
//! - `wasm32-unknown-unknown`, `wasm32-wasip1`, `wasm32-wasip2` -- WASM's
//!   own module/import/export model is structurally unlike ELF/Mach-O/COFF
//!   at the machine-code level, **confirmed directly** (unlike the
//!   Mach-O/ARM64 findings above, which are literature-only): compiled a
//!   real Rust `extern "C"` function importing a cross-module symbol to
//!   `wasm32-unknown-unknown` and inspected the actual binary
//!   (`wasm-tools dump`/`print`). A *linked* module has **no
//!   relocations left at all**: `call` (opcode `0x10`) is followed by a
//!   plain LEB128-encoded **function index** (an integer naming a slot in
//!   the module's own `import`/`func` index space, resolved by whichever
//!   host or further link step supplies `cadd_module`'s own `c_add`) --
//!   never a placeholder that needs a computed address written into it,
//!   because WASM has no flat address space for code to begin with.
//!
//!   The *unlinked* WASM object file this crate did not directly produce
//!   or inspect (`rustc`'s own `rust-lld -flavor wasm` step consumes
//!   these, per the real linker invocation this crate's own build
//!   surfaced) does carry something reloc-shaped, per the WebAssembly
//!   project's own specification
//!   (<https://github.com/WebAssembly/tool-conventions/blob/main/Linking.md>):
//!   `R_WASM_FUNCTION_INDEX_LEB` names an `(offset, symbol-table-index)`
//!   pair, always padded to a fixed 5 bytes ("All LEB128-encoded values
//!   that are to be relocated must be maximally padded so that they can
//!   be rewritten without affecting the position of any other bytes") --
//!   but the *value* written there is never computed (no `S + A - P`
//!   formula anywhere): it is simply the symbol's **new index** in the
//!   linked module's own combined index space, substituted for its old
//!   per-object index. This is a fourth, distinct relocation model
//!   (address-computation-free index substitution), not "ELF/Mach-O/COFF
//!   minus the address math" -- `ElfX86_64PendingReloc`'s own
//!   `offset`/`width`/`target`/`addend` shape assumes a computed value is
//!   always being written, which is never true for WASM. A real WASM
//!   equivalent needs its own type built around index substitution, not
//!   a variant of the existing one.
//!
//! x86_64/Intel macOS (`x86_64-apple-darwin`) and every other rustc
//! target are explicitly out of scope, not merely undone -- this
//! project does not intend to support them.
//!
//! ## Every real toolchain invocation this crate has measured: input and output
//!
//! Gathered because a command name (`cc`) is not evidence of what it is
//! *doing* -- the same `cc` binary is a real C compiler in one call and
//! nothing but a linker-invoking driver, never touching a `.c` file, in
//! another. Every row below was observed directly (`--print link-args`,
//! `--listCmd`, `-###`, `objdump`), never assumed from the command name.
//!
//! - **`rustc` (frontend + LLVM backend)**: input a `.rs` source file
//!   plus flags (`--edition`, `--crate-type`, `-C link-args`, `-l
//!   static=...` naming already-built external archives); output with
//!   `--emit=obj` a real ELF object (verified: same format `cc -c`
//!   produces). `rustc` never emits C source at any point -- LLVM is its
//!   only backend, feeding directly to codegen, never through a
//!   generated `.c` file the way Nim does.
//! - **`rustc`'s own final link step**: `rustc --print link-args`'s own
//!   output starts with `"cc"`, but the arguments that follow are never
//!   a `.c` file -- they are the `.o` files `rustc`'s own LLVM backend
//!   already produced (one per codegen unit, `*.rcgu.o`) plus `.rlib`
//!   archives (Rust's own static-library format, themselves containing
//!   already-compiled objects). **`cc` here compiles nothing**: it is
//!   invoked purely as a linker-invocation driver, passing its argument
//!   list through to `collect2` (verified via `cc -###`) and from there
//!   to the real linker (`ld`/`ld.lld`). The same command name, a wholly
//!   different role from the paragraph above.
//! - **`nim c` (Nim's C backend)**: input a `.nim` source file; output is
//!   **not** a single artifact -- Nim's own compiler transforms it into
//!   one or more real `.c` files (module-by-module, e.g.
//!   `system.nim.c`), then genuinely invokes `gcc -c <module>.nim.c -o
//!   <module>.nim.c.o` **once per generated `.c` file** (verified: `nim
//!   c --listCmd` showed five separate `gcc -c` invocations, one per
//!   `.nim.c` file, for a five-line "hello world"). Here `gcc` really is
//!   a C compiler, reading real C source it did not have a hand in
//!   writing until Nim generated it.
//! - **`nim c`'s own final link step**: after every `.nim.c.o` exists,
//!   Nim invokes `gcc` *again*, this time without `-c` and with every
//!   `.o` file from the previous step as input, producing the linked
//!   executable -- the same "compiler binary reused purely as a linker
//!   driver" role `rustc`'s own final step uses, confirmed by the
//!   absence of any `.c`/`.nim.c` file in this second invocation's own
//!   argument list.
//! - **`cc -c`/`c++ -c` on this project's own real C/C++ fixture
//!   sources** (`cadd.c`, `cppmax.cpp`): input the `.c`/`.cpp` file
//!   directly (no intermediate generation step -- these are the
//!   ecosystem's own real source, not compiler output); output a real
//!   ELF object, verified byte-for-byte with `objdump -d`/`-r`
//!   (`real_c_add_code`/`real_compute_code_and_relocs` in this crate's
//!   own tests are these exact bytes).
//! - **`x86_64-w64-mingw32-gcc -c`** (MinGW-w64 cross compiler): same
//!   role as plain `cc -c` above (a real C compiler, `.c` in, object
//!   out), but the object format is COFF (`pe-x86-64`), not ELF --
//!   confirmed by `objdump -f` reporting `file format pe-x86-64` on the
//!   identical source used for the ELF measurement.
//! - **`rustc --target wasm32-unknown-unknown`**: input a `.rs` file;
//!   output (with a full build, not `--emit=obj` alone) a linked `.wasm`
//!   module, produced via `rustc`'s own internal `rust-lld -flavor wasm`
//!   step (named directly in a real link failure's own error output --
//!   no external `cc`/linker driver is invoked at all for this target,
//!   unlike every ELF/COFF case above).
//!
//! **The pattern this crate's own measurements converge on**: every
//! frontend (`rustc`, `nim`, `cc`, `c++`) only ever transforms its own
//! source language into *either* a native object file *or* another
//! language's source file (Nim's `.c`/`.cpp`/`.m` generation) -- never
//! into a final linked executable by itself. The step that combines
//! independently-produced objects/archives into one executable is
//! always a separate invocation, sometimes of a *different* binary
//! (`ld`/`ld.lld` directly) and sometimes of the *same* compiler binary
//! reused purely as a driver for that separate linker (`cc`/`gcc` with
//! no `-c` and no source files in its argument list) -- the two roles
//! share a command name but never share what they actually read or
//! produce. This is the exact split this crate's own `SharedSymbolGraph`
//! is built to replace: not "the same pipeline, drawn differently," but
//! removing the object-file/link-step boundary these four toolchains all
//! independently reconstruct.
//!
//! ## The actual grain of an `rustc` -> LLVM call, verified from source
//!
//! Not one call per `.rs` file, and not one call per crate either.
//! Confirmed directly against `compiler/rustc_monomorphize/src/partitioning.rs`
//! (1398 lines, fetched from `rust-lang/rust`): `rustc` calls into LLVM
//! once per **codegen unit (CGU)**, and a CGU's own membership is decided
//! in two passes:
//!
//! 1. `place_mono_items` assigns each already-monomorphized item (a
//!    concrete instantiation of a function/static, generics already
//!    resolved to real types) to a CGU named after the **source-level
//!    `mod` it belongs to** (Rust's own logical module tree, independent
//!    of file boundaries -- one file can hold several `mod`s, one `mod`
//!    can span several files via `#[path]`/submodule files), split into
//!    exactly two CGUs per module: one for "stable" (non-generic) items,
//!    one for "volatile" (monomorphized-generic) items. The module's own
//!    doc comment states the reasoning directly: coarser (one CGU per
//!    module) would let one new generic reference anywhere invalidate
//!    the whole module's incremental cache; finer (one CGU per item)
//!    would prevent LLVM's own cross-module inlining entirely.
//! 2. `merge_codegen_units` then greedily merges CGUs down to
//!    `-C codegen-units=N` (16 by default, 256 for incremental builds),
//!    choosing merge pairs by **maximum inlined-item overlap** --
//!    explicitly to keep LLVM from duplicating inlined-item machine code
//!    across too many separate modules.
//!
//! **What this grain does and does not know about**: CGU membership is
//! decided entirely from this one crate's own module structure and
//! generic instantiation set -- it has no way to know, and does not try
//! to know, which of its own symbols a *different* crate (or Nimble/C/
//! C++ realm) will actually reference at link time. This is the same
//! "the party that knows the whole graph (Cargo) never touches
//! codegen, and the party doing codegen (rustc, once per crate) never
//! sees the whole graph" split this module's own `SharedSymbolGraph`
//! exists to remove -- CGU partitioning is a concrete instance of that
//! split happening *inside* a single `rustc` invocation, not just
//! between separate tool invocations.
//!
//! ## `assign_layout` must not stay a scheduling-blind ordering function
//!
//! Direct correction: `assign_layout` (below) currently assigns final
//! addresses by a single, fixed criterion -- sorted `SymbolId` order,
//! chosen only for determinism -- and has no notion of *when* each
//! symbol's own compilation finished or *how expensive* it was to
//! produce. Verified against two real, independent build-tool
//! implementations that this split (schedule now, lay out later, with
//! each step blind to the other) is exactly what both Cargo and lld
//! already do, and that treating them as one problem rather than two is
//! not a novel proposal but a known-hard combined-optimization class:
//!
//! - **Cargo's own build scheduler is not "whatever is ready, in
//!   discovery order"** -- confirmed directly against
//!   `src/util/dependency_queue.rs` (`rust-lang/cargo`, fetched and
//!   read this session). `DependencyQueue::queue_finished` computes, for
//!   every node, `cost[n] + sum(cost[d] for d in transitive dependents
//!   of n)` -- i.e. a node's priority is its own cost plus every
//!   downstream node's cost, a direct analog of "how much of the
//!   critical path depends on this finishing." `dequeue` always picks
//!   the ready node (zero unbuilt dependencies) with the **highest**
//!   such priority, confirmed by the crate's own `sort_by_highest_cost`
//!   test (a cost-4 leaf is dequeued before a cost-1 leaf that becomes
//!   ready at the same time). This is a real, working critical-path-
//!   aware scheduler, not the "just parallelize, whichever finishes
//!   first wins" assumption issue #59 originally set out to falsify.
//! - **lld's own final-layout optimizer is a separate, later, and
//!   blind pass** -- confirmed directly against `lld/ELF/Writer.cpp`
//!   and `lld/ELF/CallGraphSort.cpp` (`llvm/llvm-project`, fetched and
//!   read this session). `sortSection`/`buildSectionOrder` reorder
//!   input sections in the *already-linked* output using
//!   `--symbol-ordering-file` or a call-graph profile
//!   (`computeCallGraphProfileOrder`, implementing Call-Chain
//!   Clustering / Cache-Directed-Sort from real published research on
//!   data-center function placement). Its own module doc states the
//!   goal directly: "reduce i-TLB misses and i-cache misses" -- a
//!   **runtime** locality objective, fed by a **runtime execution
//!   profile**, with zero input from what Cargo's own scheduler knew
//!   about build-time cost or critical-path membership. Sections that
//!   were on the critical path to *build* and sections that are hot at
//!   *runtime* are, in this pipeline, two unrelated orderings computed
//!   from two disjoint data sources at two different times by two
//!   different tools.
//! - **Treating scheduling and placement as one problem, not two, is a
//!   recognized hard class, not a fresh idea**: joint task-
//!   scheduling-and-data-placement optimization (minimizing makespan
//!   *and* locality together, rather than sequentially) is established
//!   in the scheduling-theory literature as NP-hard -- the same
//!   makespan-vs-bin-packing independence issue #59 cites for its own
//!   "local optimization contradicts global optimization" hypothesis.
//!   Solving schedule and layout as two independent passes (as both
//!   Cargo and lld do today, each well-engineered on its own terms)
//!   is a real, named simplification of a provably harder joint
//!   problem, not an oversight unique to this repository's own G1.
//!
//! **What this implies for `assign_layout` specifically**: an address
//! assignment that only encodes "deterministic order" throws away two
//! real signals this graph's own `SymbolNode`/`CodeBody` already carry
//! or could carry -- (1) which symbols sat on the longest dependency
//! chain during resolution (this graph's own `mutation_seq` records
//! *when* each declaration arrived, a cheap proxy for build-time
//! critical-path position), and (2) which symbols reference each other
//! most (`RequiresEdge` already records every cross-realm call
//! relationship, the same shape a call-graph profile encodes). A
//! layout function that used both would be attempting the joint
//! problem directly, in one pass, instead of reproducing Cargo's
//! schedule-only pass followed by lld's placement-only pass. This is
//! not yet implemented -- flagged here as the concrete next design
//! question, not a design already answered by `assign_layout`'s current
//! sorted-`SymbolId` body.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// Issue #67 experiment harness (state-retention/durability/eviction
/// questions -- separate from this module's own linking-hypothesis
/// scope). See `durability`'s own module doc comment for what it does
/// and does not claim.
pub mod durability;

/// Issue #67 second-generation harness (pre-classification by `Realm`,
/// binary-heap eviction). See `durability_v2`'s own module doc comment.
pub mod durability_v2;

/// Issue #67 third harness: the actual memory-to-disk MemTable/SSTable
/// layer `durability.rs` explicitly left out of scope. See
/// `disk_tiering`'s own module doc comment.
pub mod disk_tiering;

/// Issue #67's last unaddressed verification question: whether the
/// degree `durability_v2`'s invalidation-cost reasoning uses as a proxy
/// actually correlates with real `rustc` compile time, measured against
/// two real fixture workspaces with contrasting degree distributions.
/// See `cost_correlation`'s own module doc comment.
pub mod cost_correlation;

/// The four ecosystems this hypothesis is scoped to (matching this repo's
/// own `cadd`/`app` fixture: Cargo, Nimble, C, C++). Not meant to be an
/// exhaustive or permanent list -- a placeholder for "whichever realms a
/// real cross-ecosystem build needs," kept closed here only to keep the
/// hypothesis concrete and testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Realm {
    Cargo,
    Nimble,
    C,
    Cpp,
}

/// A boundary symbol's identity: never ambiguous by name alone (two
/// realms could each declare `foo`), so identity is (realm, name) --
/// consistent with the real fixture's own `#[link(name = "cadd")]`-style
/// hints naming both a package and a symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolId {
    pub realm: Realm,
    pub name: String,
}

/// One place inside `code` whose bytes are a placeholder (all-zero, per
/// the real `cc -c` output verified directly: `objdump -d` on a function
/// calling two external symbols showed `e8 00 00 00 00` at each call
/// site -- a `call` opcode followed by a zeroed 4-byte operand) that must
/// be overwritten once `target`'s own final address is known. This is
/// the same information a real ELF `R_X86_64_PLT32`/`R_X86_64_PC32`
/// relocation record carries (`objdump -r`: offset, symbol, addend) --
/// kept here as one entry directly on the symbol's own `CodeBody` rather
/// than in a separate section/relocation-table indirection, since this
/// hypothesis has no sections to begin with.
///
/// **Platform scope, verified not assumed**: this shape (offset/width/
/// target/addend, `apply_elf_x86_64_relocations`'s own PC-relative formula) is
/// ELF/x86_64-specific -- confirmed by checking what `cc` actually is on
/// each of the four real ecosystems' target platforms. On Linux, `cc`
/// (gcc or clang) and Nim's `c`/`cpp` backends both emit ELF objects;
/// the same `cc` command name on macOS is Apple's Clang, which emits
/// **Mach-O**, a structurally different object format with its own
/// relocation type set and its own linker identity (`ld64`, not GNU
/// ld/lld) -- LLVM's own Mach-O port confirms this is a distinct linker
/// mode, not just a different flag to the same ELF logic
/// (<https://lld.llvm.org/MachO/index.html>). Nim's third native
/// backend, `nim objc`, generates Objective-C (`.m`) source specifically
/// because macOS's own toolchain expects it, and that source is in turn
/// compiled by the same platform Clang -- so it is the *same* platform
/// divergence as the C/C++ backends, not a fourth, separate case. The
/// principle this hypothesis tests (shared graph, no separate object-file/
/// link pass) is platform-independent; this concrete `ElfX86_64PendingReloc`
/// encoding is not, and a macOS/Mach-O (or Windows/COFF) port needs its
/// own relocation-shape verification against real `clang`/`cl.exe`
/// output before this crate's claims can be said to hold there.
///
/// **Windows/COFF, verified directly** (`x86_64-w64-mingw32-gcc -c` on
/// the exact same external-call C source used for `real_compute_code_and_relocs`,
/// inspected with the matching `objdump -f/-d/-r`): the physical
/// placeholder shape is identical to ELF (a `call` opcode followed by a
/// zeroed 4-byte operand, at the same kind of byte offset), but the
/// *relocation type* differs -- COFF names it `IMAGE_REL_AMD64_REL32`,
/// not `R_X86_64_PLT32`, and (unlike the ELF records, which printed an
/// explicit `-0x4` addend) `objdump -r` on the COFF object showed no
/// addend at all, suggesting `IMAGE_REL_AMD64_REL32`'s own -4 offset is
/// implicit in the relocation type itself rather than a caller-supplied
/// value -- this needs confirming against the PE/COFF spec before an
/// `apply_elf_x86_64_relocations` variant for this target is written, not assumed
/// from this one observation. Calling convention also differs (Windows
/// x64 passes the first two integer arguments in `edx`/`ecx`; the real
/// System V/ELF disassembly used `edi`/`esi`) but that is a
/// frontend-local concern (which registers a realm's own code generator
/// emits), not something `ElfX86_64PendingReloc`/`apply_elf_x86_64_relocations` need to
/// know about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfX86_64PendingReloc {
    /// Byte offset into `code` where the placeholder begins.
    pub offset: usize,
    /// How many bytes the placeholder occupies (4 for the 32-bit
    /// PC-relative call operand verified above).
    pub width: usize,
    /// The boundary symbol whose eventual address this placeholder must
    /// be computed from.
    pub target: SymbolId,
    /// Added to `target`'s resolved address before truncating to `width`
    /// bytes -- e.g. the real `-0x4` addend `objdump -r` reported,
    /// accounting for PC-relative addressing measuring from the
    /// instruction's own end, not its start.
    pub addend: i64,
}

/// What a realm's own frontend has produced for this symbol once its
/// local analysis is done -- not a bare "here is an address" claim, but
/// the actual machine code bytes plus every place inside them that still
/// needs another (possibly cross-realm) symbol's address plugged in.
/// This is "the compiler's output" this hypothesis asks for directly: no
/// object file, no section table, no separate symbol-table indirection
/// -- the code and its own outstanding cross-references travel together
/// as one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBody {
    pub code: Vec<u8>,
    pub relocations: Vec<ElfX86_64PendingReloc>,
}

/// Issue #67 problem F experiment: the minimal facts about a boundary
/// symbol that another realm's frontend can act on (register a
/// requirement against, check a signature) *before* that symbol's own
/// machine code exists -- directly modeling what Cargo's own pipelining
/// exposes via `rmeta` (a crate's public interface, available to
/// downstream crates before the crate's own full codegen finishes).
/// Deliberately minimal per the task instructions (not over-designed):
/// just the two pieces of information a real FFI declaration
/// (`extern "C" fn c_add(a: i32, b: i32) -> i32;`) already carries before
/// any code generation happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFacts {
    /// A human-readable stand-in for a real signature (this hypothesis
    /// has no type system of its own -- see this crate's own scope
    /// note); e.g. `"(i32, i32) -> i32"`.
    pub signature: String,
    /// Which other boundary symbols this symbol's own eventual code body
    /// is already known to call, discovered during semantic analysis
    /// (before codegen) -- the same information `ElfX86_64PendingReloc::target`
    /// records post-hoc, but available earlier here.
    pub depends_on: Vec<SymbolId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressState {
    /// Declared, but this realm's own frontend has not yet finished
    /// enough of its local analysis to assign a definite address.
    Unresolved,
    /// This realm's own frontend has finished **semantic analysis** for
    /// this symbol (signature known, dependency list known) but has not
    /// yet generated machine code -- directly modeling Cargo's own
    /// pipelining split (rmeta ready, full codegen still pending).
    /// Issue #67 problem F: does exposing this intermediate state let a
    /// *different* realm's `require_symbol` call proceed usefully before
    /// `Committed` exists? See `SharedSymbolGraph::require_symbol`'s own
    /// demand-driven promotion behavior below, and
    /// `demand_driven_promotion_from_analyzed_to_committed_on_require`
    /// in this module's own tests.
    Analyzed(SemanticFacts),
    /// This realm's own frontend has finished this symbol's own machine
    /// code (`CodeBody`), but that code may still contain unresolved
    /// `ElfX86_64PendingReloc` entries referencing other boundary symbols -- this
    /// state says "the bytes are ready," not "this symbol's own address
    /// is final," since final placement (where in the eventual image
    /// this code body lands) is a separate concern this hypothesis does
    /// not yet model (see this crate's own README on scope).
    Committed(CodeBody),
}

/// One boundary-crossing symbol node. Lives in the shared graph *only*
/// because some other realm might reference it -- a realm-internal symbol
/// never gets one of these.
#[derive(Debug, Clone)]
pub struct SymbolNode {
    pub id: SymbolId,
    pub address: AddressState,
}

/// A directed edge: `requiring_realm` needs `symbol`, and expects
/// `providing_realm` to supply it. Kept as data (not just an
/// `Option<Realm>` on the requirement) so more than one candidate
/// provider can be recorded before resolution -- matching this repo's own
/// real "provider@version" selection problem, not assuming resolution is
/// trivial.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequiresEdge {
    pub requiring_realm: Realm,
    pub symbol: SymbolId,
}

/// The shared, concurrently-writable graph. Realm-local semantic state
/// (types, ASTs, monomorphization) deliberately does NOT live here -- see
/// this module's own doc comment. `RwLock` per map (not one lock over the
/// whole graph) so a write from one realm's frontend does not block reads
/// or writes concerning a different realm's own boundary symbols, as long
/// as they don't touch the same key -- a real, if coarse-grained,
/// approximation of "resolution proceeds as an independent parallel pass"
/// without requiring a lock-free concurrent hash map (the real gap ld.lld
/// itself named -- see this crate's own README).
pub struct SharedSymbolGraph {
    pub(crate) nodes: RwLock<HashMap<SymbolId, SymbolNode>>,
    /// requirement -> the provider realms declared as candidates, in
    /// declaration order (first-registered is not "wins" here -- that is
    /// exactly the GNU-ld-style order dependency this hypothesis exists
    /// to avoid; order is retained only as evidence, never as a tie-break
    /// rule).
    pub(crate) edges: RwLock<HashMap<RequiresEdge, Vec<Realm>>>,
    /// Monotonic counter so every mutation this graph accepts can be
    /// ordered for later inspection/debugging without relying on
    /// wall-clock time (which is not comparable across threads reliably
    /// enough for this purpose).
    mutation_seq: AtomicU64,
    /// Issue #67 problem F: a realm's own frontend registers *how* to
    /// finish codegen for a symbol it has only analyzed so far (still in
    /// `AddressState::Analyzed`), without being forced to do that
    /// (possibly expensive) work up front. `require_symbol` calls
    /// `promote_analyzed_to_committed_if_needed` below, which looks this
    /// map up and runs it *only* on actual demand -- mirroring Cargo's
    /// own pipelining, where a downstream crate consuming rmeta does not
    /// itself force the upstream crate's full codegen; only when
    /// something needs the finished object does codegen become
    /// necessary. Kept as a boxed closure (not a trait object registry)
    /// since this hypothesis has exactly one use of it and does not need
    /// a plugin system.
    #[allow(clippy::type_complexity)]
    pending_codegen: RwLock<HashMap<SymbolId, Box<dyn Fn(&SemanticFacts) -> CodeBody + Send + Sync>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// A realm attempted to declare a boundary symbol under a different
    /// realm's own name -- e.g. Rust code trying to register a node as
    /// `Realm::C`. Never silently reassigned; this is a caller bug.
    RealmMismatch {
        declared_by: Realm,
        node_realm: Realm,
    },
}

impl Default for SharedSymbolGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSymbolGraph {
    pub fn new() -> Self {
        SharedSymbolGraph {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(HashMap::new()),
            mutation_seq: AtomicU64::new(0),
            pending_codegen: RwLock::new(HashMap::new()),
        }
    }

    /// Issue #67 problem F: declare a symbol as `Analyzed` (semantic
    /// facts known, no machine code yet), and register the closure that
    /// will produce its real `CodeBody` *if and when* something actually
    /// requires it. Never runs `finish_codegen` itself -- that only
    /// happens inside `require_symbol`'s own demand-driven promotion, or
    /// via `force_commit` for a caller that wants eager codegen anyway
    /// (kept for backward compatibility: existing callers that always
    /// went straight to `Committed` are unaffected by this addition).
    pub fn declare_analyzed_symbol(
        &self,
        declaring_realm: Realm,
        id: SymbolId,
        facts: SemanticFacts,
        finish_codegen: impl Fn(&SemanticFacts) -> CodeBody + Send + Sync + 'static,
    ) -> Result<(), RegisterError> {
        if id.realm != declaring_realm {
            return Err(RegisterError::RealmMismatch {
                declared_by: declaring_realm,
                node_realm: id.realm,
            });
        }
        self.mutation_seq.fetch_add(1, Ordering::Relaxed);
        {
            let mut nodes = self.nodes.write().expect("nodes lock poisoned");
            nodes.insert(
                id.clone(),
                SymbolNode {
                    id: id.clone(),
                    address: AddressState::Analyzed(facts),
                },
            );
        }
        let mut pending = self
            .pending_codegen
            .write()
            .expect("pending_codegen lock poisoned");
        pending.insert(id, Box::new(finish_codegen));
        Ok(())
    }

    /// If `id` is currently `Analyzed` and has a registered codegen
    /// closure, runs it now and promotes the node to `Committed` in
    /// place -- the actual demand-driven transition. A no-op (not an
    /// error) if `id` is not `Analyzed`, has no registered closure, or
    /// does not exist yet at all: those are all legitimate states this
    /// hypothesis already tolerates elsewhere (e.g. `resolve_all`
    /// already treats "not found" as `None`, never an error).
    fn promote_analyzed_to_committed_if_needed(&self, id: &SymbolId) {
        let facts = {
            let nodes = self.nodes.read().expect("nodes lock poisoned");
            match nodes.get(id).map(|n| &n.address) {
                Some(AddressState::Analyzed(facts)) => facts.clone(),
                _ => return,
            }
        };
        let codegen = {
            let mut pending = self
                .pending_codegen
                .write()
                .expect("pending_codegen lock poisoned");
            pending.remove(id)
        };
        let Some(codegen) = codegen else { return };
        let body = codegen(&facts);
        self.mutation_seq.fetch_add(1, Ordering::Relaxed);
        let mut nodes = self.nodes.write().expect("nodes lock poisoned");
        // Re-check: another thread could have raced this promotion
        // between the read above and this write (declare_symbol could
        // also have overwritten the node entirely in the meantime).
        // Only commit if it is still exactly the Analyzed state we
        // promoted from -- never clobber a concurrent, possibly newer
        // write.
        if let Some(node) = nodes.get_mut(id) {
            if matches!(node.address, AddressState::Analyzed(_)) {
                node.address = AddressState::Committed(body);
            }
        }
    }

    /// Eagerly force codegen for an `Analyzed` symbol, without waiting
    /// for a `require_symbol` call to demand it -- useful for a caller
    /// (or a test) that wants to confirm codegen behavior independent of
    /// the demand-driven path `require_symbol` exercises.
    pub fn force_commit(&self, id: &SymbolId) {
        self.promote_analyzed_to_committed_if_needed(id);
    }

    /// A realm's own frontend declares that it owns (will eventually
    /// provide) this boundary symbol. Callable concurrently by every
    /// realm's frontend thread; only the `nodes` map is locked, and only
    /// for the duration of one insert.
    pub fn declare_symbol(
        &self,
        declaring_realm: Realm,
        node: SymbolNode,
    ) -> Result<(), RegisterError> {
        if node.id.realm != declaring_realm {
            return Err(RegisterError::RealmMismatch {
                declared_by: declaring_realm,
                node_realm: node.id.realm,
            });
        }
        self.mutation_seq.fetch_add(1, Ordering::Relaxed);
        let mut nodes = self.nodes.write().expect("nodes lock poisoned");
        nodes.insert(node.id.clone(), node);
        Ok(())
    }

    /// A realm's own frontend records that it needs `symbol`, believing
    /// `expected_provider` to be the realm that supplies it. Never blocks
    /// on `symbol` actually existing yet -- registering the requirement
    /// and satisfying it are independent events, exactly the property
    /// this hypothesis needs (a Rust frontend thread can register its
    /// `extern "C" { fn c_add(...) }` requirement before the C frontend
    /// thread has finished analyzing `cadd.c`, with no ordering
    /// constraint between the two).
    pub fn require_symbol(
        &self,
        requiring_realm: Realm,
        symbol: SymbolId,
        expected_provider: Realm,
    ) {
        self.mutation_seq.fetch_add(1, Ordering::Relaxed);
        // Issue #67 problem F: a requirement is exactly the "something
        // now needs this symbol's real code" event that should trigger
        // an Analyzed -> Committed promotion, if one is pending -- the
        // demand-driven step Cargo's own pipelining models as "a
        // downstream crate's full build finally needs the upstream
        // crate's real object, not just its rmeta." No-op if `symbol`
        // is not currently Analyzed (already Committed, still
        // Unresolved, or not declared at all yet).
        self.promote_analyzed_to_committed_if_needed(&symbol);
        let edge = RequiresEdge {
            requiring_realm,
            symbol,
        };
        let mut edges = self.edges.write().expect("edges lock poisoned");
        let candidates = edges.entry(edge).or_default();
        if !candidates.contains(&expected_provider) {
            candidates.push(expected_provider);
        }
    }

    /// Total, read-only snapshot: every requirement this graph has seen,
    /// paired with whichever `SymbolNode` (if any) its `expected_provider`
    /// candidates actually declared. `None` means genuinely unresolved --
    /// not a fabricated placeholder. This is the *only* function that
    /// crosses realm boundaries to actually compute a match; declaration
    /// and requirement registration never do.
    pub fn resolve_all(&self) -> Vec<ResolvedRequirement> {
        let edges = self.edges.read().expect("edges lock poisoned");
        let nodes = self.nodes.read().expect("nodes lock poisoned");
        edges
            .iter()
            .map(|(edge, candidate_realms)| {
                let provider = candidate_realms.iter().find_map(|realm| {
                    let candidate_id = SymbolId {
                        realm: *realm,
                        name: edge.symbol.name.clone(),
                    };
                    nodes.get(&candidate_id).cloned()
                });
                ResolvedRequirement {
                    requirement: edge.clone(),
                    candidate_realms: candidate_realms.clone(),
                    provider,
                }
            })
            .collect()
    }

    /// Evidence-only counter: how many declare/require calls this graph
    /// has accepted, for a caller to confirm concurrent writers actually
    /// interleaved rather than serializing by accident (e.g. behind one
    /// giant lock this crate's own design claims to avoid).
    pub fn mutation_count(&self) -> u64 {
        self.mutation_seq.load(Ordering::Relaxed)
    }

    /// The step that replaces a traditional linker's "layout" phase:
    /// walks every declared `SymbolNode` in a deterministic order
    /// (sorted by `SymbolId`, never HashMap iteration order, so this is
    /// reproducible across runs) and assigns each one a final byte
    /// offset within one conceptual, contiguous image -- laid out back
    /// to back by each `CodeBody`'s own length, no alignment/section
    /// separation modeled yet (see this crate's own scope note). Returns
    /// the assignment as its own map rather than mutating `self` so a
    /// caller can inspect a proposed layout before committing to it.
    pub fn assign_layout(&self) -> LayoutAssignment {
        let nodes = self.nodes.read().expect("nodes lock poisoned");
        let mut entries: Vec<(&SymbolId, &SymbolNode)> = nodes.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));

        let mut addresses = HashMap::new();
        let mut cursor: u64 = 0;
        for (id, node) in entries {
            let AddressState::Committed(body) = &node.address else {
                continue;
            };
            addresses.insert(id.clone(), cursor);
            cursor += body.code.len() as u64;
        }
        LayoutAssignment { addresses }
    }

    /// The step that replaces a traditional linker's "apply relocations"
    /// phase: for every declared symbol's own `CodeBody`, overwrite each
    /// `ElfX86_64PendingReloc`'s placeholder bytes with `layout`'s resolved
    /// address for that relocation's `target`, computed the same way the
    /// real `R_X86_64_PLT32` records verified against `cadd`/`app`-shaped
    /// code do (`target_address + addend - reloc_site_address`, i.e.
    /// PC-relative from the *end* of the 4-byte operand). Returns an
    /// error naming the first symbol/relocation that could not be
    /// resolved rather than silently leaving a placeholder unpatched --
    /// a patched-looking binary with a live zero placeholder is a
    /// miscompile, never an acceptable partial result.
    pub fn apply_elf_x86_64_relocations(
        &self,
        layout: &LayoutAssignment,
    ) -> Result<PatchedImage, LinkError> {
        let nodes = self.nodes.read().expect("nodes lock poisoned");
        let mut entries: Vec<(&SymbolId, &SymbolNode)> = nodes.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));

        let mut patched = HashMap::new();
        for (id, node) in entries {
            let AddressState::Committed(body) = &node.address else {
                continue;
            };
            let site_base = *layout
                .addresses
                .get(id)
                .ok_or_else(|| LinkError::MissingLayoutEntry(id.clone()))?;
            let mut bytes = body.code.clone();
            for reloc in &body.relocations {
                let target_address = *layout.addresses.get(&reloc.target).ok_or_else(|| {
                    LinkError::UnresolvedRelocationTarget {
                        in_symbol: id.clone(),
                        target: reloc.target.clone(),
                    }
                })?;
                let reloc_site_address = site_base + reloc.offset as u64 + reloc.width as u64;
                let value = (target_address as i64 + reloc.addend) - reloc_site_address as i64;
                let value_bytes = (value as i32).to_le_bytes();
                if reloc.width != value_bytes.len() {
                    return Err(LinkError::UnsupportedRelocationWidth {
                        in_symbol: id.clone(),
                        width: reloc.width,
                    });
                }
                let start = reloc.offset;
                let end = start + reloc.width;
                if end > bytes.len() {
                    return Err(LinkError::RelocationOutOfBounds {
                        in_symbol: id.clone(),
                        offset: reloc.offset,
                    });
                }
                bytes[start..end].copy_from_slice(&value_bytes);
            }
            patched.insert(id.clone(), bytes);
        }
        Ok(PatchedImage { code: patched })
    }
}

#[derive(Debug, Clone, Default)]
pub struct LayoutAssignment {
    pub addresses: HashMap<SymbolId, u64>,
}

#[derive(Debug, Clone)]
pub struct PatchedImage {
    /// Each symbol's own code, with every `ElfX86_64PendingReloc` placeholder
    /// already overwritten by its resolved value. Never contains an
    /// un-patched all-zero placeholder for a relocation `apply_elf_x86_64_relocations`
    /// itself reported success for.
    pub code: HashMap<SymbolId, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// A symbol was declared but `assign_layout` never placed it --
    /// should be unreachable given `assign_layout`'s own logic, kept as
    /// an explicit error rather than a panic so a caller mixing
    /// layouts/graphs gets a structured failure instead of an index
    /// panic.
    MissingLayoutEntry(SymbolId),
    /// A relocation names a target this graph never saw a `Committed`
    /// declaration for -- the real "undefined reference" a linker
    /// reports, surfaced with which symbol's own code contains the
    /// unresolved reference.
    UnresolvedRelocationTarget {
        in_symbol: SymbolId,
        target: SymbolId,
    },
    /// This hypothesis only implements the 4-byte PC32/PLT32-shaped
    /// relocation verified against real `cc -c` output; any other width
    /// is refused rather than silently truncated/extended.
    UnsupportedRelocationWidth { in_symbol: SymbolId, width: usize },
    /// A `ElfX86_64PendingReloc`'s offset+width falls outside its own `CodeBody`'s
    /// actual byte length -- a producer bug, never patched around.
    RelocationOutOfBounds { in_symbol: SymbolId, offset: usize },
}

#[derive(Debug, Clone)]
pub struct ResolvedRequirement {
    pub requirement: RequiresEdge,
    pub candidate_realms: Vec<Realm>,
    pub provider: Option<SymbolNode>,
}

impl ResolvedRequirement {
    pub fn is_resolved(&self) -> bool {
        self.provider.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// The real `c_add` machine code, byte-for-byte, from `objdump -d`
    /// against an actual `cc -c` of this repo's own
    /// `fixtures/cross-ecosystem-native-executable/c/cadd/v1/cadd.c`
    /// (verified inside `laminaria-bootstrap`, see this crate's own
    /// `inspect-cadd.sh`). No external references -- `objdump -r` showed
    /// zero relocations in `.text` for this function.
    fn real_c_add_code() -> Vec<u8> {
        vec![
            0x55, 0x48, 0x89, 0xe5, 0x89, 0x7d, 0xfc, 0x89, 0x75, 0xf8, 0x8b, 0x55, 0xfc, 0x8b,
            0x45, 0xf8, 0x01, 0xd0, 0x5d, 0xc3,
        ]
    }

    /// The real `compute` machine code (`c_add(cpp_max_i32(3, 4), 1)`),
    /// byte-for-byte from an actual `cc -c`, with its two real
    /// `R_X86_64_PLT32` relocations reproduced exactly as `objdump -r`
    /// reported them (offset 0xf -> cpp_max_i32-0x4, offset 0x1b ->
    /// c_add-0x4). See `inspect-caller.sh`.
    fn real_compute_code_and_relocs() -> (Vec<u8>, Vec<ElfX86_64PendingReloc>) {
        let code = vec![
            0x55, 0x48, 0x89, 0xe5, 0xbe, 0x04, 0x00, 0x00, 0x00, 0xbf, 0x03, 0x00, 0x00, 0x00,
            0xe8, 0x00, 0x00, 0x00, 0x00, 0xbe, 0x01, 0x00, 0x00, 0x00, 0x89, 0xc7, 0xe8, 0x00,
            0x00, 0x00, 0x00, 0x5d, 0xc3,
        ];
        let relocs = vec![
            ElfX86_64PendingReloc {
                offset: 0xf,
                width: 4,
                target: SymbolId {
                    realm: Realm::Cpp,
                    name: "cpp_max_i32".to_string(),
                },
                addend: -4,
            },
            ElfX86_64PendingReloc {
                offset: 0x1b,
                width: 4,
                target: SymbolId {
                    realm: Realm::C,
                    name: "c_add".to_string(),
                },
                addend: -4,
            },
        ];
        (code, relocs)
    }

    /// The real fixture's own boundary shape (`cadd`/`app`), reproduced
    /// here as four independent threads -- one per realm -- so the test
    /// actually exercises concurrent writers, not just sequential calls
    /// that happen to compile against a thread-safe type. Each thread
    /// only ever touches its own realm's declarations/requirements; the
    /// graph itself is the only shared state.
    #[test]
    fn four_realms_declare_and_require_concurrently_and_every_boundary_symbol_resolves() {
        let graph = Arc::new(SharedSymbolGraph::new());

        let handles: Vec<thread::JoinHandle<()>> = vec![
            {
                let graph = Arc::clone(&graph);
                thread::spawn(move || {
                    // app (Cargo/Rust): requires c_add (C), cpp_max_i32
                    // (C++), nim_double (Nimble) -- never declares a
                    // boundary symbol of its own in this fixture shape.
                    graph.require_symbol(
                        Realm::Cargo,
                        SymbolId {
                            realm: Realm::C,
                            name: "c_add".to_string(),
                        },
                        Realm::C,
                    );
                    graph.require_symbol(
                        Realm::Cargo,
                        SymbolId {
                            realm: Realm::Cpp,
                            name: "cpp_max_i32".to_string(),
                        },
                        Realm::Cpp,
                    );
                    graph.require_symbol(
                        Realm::Cargo,
                        SymbolId {
                            realm: Realm::Nimble,
                            name: "nim_double".to_string(),
                        },
                        Realm::Nimble,
                    );
                })
            },
            {
                let graph = Arc::clone(&graph);
                thread::spawn(move || {
                    graph
                        .declare_symbol(
                            Realm::C,
                            SymbolNode {
                                id: SymbolId {
                                    realm: Realm::C,
                                    name: "c_add".to_string(),
                                },
                                address: AddressState::Committed(CodeBody {
                                    code: real_c_add_code(),
                                    relocations: vec![],
                                }),
                            },
                        )
                        .expect("C realm may declare its own symbol");
                })
            },
            {
                let graph = Arc::clone(&graph);
                thread::spawn(move || {
                    graph
                        .declare_symbol(
                            Realm::Cpp,
                            SymbolNode {
                                id: SymbolId {
                                    realm: Realm::Cpp,
                                    name: "cpp_max_i32".to_string(),
                                },
                                address: AddressState::Committed(CodeBody {
                                    code: vec![0xb8, 0x04, 0x00, 0x00, 0x00, 0xc3], // mov eax,4; ret
                                    relocations: vec![],
                                }),
                            },
                        )
                        .expect("C++ realm may declare its own symbol");
                })
            },
            {
                let graph = Arc::clone(&graph);
                thread::spawn(move || {
                    graph
                        .declare_symbol(
                            Realm::Nimble,
                            SymbolNode {
                                id: SymbolId {
                                    realm: Realm::Nimble,
                                    name: "nim_double".to_string(),
                                },
                                address: AddressState::Committed(CodeBody {
                                    code: vec![0xb8, 0x08, 0x00, 0x00, 0x00, 0xc3], // mov eax,8; ret
                                    relocations: vec![],
                                }),
                            },
                        )
                        .expect("Nimble realm may declare its own symbol");
                })
            },
        ];

        for handle in handles {
            handle.join().expect("realm thread must not panic");
        }

        // No separate "link" pass: resolve_all() is the only step after
        // the four realm threads finish, and it only ever reads what
        // they already wrote -- it performs no analysis of its own.
        let resolved = graph.resolve_all();
        assert_eq!(resolved.len(), 3, "app's three real FFI requirements");
        for r in &resolved {
            assert!(
                r.is_resolved(),
                "requirement {:?} must resolve against the real concurrent declarations, got {:?}",
                r.requirement,
                r
            );
        }

        // At least declare_symbol x3 + require_symbol x3 = 6 mutations;
        // confirms the graph actually accepted writes from all four
        // threads, not just the last one to run.
        assert!(graph.mutation_count() >= 6);
    }

    /// A requirement whose declared provider never actually registers a
    /// matching symbol must resolve to `None` -- never silently invent a
    /// resolution. This is the same "conservative retention" discipline
    /// `cross-layer-reachability-pruning_ja.md` requires of any pruning
    /// scheme: an unresolved requirement is observable as unresolved, not
    /// hidden.
    #[test]
    fn an_unsatisfied_requirement_resolves_to_none_not_a_fabricated_match() {
        let graph = SharedSymbolGraph::new();
        graph.require_symbol(
            Realm::Cargo,
            SymbolId {
                realm: Realm::C,
                name: "never_declared".to_string(),
            },
            Realm::C,
        );

        let resolved = graph.resolve_all();
        assert_eq!(resolved.len(), 1);
        assert!(!resolved[0].is_resolved());
        assert!(resolved[0].provider.is_none());
    }

    /// A realm attempting to declare a symbol under a different realm's
    /// name is a structural error, never silently reassigned to the
    /// caller's own realm -- this is the boundary discipline that keeps
    /// "which realm owns this symbol" unambiguous even under concurrent
    /// writes.
    #[test]
    fn declaring_a_symbol_under_the_wrong_realm_is_refused() {
        let graph = SharedSymbolGraph::new();
        let result = graph.declare_symbol(
            Realm::Cargo,
            SymbolNode {
                id: SymbolId {
                    realm: Realm::C,
                    name: "c_add".to_string(),
                },
                address: AddressState::Committed(CodeBody {
                    code: real_c_add_code(),
                    relocations: vec![],
                }),
            },
        );
        assert_eq!(
            result,
            Err(RegisterError::RealmMismatch {
                declared_by: Realm::Cargo,
                node_realm: Realm::C,
            })
        );
    }

    /// The real end-to-end case this crate's whole hypothesis exists to
    /// prove: `app`'s `compute` function -- with its two real, unresolved
    /// `call` placeholders -- gets its final machine code produced by
    /// `assign_layout` + `apply_elf_x86_64_relocations` alone, no object file and no
    /// separate linker invocation. Every provider symbol's real machine
    /// code (`c_add`, plus stand-in bodies for `cpp_max_i32`/
    /// `nim_double`) is declared first; `compute` (owned by `Cargo`,
    /// since `app` is the one requiring the calls) is declared with its
    /// real relocations, and patching must produce PC-relative operand
    /// bytes that satisfy the exact formula a real ELF loader/linker
    /// would use.
    #[test]
    fn apply_elf_x86_64_relocations_patches_real_call_placeholders_to_the_correct_pc_relative_values(
    ) {
        let graph = SharedSymbolGraph::new();
        graph
            .declare_symbol(
                Realm::C,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::C,
                        name: "c_add".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: real_c_add_code(),
                        relocations: vec![],
                    }),
                },
            )
            .unwrap();
        graph
            .declare_symbol(
                Realm::Cpp,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::Cpp,
                        name: "cpp_max_i32".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: vec![0xb8, 0x04, 0x00, 0x00, 0x00, 0xc3],
                        relocations: vec![],
                    }),
                },
            )
            .unwrap();
        let (compute_code, compute_relocs) = real_compute_code_and_relocs();
        graph
            .declare_symbol(
                Realm::Cargo,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::Cargo,
                        name: "compute".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: compute_code,
                        relocations: compute_relocs,
                    }),
                },
            )
            .unwrap();

        let layout = graph.assign_layout();
        let patched = graph
            .apply_elf_x86_64_relocations(&layout)
            .expect("every relocation target was declared");

        let compute_id = SymbolId {
            realm: Realm::Cargo,
            name: "compute".to_string(),
        };
        let patched_compute = &patched.code[&compute_id];

        // Manually recompute what the real linker formula must produce,
        // independent of apply_elf_x86_64_relocations's own implementation, so this
        // assertion cannot pass merely by mirroring a bug.
        let compute_addr = layout.addresses[&compute_id];
        let cpp_addr = layout.addresses[&SymbolId {
            realm: Realm::Cpp,
            name: "cpp_max_i32".to_string(),
        }];
        let c_add_addr = layout.addresses[&SymbolId {
            realm: Realm::C,
            name: "c_add".to_string(),
        }];

        let expected_cpp_operand = ((cpp_addr as i64 - 4) - (compute_addr as i64 + 0xf + 4)) as i32;
        let expected_c_add_operand =
            ((c_add_addr as i64 - 4) - (compute_addr as i64 + 0x1b + 4)) as i32;

        assert_eq!(
            &patched_compute[0xf..0xf + 4],
            expected_cpp_operand.to_le_bytes().as_slice(),
            "cpp_max_i32 call operand must be the real PC-relative displacement"
        );
        assert_eq!(
            &patched_compute[0x1b..0x1b + 4],
            expected_c_add_operand.to_le_bytes().as_slice(),
            "c_add call operand must be the real PC-relative displacement"
        );
        // Never a leftover placeholder: both slots must differ from the
        // real cc-emitted zero placeholder unless the true displacement
        // genuinely happens to be zero (not the case for this layout).
        assert_ne!(&patched_compute[0xf..0xf + 4], &[0, 0, 0, 0]);
        assert_ne!(&patched_compute[0x1b..0x1b + 4], &[0, 0, 0, 0]);
    }

    /// A relocation naming a target this graph never saw declared must
    /// fail loudly -- the real "undefined reference" case -- never
    /// silently leave the placeholder's all-zero bytes in place, which
    /// would look like a successfully patched (but wrong) binary.
    #[test]
    fn apply_elf_x86_64_relocations_refuses_to_silently_leave_an_undefined_reference_unpatched() {
        let graph = SharedSymbolGraph::new();
        let (compute_code, compute_relocs) = real_compute_code_and_relocs();
        graph
            .declare_symbol(
                Realm::Cargo,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::Cargo,
                        name: "compute".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: compute_code,
                        relocations: compute_relocs,
                    }),
                },
            )
            .unwrap();
        // cpp_max_i32 and c_add are deliberately never declared.

        let layout = graph.assign_layout();
        let result = graph.apply_elf_x86_64_relocations(&layout);
        assert!(matches!(
            result,
            Err(LinkError::UnresolvedRelocationTarget { .. })
        ));
    }

    /// Issue #67 problem F, the core experiment: a symbol declared only
    /// as `Analyzed` (facts known, no machine code) must stay in that
    /// state -- codegen must NOT run -- until something actually calls
    /// `require_symbol` on it. This directly tests the demand-driven
    /// claim, not just that the enum variant compiles: a shared counter
    /// records whether the registered codegen closure has run, and the
    /// test asserts it is still zero right after `declare_analyzed_symbol`,
    /// then exactly one after the first `require_symbol` call, and stays
    /// at one (not re-run) after a second `require_symbol` call on the
    /// same symbol -- matching Cargo's own pipelining, where a second
    /// downstream consumer of an already-built crate does not trigger a
    /// second build.
    #[test]
    fn demand_driven_promotion_from_analyzed_to_committed_on_require() {
        use std::sync::atomic::{AtomicUsize, Ordering as AOrdering};
        use std::sync::Arc;

        let graph = SharedSymbolGraph::new();
        let codegen_run_count = Arc::new(AtomicUsize::new(0));
        let counter_for_closure = Arc::clone(&codegen_run_count);

        let c_add_id = SymbolId {
            realm: Realm::C,
            name: "c_add".to_string(),
        };

        graph
            .declare_analyzed_symbol(
                Realm::C,
                c_add_id.clone(),
                SemanticFacts {
                    signature: "(i32, i32) -> i32".to_string(),
                    depends_on: vec![],
                },
                move |_facts| {
                    counter_for_closure.fetch_add(1, AOrdering::SeqCst);
                    CodeBody {
                        code: real_c_add_code(),
                        relocations: vec![],
                    }
                },
            )
            .unwrap();

        // Right after declaration: still Analyzed, codegen has not run.
        {
            let nodes = graph.nodes.read().unwrap();
            let node = nodes.get(&c_add_id).expect("declared node must exist");
            assert!(
                matches!(node.address, AddressState::Analyzed(_)),
                "symbol must remain Analyzed until something requires it, got {:?}",
                node.address
            );
        }
        assert_eq!(
            codegen_run_count.load(AOrdering::SeqCst),
            0,
            "codegen closure must not run merely from declare_analyzed_symbol"
        );

        // First require_symbol call: this is the demand that must
        // trigger promotion.
        graph.require_symbol(Realm::Cargo, c_add_id.clone(), Realm::C);
        {
            let nodes = graph.nodes.read().unwrap();
            let node = nodes.get(&c_add_id).expect("node must still exist");
            assert!(
                matches!(node.address, AddressState::Committed(_)),
                "symbol must be Committed immediately after the first require_symbol call, got {:?}",
                node.address
            );
            if let AddressState::Committed(body) = &node.address {
                assert_eq!(
                    body.code,
                    real_c_add_code(),
                    "promoted CodeBody must be exactly what the registered codegen closure produced"
                );
            }
        }
        assert_eq!(
            codegen_run_count.load(AOrdering::SeqCst),
            1,
            "codegen closure must run exactly once after the first require_symbol call"
        );

        // Second require_symbol call on the same, already-Committed
        // symbol: must NOT re-run codegen (already committed, nothing
        // left to promote) -- this is the "no duplicate work for a
        // second consumer" property pipelining is supposed to give.
        graph.require_symbol(Realm::Nimble, c_add_id.clone(), Realm::C);
        assert_eq!(
            codegen_run_count.load(AOrdering::SeqCst),
            1,
            "a second require_symbol call on an already-Committed symbol must not re-run codegen"
        );

        // End-to-end: resolve_all must see this symbol as resolved,
        // exactly as if it had been declared Committed directly from the
        // start -- confirms this new path does not require any special
        // handling from resolve_all/assign_layout/apply_elf_x86_64_relocations.
        graph.require_symbol(
            Realm::Cargo,
            SymbolId {
                realm: Realm::C,
                name: "c_add".to_string(),
            },
            Realm::C,
        );
        let resolved = graph.resolve_all();
        let c_add_resolution = resolved
            .iter()
            .find(|r| r.requirement.symbol == c_add_id)
            .expect("a requirement on c_add must exist");
        assert!(
            c_add_resolution.is_resolved(),
            "c_add must resolve once promoted to Committed via demand-driven require_symbol"
        );
    }

    /// Backward compatibility: existing callers that go straight to
    /// `AddressState::Committed(..)` via `declare_symbol` (never touching
    /// `declare_analyzed_symbol`/`Analyzed` at all) must keep working
    /// exactly as before -- `require_symbol`'s new
    /// `promote_analyzed_to_committed_if_needed` call must be a true
    /// no-op for a symbol that was never `Analyzed` in the first place.
    #[test]
    fn require_symbol_is_a_no_op_promotion_for_symbols_that_go_straight_to_committed() {
        let graph = SharedSymbolGraph::new();
        graph
            .declare_symbol(
                Realm::C,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::C,
                        name: "c_add".to_string(),
                    },
                    address: AddressState::Committed(CodeBody {
                        code: real_c_add_code(),
                        relocations: vec![],
                    }),
                },
            )
            .unwrap();

        graph.require_symbol(
            Realm::Cargo,
            SymbolId {
                realm: Realm::C,
                name: "c_add".to_string(),
            },
            Realm::C,
        );

        let nodes = graph.nodes.read().unwrap();
        let node = nodes
            .get(&SymbolId {
                realm: Realm::C,
                name: "c_add".to_string(),
            })
            .unwrap();
        assert!(
            matches!(node.address, AddressState::Committed(ref body) if body.code == real_c_add_code()),
            "a directly-Committed symbol must be completely unaffected by require_symbol's new \
             promotion logic, got {:?}",
            node.address
        );
    }
}

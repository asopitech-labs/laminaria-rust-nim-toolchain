//! Issue #68 follow-up: the user's own objection was that
//! `target_ir::mir_text` -- which parses `rustc --emit=mir`'s
//! human-readable *text* dump -- proves nothing specific to Rust. It
//! never touches the borrow checker's actual output, never sees a real
//! `Ty`/`Mutability`, and the earlier PoC's `Ownership` tag was carried
//! as an inert value that lowering never consulted. That is a valid
//! criticism: a hand-rolled parser for a text format is not "using
//! Rust," it is "using a string that happens to come from `rustc`."
//!
//! This crate instead calls `rustc_driver::run_compiler` directly (the
//! same `#![feature(rustc_private)]` internal-API mechanism
//! `rustc_codegen_gcc`/`rustc_codegen_cranelift` actually use, confirmed
//! in this session's own earlier research fork against real source) with
//! a `Callbacks::after_analysis` hook -- the exact point, verified
//! directly against `compiler/rustc_interface/src/passes.rs`'s own
//! `run_required_analyses`/`analysis` gate (already cited in this
//! project's `removing-intermediate-representation_ja.md` section 5.6.1)
//! where borrow checking has already run and codegen has not yet started.
//!
//! ## Structure (third round: split into a library + multiple binaries)
//!
//! Originally a single `main.rs`. Split into this `lib.rs` (the shared
//! `inspect`/`inspect_result`/`Ownership`/`InspectedItem` machinery every
//! Rust-feature investigation reuses) plus one `src/bin/*.rs` per
//! independent language-feature investigation (lifetimes, `dyn Trait`,
//! struct/closure layout, `unsafe`/raw pointers), so multiple
//! investigations can be developed in parallel without touching the same
//! file. `main.rs` keeps the original `&mut i32`/`&i32`/generics/
//! Box/enum-match demonstration and its own borrowck-gate-to-codegen
//! integration (`compile_pointer_function_respecting_borrowck_gate`).

#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_driver::Callbacks;
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;
use std::sync::{Arc, Mutex};

/// Issue #68's `target_ir::Ownership` mapped from a *real* rustc
/// `Ty<'tcx>` -- not a value the caller invented, but one derived by
/// reading the actual `TyKind::Ref(_, _, Mutability)` field, mirroring
/// (not re-implementing) the exact read `arg_attrs_for_rust_scalar`
/// performs in `compiler/rustc_ty_utils/src/abi.rs`, confirmed in this
/// project's earlier research. `Boxed` was added after the user's
/// critical review pointed out that `&mut`/`&` alone (a distinction any
/// C-like language with `const` already has) proves nothing specific to
/// Rust -- `Box<T>` is Rust's own unique-ownership *heap* type, detected
/// here via the real `Ty::is_box()` query (`compiler/rustc_middle/src/ty/sty.rs`),
/// not a name-string comparison against `"Box"`.
///
/// Issue #68, fifth-round critical review, the decisive correction this
/// enum's own earlier shape was missing: a hypothesis-testing reviewer
/// pointed out that `Ownership::Shared` (mapped from any `&T`
/// unconditionally) is a **narrower, more dangerous vocabulary than the
/// exact real rustc mechanism** it was supposed to demonstrate an
/// improvement over. Confirmed directly against
/// `compiler/rustc_abi/src/lib.rs`'s real `PointerKind` enum:
///
/// ```text
/// pub enum PointerKind {
///     /// Shared reference. `frozen` indicates the absence of any `UnsafeCell`.
///     SharedRef { frozen: bool },
///     MutableRef { unpin: bool },
///     Box { unpin: bool, global: bool },
/// }
/// ```
///
/// and against the real consumer, `arg_attrs_for_rust_scalar`
/// (`compiler/rustc_ty_utils/src/abi.rs`, lines 366-370, confirmed this
/// session):
///
/// ```text
/// let no_alias = match kind {
///     PointerKind::SharedRef { frozen } => frozen,
///     PointerKind::MutableRef { unpin } => unpin,
///     PointerKind::Box { unpin, global } => unpin && global && noalias_for_box,
/// };
/// ```
///
/// i.e. rustc itself **never** grants `noalias` to a bare `&T` -- only to
/// a `&T` that is additionally `frozen` (contains no `UnsafeCell`
/// anywhere in its pointee, confirmed via the real, public
/// `Ty::is_freeze` query, `compiler/rustc_middle/src/ty/util.rs`).
/// `Ownership::Shared` previously had no equivalent of this condition at
/// all, and this crate's earlier `lower_load_or_reload_to_code_body`
/// integration (`main.rs`) treated *every* `&T` as safe to cache across
/// a branch -- which is unsound for `&Cell<i32>` specifically, since a
/// `Cell` write through a second alias can observe or change the cached
/// value. `Shared` is now `Shared { frozen: bool }`, carrying the exact
/// condition rustc itself requires before treating a shared reference as
/// safe to alias-optimize -- narrower vocabulary was the reviewer's
/// finding, not narrower reality; this fixes it to match reality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Unique,
    /// `frozen: true` means this reference's own real `Ty<'tcx>` passed
    /// `Ty::is_freeze` (no `UnsafeCell` anywhere in the pointee) --
    /// exactly the condition `arg_attrs_for_rust_scalar` requires before
    /// treating a `&T` as `noalias`-safe. `frozen: false` (e.g. `&Cell<i32>`)
    /// must never be treated the same as a genuinely frozen `&T`.
    Shared {
        frozen: bool,
    },
    Boxed,
    NotAReference,
}

/// Derives `Ownership` from a real `Ty<'tcx>`, now including the real
/// `Freeze`/`UnsafeCell` check `arg_attrs_for_rust_scalar` itself
/// performs (via the real, public `Ty::is_freeze` query) rather than
/// treating every `&T` identically. `tcx`/`typing_env` are required
/// (unlike the earlier, `Ty`-only signature) because `is_freeze` is
/// itself a real query against the type-checking context, not something
/// derivable from `Ty` alone.
pub fn ownership_from_real_ty<'tcx>(
    ty: rustc_middle::ty::Ty<'tcx>,
    tcx: TyCtxt<'tcx>,
    typing_env: rustc_middle::ty::TypingEnv<'tcx>,
) -> Ownership {
    if ty.is_box() {
        return Ownership::Boxed;
    }
    match ty.kind() {
        rustc_middle::ty::TyKind::Ref(_, _, rustc_middle::mir::Mutability::Mut) => {
            Ownership::Unique
        }
        rustc_middle::ty::TyKind::Ref(_, referent_ty, rustc_middle::mir::Mutability::Not) => {
            Ownership::Shared {
                frozen: referent_ty.is_freeze(tcx, typing_env),
            }
        }
        _ => Ownership::NotAReference,
    }
}

/// What `Inspect::after_analysis` extracts for one item, using only
/// primitive/owned data (no `Ty<'tcx>` reference survives past the
/// callback's own lifetime `'tcx`) so it can cross out of
/// `rustc_driver::run_compiler`'s callback closure and be asserted on by
/// an ordinary `#[test]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedItem {
    pub name: String,
    pub borrowck_succeeded: bool,
    pub basic_block_count: usize,
    /// `(local index, real Ty<'tcx> debug string, derived Ownership)` for
    /// every local this item's `mir::Body` declares -- the debug string
    /// is the real type-checker output (e.g. `&'{erased} mut i32`), not a
    /// value this program invented.
    pub locals: Vec<(usize, String, Ownership)>,
    /// How many real `TerminatorKind::Drop` terminators this item's real
    /// MIR body contains, counted by walking `body.basic_blocks` and
    /// matching on the actual terminator kind (never inferred from
    /// source text) -- confirmed in this session against real
    /// `rustc --emit=mir` output for `Box<i32>` consumption (session's own
    /// captured `boxdrop.mir`: a single `drop(_1) -> [return: bb1, unwind
    /// continue];` terminator, alongside alignment/null-pointer `assert`
    /// terminators MIR building inserts for the raw dereference). Nonzero
    /// only when the compiler's own drop-elaboration pass (the CFG
    /// fixpoint computation the earlier design doc's research, section
    /// 2.2, judged un-deletable) actually inserted one.
    pub drop_terminator_count: usize,
    /// How many real `Rvalue::Discriminant(_)` reads this item's real MIR
    /// body contains -- rustc's own enum-tag-read operation, confirmed in
    /// this session against real MIR for a 3-variant `enum Shape` `match`
    /// (session's own captured `matchenum.mir`: `_2 = discriminant(_1);`
    /// followed by a 3-arm-plus-`unreachable` `switchInt`, then
    /// variant-specific field projections like `((_1 as Square).0: i32)`).
    /// This is what a real `match` on an enum lowers to -- not the
    /// `mir_text` module's own 2-arm boolean `switchInt` shape, which
    /// cannot represent this at all.
    pub discriminant_read_count: usize,
    /// How many real `TerminatorKind::Assert` terminators this item's
    /// real MIR body contains -- confirmed in this session against real
    /// MIR for a slice index (session's own captured `sliceindex.mir`
    /// from `pub fn get_elem(s: &[i32], i: usize) -> i32 { s[i] }`):
    /// `_3 = PtrMetadata(copy _1); _4 = Lt(copy _2, copy _3);
    /// assert(move _4, "index out of bounds...", ...)`. This is the real
    /// MIR shape a bounds-checked array/slice/`Vec` index lowers to --
    /// `unified_symbol_graph::target_ir::lower_bounds_checked_slice_index_to_code_body`
    /// models the corresponding x86_64 codegen for this exact shape.
    pub assert_terminator_count: usize,
}

struct Inspect {
    interesting_names: &'static [&'static str],
    collected: Arc<Mutex<Vec<InspectedItem>>>,
}

impl Callbacks for Inspect {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> rustc_driver::Compilation {
        for local_def_id in tcx.hir_body_owners() {
            let name = tcx.item_name(local_def_id.to_def_id());
            if !self.interesting_names.contains(&name.as_str()) {
                continue;
            }

            // The decisive step: mir_borrowck is not "parse the
            // function," it is rustc's own borrow-check query -- the
            // exact gate this project's design doc
            // (removing-intermediate-representation_ja.md section 5.6.1)
            // confirmed blocks codegen on failure. Calling it here proves
            // this program is consuming Rust's real borrow checker, not
            // merely reading text `rustc --emit=mir` happened to print.
            let borrowck_succeeded = tcx.mir_borrowck(local_def_id).is_ok();

            let body = tcx.optimized_mir(local_def_id.to_def_id());
            let typing_env = rustc_middle::ty::TypingEnv::fully_monomorphized();
            let locals = body
                .local_decls
                .iter_enumerated()
                .map(|(idx, decl)| {
                    (
                        idx.index(),
                        format!("{:?}", decl.ty),
                        ownership_from_real_ty(decl.ty, tcx, typing_env),
                    )
                })
                .collect();

            // Real, walked structure -- not inferred from source text.
            // `basic_blocks` is `body`'s own real CFG; every statement
            // and terminator here is what rustc's own MIR building +
            // drop elaboration + match lowering actually produced for
            // this item, confirmed against this session's own captured
            // `boxdrop.mir`/`matchenum.mir` output before writing this
            // counting logic.
            let mut drop_terminator_count = 0;
            let mut discriminant_read_count = 0;
            let mut assert_terminator_count = 0;
            for bb in body.basic_blocks.iter() {
                match bb.terminator().kind {
                    rustc_middle::mir::TerminatorKind::Drop { .. } => {
                        drop_terminator_count += 1;
                    }
                    rustc_middle::mir::TerminatorKind::Assert { .. } => {
                        assert_terminator_count += 1;
                    }
                    _ => {}
                }
                for stmt in &bb.statements {
                    if let rustc_middle::mir::StatementKind::Assign(place_and_rvalue) = &stmt.kind {
                        if matches!(
                            place_and_rvalue.1,
                            rustc_middle::mir::Rvalue::Discriminant(_)
                        ) {
                            discriminant_read_count += 1;
                        }
                    }
                }
            }

            self.collected.lock().unwrap().push(InspectedItem {
                name: name.to_string(),
                borrowck_succeeded,
                basic_block_count: body.basic_blocks.len(),
                locals,
                drop_terminator_count,
                discriminant_read_count,
                assert_terminator_count,
            });
        }
        rustc_driver::Compilation::Stop
    }
}

/// Compiles `source` (a real `.rs` file written to a scratch path -- no
/// `rustc_ast`/token stream is fabricated in-process, this genuinely
/// round-trips through `rustc_driver::run_compiler`'s own CLI-argument
/// entrypoint) and returns the real, borrow-checked `InspectedItem` for
/// every name in `interesting_names` that `after_analysis` found among
/// the crate's `hir_body_owners`.
pub fn inspect(source: &str, interesting_names: &'static [&'static str]) -> Vec<InspectedItem> {
    inspect_result(source, interesting_names)
        .expect("this convenience wrapper is only for sources known to pass borrow checking")
}

/// Issue #68 second-round critical review, third gap: the earlier
/// `inspect` unconditionally called `rustc_driver::run_compiler` and
/// never observed what happens on a genuine borrow-check failure. This
/// session's own earlier manual experiment (deliberately feeding a
/// double-`&mut` borrow) showed the process aborted with E0499 printed
/// to stderr -- but that observation never distinguished "the process
/// exited" from "a panic unwound past this function," and never
/// attempted to *catch* that failure and continue running Rust code
/// afterward. Investigating the actual mechanism (this session, reading
/// `compiler/rustc_span/src/fatal_error.rs` directly) found that
/// `raise_fatal()` (the same call this project's earlier research,
/// `removing-intermediate-representation_ja.md` section 5.6.1, already
/// named as the borrowck gate) is implemented as
/// `std::panic::resume_unwind(Box::new(FatalErrorMarker))` -- an
/// **unwinding panic**, not a process exit -- specifically so that
/// `rustc_errors::catch_fatal_errors` (`panic::catch_unwind` underneath)
/// can convert it into an ordinary `Result::Err`. This function uses
/// that real API to make the borrowck gate an observable, catchable
/// outcome a caller can act on, rather than an uncatchable process
/// termination -- the mechanism issue #68's own design doc (section
/// 5.6.6) requires ("target固有バックエンド側でどう両立させるか") to actually
/// gate codegen on borrowck's real result, not merely note that it should.
pub fn inspect_result(
    source: &str,
    interesting_names: &'static [&'static str],
) -> Result<Vec<InspectedItem>, rustc_span::fatal_error::FatalError> {
    let out_dir = std::env::temp_dir().join("rustc-driver-poc-out");
    std::fs::create_dir_all(&out_dir).expect("create scratch out dir");
    // Includes a numeric-only thread id (see `dyn_trait_check.rs`'s own
    // identical fix, `src/bin/dyn_trait_check.rs`, discovered first)
    // because `cargo test` runs every test binary's own `#[test]`s in
    // parallel threads, and this crate's own `main.rs`/`unsafe_check.rs`
    // both call `inspect_result` -- two different binaries, but each is
    // itself multi-threaded, and the hash-only path this function used
    // before this fix let two *concurrent* invocations (potentially
    // across binaries, since `/tmp` is shared) collide on the same
    // scratch file and crate name. Confirmed necessary in this session:
    // `cargo +nightly test` (the whole crate, all binaries) failed
    // intermittently (observed in 1 of 3 consecutive runs) before this
    // fix, and 0 of 3 after.
    let thread_id_numeric: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let src_path = out_dir.join(format!("src_{:x}_{thread_id_numeric}.rs", fnv1a(source)));
    std::fs::write(&src_path, source).expect("write scratch source file");

    let args = vec![
        "rustc-driver-poc".to_string(),
        "--edition".to_string(),
        "2021".to_string(),
        "--crate-type".to_string(),
        "lib".to_string(),
        src_path.to_str().unwrap().to_string(),
    ];

    let collected = Arc::new(Mutex::new(Vec::new()));
    let mut callbacks = Inspect {
        interesting_names,
        collected: Arc::clone(&collected),
    };

    rustc_errors::catch_fatal_errors(std::panic::AssertUnwindSafe(|| {
        rustc_driver::run_compiler(&args, &mut callbacks);
    }))?;

    // Not `Arc::try_unwrap`: `rustc_driver::run_compiler` runs the
    // compilation on its own internal thread (confirmed empirically --
    // `try_unwrap` failed with a second live `Arc` reference even after
    // `run_compiler` returned), so a second clone of this `Arc` can
    // legitimately still be alive briefly. Cloning the `Mutex`'s contents
    // out, rather than trying to reclaim sole ownership of the `Arc`
    // itself, sidesteps that race entirely.
    let items = collected.lock().unwrap().clone();
    Ok(items)
}

/// Issue #68 follow-up (second round): the user's critical review of the
/// first `rustc_driver` round pointed out that `&mut i32`/`&i32`/`i32`
/// exercises nothing specific to Rust -- C already has `const` for that
/// distinction. **Generic monomorphization** is a mechanism no C-like
/// custom IR experiment could accidentally replicate: `fn identity<T>(x:
/// T) -> T` has no fixed size or layout at all until a concrete `T` is
/// substituted, and each substitution genuinely produces a *different*
/// `mir::Body` -- not textually different (this project's earlier
/// `mir_text` module already showed textual MIR differs trivially across
/// inputs), but differing in the real `Layout`/`TyAndLayout` a target
/// backend would need to allocate stack space and choose register
/// widths.
///
/// `substituted_type` is the human-readable type name (`"i32"`/`"i64"`)
/// used only to label the result; `local_decl_count`/`layout_size_bytes`
/// are read from the real, monomorphization-specific `mir::Body`/
/// `TyAndLayout` rustc's own query system produces via
/// `Instance::instantiate_mir_and_normalize_erasing_regions`, not
/// something this program infers from the generic function's source text
/// (which has no size information to infer from in the first place).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonomorphizedItem {
    pub substituted_type: String,
    pub local_decl_count: usize,
    pub layout_size_bytes: u64,
}

struct InspectMono {
    generic_fn_name: &'static str,
    substituted_type_names: &'static [&'static str],
    collected: Arc<Mutex<Vec<MonomorphizedItem>>>,
}

impl Callbacks for InspectMono {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> rustc_driver::Compilation {
        let generic_def_id = tcx
            .hir_body_owners()
            .find(|def_id| tcx.item_name(def_id.to_def_id()).as_str() == self.generic_fn_name)
            .expect("the named generic fn must exist in the compiled source")
            .to_def_id();

        for type_name in self.substituted_type_names {
            // Build a real `GenericArgsRef<'tcx>` substituting the named
            // primitive type for `identity`'s own `T` -- the actual
            // monomorphization substitution rustc's own codegen-unit
            // partitioning performs per concrete call site (confirmed
            // against `compiler/rustc_monomorphize/src/partitioning.rs`
            // in this project's earlier research), not a value this
            // program invents independent of rustc's own type system.
            let ty = match *type_name {
                "i32" => tcx.types.i32,
                "i64" => tcx.types.i64,
                other => panic!("unsupported substituted_type_names entry: {other}"),
            };
            let args =
                rustc_middle::ty::GenericArgs::for_item(
                    tcx,
                    generic_def_id,
                    |param, _| match param.kind {
                        rustc_middle::ty::GenericParamDefKind::Lifetime => {
                            tcx.lifetimes.re_erased.into()
                        }
                        rustc_middle::ty::GenericParamDefKind::Type { .. } => ty.into(),
                        rustc_middle::ty::GenericParamDefKind::Const { .. } => {
                            panic!("identity<T> has no const generic parameters")
                        }
                    },
                );
            let instance = rustc_middle::ty::Instance::new_raw(generic_def_id, args);

            // The decisive query: `instantiate_mir_and_normalize_erasing_regions`
            // performs the real substitution of `T` -> the concrete type
            // inside the generic function's own MIR body -- this is
            // monomorphization as rustc's own codegen backends
            // (LLVM/Cranelift/GCC) actually consume it, not a
            // reimplementation of substitution logic by this program.
            let generic_body = tcx.optimized_mir(generic_def_id);
            let param_env = rustc_middle::ty::TypingEnv::fully_monomorphized();
            let monomorphized_body = instance.instantiate_mir_and_normalize_erasing_regions(
                tcx,
                param_env,
                rustc_middle::ty::EarlyBinder::bind(tcx, generic_body.clone()),
            );

            let layout = tcx
                .layout_of(param_env.as_query_input(ty))
                .expect("primitive types must always have a computable layout");

            self.collected.lock().unwrap().push(MonomorphizedItem {
                substituted_type: type_name.to_string(),
                local_decl_count: monomorphized_body.local_decls.len(),
                layout_size_bytes: layout.size.bytes(),
            });
        }
        rustc_driver::Compilation::Stop
    }
}

/// Compiles `source` (containing a generic function named
/// `generic_fn_name`) and, for each type name in `substituted_type_names`
/// (currently `"i32"`/`"i64"` only -- see `InspectMono::after_analysis`),
/// performs the real rustc monomorphization substitution and returns the
/// resulting `MonomorphizedItem`s.
pub fn inspect_monomorphization(
    source: &str,
    generic_fn_name: &'static str,
    substituted_type_names: &'static [&'static str],
) -> Vec<MonomorphizedItem> {
    let out_dir = std::env::temp_dir().join("rustc-driver-poc-out");
    std::fs::create_dir_all(&out_dir).expect("create scratch out dir");
    // See `inspect_result`'s own identical comment for why a thread id
    // must be included in the scratch path.
    let thread_id_numeric: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let src_path = out_dir.join(format!("src_{:x}_{thread_id_numeric}.rs", fnv1a(source)));
    std::fs::write(&src_path, source).expect("write scratch source file");

    let args = vec![
        "rustc-driver-poc".to_string(),
        "--edition".to_string(),
        "2021".to_string(),
        "--crate-type".to_string(),
        "lib".to_string(),
        src_path.to_str().unwrap().to_string(),
    ];

    let collected = Arc::new(Mutex::new(Vec::new()));
    let mut callbacks = InspectMono {
        generic_fn_name,
        substituted_type_names,
        collected: Arc::clone(&collected),
    };
    rustc_driver::run_compiler(&args, &mut callbacks);

    let items = collected.lock().unwrap().clone();
    items
}

/// A tiny, dependency-free hash (this project's own precedent, "no serde
/// dependency" etc., extended here to "no hash-crate dependency either")
/// used only to give each distinct `source` string its own scratch file
/// path so parallel `#[test]` runs never race on the same file.
pub fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

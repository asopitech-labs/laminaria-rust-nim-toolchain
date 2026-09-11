//! Issue #5 T1: the first owned target-generation route, per
//! `docs/design/issue-5-t0-target-generation-spec.md` (accepted, commit
//! `414b276`). Lowers a [`ValidatedProgram`] directly into a WebAssembly
//! 1.0 (MVP) binary module -- a pure, in-process function; no external
//! compiler/assembler/linker/WASM toolchain is ever invoked from this
//! module (verified by `source_never_spawns_a_subprocess` below, a
//! regression guard, not a structural proof -- see the T0 doc's own §6
//! correction on that distinction).
//!
//! Every `Expr`/`Stmt`/`IntWidth` variant is matched exhaustively, with
//! no wildcard arm, exactly as the T0 doc's §7 requires: the compiler
//! itself refuses to build this module the day a new variant is added
//! without a corresponding arm here.

use std::collections::BTreeMap;

use crate::types::{Expr, FnFact, FnId, IntWidth, LocalId, Stmt};
use crate::validate::ValidatedProgram;

/// No variant is constructed by this module today -- every `Expr`/
/// `Stmt`/`IntWidth` case the current IR can produce maps onto the WASM
/// ISA fixed by the T0 doc's §4 (confirmed by the exhaustive matches
/// below). Kept as a real `Result` for interface stability (the T0 doc's
/// own §9 signature), not because a failure exists today: `IntWidth` has
/// exactly one variant, so `UnsupportedIntWidth` cannot be constructed
/// until a second variant is added -- at which point the compiler forces
/// a new match arm to decide what happens, rather than silently
/// mis-encoding the new width as `i32`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    UnsupportedIntWidth(IntWidth),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodegenError::UnsupportedIntWidth(width) => {
                write!(f, "wasm_target: unsupported IntWidth: {width:?}")
            }
        }
    }
}

impl std::error::Error for CodegenError {}

// ---- WASM binary primitives (LEB128, sections) -------------------------

fn write_uleb128(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            out.push(byte | 0x80);
        } else {
            out.push(byte);
            break;
        }
    }
}

/// Signed LEB128, used only for `i32.const`'s immediate -- the value is
/// sign-extended to `i64` for the encoding loop (the standard SLEB128
/// algorithm), then the caller reconstructs an `i32` from it.
fn write_sleb128_i32(out: &mut Vec<u8>, value: i32) {
    let mut value = value as i64;
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        let sign_bit_set = (byte & 0x40) != 0;
        if (value == 0 && !sign_bit_set) || (value == -1 && sign_bit_set) {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

fn write_name(out: &mut Vec<u8>, name: &str) {
    write_uleb128(out, name.len() as u64);
    out.extend_from_slice(name.as_bytes());
}

fn write_section(module: &mut Vec<u8>, section_id: u8, body: &[u8]) {
    module.push(section_id);
    write_uleb128(module, body.len() as u64);
    module.extend_from_slice(body);
}

const WASM_I32: u8 = 0x7F;
const WASM_FUNCTYPE: u8 = 0x60;

const OP_IF: u8 = 0x04;
const OP_ELSE: u8 = 0x05;
const OP_END: u8 = 0x0B;
const OP_RETURN: u8 = 0x0F;
const OP_CALL: u8 = 0x10;
const OP_LOCAL_GET: u8 = 0x20;
const OP_LOCAL_SET: u8 = 0x21;
const OP_I32_CONST: u8 = 0x41;
const OP_I32_EQZ: u8 = 0x45;
const OP_I32_ADD: u8 = 0x6A;
const OP_I32_SUB: u8 = 0x6B;
const OP_I32_MUL: u8 = 0x6C;

// ---- Codegen context -----------------------------------------------------

/// Per-function codegen state. `scope` is a stack of `(LocalId, wasm_local_index)`
/// pairs -- one fresh WASM local per `Let` *occurrence*, resolved
/// innermost-first, mirroring `interpreter.rs`'s own insert-then-restore
/// shadowing semantics exactly (T0 doc §5's corrected local-allocation
/// design). This is not merely one design choice among several: a global
/// "one WASM local per distinct `LocalId` value" map was tried first and
/// found to alias two different bindings that happen to reuse the same
/// `LocalId` (`interpreter.rs:203-218`'s own restore-after-scope-ends
/// behavior is the direct evidence that reuse is a real, permitted case,
/// not a hypothetical).
struct FunctionCtx<'a> {
    scope: Vec<(LocalId, u32)>,
    next_local_index: u32,
    extra_locals_count: u32,
    function_index: &'a BTreeMap<&'a str, u32>,
}

impl FunctionCtx<'_> {
    fn alloc_local(&mut self) -> u32 {
        let index = self.next_local_index;
        self.next_local_index += 1;
        self.extra_locals_count += 1;
        index
    }

    fn resolve_local(&self, id: LocalId) -> u32 {
        self.scope
            .iter()
            .rev()
            .find(|(local_id, _)| *local_id == id)
            .map(|(_, index)| *index)
            .expect(
                "Expr::Local referenced a LocalId with no enclosing binding -- \
                 validate_program must already reject this before codegen ever runs",
            )
    }
}

fn lower_int_width(width: IntWidth) -> Result<(), CodegenError> {
    // Exhaustive, no wildcard: the one arm that exists today is a no-op
    // (i32 is WASM's native 32-bit integer type, nothing to encode here
    // beyond what each call site already does) -- this function exists
    // so that adding `IntWidth::I64` (or any other variant) without also
    // teaching every call site how to encode it is a compile error, not
    // a silent i32 mis-encoding.
    match width {
        IntWidth::I32 => Ok(()),
    }
}

fn lower_expr(
    expr: &Expr,
    ctx: &mut FunctionCtx<'_>,
    out: &mut Vec<u8>,
) -> Result<(), CodegenError> {
    match expr {
        Expr::IntLit(value, width, _) => {
            lower_int_width(*width)?;
            out.push(OP_I32_CONST);
            write_sleb128_i32(out, *value as i32);
        }
        Expr::Param(index, _) => {
            out.push(OP_LOCAL_GET);
            write_uleb128(out, *index as u64);
        }
        Expr::Local(id, _) => {
            out.push(OP_LOCAL_GET);
            write_uleb128(out, ctx.resolve_local(*id) as u64);
        }
        Expr::WrappingAdd(a, b, _) => {
            lower_expr(a, ctx, out)?;
            lower_expr(b, ctx, out)?;
            out.push(OP_I32_ADD);
        }
        Expr::WrappingSub(a, b, _) => {
            lower_expr(a, ctx, out)?;
            lower_expr(b, ctx, out)?;
            out.push(OP_I32_SUB);
        }
        Expr::WrappingMul(a, b, _) => {
            lower_expr(a, ctx, out)?;
            lower_expr(b, ctx, out)?;
            out.push(OP_I32_MUL);
        }
        Expr::NotEqZero(inner, _) => {
            // T0 doc §7: NOT(x == 0) = i32.eqz(i32.eqz(x)) -- no extra
            // WASM instruction beyond i32.eqz is needed.
            lower_expr(inner, ctx, out)?;
            out.push(OP_I32_EQZ);
            out.push(OP_I32_EQZ);
        }
        Expr::Call(FnId(name), args, _) => {
            for arg in args {
                lower_expr(arg, ctx, out)?;
            }
            let index = *ctx.function_index.get(name.as_str()).expect(
                "Expr::Call referenced a function name absent from Program::functions -- \
                 validate_program must already reject this before codegen ever runs",
            );
            out.push(OP_CALL);
            write_uleb128(out, index as u64);
        }
        Expr::Let {
            local, value, body, ..
        } => {
            lower_expr(value, ctx, out)?;
            let index = ctx.alloc_local();
            out.push(OP_LOCAL_SET);
            write_uleb128(out, index as u64);
            ctx.scope.push((*local, index));
            let result = lower_expr(body, ctx, out);
            ctx.scope.pop();
            result?;
        }
    }
    Ok(())
}

fn lower_stmt(
    stmt: &Stmt,
    ctx: &mut FunctionCtx<'_>,
    out: &mut Vec<u8>,
) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Let {
            local, value, body, ..
        } => {
            lower_expr(value, ctx, out)?;
            let index = ctx.alloc_local();
            out.push(OP_LOCAL_SET);
            write_uleb128(out, index as u64);
            ctx.scope.push((*local, index));
            let result = lower_stmt(body, ctx, out);
            ctx.scope.pop();
            result?;
        }
        Stmt::If {
            cond, then, els, ..
        } => {
            lower_expr(cond, ctx, out)?;
            out.push(OP_IF);
            // Blocktype must be `i32`, not "empty": both `then`/`els` are
            // `Stmt`s, and this IR's grammar guarantees every `Stmt` leaf
            // is a `Return` (Let/If bodies are themselves `Stmt`s that
            // bottom out the same way) -- so both arms always terminate
            // via an explicit `return` before falling off their own end.
            // WASM's block-typing is structural, not reachability-aware,
            // though: regardless of that internal early-return, exiting
            // `if...end` still pushes exactly the *declared* blocktype's
            // result arity onto the enclosing stack. An empty (0x40)
            // blocktype here pushed zero values, starving the function
            // body's own final `end`, which always expects one `i32`
            // (`type mismatch: expected i32 but nothing on stack`) --
            // caught by `debug_if_else` isolating this exact construct.
            // `i32` here is always correct, never merely convenient: both
            // arms are unreachable at their own `end` (post-`return`), so
            // wasmtime's polymorphic-stack rule accepts any synthesized
            // value there, and it is provably never observed at runtime.
            out.push(WASM_I32);
            lower_stmt(then, ctx, out)?;
            out.push(OP_ELSE);
            lower_stmt(els, ctx, out)?;
            out.push(OP_END);
        }
        Stmt::Return(expr, _) => {
            lower_expr(expr, ctx, out)?;
            out.push(OP_RETURN);
        }
    }
    Ok(())
}

fn lower_function(
    f: &FnFact,
    function_index: &BTreeMap<&str, u32>,
) -> Result<Vec<u8>, CodegenError> {
    for (_, width) in &f.params {
        lower_int_width(*width)?;
    }
    lower_int_width(f.return_width)?;

    let mut ctx = FunctionCtx {
        scope: Vec::new(),
        next_local_index: f.params.len() as u32,
        extra_locals_count: 0,
        function_index,
    };
    let mut body = Vec::new();
    lower_stmt(&f.body, &mut ctx, &mut body)?;
    body.push(OP_END);

    let mut code = Vec::new();
    if ctx.extra_locals_count > 0 {
        write_uleb128(&mut code, 1); // one locals-declaration group
        write_uleb128(&mut code, ctx.extra_locals_count as u64);
        code.push(WASM_I32);
    } else {
        write_uleb128(&mut code, 0);
    }
    code.extend_from_slice(&body);
    Ok(code)
}

/// Lowers a validated `Program` into a complete WebAssembly 1.0 (MVP)
/// binary module: `Type`/`Function`/`Export`/`Code` sections only (no
/// `memory`/`table`/`global`, since the current IR has no arrays,
/// pointers, or global state -- T0 doc §4). Every `FnFact` becomes one
/// exported WASM function with the same name, `(params.len() i32s) ->
/// (1 i32)` signature (`FnFact.return_width` is always `IntWidth::I32`,
/// so the result type is always a single `i32`).
pub fn generate_wasm_module(program: &ValidatedProgram) -> Result<Vec<u8>, CodegenError> {
    let program = program.program();
    let names: Vec<&str> = program.functions.keys().map(String::as_str).collect();
    let function_index: BTreeMap<&str, u32> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (*name, i as u32))
        .collect();

    let mut module = Vec::new();
    module.extend_from_slice(b"\0asm");
    module.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    // Type section (id 1): one functype per function, in the same
    // BTreeMap-sorted (name-ascending) order as `function_index`.
    let mut type_section = Vec::new();
    write_uleb128(&mut type_section, names.len() as u64);
    for name in &names {
        let f = &program.functions[*name];
        type_section.push(WASM_FUNCTYPE);
        write_uleb128(&mut type_section, f.params.len() as u64);
        type_section.resize(type_section.len() + f.params.len(), WASM_I32);
        write_uleb128(&mut type_section, 1);
        type_section.push(WASM_I32);
    }
    write_section(&mut module, 1, &type_section);

    // Function section (id 3): type index == function index, 1:1.
    let mut function_section = Vec::new();
    write_uleb128(&mut function_section, names.len() as u64);
    for i in 0..names.len() {
        write_uleb128(&mut function_section, i as u64);
    }
    write_section(&mut module, 3, &function_section);

    // Export section (id 7): every function exported by its source name.
    let mut export_section = Vec::new();
    write_uleb128(&mut export_section, names.len() as u64);
    for (i, name) in names.iter().enumerate() {
        write_name(&mut export_section, name);
        export_section.push(0x00); // export kind: func
        write_uleb128(&mut export_section, i as u64);
    }
    write_section(&mut module, 7, &export_section);

    // Code section (id 10).
    let mut code_section = Vec::new();
    write_uleb128(&mut code_section, names.len() as u64);
    for name in &names {
        let f = &program.functions[*name];
        let code = lower_function(f, &function_index)?;
        write_uleb128(&mut code_section, code.len() as u64);
        code_section.extend_from_slice(&code);
    }
    write_section(&mut module, 10, &code_section);

    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FnFact, Program, Provenance, SourceLanguage, SourcePosition, SourceSpan};
    use crate::validate::validate_program;
    use std::path::PathBuf;

    fn prov() -> Provenance {
        Provenance {
            source_file: PathBuf::from("test"),
            span: SourceSpan {
                start: SourcePosition { line: 1, column: 1 },
                end: SourcePosition { line: 1, column: 1 },
            },
            language: SourceLanguage::Rust,
        }
    }

    /// The source-text-level regression guard T0 doc §6-2 describes --
    /// *not* a structural proof (the doc explicitly withdraws that
    /// earlier, false claim): confirms this module's own source spawns no
    /// external subprocess (compiler/assembler/linker/WASM toolchain)
    /// anywhere in the compilation path. The needle itself is never
    /// spelled out verbatim in this file (see the test body) -- doing so
    /// here in the doc comment would make the check self-matching via
    /// `include_str!` the same way the assertion string almost did.
    #[test]
    fn source_never_spawns_a_subprocess() {
        let source = include_str!("wasm_target.rs");
        // Built indirectly so this needle itself doesn't appear verbatim
        // in the file -- otherwise `include_str!` would pull in this very
        // assertion's own literal and the grep would trivially find itself.
        let subprocess_spawn_needle = format!("{}{}", "Command", "::new");
        assert!(
            !source.contains(&subprocess_spawn_needle),
            "wasm_target.rs must never spawn a subprocess as part of generating a WASM module \
             (this is a regression guard on the source text, not a structural type-level proof)"
        );
    }

    #[test]
    fn module_starts_with_the_wasm_magic_and_version() {
        let mut program = Program::default();
        program.insert(FnFact {
            name: "f".to_string(),
            params: vec![("x".to_string(), IntWidth::I32)],
            return_width: IntWidth::I32,
            provenance: prov(),
            body: Stmt::Return(Expr::Param(0, prov()), prov()),
        });
        let validated =
            validate_program(&program).expect("trivial identity function must validate");
        let bytes = generate_wasm_module(&validated).expect("codegen must succeed for this subset");
        assert_eq!(&bytes[0..4], b"\0asm");
        assert_eq!(&bytes[4..8], &[0x01, 0x00, 0x00, 0x00]);
    }

    /// T0 doc §8's 3-way agreement: the existing interpreter's tree-walk
    /// result, this module's freshly generated WASM executed via
    /// `wasmtime`, and the D0-confirmed real-rustc/real-nim cross-check
    /// values already established by `lib.rs`'s own
    /// `fixture_parity_tests` (not re-derived here).
    #[test]
    fn wasm_execution_agrees_with_the_interpreter_and_the_d0_confirmed_values() {
        let source = r#"
            fn double(x: i32) -> i32 {
                x.wrapping_add(x)
            }
            fn add_or_double(a: i32, b: i32, use_double: i32) -> i32 {
                if use_double != 0 {
                    double(a)
                } else {
                    a.wrapping_add(b)
                }
            }
        "#;
        let program = crate::rust_frontend::lower_rust_source(
            &PathBuf::from("test.rs"),
            source,
            &["double", "add_or_double"],
        )
        .expect("this source is within the declared subset");
        let validated = validate_program(&program).expect("must validate");
        let wasm_bytes =
            generate_wasm_module(&validated).expect("codegen must succeed for this subset");

        // D0-confirmed cases (issue #35, crates/laminaria-ir/src/lib.rs:99-150's
        // own fixture_parity_tests -- reused verbatim, not re-derived).
        let cases: &[((i64, i64, i64), i64)] = &[
            ((3, 4, 0), 7),
            ((3, 4, 1), 6),
            ((i32::MAX as i64, 1, 0), i32::MIN as i64),
            ((-5, 10, 1), -10),
        ];

        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &wasm_bytes).expect("module must validate");
        let mut store = wasmtime::Store::new(&engine, ());
        let instance =
            wasmtime::Instance::new(&mut store, &module, &[]).expect("instantiation must succeed");
        let add_or_double = instance
            .get_typed_func::<(i32, i32, i32), i32>(&mut store, "add_or_double")
            .expect("add_or_double must be exported with this exact signature");

        for &((a, b, use_double), expected) in cases {
            let interpreter_result =
                crate::interpreter::eval_function(&program, "add_or_double", &[a, b, use_double])
                    .expect("interpreter must accept this input")
                    .value;
            assert_eq!(
                interpreter_result, expected,
                "interpreter mismatch for ({a},{b},{use_double})"
            );

            let wasm_result = add_or_double
                .call(&mut store, (a as i32, b as i32, use_double as i32))
                .expect("wasmtime call must succeed");
            assert_eq!(
                wasm_result as i64, expected,
                "wasm execution mismatch for ({a},{b},{use_double})"
            );
        }
    }

    /// New fixed acceptance case (review round on issue #5 T1): the same
    /// `LocalId` reused for two different, nested bindings must not
    /// alias onto the same WASM local -- direct regression test for the
    /// exact bug the T0 doc's §5 correction fixed. Hand-built IR (not
    /// routed through a frontend, whose own numbering strategy is not
    /// what's under test here): `let L0 = x; (let L0 = x+100; L0) + L0`.
    /// The inner `L0` must read `x+100`; the *outer* `L0`, referenced
    /// again after the inner `Let`'s body closes, must read the
    /// restored `x` -- exactly the shadow/restore shape
    /// `interpreter.rs:203-218` already implements, and which this
    /// module's own `FunctionCtx::scope` stack is built to mirror.
    #[test]
    fn nested_shadowing_of_the_same_local_id_agrees_between_interpreter_and_wasm() {
        let local = LocalId(0);
        let body = Stmt::Return(
            Expr::Let {
                local,
                value: Box::new(Expr::Param(0, prov())),
                body: Box::new(Expr::WrappingAdd(
                    Box::new(Expr::Let {
                        local,
                        value: Box::new(Expr::WrappingAdd(
                            Box::new(Expr::Param(0, prov())),
                            Box::new(Expr::IntLit(100, IntWidth::I32, prov())),
                            prov(),
                        )),
                        body: Box::new(Expr::Local(local, prov())),
                        provenance: prov(),
                    }),
                    Box::new(Expr::Local(local, prov())),
                    prov(),
                )),
                provenance: prov(),
            },
            prov(),
        );
        let mut program = Program::default();
        program.insert(FnFact {
            name: "shadow_test".to_string(),
            params: vec![("x".to_string(), IntWidth::I32)],
            return_width: IntWidth::I32,
            provenance: prov(),
            body,
        });
        let validated = validate_program(&program).expect("hand-built IR must validate");

        // Expected: inner Let binds L0 = x+100, its own body returns that
        // (x+100); after the inner Let's body closes, the outer WrappingAdd's
        // second operand's `Local(L0)` must see the *restored* outer binding
        // (x), not the inner one -- result = (x+100) + x = 2x + 100.
        let x = 7i64;
        let expected = 2 * x + 100;

        let interpreter_result = crate::interpreter::eval_function(&program, "shadow_test", &[x])
            .expect("must evaluate")
            .value;
        assert_eq!(
            interpreter_result, expected,
            "interpreter's own restore semantics"
        );

        let wasm_bytes = generate_wasm_module(&validated).expect("codegen must succeed");
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &wasm_bytes).expect("module must validate");
        let mut store = wasmtime::Store::new(&engine, ());
        let instance =
            wasmtime::Instance::new(&mut store, &module, &[]).expect("instantiation must succeed");
        let shadow_test = instance
            .get_typed_func::<i32, i32>(&mut store, "shadow_test")
            .expect("shadow_test must be exported");
        let wasm_result = shadow_test
            .call(&mut store, x as i32)
            .expect("call must succeed");
        assert_eq!(
            wasm_result as i64, expected,
            "a global one-local-per-LocalId-value scheme would alias the inner binding onto the \
             outer one and read x+100 here instead of the restored x"
        );
    }

    /// New fixed acceptance case (review round on issue #5 T1): finite
    /// recursion lowers to an ordinary WASM `call`, exactly like any
    /// other call, per the T0 doc's own recursion decision (§9's
    /// correction). `countdown(n) = if n != 0 { countdown(n-1) + 1 }
    /// else { 0 }`, self-recursive via `Expr::Call` naming its own
    /// function -- `interpreter.rs`'s own module doc already calls
    /// itself "A direct recursive reference evaluator" with no cycle
    /// detection, so this is exercising an already-supported shape, not
    /// a new one.
    #[test]
    fn finite_recursion_agrees_between_interpreter_and_wasm() {
        let n_local = LocalId(0);
        let body = Stmt::If {
            cond: Expr::NotEqZero(Box::new(Expr::Param(0, prov())), prov()),
            then: Box::new(Stmt::Return(
                Expr::WrappingAdd(
                    Box::new(Expr::Call(
                        FnId("countdown".to_string()),
                        vec![Expr::WrappingSub(
                            Box::new(Expr::Param(0, prov())),
                            Box::new(Expr::IntLit(1, IntWidth::I32, prov())),
                            prov(),
                        )],
                        prov(),
                    )),
                    Box::new(Expr::IntLit(1, IntWidth::I32, prov())),
                    prov(),
                ),
                prov(),
            )),
            els: Box::new(Stmt::Return(Expr::IntLit(0, IntWidth::I32, prov()), prov())),
            provenance: prov(),
        };
        // `n_local` isn't actually used (no Let in this function) --
        // kept only so the test reads as self-documenting about which
        // LocalId space this function would use if it had one; silence
        // the unused-variable lint honestly rather than deleting a name
        // that documents intent.
        let _ = n_local;

        let mut program = Program::default();
        program.insert(FnFact {
            name: "countdown".to_string(),
            params: vec![("n".to_string(), IntWidth::I32)],
            return_width: IntWidth::I32,
            provenance: prov(),
            body,
        });
        let validated = validate_program(&program).expect("hand-built recursive IR must validate");

        let n = 5i64;
        let expected = 5i64; // adds 1 exactly n times before hitting the n==0 base case

        let interpreter_result = crate::interpreter::eval_function(&program, "countdown", &[n])
            .expect("must evaluate")
            .value;
        assert_eq!(interpreter_result, expected);

        let wasm_bytes = generate_wasm_module(&validated).expect("codegen must succeed");
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &wasm_bytes).expect("module must validate");
        let mut store = wasmtime::Store::new(&engine, ());
        let instance =
            wasmtime::Instance::new(&mut store, &module, &[]).expect("instantiation must succeed");
        let countdown = instance
            .get_typed_func::<i32, i32>(&mut store, "countdown")
            .expect("countdown must be exported");
        let wasm_result = countdown
            .call(&mut store, n as i32)
            .expect("call must succeed");
        assert_eq!(
            wasm_result as i64, expected,
            "finite self-recursion via an ordinary WASM call"
        );
    }
}

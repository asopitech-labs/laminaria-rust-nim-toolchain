//! Owned AArch64/Mach-O code generation.  No external compiler, assembler, or
//! linker participates in this module.

use std::collections::BTreeMap;

use crate::types::{Expr, IntWidth, LocalId, Stmt};
use crate::validate::ValidatedProgram;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    MissingEntry(String),
    TooManyArguments(usize),
    StackFrameTooLarge(usize),
    UnboundLocal(LocalId),
    UnsupportedConstruct(&'static str),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEntry(entry) => {
                write!(f, "aarch64-darwin target: missing entry {entry:?}")
            }
            Self::TooManyArguments(count) => write!(
                f,
                "aarch64-darwin target: {count} arguments exceeds the eight-register ABI core"
            ),
            Self::StackFrameTooLarge(slots) => write!(
                f,
                "aarch64-darwin target: {slots} stack slots exceeds the reloc-free native core"
            ),
            Self::UnboundLocal(local) => write!(
                f,
                "aarch64-darwin target: validated lowering exposed unbound local {local:?}"
            ),
            Self::UnsupportedConstruct(kind) => write!(
                f,
                "aarch64-darwin target: native core does not yet lower {kind}"
            ),
        }
    }
}
impl std::error::Error for CodegenError {}

/// Produces a reloc-free ARM64 Mach-O object containing one owned function.
/// The object is the target-code artifact; a later, explicit Darwin runtime
/// and link contract turns it into a process executable.
pub fn generate_object(program: &ValidatedProgram, entry: &str) -> Result<Vec<u8>, CodegenError> {
    let function = program
        .program()
        .functions
        .get(entry)
        .ok_or_else(|| CodegenError::MissingEntry(entry.to_owned()))?;
    if function.params.len() > 8 {
        return Err(CodegenError::TooManyArguments(function.params.len()));
    }

    let layout = FrameLayout::for_stmt(&function.body)?;
    let mut text = Vec::new();
    emit_prologue(&mut text, layout.frame_bytes);
    let mut context = EmitContext::new(layout);
    // Function receives i32 arguments in w0..w7 by the Darwin AArch64
    // integer calling convention.  Every expression result is materialized in
    // our own fixed stack frame before its consumer reads it.  This gives a
    // source `let` a stable location without using a callee-saved register or
    // changing the public ABI.
    emit_stmt(&mut text, &function.body, &mut context)?;
    debug_assert!(context.is_finished());
    Ok(write_mach_o_object(&text, entry))
}

#[derive(Debug, Clone, Copy)]
struct StackSlot(usize);

/// The first part of a frame is reserved for bindings, one slot per lexical
/// binding occurrence; the remainder holds materialized expression results.
/// Binding occurrences, rather than `LocalId` values, receive slots so even a
/// manually-built IR tree which shadows an identical `LocalId` preserves the
/// interpreter's save-and-restore semantics.
#[derive(Debug, Clone, Copy)]
struct FrameLayout {
    local_slots: usize,
    total_slots: usize,
    frame_bytes: u32,
}

impl FrameLayout {
    fn for_stmt(stmt: &Stmt) -> Result<Self, CodegenError> {
        let local_slots = count_stmt_bindings(stmt);
        let total_slots = local_slots + count_stmt_expressions(stmt);
        // A 32-bit LDR/STR with an unsigned immediate can address 4096
        // four-byte slots from x16.  Do not silently synthesize a different
        // addressing contract once that small, reloc-free core is exceeded.
        if total_slots > 4096 {
            return Err(CodegenError::StackFrameTooLarge(total_slots));
        }
        let unaligned = total_slots
            .checked_mul(4)
            .ok_or(CodegenError::StackFrameTooLarge(total_slots))?;
        let frame_bytes = unaligned
            .checked_add(15)
            .map(|bytes| bytes & !15)
            .ok_or(CodegenError::StackFrameTooLarge(total_slots))?;
        Ok(Self {
            local_slots,
            total_slots,
            frame_bytes: frame_bytes as u32,
        })
    }
}

/// Compiler-local state for one emitted function. `locals` is a stack of
/// slots because IR validation intentionally permits lexical shadowing.
struct EmitContext {
    layout: FrameLayout,
    next_local_slot: usize,
    next_temp_slot: usize,
    locals: BTreeMap<LocalId, Vec<StackSlot>>,
}

impl EmitContext {
    fn new(layout: FrameLayout) -> Self {
        Self {
            next_local_slot: 0,
            next_temp_slot: layout.local_slots,
            layout,
            locals: BTreeMap::new(),
        }
    }

    fn bind(&mut self, local: LocalId) -> StackSlot {
        let slot = StackSlot(self.next_local_slot);
        self.next_local_slot += 1;
        self.locals.entry(local).or_default().push(slot);
        slot
    }

    fn unbind(&mut self, local: LocalId) {
        let bindings = self
            .locals
            .get_mut(&local)
            .expect("code generation only unbinds bindings it introduced");
        bindings.pop();
        if bindings.is_empty() {
            self.locals.remove(&local);
        }
    }

    fn local(&self, local: LocalId) -> Result<StackSlot, CodegenError> {
        self.locals
            .get(&local)
            .and_then(|bindings| bindings.last())
            .copied()
            .ok_or(CodegenError::UnboundLocal(local))
    }

    fn temp(&mut self) -> StackSlot {
        let slot = StackSlot(self.next_temp_slot);
        self.next_temp_slot += 1;
        debug_assert!(self.next_temp_slot <= self.layout.total_slots);
        slot
    }

    fn is_finished(&self) -> bool {
        self.next_local_slot == self.layout.local_slots
            && self.next_temp_slot == self.layout.total_slots
            && self.locals.is_empty()
    }
}

fn count_stmt_bindings(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Let { value, body, .. } => 1 + count_expr_bindings(value) + count_stmt_bindings(body),
        Stmt::If {
            cond, then, els, ..
        } => count_expr_bindings(cond) + count_stmt_bindings(then) + count_stmt_bindings(els),
        Stmt::Return(expr, _) => count_expr_bindings(expr),
    }
}

fn count_stmt_expressions(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Let { value, body, .. } => count_expr_slots(value) + count_stmt_expressions(body),
        Stmt::If {
            cond, then, els, ..
        } => count_expr_slots(cond) + count_stmt_expressions(then) + count_stmt_expressions(els),
        Stmt::Return(expr, _) => count_expr_slots(expr),
    }
}

fn count_expr_bindings(expr: &Expr) -> usize {
    match expr {
        Expr::IntLit(..) | Expr::Param(..) | Expr::Local(..) => 0,
        Expr::WrappingAdd(left, right, _)
        | Expr::WrappingSub(left, right, _)
        | Expr::WrappingMul(left, right, _) => {
            count_expr_bindings(left) + count_expr_bindings(right)
        }
        Expr::NotEqZero(inner, _) => count_expr_bindings(inner),
        Expr::Call(_, args, _) => args.iter().map(count_expr_bindings).sum(),
        Expr::Let { value, body, .. } => 1 + count_expr_bindings(value) + count_expr_bindings(body),
    }
}

/// Every lowered value expression writes one owned i32 result slot.  This is
/// a deliberately straightforward first native lowering contract: it avoids
/// accidental register-lifetime coupling between syntax shape and semantic
/// evaluation order, and it lets a lexical binding name an actual owned
/// location rather than a delegated compiler temporary. An `Expr::Let`
/// itself reuses its body's result slot after materializing its initializer.
fn count_expr_slots(expr: &Expr) -> usize {
    match expr {
        Expr::IntLit(..) | Expr::Param(..) | Expr::Local(..) => 1,
        Expr::WrappingAdd(left, right, _)
        | Expr::WrappingSub(left, right, _)
        | Expr::WrappingMul(left, right, _) => 1 + count_expr_slots(left) + count_expr_slots(right),
        Expr::NotEqZero(inner, _) => 1 + count_expr_slots(inner),
        Expr::Call(_, args, _) => 1 + args.iter().map(count_expr_slots).sum::<usize>(),
        Expr::Let { value, body, .. } => count_expr_slots(value) + count_expr_slots(body),
    }
}

fn emit_stmt(
    code: &mut Vec<u8>,
    stmt: &Stmt,
    context: &mut EmitContext,
) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Return(expr, _) => {
            let result = emit_expr(code, expr, context)?;
            emit_load_slot(code, result, 0);
            emit_epilogue(code);
            Ok(())
        }
        Stmt::Let {
            local, value, body, ..
        } => {
            // Evaluate before binding: the initializer must still observe an
            // outer binding with the same LocalId, just like the interpreter.
            let value = emit_expr(code, value, context)?;
            let slot = context.bind(*local);
            emit_copy_slot(code, value, slot);
            let result = emit_stmt(code, body, context);
            context.unbind(*local);
            result
        }
        Stmt::If { .. } => Err(CodegenError::UnsupportedConstruct("conditional")),
    }
}

fn emit_expr(
    code: &mut Vec<u8>,
    expr: &Expr,
    context: &mut EmitContext,
) -> Result<StackSlot, CodegenError> {
    match expr {
        Expr::IntLit(value, IntWidth::I32, _) => {
            let result = context.temp();
            emit_i32(code, 17, *value as i32);
            emit_store_slot(code, 17, result);
            Ok(result)
        }
        Expr::Param(index, _) => {
            if *index >= 8 {
                return Err(CodegenError::TooManyArguments(*index + 1));
            }
            let result = context.temp();
            emit_store_slot(code, *index as u8, result);
            Ok(result)
        }
        Expr::WrappingAdd(left, right, _) => emit_binary(code, left, right, context, 0x0b00_0000),
        Expr::WrappingSub(left, right, _) => emit_binary(code, left, right, context, 0x4b00_0000),
        Expr::WrappingMul(left, right, _) => emit_binary(code, left, right, context, 0x1b00_7c00),
        Expr::Local(local, _) => {
            let source = context.local(*local)?;
            let result = context.temp();
            emit_copy_slot(code, source, result);
            Ok(result)
        }
        Expr::NotEqZero(..) => Err(CodegenError::UnsupportedConstruct("condition")),
        Expr::Call(..) => Err(CodegenError::UnsupportedConstruct("function call")),
        Expr::Let {
            local, value, body, ..
        } => {
            let value = emit_expr(code, value, context)?;
            let slot = context.bind(*local);
            emit_copy_slot(code, value, slot);
            let result = emit_expr(code, body, context);
            context.unbind(*local);
            result
        }
    }
}

fn emit_binary(
    code: &mut Vec<u8>,
    left: &Expr,
    right: &Expr,
    context: &mut EmitContext,
    opcode: u32,
) -> Result<StackSlot, CodegenError> {
    // Evaluate left then right, matching the owned IR's source order even
    // before calls are lowered.  Each value already has a slot, so a deeply
    // nested expression cannot overwrite an outer operand's scratch register.
    let left = emit_expr(code, left, context)?;
    let right = emit_expr(code, right, context)?;
    let result = context.temp();
    emit_load_slot(code, left, 17);
    emit_load_slot(code, right, 18);
    emit(code, opcode | (18 << 16) | (17 << 5) | 17);
    emit_store_slot(code, 17, result);
    Ok(result)
}

fn emit_prologue(code: &mut Vec<u8>, frame_bytes: u32) {
    // stp x29, x30, [sp, #-16]!; mov x29, sp
    emit(code, 0xa9bf_7bfd);
    emit(code, 0x9100_03fd);
    emit_sub_sp(code, frame_bytes);
    // x16 is caller-saved and no call exists in this lowering subset.  It
    // anchors every local/result slot while keeping sp 16-byte aligned.
    emit(code, 0x9100_03f0); // mov x16, sp
}

fn emit_sub_sp(code: &mut Vec<u8>, mut bytes: u32) {
    while bytes != 0 {
        let chunk = bytes.min(4095);
        emit(code, 0xd100_03ff | (chunk << 10)); // sub sp, sp, #chunk
        bytes -= chunk;
    }
}

fn emit_epilogue(code: &mut Vec<u8>) {
    emit(code, 0x9100_03bf); // mov sp, x29
    emit(code, 0xa8c1_7bfd); // ldp x29, x30, [sp], #16
    emit(code, 0xd65f_03c0); // ret
}

fn emit_load_slot(code: &mut Vec<u8>, slot: StackSlot, register: u8) {
    let offset = u32::try_from(slot.0).expect("frame layout bounds stack slots") << 2;
    emit(
        code,
        0xb940_0000 | (offset << 8) | (16 << 5) | register as u32,
    );
}

fn emit_store_slot(code: &mut Vec<u8>, register: u8, slot: StackSlot) {
    let offset = u32::try_from(slot.0).expect("frame layout bounds stack slots") << 2;
    emit(
        code,
        0xb900_0000 | (offset << 8) | (16 << 5) | register as u32,
    );
}

fn emit_copy_slot(code: &mut Vec<u8>, from: StackSlot, to: StackSlot) {
    emit_load_slot(code, from, 17);
    emit_store_slot(code, 17, to);
}

fn emit(code: &mut Vec<u8>, instruction: u32) {
    code.extend_from_slice(&instruction.to_le_bytes());
}
fn emit_i32(code: &mut Vec<u8>, dst: u8, value: i32) {
    let value = value as u32;
    emit(code, 0x5280_0000 | ((value & 0xffff) << 5) | dst as u32);
    if value >> 16 != 0 {
        emit(code, 0x72a0_0000 | ((value >> 16) << 5) | dst as u32);
    }
}
fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn u64le(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn name(out: &mut Vec<u8>, value: &str) {
    let mut bytes = [0; 16];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    out.extend_from_slice(&bytes);
}

fn write_mach_o_object(text: &[u8], symbol: &str) -> Vec<u8> {
    const HEADER_SIZE: usize = 32;
    const SEGMENT_COMMAND_SIZE: usize = 152;
    const SYMTAB_COMMAND_SIZE: usize = 24;
    const NLIST64_SIZE: usize = 16;
    let section_offset = HEADER_SIZE + SEGMENT_COMMAND_SIZE + SYMTAB_COMMAND_SIZE;
    let symtab_offset = (section_offset + text.len() + 7) & !7;
    let string_table_offset = symtab_offset + NLIST64_SIZE;
    let external_symbol = format!("_{symbol}");
    let string_table_size = external_symbol.len() + 2; // leading and trailing NUL
    let mut image = Vec::with_capacity(string_table_offset + string_table_size);
    u32le(&mut image, 0xfeed_facf);
    u32le(&mut image, 0x0100_000c);
    u32le(&mut image, 0);
    u32le(&mut image, 1);
    u32le(&mut image, 2);
    u32le(
        &mut image,
        (SEGMENT_COMMAND_SIZE + SYMTAB_COMMAND_SIZE) as u32,
    );
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    // LC_SEGMENT_64 __TEXT with one regular, reloc-free __text section.
    u32le(&mut image, 0x19);
    u32le(&mut image, 152);
    name(&mut image, "__TEXT");
    u64le(&mut image, 0);
    u64le(&mut image, 0);
    u64le(&mut image, 0);
    u64le(&mut image, 0);
    u32le(&mut image, 7);
    u32le(&mut image, 5);
    u32le(&mut image, 1);
    u32le(&mut image, 0);
    name(&mut image, "__text");
    name(&mut image, "__TEXT");
    u64le(&mut image, 0);
    u64le(&mut image, text.len() as u64);
    u32le(&mut image, section_offset as u32);
    u32le(&mut image, 2);
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    // S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS.
    u32le(&mut image, 0x8000_0400);
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    // LC_SYMTAB and a single external `N_SECT` symbol. This is the exact
    // native-link interface: the linker sees a symbol emitted by LAMINARIA,
    // not a name inferred from source or delegated compiler output.
    u32le(&mut image, 0x2);
    u32le(&mut image, SYMTAB_COMMAND_SIZE as u32);
    u32le(&mut image, symtab_offset as u32);
    u32le(&mut image, 1);
    u32le(&mut image, string_table_offset as u32);
    u32le(&mut image, string_table_size as u32);
    image.extend_from_slice(text);
    image.resize(symtab_offset, 0);
    u32le(&mut image, 1); // string-table index of the external symbol
    image.push(0x0f); // N_SECT | N_EXT
    image.push(1); // __text section
    image.extend_from_slice(&0_u16.to_le_bytes());
    u64le(&mut image, 0); // offset within a relocatable object
    image.push(0);
    image.extend_from_slice(external_symbol.as_bytes());
    image.push(0);
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_frontend::lower_rust_source;
    use crate::types::{FnFact, Program, Provenance, SourceLanguage, SourcePosition, SourceSpan};
    use crate::validate::validate_program;
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    use std::process::Command;

    #[test]
    fn emits_a_mach_o_object_from_real_rust_source() {
        let source = "fn add(a: i32, b: i32) -> i32 { a.wrapping_add(b) }";
        let program = lower_rust_source(std::path::Path::new("add.rs"), source, &["add"]).unwrap();
        let image = generate_object(&validate_program(&program).unwrap(), "add").unwrap();
        assert_eq!(&image[..4], &[0xcf, 0xfa, 0xed, 0xfe]);
        assert_eq!(u32::from_le_bytes(image[12..16].try_into().unwrap()), 1);
        assert!(image
            .windows(b"_add\0".len())
            .any(|window| window == b"_add\0"));
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn source_let_bindings_link_and_launch_from_an_owned_object() {
        // This is real Rust source accepted by the owned frontend.  Its
        // expected exit status follows directly from (2 + 3) * 4 + (2 + 3),
        // rather than from another implementation's generated output.
        let source = r#"
            fn main() -> i32 {
                let first = 2i32.wrapping_add(3);
                let second = first.wrapping_mul(4);
                second.wrapping_add(first)
            }
        "#;
        let program =
            lower_rust_source(std::path::Path::new("main.rs"), source, &["main"]).unwrap();
        assert_owned_object_exits(&validate_program(&program).unwrap(), 25);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn expression_let_shadowing_preserves_the_outer_value_until_binding() {
        // `Expr::Let` is introduced by owned transforms rather than directly
        // by this Rust frontend.  Exercise it as production IR, including a
        // deliberately identical LocalId on both binding levels: the inner
        // initializer must observe 4 and only its body must observe 7.
        let provenance = test_provenance();
        let mut program = Program::default();
        program.insert(FnFact {
            name: "main".to_owned(),
            params: vec![],
            return_width: IntWidth::I32,
            provenance: provenance.clone(),
            body: Stmt::Let {
                local: LocalId(0),
                value: Expr::IntLit(4, IntWidth::I32, provenance.clone()),
                body: Box::new(Stmt::Return(
                    Expr::Let {
                        local: LocalId(0),
                        value: Box::new(Expr::WrappingAdd(
                            Box::new(Expr::Local(LocalId(0), provenance.clone())),
                            Box::new(Expr::IntLit(3, IntWidth::I32, provenance.clone())),
                            provenance.clone(),
                        )),
                        body: Box::new(Expr::WrappingMul(
                            Box::new(Expr::Local(LocalId(0), provenance.clone())),
                            Box::new(Expr::IntLit(2, IntWidth::I32, provenance.clone())),
                            provenance.clone(),
                        )),
                        provenance: provenance.clone(),
                    },
                    provenance.clone(),
                )),
                provenance,
            },
        });
        assert_owned_object_exits(&validate_program(&program).unwrap(), 14);
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    fn assert_owned_object_exits(program: &ValidatedProgram, expected: i32) {
        let root = std::env::temp_dir().join(format!(
            "laminaria-aarch64-let-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&root).unwrap();
        let object = root.join("laminaria-main.o");
        let executable = root.join("laminaria-main");
        std::fs::write(&object, generate_object(program, "main").unwrap()).unwrap();

        let sdk = Command::new("xcrun")
            .arg("--show-sdk-path")
            .output()
            .expect("xcrun must be available on the declared Darwin target");
        assert!(sdk.status.success());
        let sdk = String::from_utf8(sdk.stdout).unwrap();
        let status = Command::new("/usr/bin/ld")
            .args([
                "-dynamic",
                "-arch",
                "arm64",
                "-platform_version",
                "macos",
                "11.0",
                "11.0",
                "-syslibroot",
                sdk.trim(),
                "-o",
            ])
            .arg(&executable)
            .arg(&object)
            .arg("-lSystem")
            .status()
            .expect("the declared Darwin linker must launch");
        assert!(status.success());
        let exit_status = Command::new(&executable).status().unwrap().code();
        let _ = std::fs::remove_dir_all(root);
        assert_eq!(exit_status, Some(expected));
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    fn test_provenance() -> Provenance {
        Provenance {
            source_file: std::path::PathBuf::from("aarch64-let-test"),
            span: SourceSpan {
                start: SourcePosition { line: 1, column: 1 },
                end: SourcePosition { line: 1, column: 1 },
            },
            language: SourceLanguage::Rust,
        }
    }
}

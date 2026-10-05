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
    BranchOutOfRange { from: usize, target: usize },
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
            Self::BranchOutOfRange { from, target } => write!(
                f,
                "aarch64-darwin target: conditional branch from {from:#x} to {target:#x} is out of range"
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

/// Produces a reloc-free ARM64 Mach-O object containing every function in the
/// validated program. The object is the target-code artifact; a later,
/// explicit Darwin runtime and link contract turns it into a process
/// executable. Calls among those emitted functions are patched directly by
/// LAMINARIA, so they neither invoke nor depend on an assembler or linker
/// relocation for their target.
pub fn generate_object(program: &ValidatedProgram, entry: &str) -> Result<Vec<u8>, CodegenError> {
    if !program.program().functions.contains_key(entry) {
        return Err(CodegenError::MissingEntry(entry.to_owned()));
    }

    let mut text = Vec::new();
    let mut symbols = BTreeMap::new();
    let mut calls = Vec::new();
    for (name, function) in &program.program().functions {
        if function.params.len() > 8 {
            return Err(CodegenError::TooManyArguments(function.params.len()));
        }
        symbols.insert(name.clone(), text.len());
        let layout = FrameLayout::for_function(function.params.len(), &function.body)?;
        emit_prologue(&mut text, layout.frame_bytes);
        let mut context = EmitContext::new(layout);
        emit_parameter_spills(&mut text, function.params.len());
        // AArch64 receives integer arguments in w0..w7. Every expression
        // result lives in this function's owned frame before a consumer reads
        // it; x19 anchors that frame across nested calls under AAPCS64.
        emit_stmt(&mut text, &function.body, &mut context, &mut calls)?;
        debug_assert!(context.is_finished());
    }
    for call in calls {
        let target = *symbols
            .get(&call.callee)
            .expect("validated programs only call functions in this object");
        patch_bl(&mut text, call.offset, target)?;
    }
    Ok(write_mach_o_object(&text, &symbols))
}

struct PendingCall {
    offset: usize,
    callee: String,
}

#[derive(Debug, Clone, Copy)]
struct StackSlot(usize);

/// The first part of a frame owns incoming ABI parameters, followed by one
/// slot per lexical binding occurrence; the remainder holds materialized
/// expression results. Binding occurrences, rather than `LocalId` values,
/// receive slots so even a manually-built IR tree which shadows an identical
/// `LocalId` preserves the interpreter's save-and-restore semantics.
#[derive(Debug, Clone, Copy)]
struct FrameLayout {
    param_slots: usize,
    local_slots: usize,
    total_slots: usize,
    frame_bytes: u32,
}

impl FrameLayout {
    fn for_function(param_slots: usize, stmt: &Stmt) -> Result<Self, CodegenError> {
        let local_slots = count_stmt_bindings(stmt);
        let total_slots = param_slots + local_slots + count_stmt_expressions(stmt);
        // A 32-bit LDR/STR with an unsigned immediate can address 4096
        // four-byte slots from x19. Do not silently synthesize a different
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
            param_slots,
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
            next_temp_slot: layout.param_slots + layout.local_slots,
            layout,
            locals: BTreeMap::new(),
        }
    }

    fn bind(&mut self, local: LocalId) -> StackSlot {
        let slot = StackSlot(self.layout.param_slots + self.next_local_slot);
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

    fn parameter(&self, index: usize) -> Result<StackSlot, CodegenError> {
        if index < self.layout.param_slots {
            Ok(StackSlot(index))
        } else {
            Err(CodegenError::TooManyArguments(index + 1))
        }
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
    calls: &mut Vec<PendingCall>,
) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Return(expr, _) => {
            let result = emit_expr(code, expr, context, calls)?;
            emit_load_slot(code, result, 0);
            emit_epilogue(code, context.layout.frame_bytes);
            Ok(())
        }
        Stmt::Let {
            local, value, body, ..
        } => {
            // Evaluate before binding: the initializer must still observe an
            // outer binding with the same LocalId, just like the interpreter.
            let value = emit_expr(code, value, context, calls)?;
            let slot = context.bind(*local);
            emit_copy_slot(code, value, slot);
            let result = emit_stmt(code, body, context, calls);
            context.unbind(*local);
            result
        }
        Stmt::If {
            cond, then, els, ..
        } => {
            // The validated IR permits `NotEqZero` only as an `if`
            // condition. Materialize it like every other expression, then
            // branch directly on its owned stack slot. Each branch ends in
            // `ret` by construction of `Stmt`, so no compensating jump is
            // needed after the `then` body.
            let condition = emit_expr(code, cond, context, calls)?;
            emit_load_slot(code, condition, 17);
            let else_branch = emit_cbz_placeholder(code, 17);
            emit_stmt(code, then, context, calls)?;
            let else_offset = code.len();
            patch_cbz(code, else_branch, else_offset)?;
            emit_stmt(code, els, context, calls)
        }
    }
}

fn emit_expr(
    code: &mut Vec<u8>,
    expr: &Expr,
    context: &mut EmitContext,
    calls: &mut Vec<PendingCall>,
) -> Result<StackSlot, CodegenError> {
    match expr {
        Expr::IntLit(value, IntWidth::I32, _) => {
            let result = context.temp();
            emit_i32(code, 17, *value as i32);
            emit_store_slot(code, 17, result);
            Ok(result)
        }
        Expr::Param(index, _) => {
            let parameter = context.parameter(*index)?;
            let result = context.temp();
            emit_copy_slot(code, parameter, result);
            Ok(result)
        }
        Expr::WrappingAdd(left, right, _) => {
            emit_binary(code, left, right, context, calls, 0x0b00_0000)
        }
        Expr::WrappingSub(left, right, _) => {
            emit_binary(code, left, right, context, calls, 0x4b00_0000)
        }
        Expr::WrappingMul(left, right, _) => {
            emit_binary(code, left, right, context, calls, 0x1b00_7c00)
        }
        Expr::Local(local, _) => {
            let source = context.local(*local)?;
            let result = context.temp();
            emit_copy_slot(code, source, result);
            Ok(result)
        }
        Expr::NotEqZero(inner, _) => {
            let inner = emit_expr(code, inner, context, calls)?;
            let result = context.temp();
            emit_load_slot(code, inner, 17);
            // `cmp w17, #0; cset w17, ne`. The condition code is encoded as
            // NE (0001), matching the IR operation exactly.
            emit(code, 0x7100_023f);
            emit(code, 0x1a9f_07f1);
            emit_store_slot(code, 17, result);
            Ok(result)
        }
        Expr::Call(callee, args, _) => {
            if args.len() > 8 {
                return Err(CodegenError::TooManyArguments(args.len()));
            }
            // Source order is semantically observable once nested calls are
            // available. Materialize every argument first, then load the ABI
            // registers immediately before BL so a later argument cannot
            // overwrite an earlier value.
            let mut values = Vec::with_capacity(args.len());
            for argument in args {
                values.push(emit_expr(code, argument, context, calls)?);
            }
            for (index, value) in values.into_iter().enumerate() {
                emit_load_slot(code, value, index as u8);
            }
            let offset = code.len();
            emit(code, 0x9400_0000); // bl <same-object target>, patched below
            calls.push(PendingCall {
                offset,
                callee: callee.0.clone(),
            });
            let result = context.temp();
            emit_store_slot(code, 0, result);
            Ok(result)
        }
        Expr::Let {
            local, value, body, ..
        } => {
            let value = emit_expr(code, value, context, calls)?;
            let slot = context.bind(*local);
            emit_copy_slot(code, value, slot);
            let result = emit_expr(code, body, context, calls);
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
    calls: &mut Vec<PendingCall>,
    opcode: u32,
) -> Result<StackSlot, CodegenError> {
    // Evaluate left then right, matching the owned IR's source order even
    // before calls are lowered.  Each value already has a slot, so a deeply
    // nested expression cannot overwrite an outer operand's scratch register.
    let left = emit_expr(code, left, context, calls)?;
    let right = emit_expr(code, right, context, calls)?;
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
    // `x19` is callee-saved under AAPCS64. Keep the owned frame base there,
    // rather than in x16/x17, because calls are now a normal lowering
    // operation. The extra 16-byte decrement preserves the ABI's required
    // 16-byte SP alignment while giving us an aligned save slot for x19.
    emit_sub_sp(code, 16);
    emit_store_u64_sp(code, 19, frame_bytes);
    emit(code, 0x9100_03f3); // mov x19, sp
}

/// Preserve every incoming ABI integer argument before any expression can
/// issue a nested call. A later `BL` is allowed to overwrite w0..w7, while
/// `Expr::Param` always reads the owned copies in x19's frame.
fn emit_parameter_spills(code: &mut Vec<u8>, count: usize) {
    debug_assert!(count <= 8);
    for register in 0..count {
        emit_store_slot(code, register as u8, StackSlot(register));
    }
}

fn emit_sub_sp(code: &mut Vec<u8>, mut bytes: u32) {
    while bytes != 0 {
        let chunk = bytes.min(4095);
        emit(code, 0xd100_03ff | (chunk << 10)); // sub sp, sp, #chunk
        bytes -= chunk;
    }
}

fn emit_epilogue(code: &mut Vec<u8>, frame_bytes: u32) {
    emit_load_u64_sp(code, frame_bytes, 19);
    emit(code, 0x9100_03bf); // mov sp, x29
    emit(code, 0xa8c1_7bfd); // ldp x29, x30, [sp], #16
    emit(code, 0xd65f_03c0); // ret
}

fn emit_load_slot(code: &mut Vec<u8>, slot: StackSlot, register: u8) {
    let offset = u32::try_from(slot.0).expect("frame layout bounds stack slots") << 2;
    emit(
        code,
        0xb940_0000 | (offset << 8) | (19 << 5) | register as u32,
    );
}

fn emit_store_slot(code: &mut Vec<u8>, register: u8, slot: StackSlot) {
    let offset = u32::try_from(slot.0).expect("frame layout bounds stack slots") << 2;
    emit(
        code,
        0xb900_0000 | (offset << 8) | (19 << 5) | register as u32,
    );
}

fn emit_copy_slot(code: &mut Vec<u8>, from: StackSlot, to: StackSlot) {
    emit_load_slot(code, from, 17);
    emit_store_slot(code, 17, to);
}

fn emit_store_u64_sp(code: &mut Vec<u8>, register: u8, byte_offset: u32) {
    debug_assert_eq!(byte_offset % 8, 0);
    emit(
        code,
        0xf900_03e0 | ((byte_offset / 8) << 10) | register as u32,
    );
}

fn emit_load_u64_sp(code: &mut Vec<u8>, byte_offset: u32, register: u8) {
    debug_assert_eq!(byte_offset % 8, 0);
    emit(
        code,
        0xf940_03e0 | ((byte_offset / 8) << 10) | register as u32,
    );
}

/// Emits `cbz w<register>, <target>` with a later-bound target and returns
/// the byte offset of the instruction. `imm19` is PC-relative in four-byte
/// units; keeping the object reloc-free is correct because both branch
/// endpoints belong to the same generated function.
fn emit_cbz_placeholder(code: &mut Vec<u8>, register: u8) -> usize {
    let offset = code.len();
    emit(code, 0x3400_0000 | register as u32);
    offset
}

fn patch_cbz(code: &mut [u8], from: usize, target: usize) -> Result<(), CodegenError> {
    let distance = target as isize - from as isize;
    let words = distance / 4;
    if distance % 4 != 0 || !(-(1 << 18)..(1 << 18)).contains(&words) {
        return Err(CodegenError::BranchOutOfRange { from, target });
    }
    let mut instruction = u32::from_le_bytes(
        code[from..from + 4]
            .try_into()
            .expect("branch instruction is fully emitted before patching"),
    );
    instruction |= ((words as i32 as u32) & 0x7ffff) << 5;
    code[from..from + 4].copy_from_slice(&instruction.to_le_bytes());
    Ok(())
}

/// Patch a `BL` whose destination is another function in this generated
/// `__text` section. AArch64 `imm26` is PC-relative in instruction words;
/// this never becomes a Mach-O relocation because both endpoints are owned
/// by the same emitted object.
fn patch_bl(code: &mut [u8], from: usize, target: usize) -> Result<(), CodegenError> {
    let distance = target as isize - from as isize;
    let words = distance / 4;
    if distance % 4 != 0 || !(-(1 << 25)..(1 << 25)).contains(&words) {
        return Err(CodegenError::BranchOutOfRange { from, target });
    }
    let mut instruction = u32::from_le_bytes(
        code[from..from + 4]
            .try_into()
            .expect("branch instruction is fully emitted before patching"),
    );
    instruction |= (words as i32 as u32) & 0x03ff_ffff;
    code[from..from + 4].copy_from_slice(&instruction.to_le_bytes());
    Ok(())
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

fn write_mach_o_object(text: &[u8], symbols: &BTreeMap<String, usize>) -> Vec<u8> {
    const HEADER_SIZE: usize = 32;
    const SEGMENT_COMMAND_SIZE: usize = 152;
    const SYMTAB_COMMAND_SIZE: usize = 24;
    const NLIST64_SIZE: usize = 16;
    let section_offset = HEADER_SIZE + SEGMENT_COMMAND_SIZE + SYMTAB_COMMAND_SIZE;
    let symtab_offset = (section_offset + text.len() + 7) & !7;
    let string_table_offset = symtab_offset + NLIST64_SIZE * symbols.len();
    let external_symbols: Vec<_> = symbols
        .iter()
        .map(|(name, offset)| (format!("_{name}"), *offset))
        .collect();
    let string_table_size = 1 + external_symbols
        .iter()
        .map(|(symbol, _)| symbol.len() + 1)
        .sum::<usize>();
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
    // LC_SYMTAB and one external `N_SECT` symbol per owned function. This is
    // the native-link interface: the linker sees names and section offsets
    // emitted by LAMINARIA, not delegated compiler output.
    u32le(&mut image, 0x2);
    u32le(&mut image, SYMTAB_COMMAND_SIZE as u32);
    u32le(&mut image, symtab_offset as u32);
    u32le(&mut image, external_symbols.len() as u32);
    u32le(&mut image, string_table_offset as u32);
    u32le(&mut image, string_table_size as u32);
    image.extend_from_slice(text);
    image.resize(symtab_offset, 0);
    let mut string_index = 1_u32;
    for (symbol, offset) in external_symbols {
        u32le(&mut image, string_index);
        image.push(0x0f); // N_SECT | N_EXT
        image.push(1); // __text section
        image.extend_from_slice(&0_u16.to_le_bytes());
        u64le(&mut image, offset as u64); // section offset in MH_OBJECT
        string_index += (symbol.len() + 1) as u32;
    }
    image.push(0); // string-table index zero is the empty name
    for (symbol, _) in symbols
        .iter()
        .map(|(name, offset)| (format!("_{name}"), offset))
    {
        image.extend_from_slice(symbol.as_bytes());
        image.push(0);
    }
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
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    use std::sync::{Mutex, OnceLock};

    #[test]
    fn emits_a_mach_o_object_from_real_rust_source() {
        let source = r#"
            fn add(a: i32, b: i32) -> i32 { a.wrapping_add(b) }
            fn main() -> i32 { add(2, 3) }
        "#;
        let program =
            lower_rust_source(std::path::Path::new("add.rs"), source, &["add", "main"]).unwrap();
        let image = generate_object(&validate_program(&program).unwrap(), "main").unwrap();
        assert_eq!(&image[..4], &[0xcf, 0xfa, 0xed, 0xfe]);
        assert_eq!(u32::from_le_bytes(image[12..16].try_into().unwrap()), 1);
        // `LC_SYMTAB.nsyms`: both owned functions are external `N_SECT`
        // entries, giving a later native link a complete object interface.
        assert_eq!(u32::from_le_bytes(image[196..200].try_into().unwrap()), 2);
        assert!(image
            .windows(b"_add\0".len())
            .any(|window| window == b"_add\0"));
        assert!(image
            .windows(b"_main\0".len())
            .any(|window| window == b"_main\0"));
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
    fn source_conditionals_link_and_launch_from_an_owned_object() {
        // These sources cover both emitted CBZ paths. The truth values are
        // derived in owned arithmetic before the condition, so this tests
        // expression materialization, branch patching, and both tail bodies.
        for (source, expected) in [
            (
                r#"fn main() -> i32 {
                    if 3i32.wrapping_sub(2) != 0 { 27 } else { 11 }
                }"#,
                27,
            ),
            (
                r#"fn main() -> i32 {
                    if 3i32.wrapping_sub(3) != 0 { 27 } else { 11 }
                }"#,
                11,
            ),
        ] {
            let program =
                lower_rust_source(std::path::Path::new("main.rs"), source, &["main"]).unwrap();
            assert_owned_object_exits(&validate_program(&program).unwrap(), expected);
        }
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn source_calls_preserve_parameters_across_nested_calls_and_link_as_one_object() {
        // `combine` needs its incoming `x` after the nested `increment(x)`
        // has used w0. `main` also supplies two call-valued arguments to
        // `pack` in written left-to-right order. This is an end-to-end owned
        // source -> object -> ld -> executable regression, not a compiler
        // output comparison.
        let source = r#"
            fn increment(x: i32) -> i32 { x.wrapping_add(1) }
            fn combine(x: i32) -> i32 { x.wrapping_add(increment(x)) }
            fn pack(left: i32, right: i32) -> i32 {
                left.wrapping_mul(10).wrapping_add(right)
            }
            fn main() -> i32 { pack(combine(3), increment(4)) }
        "#;
        let program = lower_rust_source(
            std::path::Path::new("calls.rs"),
            source,
            &["increment", "combine", "pack", "main"],
        )
        .unwrap();
        assert_owned_object_exits(&validate_program(&program).unwrap(), 75);
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
        // Darwin's native linker is an external integration boundary, and
        // these executable tests launch it against temporary Mach-O paths.
        // Serialize that boundary so parallel unit tests cannot cross-talk
        // through the platform linker or process-launch environment.
        let _linker = native_link_lock().lock().unwrap();
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
    fn native_link_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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

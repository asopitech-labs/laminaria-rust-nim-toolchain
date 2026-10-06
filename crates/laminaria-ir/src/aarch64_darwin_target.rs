//! Owned AArch64/Mach-O code generation.  No external compiler, assembler, or
//! linker participates in this module.

use std::collections::{BTreeMap, BTreeSet};

use crate::types::{Expr, ExternalTarget, IntWidth, LocalId, Stmt};
use crate::validate::ValidatedProgram;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    MissingEntry(String),
    EntrySymbolCollision { entry: String },
    InvalidMacosVersion { version: String },
    TooManyArguments(usize),
    StackFrameTooLarge(usize),
    BranchOutOfRange { from: usize, target: usize },
    UnboundLocal(LocalId),
    ExternalSymbolCollision(String),
    UnsupportedConstruct(&'static str),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEntry(entry) => {
                write!(f, "aarch64-darwin target: missing entry {entry:?}")
            }
            Self::EntrySymbolCollision { entry } => write!(
                f,
                "aarch64-darwin target: entry {entry:?} would collide with a separately emitted source `main` symbol"
            ),
            Self::InvalidMacosVersion { version } => write!(
                f,
                "aarch64-darwin target: deployment target {version:?} must be X.Y or X.Y.Z with a 16-bit major and 8-bit minor/patch components"
            ),
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
            Self::ExternalSymbolCollision(symbol) => write!(
                f,
                "aarch64-darwin target: external symbol {symbol:?} collides with an owned definition"
            ),
            Self::UnsupportedConstruct(kind) => write!(
                f,
                "aarch64-darwin target: native core does not yet lower {kind}"
            ),
        }
    }
}
impl std::error::Error for CodegenError {}

/// A macOS deployment target in the exact packed form used by Mach-O's
/// `LC_BUILD_VERSION` and Darwin `ld -platform_version`. Keeping this as a
/// typed target prevents the object metadata and the declared link action from
/// independently interpreting one caller-provided version string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacosVersion(u32);

impl MacosVersion {
    pub const DEFAULT: Self = Self(11 << 16);

    /// Parses the `X.Y` or `X.Y.Z` grammar accepted by the declared Darwin
    /// linker. The packed result is `xxxx.yy.zz`, as specified by cctools'
    /// `build_version_command` and implemented by ld64's
    /// `parsePackedVersion32`.
    pub fn parse(version: &str) -> Result<Self, CodegenError> {
        let invalid = || CodegenError::InvalidMacosVersion {
            version: version.to_owned(),
        };
        let mut components = version.split('.');
        let major = parse_version_component(components.next().ok_or_else(invalid)?, 0xffff)
            .ok_or_else(invalid)?;
        let minor = parse_version_component(components.next().ok_or_else(invalid)?, 0xff)
            .ok_or_else(invalid)?;
        let patch = match components.next() {
            Some(component) => parse_version_component(component, 0xff).ok_or_else(invalid)?,
            None => 0,
        };
        if components.next().is_some() {
            return Err(invalid());
        }
        Ok(Self((major << 16) | (minor << 8) | patch))
    }

    pub fn packed(self) -> u32 {
        self.0
    }
}

fn parse_version_component(component: &str, maximum: u32) -> Option<u32> {
    if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = component.parse().ok()?;
    (value <= maximum).then_some(value)
}

/// Produces an ARM64 Mach-O object containing every function in the validated
/// program. Calls among those emitted functions are patched directly by
/// LAMINARIA; a closed set of declared Darwin external targets is represented
/// with real section relocations and undefined symbols for the native linker.
pub fn generate_object(program: &ValidatedProgram, entry: &str) -> Result<Vec<u8>, CodegenError> {
    generate_object_for_macos(program, entry, MacosVersion::DEFAULT)
}

/// Produces an ARM64 Mach-O object for one parsed macOS deployment target.
/// The emitted `LC_BUILD_VERSION` is a fact about this input object, rather
/// than a linker-side default inferred later from the host.
pub fn generate_object_for_macos(
    program: &ValidatedProgram,
    entry: &str,
    deployment_target: MacosVersion,
) -> Result<Vec<u8>, CodegenError> {
    if !program.program().functions.contains_key(entry) {
        return Err(CodegenError::MissingEntry(entry.to_owned()));
    }
    if entry != "main" && program.program().functions.contains_key("main") {
        return Err(CodegenError::EntrySymbolCollision {
            entry: entry.to_owned(),
        });
    }

    let mut text = Vec::new();
    let mut symbols = BTreeMap::new();
    let mut branches = Vec::new();
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
        emit_stmt(&mut text, &function.body, &mut context, &mut branches)?;
        debug_assert!(context.is_finished());
    }
    let mut relocations = Vec::new();
    for branch in branches {
        match branch.target {
            BranchTarget::Internal(callee) => {
                let target = *symbols
                    .get(&callee)
                    .expect("validated programs only call functions in this object");
                patch_bl(&mut text, branch.offset, target)?;
            }
            BranchTarget::External(target) => relocations.push(ExternalRelocation {
                offset: branch.offset,
                symbol: darwin_external_symbol(target).to_owned(),
            }),
        }
    }
    write_mach_o_object(&text, &symbols, entry, &relocations, deployment_target)
}

struct PendingBranch {
    offset: usize,
    target: BranchTarget,
}

enum BranchTarget {
    Internal(String),
    External(ExternalTarget),
}

struct ExternalRelocation {
    offset: usize,
    symbol: String,
}

fn darwin_external_symbol(target: ExternalTarget) -> &'static str {
    match target {
        // Darwin's object-file spelling applies the leading underscore to
        // C's `exit`; it does not select POSIX's distinct `_exit` API.
        ExternalTarget::ProcessExit => "_exit",
    }
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
        Stmt::ExternalCall { args, .. } => args.iter().map(count_expr_bindings).sum(),
    }
}

fn count_stmt_expressions(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Let { value, body, .. } => count_expr_slots(value) + count_stmt_expressions(body),
        Stmt::If {
            cond, then, els, ..
        } => count_expr_slots(cond) + count_stmt_expressions(then) + count_stmt_expressions(els),
        Stmt::Return(expr, _) => count_expr_slots(expr),
        Stmt::ExternalCall { args, .. } => args.iter().map(count_expr_slots).sum(),
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
    branches: &mut Vec<PendingBranch>,
) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Return(expr, _) => {
            let result = emit_expr(code, expr, context, branches)?;
            emit_load_slot(code, result, 0);
            emit_epilogue(code, context.layout.frame_bytes);
            Ok(())
        }
        Stmt::Let {
            local, value, body, ..
        } => {
            // Evaluate before binding: the initializer must still observe an
            // outer binding with the same LocalId, just like the interpreter.
            let value = emit_expr(code, value, context, branches)?;
            let slot = context.bind(*local);
            emit_copy_slot(code, value, slot);
            let result = emit_stmt(code, body, context, branches);
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
            let condition = emit_expr(code, cond, context, branches)?;
            emit_load_slot(code, condition, 17);
            let else_branch = emit_cbz_placeholder(code, 17);
            emit_stmt(code, then, context, branches)?;
            let else_offset = code.len();
            patch_cbz(code, else_branch, else_offset)?;
            emit_stmt(code, els, context, branches)
        }
        Stmt::ExternalCall { target, args, .. } => {
            debug_assert_eq!(args.len(), 1, "validated external call arity");
            let status = emit_expr(code, &args[0], context, branches)?;
            emit_load_slot(code, status, 0);
            let offset = code.len();
            emit(code, 0x9400_0000); // bl <external target>, relocated by ld
            branches.push(PendingBranch {
                offset,
                target: BranchTarget::External(*target),
            });
            // `ProcessExit` is specified not to return.  Make an ABI or
            // linker contract violation fail closed rather than accidentally
            // falling through into another emitted function.
            emit(code, 0xd420_0000); // brk #0
            Ok(())
        }
    }
}

fn emit_expr(
    code: &mut Vec<u8>,
    expr: &Expr,
    context: &mut EmitContext,
    branches: &mut Vec<PendingBranch>,
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
            emit_binary(code, left, right, context, branches, 0x0b00_0000)
        }
        Expr::WrappingSub(left, right, _) => {
            emit_binary(code, left, right, context, branches, 0x4b00_0000)
        }
        Expr::WrappingMul(left, right, _) => {
            emit_binary(code, left, right, context, branches, 0x1b00_7c00)
        }
        Expr::Local(local, _) => {
            let source = context.local(*local)?;
            let result = context.temp();
            emit_copy_slot(code, source, result);
            Ok(result)
        }
        Expr::NotEqZero(inner, _) => {
            let inner = emit_expr(code, inner, context, branches)?;
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
                values.push(emit_expr(code, argument, context, branches)?);
            }
            for (index, value) in values.into_iter().enumerate() {
                emit_load_slot(code, value, index as u8);
            }
            let offset = code.len();
            emit(code, 0x9400_0000); // bl <same-object target>, patched below
            branches.push(PendingBranch {
                offset,
                target: BranchTarget::Internal(callee.0.clone()),
            });
            let result = context.temp();
            emit_store_slot(code, 0, result);
            Ok(result)
        }
        Expr::Let {
            local, value, body, ..
        } => {
            let value = emit_expr(code, value, context, branches)?;
            let slot = context.bind(*local);
            emit_copy_slot(code, value, slot);
            let result = emit_expr(code, body, context, branches);
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
    branches: &mut Vec<PendingBranch>,
    opcode: u32,
) -> Result<StackSlot, CodegenError> {
    // Evaluate left then right, matching the owned IR's source order even
    // before calls are lowered.  Each value already has a slot, so a deeply
    // nested expression cannot overwrite an outer operand's scratch register.
    let left = emit_expr(code, left, context, branches)?;
    let right = emit_expr(code, right, context, branches)?;
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

fn write_mach_o_object(
    text: &[u8],
    symbols: &BTreeMap<String, usize>,
    entry: &str,
    relocations: &[ExternalRelocation],
    deployment_target: MacosVersion,
) -> Result<Vec<u8>, CodegenError> {
    const HEADER_SIZE: usize = 32;
    const SEGMENT_COMMAND_SIZE: usize = 152;
    const BUILD_VERSION_COMMAND_SIZE: usize = 24;
    const SYMTAB_COMMAND_SIZE: usize = 24;
    const NLIST64_SIZE: usize = 16;
    let section_offset =
        HEADER_SIZE + SEGMENT_COMMAND_SIZE + BUILD_VERSION_COMMAND_SIZE + SYMTAB_COMMAND_SIZE;
    let defined_symbols: Vec<_> = symbols
        .iter()
        .map(|(name, offset)| {
            (
                if name == entry {
                    "_main".to_owned()
                } else {
                    format!("_{name}")
                },
                *offset,
            )
        })
        .collect();
    let undefined_symbols: Vec<_> = relocations
        .iter()
        .map(|relocation| relocation.symbol.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    for symbol in &undefined_symbols {
        if defined_symbols.iter().any(|(defined, _)| defined == symbol) {
            return Err(CodegenError::ExternalSymbolCollision(symbol.clone()));
        }
    }
    let relocation_offset = (section_offset + text.len() + 3) & !3;
    let symtab_offset = (relocation_offset + relocations.len() * 8 + 7) & !7;
    let string_table_offset =
        symtab_offset + NLIST64_SIZE * (defined_symbols.len() + undefined_symbols.len());
    let string_table_size = 1
        + defined_symbols
            .iter()
            .map(|(symbol, _)| symbol.len() + 1)
            .sum::<usize>()
        + undefined_symbols
            .iter()
            .map(|symbol| symbol.len() + 1)
            .sum::<usize>();
    let symbol_indexes: BTreeMap<_, _> = defined_symbols
        .iter()
        .map(|(symbol, _)| symbol.clone())
        .chain(undefined_symbols.iter().cloned())
        .enumerate()
        .map(|(index, symbol)| (symbol, index as u32))
        .collect();
    let mut image = Vec::with_capacity(string_table_offset + string_table_size);
    u32le(&mut image, 0xfeed_facf);
    u32le(&mut image, 0x0100_000c);
    u32le(&mut image, 0);
    u32le(&mut image, 1);
    u32le(&mut image, 3);
    u32le(
        &mut image,
        (SEGMENT_COMMAND_SIZE + BUILD_VERSION_COMMAND_SIZE + SYMTAB_COMMAND_SIZE) as u32,
    );
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    // LC_SEGMENT_64 __TEXT with one regular __text section.  Relocations, if
    // any, stay section-owned in this MH_OBJECT; they do not need a dylib-only
    // LC_DYSYMTAB table.
    u32le(&mut image, 0x19);
    u32le(&mut image, 152);
    name(&mut image, "__TEXT");
    u64le(&mut image, 0);
    // In an MH_OBJECT, cctools lays section addresses relative to zero and
    // gives the one synthetic segment exactly the text extent. Its file range
    // begins after the Mach-O header/load commands where __text is written.
    // The LC_BUILD_VERSION path makes ld64 validate this containment instead
    // of accepting the former all-zero segment as a legacy object.
    u64le(&mut image, text.len() as u64);
    u64le(&mut image, section_offset as u64);
    u64le(&mut image, text.len() as u64);
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
    u32le(
        &mut image,
        if relocations.is_empty() {
            0
        } else {
            relocation_offset as u32
        },
    );
    u32le(&mut image, relocations.len() as u32);
    // S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS.
    u32le(&mut image, 0x8000_0400);
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    // LC_BUILD_VERSION is present in every arm64 object we emit. The
    // deployment target is parsed once by the explicit link boundary, then
    // applied both here and to `ld -platform_version`; neither side guesses a
    // host default.
    u32le(&mut image, 0x32);
    u32le(&mut image, BUILD_VERSION_COMMAND_SIZE as u32);
    u32le(&mut image, 1); // PLATFORM_MACOS
    u32le(&mut image, deployment_target.packed());
    u32le(&mut image, deployment_target.packed());
    u32le(&mut image, 0); // no build-tool records

    // LC_SYMTAB carries owned external definitions followed by undefined
    // external references. This is the native-link interface: the linker sees
    // names, section offsets, and relocation targets emitted by LAMINARIA,
    // not delegated compiler output.
    u32le(&mut image, 0x2);
    u32le(&mut image, SYMTAB_COMMAND_SIZE as u32);
    u32le(&mut image, symtab_offset as u32);
    u32le(
        &mut image,
        (defined_symbols.len() + undefined_symbols.len()) as u32,
    );
    u32le(&mut image, string_table_offset as u32);
    u32le(&mut image, string_table_size as u32);
    image.extend_from_slice(text);
    image.resize(relocation_offset, 0);
    for relocation in relocations {
        let symbol_index = symbol_indexes[&relocation.symbol];
        // `relocation_info` from cctools: section-relative `r_address`, then
        // r_symbolnum:24 | r_pcrel:1 | r_length:2 | r_extern:1 | r_type:4.
        // ARM64_RELOC_BRANCH26 is enum value 2, and cctools' canonical
        // `bl _foo` example sets pcrel/external/length to 1/1/2.
        u32le(&mut image, relocation.offset as u32);
        u32le(
            &mut image,
            symbol_index | (1 << 24) | (2 << 25) | (1 << 27) | (2 << 28),
        );
    }
    image.resize(symtab_offset, 0);
    let mut string_index = 1_u32;
    for (symbol, offset) in &defined_symbols {
        u32le(&mut image, string_index);
        image.push(0x0f); // N_SECT | N_EXT
        image.push(1); // __text section
        image.extend_from_slice(&0_u16.to_le_bytes());
        u64le(&mut image, *offset as u64); // section offset in MH_OBJECT
        string_index += (symbol.len() + 1) as u32;
    }
    for symbol in &undefined_symbols {
        u32le(&mut image, string_index);
        image.push(0x01); // N_UNDF | N_EXT
        image.push(0); // NO_SECT
        image.extend_from_slice(&0_u16.to_le_bytes());
        u64le(&mut image, 0);
        string_index += (symbol.len() + 1) as u32;
    }
    image.push(0); // string-table index zero is the empty name
    for (symbol, _) in &defined_symbols {
        image.extend_from_slice(symbol.as_bytes());
        image.push(0);
    }
    for symbol in &undefined_symbols {
        image.extend_from_slice(symbol.as_bytes());
        image.push(0);
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_frontend::{lower_rust_process_main, lower_rust_source};
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
        assert_eq!(u32::from_le_bytes(image[16..20].try_into().unwrap()), 3);
        // The backwards-compatible object API uses the explicit macOS 11.0
        // default, rather than omitting platform identity for the linker to
        // infer from its host.
        assert_eq!(
            u32::from_le_bytes(image[184..188].try_into().unwrap()),
            0x32
        );
        assert_eq!(u32::from_le_bytes(image[192..196].try_into().unwrap()), 1);
        assert_eq!(
            u32::from_le_bytes(image[196..200].try_into().unwrap()),
            0x000b_0000
        );
        // `LC_SYMTAB.nsyms`: both owned functions are external `N_SECT`
        // entries, giving a later native link a complete object interface.
        assert_eq!(u32::from_le_bytes(image[220..224].try_into().unwrap()), 2);
        assert!(image
            .windows(b"_add\0".len())
            .any(|window| window == b"_add\0"));
        assert!(image
            .windows(b"_main\0".len())
            .any(|window| window == b"_main\0"));
    }

    #[test]
    fn selected_entry_is_exported_as_darwin_main() {
        let source = r#"
            fn increment(x: i32) -> i32 { x.wrapping_add(1) }
            fn selected_entry() -> i32 { increment(6) }
        "#;
        let program = lower_rust_source(
            std::path::Path::new("selected.rs"),
            source,
            &["increment", "selected_entry"],
        )
        .unwrap();
        let image =
            generate_object(&validate_program(&program).unwrap(), "selected_entry").unwrap();
        assert!(image
            .windows(b"_increment\0".len())
            .any(|window| window == b"_increment\0"));
        assert!(image
            .windows(b"_main\0".len())
            .any(|window| window == b"_main\0"));
        assert!(!image
            .windows(b"_selected_entry\0".len())
            .any(|window| window == b"_selected_entry\0"));
    }

    #[test]
    fn macos_version_matches_ld_platform_version_encoding() {
        assert_eq!(MacosVersion::parse("11.0").unwrap().packed(), 0x000b_0000);
        assert_eq!(MacosVersion::parse("12.3.4").unwrap().packed(), 0x000c_0304);
        for version in ["11", "11.0.0.1", "11.-1", "11.256", "x.0"] {
            assert_eq!(
                MacosVersion::parse(version),
                Err(CodegenError::InvalidMacosVersion {
                    version: version.to_owned(),
                })
            );
        }
    }

    #[test]
    fn requested_macos_version_is_written_to_build_version_command() {
        let source = "fn main() -> i32 { 0 }";
        let program =
            lower_rust_source(std::path::Path::new("main.rs"), source, &["main"]).unwrap();
        let image = generate_object_for_macos(
            &validate_program(&program).unwrap(),
            "main",
            MacosVersion::parse("12.3.4").unwrap(),
        )
        .unwrap();
        assert_eq!(
            u32::from_le_bytes(image[184..188].try_into().unwrap()),
            0x32
        );
        assert_eq!(
            u32::from_le_bytes(image[196..200].try_into().unwrap()),
            0x000c_0304
        );
        assert_eq!(
            u32::from_le_bytes(image[200..204].try_into().unwrap()),
            0x000c_0304
        );
    }

    #[test]
    fn process_main_uses_a_real_branch26_relocation_for_libsystem_exit() {
        let source = r#"
            fn status() -> i32 { 75 }
            fn main() { std::process::exit(status()); }
        "#;
        let program = lower_rust_process_main(
            std::path::Path::new("process-main.rs"),
            source,
            &["status", "main"],
        )
        .unwrap();
        let image = generate_object(&validate_program(&program).unwrap(), "main").unwrap();

        // Exactly the three object-file commands we emit: __TEXT,
        // LC_BUILD_VERSION, and SYMTAB. In particular, an MH_OBJECT owns its
        // relocation from section_64 and does not need LC_DYSYMTAB.
        assert_eq!(u32::from_le_bytes(image[16..20].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(image[32..36].try_into().unwrap()), 0x19);
        assert_eq!(
            u32::from_le_bytes(image[184..188].try_into().unwrap()),
            0x32
        );
        assert_eq!(u32::from_le_bytes(image[188..192].try_into().unwrap()), 24);
        assert_eq!(u32::from_le_bytes(image[192..196].try_into().unwrap()), 1);
        assert_eq!(
            u32::from_le_bytes(image[196..200].try_into().unwrap()),
            0x000b_0000
        );
        assert_eq!(
            u32::from_le_bytes(image[200..204].try_into().unwrap()),
            0x000b_0000
        );
        assert_eq!(u32::from_le_bytes(image[204..208].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(image[208..212].try_into().unwrap()), 0x2);

        let relocation_offset = u32::from_le_bytes(image[160..164].try_into().unwrap()) as usize;
        assert_ne!(relocation_offset, 0);
        assert_eq!(u32::from_le_bytes(image[164..168].try_into().unwrap()), 1);
        let branch_offset = u32::from_le_bytes(
            image[relocation_offset..relocation_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let relocation_word = u32::from_le_bytes(
            image[relocation_offset + 4..relocation_offset + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            u32::from_le_bytes(
                image[232 + branch_offset..236 + branch_offset]
                    .try_into()
                    .unwrap()
            ),
            0x9400_0000
        );
        assert_eq!((relocation_word >> 24) & 1, 1); // r_pcrel
        assert_eq!((relocation_word >> 25) & 3, 2); // r_length = 32-bit instruction
        assert_eq!((relocation_word >> 27) & 1, 1); // r_extern
        assert_eq!((relocation_word >> 28) & 0xf, 2); // ARM64_RELOC_BRANCH26

        let symtab_offset = u32::from_le_bytes(image[216..220].try_into().unwrap()) as usize;
        let symbol_count = u32::from_le_bytes(image[220..224].try_into().unwrap()) as usize;
        let string_table_offset = u32::from_le_bytes(image[224..228].try_into().unwrap()) as usize;
        let exit_index = (0..symbol_count)
            .find(|index| {
                let nlist = symtab_offset + index * 16;
                let string_index =
                    u32::from_le_bytes(image[nlist..nlist + 4].try_into().unwrap()) as usize;
                let end = image[string_table_offset + string_index..]
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap();
                &image[string_table_offset + string_index..string_table_offset + string_index + end]
                    == b"_exit"
            })
            .expect("an undefined Darwin _exit symbol");
        let exit_nlist = symtab_offset + exit_index * 16;
        assert_eq!(image[exit_nlist + 4], 0x01); // N_UNDF | N_EXT
        assert_eq!(image[exit_nlist + 5], 0); // NO_SECT
        assert_eq!(
            u64::from_le_bytes(image[exit_nlist + 8..exit_nlist + 16].try_into().unwrap()),
            0
        );
        assert_eq!(relocation_word & 0x00ff_ffff, exit_index as u32);
    }

    #[test]
    fn source_main_cannot_collide_with_a_different_selected_entry() {
        let source = r#"
            fn selected_entry() -> i32 { 7 }
            fn main() -> i32 { 9 }
        "#;
        let program = lower_rust_source(
            std::path::Path::new("collision.rs"),
            source,
            &["selected_entry", "main"],
        )
        .unwrap();
        assert_eq!(
            generate_object(&validate_program(&program).unwrap(), "selected_entry"),
            Err(CodegenError::EntrySymbolCollision {
                entry: "selected_entry".to_owned(),
            })
        );
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
    fn conventional_process_main_links_the_exit_relocation_and_launches() {
        let source = r#"
            fn increment(x: i32) -> i32 { x.wrapping_add(1) }
            fn status() -> i32 { increment(74) }
            fn main() { std::process::exit(status()); }
        "#;
        let program = lower_rust_process_main(
            std::path::Path::new("process-main.rs"),
            source,
            &["increment", "status", "main"],
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
            result: crate::types::FunctionResult::I32,
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
        let linked = Command::new("/usr/bin/ld")
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
            .output()
            .expect("the declared Darwin linker must launch");
        assert!(
            linked.status.success(),
            "Darwin linker failed: {}",
            String::from_utf8_lossy(&linked.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&linked.stderr).contains("no platform load command"),
            "owned object must declare LC_BUILD_VERSION: {}",
            String::from_utf8_lossy(&linked.stderr)
        );
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

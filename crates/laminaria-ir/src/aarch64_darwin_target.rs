//! Owned AArch64/Mach-O code generation.  No external compiler, assembler, or
//! linker participates in this module.

use crate::types::{Expr, IntWidth, Stmt};
use crate::validate::ValidatedProgram;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    MissingEntry(String),
    TooManyArguments(usize),
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

    let mut text = Vec::new();
    // Function uses w0..w7 by the Darwin AArch64 integer calling convention.
    emit_stmt(&mut text, &function.body)?;
    Ok(write_mach_o_object(&text))
}

fn emit_stmt(code: &mut Vec<u8>, stmt: &Stmt) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Return(expr, _) => {
            emit_expr(code, expr, 0)?;
            emit(code, 0xd65f_03c0);
            Ok(())
        }
        Stmt::Let { .. } => Err(CodegenError::UnsupportedConstruct("let binding")),
        Stmt::If { .. } => Err(CodegenError::UnsupportedConstruct("conditional")),
    }
}

fn emit_expr(code: &mut Vec<u8>, expr: &Expr, dst: u8) -> Result<(), CodegenError> {
    match expr {
        Expr::IntLit(value, IntWidth::I32, _) => {
            emit_i32(code, dst, *value as i32);
            Ok(())
        }
        Expr::Param(index, _) => {
            if *index >= 8 {
                return Err(CodegenError::TooManyArguments(*index + 1));
            }
            if dst != *index as u8 {
                emit(code, 0x2a00_03e0 | ((*index as u32) << 16) | dst as u32);
            }
            Ok(())
        }
        Expr::WrappingAdd(left, right, _) => emit_binary(code, left, right, dst, 0x0b00_0000),
        Expr::WrappingSub(left, right, _) => emit_binary(code, left, right, dst, 0x4b00_0000),
        Expr::WrappingMul(left, right, _) => emit_binary(code, left, right, dst, 0x1b00_7c00),
        Expr::Local(..) => Err(CodegenError::UnsupportedConstruct("local reference")),
        Expr::NotEqZero(..) => Err(CodegenError::UnsupportedConstruct("condition")),
        Expr::Call(..) => Err(CodegenError::UnsupportedConstruct("function call")),
        Expr::Let { .. } => Err(CodegenError::UnsupportedConstruct("expression let binding")),
    }
}

fn emit_binary(
    code: &mut Vec<u8>,
    left: &Expr,
    right: &Expr,
    dst: u8,
    opcode: u32,
) -> Result<(), CodegenError> {
    // The first core deliberately uses w9 as the scratch register.  Evaluate
    // right first so a parameter loaded into `dst` cannot be overwritten.
    emit_expr(code, right, 9)?;
    emit_expr(code, left, dst)?;
    emit(code, opcode | (9 << 16) | ((dst as u32) << 5) | dst as u32);
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

fn write_mach_o_object(text: &[u8]) -> Vec<u8> {
    let section_offset = 32 + 152;
    let mut image = Vec::with_capacity(section_offset + text.len());
    u32le(&mut image, 0xfeed_facf);
    u32le(&mut image, 0x0100_000c);
    u32le(&mut image, 0);
    u32le(&mut image, 1);
    u32le(&mut image, 1);
    u32le(&mut image, 152);
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
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    u32le(&mut image, 0);
    image.extend_from_slice(text);
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_frontend::lower_rust_source;
    use crate::validate::validate_program;

    #[test]
    fn emits_a_mach_o_object_from_real_rust_source() {
        let source = "fn add(a: i32, b: i32) -> i32 { a.wrapping_add(b) }";
        let program = lower_rust_source(std::path::Path::new("add.rs"), source, &["add"]).unwrap();
        let image = generate_object(&validate_program(&program).unwrap(), "add").unwrap();
        assert_eq!(&image[..4], &[0xcf, 0xfa, 0xed, 0xfe]);
        assert_eq!(u32::from_le_bytes(image[12..16].try_into().unwrap()), 1);
    }
}

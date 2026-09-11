//! LAMINARIA-owned lowering from validated IR to the WebAssembly 1.0 binary
//! format selected by issue #5 T0.
//!
//! This module constructs the module bytes directly. It does not invoke an
//! external compiler, assembler, linker, or WebAssembly tool.

use crate::types::{Expr, FnFact, FnId, IntWidth, LocalId, Program, Stmt};
use crate::validate::ValidatedProgram;
use std::collections::BTreeMap;
use std::fmt;

const I32: u8 = 0x7f;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    TooManyItems(&'static str),
    MissingFunction(String),
    MissingLocal(LocalId),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyItems(kind) => write!(f, "too many {kind} for a WebAssembly u32 index"),
            Self::MissingFunction(name) => {
                write!(
                    f,
                    "validated call target `{name}` is absent during code generation"
                )
            }
            Self::MissingLocal(local) => write!(
                f,
                "validated local {:?} is out of scope during code generation",
                local
            ),
        }
    }
}

impl std::error::Error for CodegenError {}

/// Generate a deterministic WebAssembly 1.0 module for every function in
/// `program`. The target subset is deliberately limited to i32 values,
/// structured conditionals, locals, calls, and wrapping arithmetic.
pub fn generate_wasm_module(program: &ValidatedProgram) -> Result<Vec<u8>, CodegenError> {
    let program = program.program();
    let function_count = u32_len(program.functions.len(), "functions")?;
    let function_indices: BTreeMap<&str, u32> = program
        .functions
        .keys()
        .enumerate()
        .map(|(index, name)| Ok((name.as_str(), u32_index(index, "functions")?)))
        .collect::<Result<_, CodegenError>>()?;

    let mut module = b"\0asm\x01\0\0\0".to_vec();

    let mut types = Vec::new();
    push_u32_leb(&mut types, function_count);
    for function in program.functions.values() {
        types.push(0x60);
        push_u32_leb(&mut types, u32_len(function.params.len(), "parameters")?);
        for (_, width) in &function.params {
            types.push(wasm_value_type(*width));
        }
        push_u32_leb(&mut types, 1);
        types.push(wasm_value_type(function.return_width));
    }
    push_section(&mut module, 1, types)?;

    let mut functions = Vec::new();
    push_u32_leb(&mut functions, function_count);
    for type_index in 0..function_count {
        push_u32_leb(&mut functions, type_index);
    }
    push_section(&mut module, 3, functions)?;

    let mut exports = Vec::new();
    push_u32_leb(&mut exports, function_count);
    for (name, function_index) in &function_indices {
        push_name(&mut exports, name)?;
        exports.push(0x00);
        push_u32_leb(&mut exports, *function_index);
    }
    push_section(&mut module, 7, exports)?;

    let mut code = Vec::new();
    push_u32_leb(&mut code, function_count);
    for function in program.functions.values() {
        let body = encode_function(program, function, &function_indices)?;
        push_u32_leb(&mut code, u32_len(body.len(), "function body bytes")?);
        code.extend_from_slice(&body);
    }
    push_section(&mut module, 10, code)?;

    Ok(module)
}

fn wasm_value_type(width: IntWidth) -> u8 {
    match width {
        IntWidth::I32 => I32,
    }
}

fn encode_function(
    program: &Program,
    function: &FnFact,
    function_indices: &BTreeMap<&str, u32>,
) -> Result<Vec<u8>, CodegenError> {
    let local_count = count_stmt_lets(&function.body)?;
    let param_count = u32_len(function.params.len(), "parameters")?;
    let mut body = Vec::new();
    if local_count == 0 {
        push_u32_leb(&mut body, 0);
    } else {
        push_u32_leb(&mut body, 1);
        push_u32_leb(&mut body, local_count);
        body.push(I32);
    }

    let mut encoder = FunctionEncoder {
        program,
        function_indices,
        body: &mut body,
        scope: Vec::new(),
        next_local: param_count,
    };
    encoder.stmt(&function.body)?;
    // `Stmt` is structurally return-terminated, but an empty-result WebAssembly
    // `if` rejoins as reachable even when both arms emit `return`. Supply the
    // function validator's required result for that statically reachable join;
    // execution can only reach it if a future malformed `Stmt` violates the
    // validated IR contract.
    body.extend_from_slice(&[0x41, 0x00]);
    body.push(0x0b);
    Ok(body)
}

struct FunctionEncoder<'a> {
    program: &'a Program,
    function_indices: &'a BTreeMap<&'a str, u32>,
    body: &'a mut Vec<u8>,
    scope: Vec<(LocalId, u32)>,
    next_local: u32,
}

impl FunctionEncoder<'_> {
    fn stmt(&mut self, stmt: &Stmt) -> Result<(), CodegenError> {
        match stmt {
            Stmt::Let {
                local, value, body, ..
            } => {
                self.expr(value)?;
                let slot = self.allocate_local()?;
                self.body.push(0x21);
                push_u32_leb(self.body, slot);
                self.scope.push((*local, slot));
                let result = self.stmt(body);
                self.scope.pop();
                result
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                self.expr(cond)?;
                self.body.extend_from_slice(&[0x04, 0x40]);
                self.stmt(then)?;
                self.body.push(0x05);
                self.stmt(els)?;
                self.body.push(0x0b);
                Ok(())
            }
            Stmt::Return(expr, _) => {
                self.expr(expr)?;
                self.body.push(0x0f);
                Ok(())
            }
        }
    }

    fn expr(&mut self, expr: &Expr) -> Result<(), CodegenError> {
        match expr {
            Expr::IntLit(value, width, _) => {
                let value = match width {
                    IntWidth::I32 => *value as i32,
                };
                self.body.push(0x41);
                push_i32_leb(self.body, value);
                Ok(())
            }
            Expr::Param(index, _) => {
                self.body.push(0x20);
                push_u32_leb(self.body, u32_index(*index, "parameters")?);
                Ok(())
            }
            Expr::Local(local, _) => {
                let slot = self
                    .scope
                    .iter()
                    .rev()
                    .find_map(|(candidate, slot)| (candidate == local).then_some(*slot))
                    .ok_or(CodegenError::MissingLocal(*local))?;
                self.body.push(0x20);
                push_u32_leb(self.body, slot);
                Ok(())
            }
            Expr::WrappingAdd(left, right, _) => self.binary(left, right, 0x6a),
            Expr::WrappingSub(left, right, _) => self.binary(left, right, 0x6b),
            Expr::WrappingMul(left, right, _) => self.binary(left, right, 0x6c),
            Expr::NotEqZero(value, _) => {
                self.expr(value)?;
                self.body.extend_from_slice(&[0x45, 0x45]);
                Ok(())
            }
            Expr::Call(FnId(name), arguments, _) => {
                for argument in arguments {
                    self.expr(argument)?;
                }
                let function_index = self
                    .function_indices
                    .get(name.as_str())
                    .copied()
                    .ok_or_else(|| CodegenError::MissingFunction(name.clone()))?;
                debug_assert!(self.program.functions.contains_key(name));
                self.body.push(0x10);
                push_u32_leb(self.body, function_index);
                Ok(())
            }
            Expr::Let {
                local, value, body, ..
            } => {
                self.expr(value)?;
                let slot = self.allocate_local()?;
                self.body.push(0x21);
                push_u32_leb(self.body, slot);
                self.scope.push((*local, slot));
                let result = self.expr(body);
                self.scope.pop();
                result
            }
        }
    }

    fn binary(&mut self, left: &Expr, right: &Expr, opcode: u8) -> Result<(), CodegenError> {
        self.expr(left)?;
        self.expr(right)?;
        self.body.push(opcode);
        Ok(())
    }

    fn allocate_local(&mut self) -> Result<u32, CodegenError> {
        let slot = self.next_local;
        self.next_local = self
            .next_local
            .checked_add(1)
            .ok_or(CodegenError::TooManyItems("locals"))?;
        Ok(slot)
    }
}

fn count_stmt_lets(stmt: &Stmt) -> Result<u32, CodegenError> {
    match stmt {
        Stmt::Let { value, body, .. } => {
            checked_sum(1, count_expr_lets(value)?, count_stmt_lets(body)?)
        }
        Stmt::If {
            cond, then, els, ..
        } => checked_sum(
            count_expr_lets(cond)?,
            count_stmt_lets(then)?,
            count_stmt_lets(els)?,
        ),
        Stmt::Return(expr, _) => count_expr_lets(expr),
    }
}

fn count_expr_lets(expr: &Expr) -> Result<u32, CodegenError> {
    match expr {
        Expr::IntLit(_, width, _) => {
            let _ = wasm_value_type(*width);
            Ok(0)
        }
        Expr::Param(_, _) | Expr::Local(_, _) => Ok(0),
        Expr::WrappingAdd(left, right, _)
        | Expr::WrappingSub(left, right, _)
        | Expr::WrappingMul(left, right, _) => {
            checked_add(count_expr_lets(left)?, count_expr_lets(right)?)
        }
        Expr::NotEqZero(value, _) => count_expr_lets(value),
        Expr::Call(_, arguments, _) => arguments.iter().try_fold(0, |total, argument| {
            checked_add(total, count_expr_lets(argument)?)
        }),
        Expr::Let { value, body, .. } => {
            checked_sum(1, count_expr_lets(value)?, count_expr_lets(body)?)
        }
    }
}

fn checked_sum(a: u32, b: u32, c: u32) -> Result<u32, CodegenError> {
    checked_add(checked_add(a, b)?, c)
}

fn checked_add(a: u32, b: u32) -> Result<u32, CodegenError> {
    a.checked_add(b).ok_or(CodegenError::TooManyItems("locals"))
}

fn push_section(module: &mut Vec<u8>, id: u8, contents: Vec<u8>) -> Result<(), CodegenError> {
    module.push(id);
    push_u32_leb(module, u32_len(contents.len(), "section bytes")?);
    module.extend_from_slice(&contents);
    Ok(())
}

fn push_name(target: &mut Vec<u8>, name: &str) -> Result<(), CodegenError> {
    push_u32_leb(target, u32_len(name.len(), "name bytes")?);
    target.extend_from_slice(name.as_bytes());
    Ok(())
}

fn u32_len(value: usize, kind: &'static str) -> Result<u32, CodegenError> {
    u32::try_from(value).map_err(|_| CodegenError::TooManyItems(kind))
}

fn u32_index(value: usize, kind: &'static str) -> Result<u32, CodegenError> {
    u32_len(value, kind)
}

fn push_u32_leb(target: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        target.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn push_i32_leb(target: &mut Vec<u8>, mut value: i32) {
    loop {
        let byte = (value as u8) & 0x7f;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        target.push(if done { byte } else { byte | 0x80 });
        if done {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::eval_function;
    use crate::rust_frontend::lower_rust_source;
    use crate::types::{Provenance, SourceLanguage, SourcePosition, SourceSpan};
    use crate::validate::validate_program;
    use std::path::{Path, PathBuf};
    use wasmtime::{Engine, Instance, Module, Store};

    const TEST_INPUTS: &[(i32, i32, i32, i32)] = &[
        (3, 4, 0, 7),
        (3, 4, 1, 6),
        (i32::MAX, 1, 0, i32::MIN),
        (-5, 10, 1, -10),
    ];

    fn fixture_program() -> ValidatedProgram {
        let path = repo_root()
            .join("fixtures/laminaria-semantic-substrate-prototype/rust-src/add_or_double.rs");
        let source = std::fs::read_to_string(&path).unwrap();
        let program = lower_rust_source(&path, &source, &["double", "add_or_double"]).unwrap();
        validate_program(&program).unwrap()
    }

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    fn provenance() -> Provenance {
        Provenance {
            source_file: PathBuf::from("wasm_target_test"),
            span: SourceSpan {
                start: SourcePosition { line: 1, column: 1 },
                end: SourcePosition { line: 1, column: 1 },
            },
            language: SourceLanguage::Rust,
        }
    }

    fn execute_one_i32(program: &Program, function_name: &str, argument: i32) -> i32 {
        let validated = validate_program(program).unwrap();
        let bytes = generate_wasm_module(&validated).unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        instance
            .get_typed_func::<i32, i32>(&mut store, function_name)
            .unwrap()
            .call(&mut store, argument)
            .unwrap()
    }

    #[test]
    fn generated_module_starts_with_wasm_mvp_magic_and_version() {
        let bytes = generate_wasm_module(&fixture_program()).unwrap();
        assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
    }

    #[test]
    fn generated_fixture_matches_interpreter_and_fixed_expected_values() {
        let program = fixture_program();
        let bytes = generate_wasm_module(&program).unwrap();
        let engine = Engine::default();
        let module = Module::new(&engine, &bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let function = instance
            .get_typed_func::<(i32, i32, i32), i32>(&mut store, "add_or_double")
            .unwrap();

        for &(a, b, use_double, expected) in TEST_INPUTS {
            let interpreted = eval_function(
                program.program(),
                "add_or_double",
                &[i64::from(a), i64::from(b), i64::from(use_double)],
            )
            .unwrap()
            .value as i32;
            let generated = function.call(&mut store, (a, b, use_double)).unwrap();
            assert_eq!(interpreted, expected);
            assert_eq!(generated, expected);
        }
    }

    #[test]
    fn target_generator_contains_no_external_command_invocation() {
        let source = include_str!("wasm_target.rs");
        let forbidden = ["Command", "::", "new"].concat();
        assert!(!source.contains(&forbidden));
    }

    #[test]
    fn statement_and_expression_locals_preserve_wrapping_arithmetic() {
        let p = provenance();
        let mut program = Program::default();
        program.insert(FnFact {
            name: "locals".to_string(),
            params: vec![("a".to_string(), IntWidth::I32)],
            return_width: IntWidth::I32,
            provenance: p.clone(),
            body: Stmt::Let {
                local: LocalId(0),
                value: Expr::WrappingSub(
                    Box::new(Expr::Param(0, p.clone())),
                    Box::new(Expr::IntLit(2, IntWidth::I32, p.clone())),
                    p.clone(),
                ),
                body: Box::new(Stmt::Return(
                    Expr::Let {
                        local: LocalId(1),
                        value: Box::new(Expr::WrappingMul(
                            Box::new(Expr::Local(LocalId(0), p.clone())),
                            Box::new(Expr::IntLit(3, IntWidth::I32, p.clone())),
                            p.clone(),
                        )),
                        body: Box::new(Expr::WrappingAdd(
                            Box::new(Expr::Local(LocalId(1), p.clone())),
                            Box::new(Expr::IntLit(1, IntWidth::I32, p.clone())),
                            p.clone(),
                        )),
                        provenance: p.clone(),
                    },
                    p.clone(),
                )),
                provenance: p,
            },
        });

        assert_eq!(execute_one_i32(&program, "locals", 10), 25);
    }

    #[test]
    fn finite_recursive_calls_use_the_wasm_call_stack() {
        let p = provenance();
        let mut program = Program::default();
        program.insert(FnFact {
            name: "countdown".to_string(),
            params: vec![("n".to_string(), IntWidth::I32)],
            return_width: IntWidth::I32,
            provenance: p.clone(),
            body: Stmt::If {
                cond: Expr::NotEqZero(Box::new(Expr::Param(0, p.clone())), p.clone()),
                then: Box::new(Stmt::Return(
                    Expr::WrappingAdd(
                        Box::new(Expr::Call(
                            FnId("countdown".to_string()),
                            vec![Expr::WrappingSub(
                                Box::new(Expr::Param(0, p.clone())),
                                Box::new(Expr::IntLit(1, IntWidth::I32, p.clone())),
                                p.clone(),
                            )],
                            p.clone(),
                        )),
                        Box::new(Expr::IntLit(1, IntWidth::I32, p.clone())),
                        p.clone(),
                    ),
                    p.clone(),
                )),
                els: Box::new(Stmt::Return(
                    Expr::IntLit(0, IntWidth::I32, p.clone()),
                    p.clone(),
                )),
                provenance: p,
            },
        });

        assert_eq!(execute_one_i32(&program, "countdown", 5), 5);
    }
}

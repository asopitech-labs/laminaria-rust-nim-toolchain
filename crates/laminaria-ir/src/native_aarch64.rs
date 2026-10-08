//! Owned AArch64 Mach-O object generation for the validated scalar IR.
//! The platform linker may link this object; no external compiler produces it.

use std::collections::BTreeMap;

use object::write::{Object, Relocation, StandardSection, Symbol, SymbolId, SymbolSection};
use object::{
    Architecture, BinaryFormat, Endianness, RelocationEncoding, RelocationKind, SymbolFlags,
    SymbolKind, SymbolScope,
};

use crate::types::{Expr, FnFact, FnId, IntWidth, LocalId, Stmt};
use crate::validate::ValidatedProgram;

#[derive(Debug)]
pub enum NativeCodegenError {
    TooManyParameters(String),
    FrameTooLarge(String),
    Object(object::write::Error),
}

impl std::fmt::Display for NativeCodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyParameters(name) => {
                write!(f, "{name}: more than eight integer parameters")
            }
            Self::FrameTooLarge(name) => write!(f, "{name}: scalar frame exceeds AArch64 subset"),
            Self::Object(error) => write!(f, "native object generation: {error}"),
        }
    }
}

impl std::error::Error for NativeCodegenError {}

impl From<object::write::Error> for NativeCodegenError {
    fn from(value: object::write::Error) -> Self {
        Self::Object(value)
    }
}

struct FunctionCode {
    words: Vec<u32>,
    calls: Vec<(usize, FnId)>,
    params: usize,
    next_slot: usize,
    locals: BTreeMap<LocalId, Vec<usize>>,
    frame_bytes: usize,
}

impl FunctionCode {
    fn new(fact: &FnFact) -> Result<Self, NativeCodegenError> {
        if fact.params.len() > 8 {
            return Err(NativeCodegenError::TooManyParameters(fact.name.clone()));
        }
        let slots = fact.params.len() + count_stmt_slots(&fact.body);
        let frame_bytes = (slots * 4).div_ceil(16) * 16;
        // LDUR/STUR use a signed 9-bit byte offset from x29. Reserve the
        // first four bytes below the frame pointer for slot zero.
        if frame_bytes > 256 {
            return Err(NativeCodegenError::FrameTooLarge(fact.name.clone()));
        }
        Ok(Self {
            words: Vec::new(),
            calls: Vec::new(),
            params: fact.params.len(),
            next_slot: fact.params.len(),
            locals: BTreeMap::new(),
            frame_bytes,
        })
    }

    fn emit(&mut self, word: u32) {
        self.words.push(word);
    }

    fn slot_offset(slot: usize) -> u32 {
        ((-(4 * (slot as i32 + 1))) & 0x1ff) as u32
    }

    fn store(&mut self, register: u32, slot: usize) {
        self.emit(0xb800_0000 | (Self::slot_offset(slot) << 12) | (29 << 5) | register);
    }

    fn load(&mut self, register: u32, slot: usize) {
        self.emit(0xb840_0000 | (Self::slot_offset(slot) << 12) | (29 << 5) | register);
    }

    fn alloc(&mut self) -> usize {
        let slot = self.next_slot;
        self.next_slot += 1;
        slot
    }

    fn prologue(&mut self) {
        self.emit(0xa9bf_7bfd); // stp x29, x30, [sp, #-16]!
        self.emit(0x9100_03fd); // mov x29, sp
        if self.frame_bytes != 0 {
            self.emit(0xd100_03ff | ((self.frame_bytes as u32) << 10)); // sub sp, sp, #frame
        }
        for param in 0..self.params {
            self.store(param as u32, param);
        }
    }

    fn epilogue(&mut self) {
        if self.frame_bytes != 0 {
            self.emit(0x9100_03ff | ((self.frame_bytes as u32) << 10)); // add sp, sp, #frame
        }
        self.emit(0xa8c1_7bfd); // ldp x29, x30, [sp], #16
        self.emit(0xd65f_03c0); // ret
    }

    fn push_local(&mut self, local: LocalId, slot: usize) {
        self.locals.entry(local).or_default().push(slot);
    }

    fn pop_local(&mut self, local: LocalId) {
        let bindings = self.locals.get_mut(&local).expect("validated let binding");
        bindings.pop();
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                local, value, body, ..
            } => {
                self.expr(value);
                let slot = self.alloc();
                self.store(0, slot);
                self.push_local(*local, slot);
                self.stmt(body);
                self.pop_local(*local);
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                self.expr(cond);
                let else_branch = self.words.len();
                self.emit(0x3400_0000); // cbz w0, else
                self.stmt(then);
                let end_branch = self.words.len();
                self.emit(0x1400_0000); // b end
                let else_start = self.words.len();
                self.stmt(els);
                let end = self.words.len();
                self.words[else_branch] |= (((else_start - else_branch) as u32) & 0x7ffff) << 5;
                self.words[end_branch] |= ((end - end_branch) as u32) & 0x03ff_ffff;
            }
            Stmt::Return(value, _) => {
                self.expr(value);
                self.epilogue();
            }
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::IntLit(value, IntWidth::I32, _) => {
                let bits = *value as u32;
                self.emit(0x5280_0000 | ((bits & 0xffff) << 5)); // movz w0, low
                if bits >> 16 != 0 {
                    self.emit(0x72a0_0000 | ((bits >> 16) << 5)); // movk w0, high
                }
            }
            Expr::Param(index, _) => self.load(0, *index),
            Expr::Local(local, _) => {
                let slot = *self.locals[local].last().expect("validated local");
                self.load(0, slot);
            }
            Expr::WrappingAdd(left, right, _)
            | Expr::WrappingSub(left, right, _)
            | Expr::WrappingMul(left, right, _) => {
                self.expr(left);
                let slot = self.alloc();
                self.store(0, slot);
                self.expr(right);
                self.load(9, slot);
                let base = match expr {
                    Expr::WrappingAdd(..) => 0x0b00_0000,
                    Expr::WrappingSub(..) => 0x4b00_0000,
                    Expr::WrappingMul(..) => 0x1b00_0000 | (31 << 10),
                    _ => unreachable!(),
                };
                self.emit(base | (9 << 5)); // w0 = w9 op w0
            }
            Expr::NotEqZero(value, _) => {
                self.expr(value);
                self.emit(0x7100_001f); // cmp w0, #0
                self.emit(0x1a9f_07e0); // cset w0, ne
            }
            Expr::Call(callee, args, _) => {
                let slots: Vec<usize> = args
                    .iter()
                    .map(|arg| {
                        self.expr(arg);
                        let slot = self.alloc();
                        self.store(0, slot);
                        slot
                    })
                    .collect();
                for (register, slot) in slots.into_iter().enumerate() {
                    self.load(register as u32, slot);
                }
                let at = self.words.len();
                self.emit(0x9400_0000); // bl <relocation>
                self.calls.push((at, callee.clone()));
            }
            Expr::Let {
                local, value, body, ..
            } => {
                self.expr(value);
                let slot = self.alloc();
                self.store(0, slot);
                self.push_local(*local, slot);
                self.expr(body);
                self.pop_local(*local);
            }
        }
    }
}

fn count_expr_slots(expr: &Expr) -> usize {
    match expr {
        Expr::IntLit(..) | Expr::Param(..) | Expr::Local(..) => 0,
        Expr::WrappingAdd(a, b, _) | Expr::WrappingSub(a, b, _) | Expr::WrappingMul(a, b, _) => {
            1 + count_expr_slots(a) + count_expr_slots(b)
        }
        Expr::NotEqZero(inner, _) => count_expr_slots(inner),
        Expr::Call(_, args, _) => args.len() + args.iter().map(count_expr_slots).sum::<usize>(),
        Expr::Let { value, body, .. } => 1 + count_expr_slots(value) + count_expr_slots(body),
    }
}

fn count_stmt_slots(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Let { value, body, .. } => 1 + count_expr_slots(value) + count_stmt_slots(body),
        Stmt::If {
            cond, then, els, ..
        } => count_expr_slots(cond) + count_stmt_slots(then) + count_stmt_slots(els),
        Stmt::Return(value, _) => count_expr_slots(value),
    }
}

/// Produces a relocatable object directly from validated LAMINARIA IR.
/// Exported names use the platform's normal C ABI spelling so a final linker
/// can connect an independently compiled caller without Rust compilation.
pub fn generate_macho_aarch64_object(
    program: &ValidatedProgram,
) -> Result<Vec<u8>, NativeCodegenError> {
    let mut object = Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );
    let text = object.section_id(StandardSection::Text);
    let mut symbols: BTreeMap<String, SymbolId> = BTreeMap::new();
    for name in program.program().functions.keys() {
        let id = object.add_symbol(Symbol {
            name: name.as_bytes().to_vec(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
        symbols.insert(name.clone(), id);
    }
    for (name, fact) in &program.program().functions {
        let mut code = FunctionCode::new(fact)?;
        code.prologue();
        code.stmt(&fact.body);
        let bytes: Vec<u8> = code
            .words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        let offset = object.append_section_data(text, &bytes, 4);
        let symbol = object.symbol_mut(symbols[name]);
        symbol.value = offset;
        symbol.size = bytes.len() as u64;
        for (word_index, callee) in code.calls {
            object.add_relocation(
                text,
                Relocation {
                    offset: offset + (word_index * 4) as u64,
                    size: 26,
                    kind: RelocationKind::Relative,
                    encoding: RelocationEncoding::AArch64Call,
                    symbol: symbols[&callee.0],
                    addend: 0,
                },
            )?;
        }
    }
    object.write().map_err(NativeCodegenError::Object)
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use std::fs;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::rust_frontend::lower_rust_source;
    use crate::validate::validate_program;

    use super::generate_macho_aarch64_object;

    #[test]
    fn generated_object_links_and_agrees_with_the_existing_rust_oracle() {
        let source_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../fixtures/laminaria-semantic-substrate-prototype/rust-src/add_or_double.rs",
        );
        let source = fs::read_to_string(&source_path).unwrap();
        let program = lower_rust_source(&source_path, &source, &["double", "add_or_double"])
            .expect("fixture is in the declared owned subset");
        let validated = validate_program(&program).unwrap();
        let object = generate_macho_aarch64_object(&validated).unwrap();

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("laminaria-native-{}-{unique}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let object_path = dir.join("owned.o");
        let driver_path = dir.join("driver.c");
        let executable_path = dir.join("driver");
        fs::write(&object_path, object).unwrap();
        fs::write(
            &driver_path,
            "#include <stdio.h>\n#include <stdint.h>\nextern int32_t add_or_double(int32_t, int32_t, int32_t);\nint main(void) {\n  printf(\"%d\\n\", add_or_double(3, 4, 0));\n  printf(\"%d\\n\", add_or_double(3, 4, 1));\n  printf(\"%d\\n\", add_or_double(2147483647, 1, 0));\n  printf(\"%d\\n\", add_or_double(-5, 10, 1));\n  return 0;\n}\n",
        )
        .unwrap();
        let link = Command::new("/usr/bin/cc")
            .args([
                "-o",
                executable_path.to_str().unwrap(),
                driver_path.to_str().unwrap(),
                object_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "link failed: {}",
            String::from_utf8_lossy(&link.stderr)
        );
        let run = Command::new(&executable_path).output().unwrap();
        assert!(
            run.status.success(),
            "native executable failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8(run.stdout).unwrap(),
            "7\n6\n-2147483648\n-10\n"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}

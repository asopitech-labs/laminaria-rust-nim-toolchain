//! Owned ELF x86_64 object generation for a source-derived scalar fold.
//! The System V ABI accepts the slice pointer in RDI and length in RSI;
//! the scalar result is returned in EAX/RAX.

use object::write::{Object, StandardSection, Symbol, SymbolSection};
use object::{Architecture, BinaryFormat, Endianness, SymbolFlags, SymbolKind, SymbolScope};

use crate::rust_generic_fold::{GenericFoldIr, OverflowPolicy, ScalarType};

fn patch_rel8(code: &mut [u8], displacement_at: usize, target: usize) {
    let delta = target as isize - (displacement_at + 1) as isize;
    let displacement = i8::try_from(delta).expect("scalar fold branches fit one-byte displacement");
    code[displacement_at] = displacement as u8;
}

fn fold_machine_code(ir: &GenericFoldIr) -> Vec<u8> {
    let mut code = vec![
        0x31, 0xc0, // xor eax, eax: zero the accumulator
        0x31, 0xc9, // xor ecx, ecx: zero the element index
    ];
    let loop_start = code.len();
    code.extend_from_slice(&[
        0x48, 0x39, 0xf1, // cmp rcx, rsi
        0x73, 0x00, // jae end
    ]);
    let end_displacement = code.len() - 1;
    code.extend_from_slice(match ir.scalar {
        ScalarType::I32 => &[0x03, 0x04, 0x8f][..], // add eax, [rdi + rcx*4]
        ScalarType::I64 => &[0x48, 0x03, 0x04, 0xcf][..], // add rax, [rdi + rcx*8]
    });
    let overflow_displacement = if ir.overflow == OverflowPolicy::Checked {
        code.extend_from_slice(&[0x70, 0x00]); // jo overflow
        Some(code.len() - 1)
    } else {
        None
    };
    code.extend_from_slice(&[
        0x48, 0xff, 0xc1, // inc rcx
        0xeb, 0x00, // jmp loop_start
    ]);
    let loop_displacement = code.len() - 1;
    let end = code.len();
    code.push(0xc3); // ret
    if let Some(at) = overflow_displacement {
        let overflow = code.len();
        code.extend_from_slice(&[0x0f, 0x0b]); // ud2: checked overflow trap
        patch_rel8(&mut code, at, overflow);
    }
    patch_rel8(&mut code, end_displacement, end);
    patch_rel8(&mut code, loop_displacement, loop_start);
    code
}

/// Produces a relocatable Linux x86_64 object for one concrete scalar fold.
/// Checked overflow traps because the owned native runtime has no Rust
/// panic/unwind machinery. The object never delegates Rust code generation
/// to an external compiler.
pub fn generate_elf_x86_64_generic_fold_object(
    ir: &GenericFoldIr,
    symbol_name: &str,
) -> Result<Vec<u8>, object::write::Error> {
    let mut object = Object::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
    let text = object.section_id(StandardSection::Text);
    let code = fold_machine_code(ir);
    let offset = object.append_section_data(text, &code, 16);
    object.add_symbol(Symbol {
        name: symbol_name.as_bytes().to_vec(),
        value: offset,
        size: code.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Section(text),
        flags: SymbolFlags::None,
    });
    object.write()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};

    use crate::rust_generic_demand::{discover_generic_functions, discover_generic_instances};
    use crate::rust_generic_fold::{lower_generic_fold, OverflowPolicy};

    use super::generate_elf_x86_64_generic_fold_object;

    fn fixture_fold() -> crate::rust_generic_fold::GenericFoldIr {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rust-heavy-workspace");
        let core_path = root.join("crates/fixture-core/src/lib.rs");
        let core = fs::read_to_string(&core_path).unwrap();
        let binary = fs::read_to_string(root.join("crates/fixture-bin/src/main.rs")).unwrap();
        let known = discover_generic_functions("fixture-core", &core).unwrap();
        let instance = discover_generic_instances("fixture-bin", &binary, &known, false)
            .unwrap()
            .pop_first()
            .unwrap();
        lower_generic_fold(&core_path, &core, &instance, OverflowPolicy::Checked).unwrap()
    }

    #[test]
    fn source_derived_fold_is_a_linkable_linux_x86_64_object() {
        let object =
            generate_elf_x86_64_generic_fold_object(&fixture_fold(), "sum_generic_i64").unwrap();
        let parsed = object::File::parse(object.as_slice()).unwrap();
        assert_eq!(parsed.format(), object::BinaryFormat::Elf);
        assert_eq!(parsed.architecture(), object::Architecture::X86_64);
        let symbol = parsed
            .symbols()
            .find(|symbol| symbol.name().unwrap() == "sum_generic_i64")
            .unwrap();
        assert!(symbol.is_definition());
        assert!(symbol.size() > 0);
        let section = parsed
            .section_by_index(symbol.section_index().unwrap())
            .unwrap();
        let start = symbol.address() as usize;
        let end = start + symbol.size() as usize;
        // Independently assembled from the equivalent SysV x86_64 loop with
        // clang's assembler. This checks branch offsets and instruction
        // encoding on hosts that cannot execute Linux x86_64 code.
        let assembler_reference = [
            0x31, 0xc0, 0x31, 0xc9, 0x48, 0x39, 0xf1, 0x73, 0x0b, 0x48, 0x03, 0x04, 0xcf, 0x70,
            0x06, 0x48, 0xff, 0xc1, 0xeb, 0xf0, 0xc3, 0x0f, 0x0b,
        ];
        assert_eq!(&section.data().unwrap()[start..end], &assembler_reference);
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn source_derived_fold_links_and_runs_on_linux_x86_64() {
        use std::process::Command;
        use std::time::{SystemTime, UNIX_EPOCH};

        let object =
            generate_elf_x86_64_generic_fold_object(&fixture_fold(), "sum_generic_i64").unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "laminaria-linux-fold-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        let object_path = dir.join("fold.o");
        let driver_path = dir.join("driver.c");
        let executable_path = dir.join("driver");
        fs::write(&object_path, object).unwrap();
        fs::write(
            &driver_path,
            "#include <stdint.h>\n#include <stddef.h>\n#include <stdio.h>\nextern int64_t sum_generic_i64(const int64_t*, size_t);\nint main(void) {\n  int64_t values[] = {12, 30};\n  printf(\"%lld\\n\", (long long)sum_generic_i64(values, 2));\n  printf(\"%lld\\n\", (long long)sum_generic_i64(values, 0));\n  return 0;\n}\n",
        )
        .unwrap();
        let link = Command::new("cc")
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
            "native fold failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8(run.stdout).unwrap(), "42\n0\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}

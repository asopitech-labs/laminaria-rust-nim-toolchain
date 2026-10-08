//! Direct Mach-O entry-object generation from source-derived Rust entry facts.
//! The linker is external, but the target code and its data are emitted here.

use object::write::{Object, Relocation, StandardSection, Symbol, SymbolId, SymbolSection};
use object::{
    Architecture, BinaryFormat, Endianness, RelocationEncoding, RelocationKind, SymbolFlags,
    SymbolKind, SymbolScope,
};

use crate::rust_fixture_main_semantics::{
    EntryOperation, EntryOutputValue, EntryType, RustEntryIr,
};

#[derive(Debug)]
pub enum NativeEntryError {
    UnsupportedEntry,
    InvalidInput(&'static str),
    DataTooLarge,
    Object(object::write::Error),
}

impl std::fmt::Display for NativeEntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedEntry => write!(f, "unsupported source-derived Rust entry IR"),
            Self::InvalidInput(reason) => write!(f, "invalid entry specialization: {reason}"),
            Self::DataTooLarge => write!(f, "entry data exceeds AArch64 ADR reach"),
            Self::Object(error) => write!(f, "native entry object generation: {error}"),
        }
    }
}

impl std::error::Error for NativeEntryError {}

impl From<object::write::Error> for NativeEntryError {
    fn from(value: object::write::Error) -> Self {
        Self::Object(value)
    }
}

/// Emits `main` from the narrow entry IR. The supplied aggregate values must
/// have been computed by a separate owned producer; `sum_x` is deliberately
/// absent because generated code calls the selected owned generic object at
/// runtime using the static `xs` payload.
pub fn generate_macho_aarch64_entry_object(
    entry: &RustEntryIr,
    points_len: usize,
    perimeter: i64,
    centroid_debug: &str,
    xs: &[i64],
    generic_symbol: &str,
) -> Result<Vec<u8>, NativeEntryError> {
    validate_entry(entry)?;
    if xs.len() != points_len {
        return Err(NativeEntryError::InvalidInput(
            "point count and collected x-coordinate count differ",
        ));
    }
    if generic_symbol.is_empty()
        || !generic_symbol
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(NativeEntryError::InvalidInput("invalid generic symbol"));
    }
    if centroid_debug.contains(['\0', '\n', '\r']) {
        return Err(NativeEntryError::InvalidInput(
            "centroid display contains a control delimiter",
        ));
    }
    let first = entry.prints[0]
        .format
        .replacen("{}", &points_len.to_string(), 1);
    let second = entry.prints[1]
        .format
        .replacen("{perimeter}", &perimeter.to_string(), 1);
    let third = entry.prints[2]
        .format
        .replacen("{centroid:?}", centroid_debug, 1);
    let fourth = entry.prints[3].format.replacen("{sum_x}", "%lld\n", 1);

    let mut object = Object::new(
        BinaryFormat::MachO,
        Architecture::Aarch64,
        Endianness::Little,
    );
    let text = object.section_id(StandardSection::Text);
    let fold = undefined(&mut object, generic_symbol, SymbolKind::Text);
    let puts = undefined(&mut object, "puts", SymbolKind::Text);
    let printf = undefined(&mut object, "printf", SymbolKind::Text);

    let mut words = Vec::new();
    let mut calls = Vec::new();
    let mut addresses = Vec::new();
    words.extend([
        0xa9bf_7bfd, // stp x29, x30, [sp, #-16]!
        0x9100_03fd, // mov x29, sp
        0xa9bf_53f3, // stp x19, x20, [sp, #-16]!
    ]);
    addresses.push((words.len(), 0, "xs"));
    words.push(0); // adr x0, xs
    emit_mov_u64(&mut words, 1, xs.len() as u64);
    calls.push((words.len(), fold));
    words.push(0x9400_0000); // bl selected owned sum_generic<i64>
    words.push(0xaa00_03f3); // mov x19, x0
    for name in ["first", "second", "third"] {
        addresses.push((words.len(), 0, name));
        words.push(0); // adr x0, formatted line
        calls.push((words.len(), puts));
        words.push(0x9400_0000); // bl puts
    }
    addresses.push((words.len(), 0, "fourth"));
    words.push(0); // adr x0, source-derived sum format
    words.extend([
        0xd100_43ff, // sub sp, sp, #16: Apple ARM64 varargs use stack
        0xf900_03f3, // str x19, [sp]
    ]);
    calls.push((words.len(), printf));
    words.push(0x9400_0000); // bl printf
    words.extend([
        0x9100_43ff, // add sp, sp, #16
        0xa8c1_53f3, // ldp x19, x20, [sp], #16
        0xa8c1_7bfd, // ldp x29, x30, [sp], #16
        0x5280_0000, // mov w0, #0
        0xd65f_03c0, // ret
    ]);

    let mut bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    bytes.resize(bytes.len().div_ceil(8) * 8, 0);
    let mut data = std::collections::BTreeMap::new();
    data.insert("xs", bytes.len());
    for value in xs {
        bytes.extend(value.to_le_bytes());
    }
    for (name, content) in [
        ("first", first),
        ("second", second),
        ("third", third),
        ("fourth", fourth),
    ] {
        data.insert(name, bytes.len());
        bytes.extend(content.as_bytes());
        bytes.push(0);
    }
    for (at, register, name) in addresses {
        let delta = data[name] as i64 - (at * 4) as i64;
        let word = encode_adr(register, delta)?;
        bytes[at * 4..at * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let offset = object.append_section_data(text, &bytes, 8);
    object.add_symbol(Symbol {
        name: b"main".to_vec(),
        value: offset,
        size: (words.len() * 4) as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Section(text),
        flags: SymbolFlags::None,
    });
    for (at, symbol) in calls {
        object.add_relocation(
            text,
            Relocation {
                offset: offset + (at * 4) as u64,
                size: 26,
                kind: RelocationKind::Relative,
                encoding: RelocationEncoding::AArch64Call,
                symbol,
                addend: 0,
            },
        )?;
    }
    object.write().map_err(NativeEntryError::Object)
}

fn undefined(object: &mut Object, name: &str, kind: SymbolKind) -> SymbolId {
    object.add_symbol(Symbol {
        name: name.as_bytes().to_vec(),
        value: 0,
        size: 0,
        kind,
        scope: SymbolScope::Unknown,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    })
}

fn emit_mov_u64(words: &mut Vec<u32>, register: u32, value: u64) {
    words.push(0xd280_0000 | (((value & 0xffff) as u32) << 5) | register); // movz
    for shift in 1..4 {
        let half = ((value >> (shift * 16)) & 0xffff) as u32;
        if half != 0 {
            words.push(0xf280_0000 | ((shift as u32) << 21) | (half << 5) | register);
            // movk
        }
    }
}

fn encode_adr(register: u32, delta: i64) -> Result<u32, NativeEntryError> {
    if !(-1_048_576..1_048_576).contains(&delta) {
        return Err(NativeEntryError::DataTooLarge);
    }
    let immediate = delta as u32 & 0x1f_ffff;
    Ok(0x1000_0000 | ((immediate & 3) << 29) | (((immediate >> 2) & 0x7ffff) << 5) | register)
}

fn validate_entry(entry: &RustEntryIr) -> Result<(), NativeEntryError> {
    let [cluster, perimeter, centroid, xs, sum_x] = entry.computations.as_slice() else {
        return Err(NativeEntryError::UnsupportedEntry);
    };
    let valid = cluster.binding.name == "cluster"
        && cluster.binding.ty == EntryType::Cluster
        && matches!(cluster.operation, EntryOperation::PrimeGrid { .. })
        && perimeter.binding.name == "perimeter"
        && perimeter.binding.ty == EntryType::I64
        && matches!(&perimeter.operation, EntryOperation::TotalPerimeter { cluster } if cluster == "cluster")
        && centroid.binding.name == "centroid"
        && centroid.binding.ty == EntryType::OptionalPoint
        && matches!(&centroid.operation, EntryOperation::Centroid { cluster } if cluster == "cluster")
        && xs.binding.name == "xs"
        && xs.binding.ty == EntryType::I64Vector
        && matches!(&xs.operation, EntryOperation::CollectXCoordinates { cluster } if cluster == "cluster")
        && sum_x.binding.name == "sum_x"
        && sum_x.binding.ty == EntryType::I64
        && matches!(&sum_x.operation, EntryOperation::SumGenericI64 { values } if values == "xs");
    if !valid {
        return Err(NativeEntryError::UnsupportedEntry);
    }
    let [points, perimeter, centroid, sum] = entry.prints.as_slice() else {
        return Err(NativeEntryError::UnsupportedEntry);
    };
    if points.format != "points={}"
        || !matches!(&points.value, EntryOutputValue::PointsLength { cluster } if cluster == "cluster")
        || perimeter.format != "perimeter={perimeter}"
        || !matches!(&perimeter.value, EntryOutputValue::Perimeter { binding } if binding == "perimeter")
        || centroid.format != "centroid={centroid:?}"
        || !matches!(&centroid.value, EntryOutputValue::CentroidDebug { binding } if binding == "centroid")
        || sum.format != "sum_x={sum_x}"
        || !matches!(&sum.value, EntryOutputValue::SumX { binding } if binding == "sum_x")
    {
        return Err(NativeEntryError::UnsupportedEntry);
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;
    use crate::native_aarch64::generate_macho_aarch64_generic_fold_object;
    use crate::rust_fixture_main_semantics::lower_rust_entry;
    use crate::rust_generic_demand::RustGenericInstance;
    use crate::rust_generic_fold::{lower_generic_fold, OverflowPolicy};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn links_and_runs_owned_entry_and_owned_generic_without_a_c_target_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rust-heavy-workspace/crates");
        let entry_path = root.join("fixture-bin/src/main.rs");
        let core_path = root.join("fixture-core/src/lib.rs");
        let entry =
            lower_rust_entry(&entry_path, &fs::read_to_string(&entry_path).unwrap()).unwrap();
        let fold = lower_generic_fold(
            &core_path,
            &fs::read_to_string(&core_path).unwrap(),
            &RustGenericInstance {
                package: "fixture-core".into(),
                function: "sum_generic".into(),
                type_arguments: vec!["i64".into()],
            },
            OverflowPolicy::Checked,
        )
        .unwrap();
        let symbol = "laminaria_entry_test_sum_i64";
        let generic = generate_macho_aarch64_generic_fold_object(&fold, symbol).unwrap();
        let main = generate_macho_aarch64_entry_object(
            &entry,
            3,
            11,
            "Some(Point { x: 5, y: 7 })",
            &[1, 2, 3],
            symbol,
        )
        .unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "laminaria-owned-entry-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        let main_path = dir.join("main.o");
        let generic_path = dir.join("generic.o");
        let executable = dir.join("fixture-bin");
        fs::write(&main_path, main).unwrap();
        fs::write(&generic_path, generic).unwrap();
        let link = Command::new("/usr/bin/cc")
            .args([
                "-o",
                executable.to_str().unwrap(),
                main_path.to_str().unwrap(),
                generic_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "link failed: {}",
            String::from_utf8_lossy(&link.stderr)
        );
        let run = Command::new(&executable).output().unwrap();
        assert!(
            run.status.success(),
            "entry failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8(run.stdout).unwrap(),
            "points=3\nperimeter=11\ncentroid=Some(Point { x: 5, y: 7 })\nsum_x=6\n"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_mutated_entry_ir_before_native_publication() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rust-heavy-workspace/crates/fixture-bin/src/main.rs");
        let mut entry = lower_rust_entry(&path, &fs::read_to_string(&path).unwrap()).unwrap();
        entry.prints.swap(1, 2);
        assert!(matches!(
            generate_macho_aarch64_entry_object(&entry, 0, 0, "None", &[], "owned_sum"),
            Err(NativeEntryError::UnsupportedEntry)
        ));
    }
}

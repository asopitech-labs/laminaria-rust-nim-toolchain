//! Specializes a source-derived, input-free Rust entry over the owned prime,
//! point, cluster, and selected generic semantics. The resulting values are
//! inputs to native entry code generation, not reference-compiler output.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use laminaria_ir::native_aarch64_entry::generate_macho_aarch64_entry_object;
use laminaria_ir::rust_cluster_semantics::{lower_cluster_semantics, ClusterValue};
use laminaria_ir::rust_fixture_main_semantics::{
    lower_rust_entry, EntryOperation, EntryOutputValue, RustEntryIr,
};
use laminaria_ir::rust_generic_demand::RustGenericInstance as SourceGenericInstance;
use laminaria_ir::rust_generic_fold::{lower_generic_fold, GenericFoldIr, ScalarType};
use laminaria_ir::rust_point_semantics::{lower_point_semantics, PointValue};
use laminaria_ir::rust_prime_semantics::lower_prime_semantics;

use crate::rust_generic_semantic_bridge::{NativeGenericObject, NativeGenericTarget};

pub struct RustSourceSnapshot<'a> {
    pub path: &'a Path,
    pub text: &'a str,
}

#[derive(Debug, Clone)]
pub struct EntrySpecialization {
    pub entry: RustEntryIr,
    pub points_len: usize,
    pub perimeter: i64,
    pub centroid_debug: String,
    pub xs: Vec<i64>,
    /// Independent owned semantic expectation for the native generic call.
    pub expected_sum_x: i64,
}

#[derive(Debug)]
pub struct EntrySpecializationError(pub String);

impl std::fmt::Display for EntrySpecializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EntrySpecializationError {}

enum Value {
    Cluster(ClusterValue),
    Integer(i64),
    OptionalPoint(Option<PointValue>),
    Integers(Vec<i64>),
}

fn error(message: impl Into<String>) -> EntrySpecializationError {
    EntrySpecializationError(message.into())
}

/// Execute the supported pure entry operations in source order. Every input
/// program is lowered from its source snapshot first; unsupported edits fail
/// before a native object can be published.
pub fn specialize_rust_entry(
    core: RustSourceSnapshot<'_>,
    middle: RustSourceSnapshot<'_>,
    binary: RustSourceSnapshot<'_>,
    generic: &GenericFoldIr,
) -> Result<EntrySpecialization, EntrySpecializationError> {
    let prime = lower_prime_semantics(core.path, core.text, "is_prime", "primes_up_to")
        .map_err(|cause| error(format!("core prime lowering: {cause}")))?;
    let point = lower_point_semantics(core.path, core.text, "Point")
        .map_err(|cause| error(format!("core point lowering: {cause}")))?;
    let cluster = lower_cluster_semantics(
        middle.path,
        middle.text,
        "Cluster",
        &point.struct_name,
        &prime.sequence_name,
    )
    .map_err(|cause| error(format!("middle cluster lowering: {cause}")))?;
    let entry = lower_rust_entry(binary.path, binary.text)
        .map_err(|cause| error(format!("binary entry lowering: {cause}")))?;
    if generic.scalar != ScalarType::I64 || generic.function != "sum_generic" {
        return Err(error("entry requires selected sum_generic<i64> IR"));
    }
    let rechecked_generic = lower_generic_fold(
        core.path,
        core.text,
        &SourceGenericInstance {
            package: generic.package.clone(),
            function: generic.function.clone(),
            type_arguments: vec!["i64".into()],
        },
        generic.overflow,
    )
    .map_err(|cause| error(format!("selected generic source recheck: {cause}")))?;
    if &rechecked_generic != generic {
        return Err(error(
            "selected generic IR differs from core source snapshot",
        ));
    }
    let mut values = BTreeMap::<String, Value>::new();
    for computation in &entry.computations {
        let value = match &computation.operation {
            EntryOperation::PrimeGrid { limit } => Value::Cluster(
                cluster
                    .from_prime_grid(*limit, &prime, &point, generic.overflow)
                    .map_err(|cause| error(format!("prime grid evaluation: {cause}")))?,
            ),
            EntryOperation::TotalPerimeter { cluster: name } => {
                let Value::Cluster(value) = require(&values, name)? else {
                    return Err(error(format!("{name} is not a cluster")));
                };
                Value::Integer(
                    cluster
                        .total_perimeter(value, &point, generic.overflow)
                        .map_err(|cause| error(format!("perimeter evaluation: {cause}")))?,
                )
            }
            EntryOperation::Centroid { cluster: name } => {
                let Value::Cluster(value) = require(&values, name)? else {
                    return Err(error(format!("{name} is not a cluster")));
                };
                Value::OptionalPoint(
                    cluster
                        .centroid(value, &point, generic.overflow)
                        .map_err(|cause| error(format!("centroid evaluation: {cause}")))?,
                )
            }
            EntryOperation::CollectXCoordinates { cluster: name } => {
                let Value::Cluster(value) = require(&values, name)? else {
                    return Err(error(format!("{name} is not a cluster")));
                };
                Value::Integers(
                    value
                        .points
                        .iter()
                        .map(|item| {
                            item.field("x")
                                .ok_or_else(|| error("cluster point has no x field"))
                        })
                        .collect::<Result<_, _>>()?,
                )
            }
            EntryOperation::SumGenericI64 { values: name } => {
                let Value::Integers(items) = require(&values, name)? else {
                    return Err(error(format!("{name} is not an i64 vector")));
                };
                Value::Integer(
                    generic
                        .evaluate_i64(items)
                        .map_err(|cause| error(format!("generic fold evaluation: {cause}")))?,
                )
            }
        };
        if values
            .insert(computation.binding.name.clone(), value)
            .is_some()
        {
            return Err(error(format!(
                "duplicate entry binding {}",
                computation.binding.name
            )));
        }
    }

    let mut points_len = None;
    let mut perimeter = None;
    let mut centroid_debug = None;
    let mut expected_sum_x = None;
    for print in &entry.prints {
        match &print.value {
            EntryOutputValue::PointsLength { cluster: name } => {
                let Value::Cluster(value) = require(&values, name)? else {
                    return Err(error(format!("{name} is not a cluster")));
                };
                points_len = Some(value.points.len());
            }
            EntryOutputValue::Perimeter { binding } => {
                let Value::Integer(value) = require(&values, binding)? else {
                    return Err(error(format!("{binding} is not an integer")));
                };
                perimeter = Some(*value);
            }
            EntryOutputValue::CentroidDebug { binding } => {
                let Value::OptionalPoint(value) = require(&values, binding)? else {
                    return Err(error(format!("{binding} is not an optional point")));
                };
                centroid_debug = Some(render_optional_point(
                    value.as_ref(),
                    &point.fields,
                    &point.struct_name,
                )?);
            }
            EntryOutputValue::SumX { binding } => {
                let Value::Integer(value) = require(&values, binding)? else {
                    return Err(error(format!("{binding} is not an integer")));
                };
                expected_sum_x = Some(*value);
            }
        }
    }
    let xs = match require(&values, "xs")? {
        Value::Integers(items) => items.clone(),
        _ => return Err(error("xs is not an i64 vector")),
    };
    Ok(EntrySpecialization {
        entry,
        points_len: points_len.ok_or_else(|| error("missing points print"))?,
        perimeter: perimeter.ok_or_else(|| error("missing perimeter print"))?,
        centroid_debug: centroid_debug.ok_or_else(|| error("missing centroid print"))?,
        xs,
        expected_sum_x: expected_sum_x.ok_or_else(|| error("missing sum print"))?,
    })
}

fn require<'a>(
    values: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<&'a Value, EntrySpecializationError> {
    values
        .get(name)
        .ok_or_else(|| error(format!("entry binding {name:?} is unavailable")))
}

fn render_optional_point(
    value: Option<&PointValue>,
    field_order: &[String],
    struct_name: &str,
) -> Result<String, EntrySpecializationError> {
    let Some(value) = value else {
        return Ok("None".to_string());
    };
    if value.struct_name != struct_name {
        return Err(error("centroid point type differs from lowered struct"));
    }
    let fields = field_order
        .iter()
        .map(|field| {
            value
                .field(field)
                .map(|number| format!("{field}: {number}"))
                .ok_or_else(|| error(format!("centroid is missing {field}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("Some({struct_name} {{ {} }})", fields.join(", ")))
}

#[derive(Debug, Clone)]
pub struct LinkedEntryArtifact {
    pub entry_object: std::path::PathBuf,
    pub generic_object: std::path::PathBuf,
    pub executable: std::path::PathBuf,
}

/// Emit the owned entry and selected generic objects, then invoke the macOS
/// toolchain solely as a final linker. The invocation contains object paths
/// only; no C or Rust source is handed to the external toolchain.
pub fn link_macos_aarch64_entry(
    specialization: &EntrySpecialization,
    generic: &NativeGenericObject,
    executable: &Path,
) -> Result<LinkedEntryArtifact, EntrySpecializationError> {
    if generic.target != NativeGenericTarget::MacOsAarch64 {
        return Err(error("selected generic object is not macOS AArch64"));
    }
    let parent = executable
        .parent()
        .ok_or_else(|| error("executable path has no parent"))?;
    fs::create_dir_all(parent).map_err(|cause| error(format!("artifact directory: {cause}")))?;
    let entry_object = executable.with_extension("entry.o");
    let generic_object = executable.with_extension("generic.o");
    let entry_bytes = generate_macho_aarch64_entry_object(
        &specialization.entry,
        specialization.points_len,
        specialization.perimeter,
        &specialization.centroid_debug,
        &specialization.xs,
        &generic.symbol_name,
    )
    .map_err(|cause| error(format!("native entry code generation: {cause}")))?;
    fs::write(&entry_object, entry_bytes)
        .map_err(|cause| error(format!("entry object publication: {cause}")))?;
    fs::write(&generic_object, &generic.bytes)
        .map_err(|cause| error(format!("generic object publication: {cause}")))?;
    let result = Command::new("/usr/bin/cc")
        .arg("-o")
        .arg(executable)
        .arg(&entry_object)
        .arg(&generic_object)
        .output()
        .map_err(|cause| error(format!("native linker launch: {cause}")))?;
    if !result.status.success() {
        return Err(error(format!(
            "native linker rejected owned objects: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(LinkedEntryArtifact {
        entry_object,
        generic_object,
        executable: executable.to_path_buf(),
    })
}

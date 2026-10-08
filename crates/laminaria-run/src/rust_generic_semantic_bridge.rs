//! Executes demanded generic semantic lowering from the artifact feedback plan.
//! Only selected instances enter owned source-to-IR work; the eager path is
//! available for an executed-work comparison on the same source snapshots.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use laminaria_ir::native_aarch64::{
    generate_macho_aarch64_generic_fold_object, NativeCodegenError,
};
use laminaria_ir::rust_generic_demand::RustGenericInstance as SourceInstance;
use laminaria_ir::rust_generic_fold::{
    lower_generic_fold, GenericFoldError, GenericFoldIr, OverflowPolicy, ScalarType,
};
use laminaria_plan::rust_cross_layer::{
    RustArtifactFeedbackPlan, RustGenericInstance, RustGenericWorkStage,
};

pub struct GenericProviderSource<'a> {
    pub path: PathBuf,
    pub text: &'a str,
}

pub struct NativeGenericObject {
    pub symbol_name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum GenericSemanticExecutionError {
    MissingProviderSource(String),
    LoweringFailed {
        instance: RustGenericInstance,
        detail: GenericFoldError,
    },
    MissingLoweredInstance(RustGenericInstance),
    InconsistentLoweredInstance(RustGenericInstance),
    CodegenFailed {
        instance: RustGenericInstance,
        detail: NativeCodegenError,
    },
}

impl std::fmt::Display for GenericSemanticExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProviderSource(package) => {
                write!(f, "no source snapshot for selected provider {package:?}")
            }
            Self::LoweringFailed { instance, detail } => {
                write!(
                    f,
                    "cannot lower selected generic instance {instance:?}: {detail}"
                )
            }
            Self::MissingLoweredInstance(instance) => {
                write!(f, "selected codegen lacks lowered instance {instance:?}")
            }
            Self::InconsistentLoweredInstance(instance) => {
                write!(f, "selected codegen has mismatched IR for {instance:?}")
            }
            Self::CodegenFailed { instance, detail } => {
                write!(
                    f,
                    "cannot generate selected generic instance {instance:?}: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for GenericSemanticExecutionError {}

/// Executes one source-derived generic IR lowering per demanded instance.
/// The returned map contains only successful work. An unsupported selected
/// instance rejects the entire result; a partial IR set is never published.
pub fn lower_selected_generic_folds(
    plan: &RustArtifactFeedbackPlan,
    provider_sources: &BTreeMap<String, GenericProviderSource<'_>>,
    overflow: OverflowPolicy,
    feedback: bool,
) -> Result<BTreeMap<RustGenericInstance, GenericFoldIr>, GenericSemanticExecutionError> {
    let work = if feedback {
        &plan.generic_work_plan.feedback_work
    } else {
        &plan.generic_work_plan.eager_work
    };
    let instances: BTreeSet<_> = work
        .iter()
        .filter(|work| work.stage == RustGenericWorkStage::LowerToIr)
        .map(|work| work.instance.clone())
        .collect();
    let mut lowered = BTreeMap::new();
    for instance in instances {
        let source = provider_sources.get(&instance.package).ok_or_else(|| {
            GenericSemanticExecutionError::MissingProviderSource(instance.package.clone())
        })?;
        let source_instance = SourceInstance {
            package: instance.package.clone(),
            function: instance.function.clone(),
            type_arguments: instance.type_arguments.clone(),
        };
        let ir = lower_generic_fold(&source.path, source.text, &source_instance, overflow)
            .map_err(|detail| GenericSemanticExecutionError::LoweringFailed {
                instance: instance.clone(),
                detail,
            })?;
        lowered.insert(instance, ir);
    }
    Ok(lowered)
}

/// Generates one owned Mach-O object per selected generic Codegen identity.
/// The caller must supply IR lowered from the same source snapshot and plan.
pub fn generate_selected_generic_fold_objects(
    plan: &RustArtifactFeedbackPlan,
    lowered: &BTreeMap<RustGenericInstance, GenericFoldIr>,
    feedback: bool,
) -> Result<BTreeMap<RustGenericInstance, NativeGenericObject>, GenericSemanticExecutionError> {
    let work = if feedback {
        &plan.generic_work_plan.feedback_work
    } else {
        &plan.generic_work_plan.eager_work
    };
    let instances: BTreeSet<_> = work
        .iter()
        .filter(|work| work.stage == RustGenericWorkStage::Codegen)
        .map(|work| work.instance.clone())
        .collect();
    let mut objects = BTreeMap::new();
    for instance in instances {
        let ir = lowered.get(&instance).ok_or_else(|| {
            GenericSemanticExecutionError::MissingLoweredInstance(instance.clone())
        })?;
        let scalar = match instance.type_arguments.as_slice() {
            [name] if name == "i32" => ScalarType::I32,
            [name] if name == "i64" => ScalarType::I64,
            _ => {
                return Err(GenericSemanticExecutionError::InconsistentLoweredInstance(
                    instance,
                ))
            }
        };
        if ir.package != instance.package || ir.function != instance.function || ir.scalar != scalar
        {
            return Err(GenericSemanticExecutionError::InconsistentLoweredInstance(
                instance,
            ));
        }
        let symbol_name = format!(
            "laminaria_{}_{}_{}",
            hex_bytes(instance.package.as_bytes()),
            hex_bytes(instance.function.as_bytes()),
            instance.type_arguments[0]
        );
        let bytes =
            generate_macho_aarch64_generic_fold_object(ir, &symbol_name).map_err(|detail| {
                GenericSemanticExecutionError::CodegenFailed {
                    instance: instance.clone(),
                    detail,
                }
            })?;
        objects.insert(instance, NativeGenericObject { symbol_name, bytes });
    }
    Ok(objects)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    hex
}

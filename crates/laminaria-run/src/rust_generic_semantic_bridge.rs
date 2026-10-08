//! Executes demanded generic semantic lowering from the artifact feedback plan.
//! Only selected instances enter owned source-to-IR work; the eager path is
//! available for an executed-work comparison on the same source snapshots.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use laminaria_ir::rust_generic_demand::RustGenericInstance as SourceInstance;
use laminaria_ir::rust_generic_fold::{
    lower_generic_fold, GenericFoldError, GenericFoldIr, OverflowPolicy,
};
use laminaria_plan::rust_cross_layer::{
    RustArtifactFeedbackPlan, RustGenericInstance, RustGenericWorkStage,
};

pub struct GenericProviderSource<'a> {
    pub path: PathBuf,
    pub text: &'a str,
}

#[derive(Debug)]
pub enum GenericSemanticExecutionError {
    MissingProviderSource(String),
    LoweringFailed {
        instance: RustGenericInstance,
        detail: GenericFoldError,
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

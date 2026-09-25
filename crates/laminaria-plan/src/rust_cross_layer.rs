//! The pure P0 selection kernel for issue #50.
//!
//! A caller supplies Cargo-compatible package candidates plus references
//! derived from Rust source.  The latter decide which candidate units may
//! enter parse/typecheck/lower/codegen; this module never reads files or
//! starts a compiler.

use std::collections::BTreeSet;

/// The compiler work a package unit would otherwise require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RustWorkStage {
    Parse,
    Typecheck,
    Lower,
    Codegen,
}

/// One planned unit of owned Rust work.  Planning it is not evidence that it
/// ran; the set makes the eager/feedback comparison explicit before execution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RustWork {
    pub package: String,
    pub stage: RustWorkStage,
}

/// The minimum facts needed to select Rust package units without compiling
/// them. `semantic_references` are crate roots from owned source discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCrossLayerInput {
    pub requested_artifact: String,
    pub entry_package: String,
    pub package_candidates: BTreeSet<String>,
    /// The entry package's Cargo-compatible `[dependencies]` names. Source
    /// references cannot make an unrelated workspace member a provider.
    pub declared_dependency_packages: BTreeSet<String>,
    pub semantic_references: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCrossLayerPlan {
    pub requested_artifact: String,
    pub selected_packages: BTreeSet<String>,
    pub pruned_packages: BTreeSet<String>,
    pub eager_work: BTreeSet<RustWork>,
    pub feedback_work: BTreeSet<RustWork>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustCrossLayerReject {
    MissingEntryPackage(String),
    MissingSemanticProvider(String),
    AmbiguousSemanticProvider {
        reference: String,
        candidates: Vec<String>,
    },
}

/// A demand-relative generic function instance. This is intentionally a
/// planning identity, not evidence that the instance was compiled or emitted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RustGenericInstance {
    pub package: String,
    pub function: String,
    pub type_arguments: Vec<String>,
}

/// Compiler work scoped to one concrete generic instance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RustGenericWork {
    pub instance: RustGenericInstance,
    pub stage: RustGenericWorkStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RustGenericWorkStage {
    SemanticAnalysis,
    LowerToIr,
    Monomorphize,
    Codegen,
    SymbolLiveness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustGenericWorkPlan {
    pub eager_work: BTreeSet<RustGenericWork>,
    pub feedback_work: BTreeSet<RustGenericWork>,
    pub pruned_work: BTreeSet<RustGenericWork>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustGenericWorkReject {
    RequestedInstanceNotInEagerSet(RustGenericInstance),
}

/// One artifact-rooted view joining package selection with generic-instance
/// demand. The package and specialization plans remain independently
/// inspectable, while provider membership is checked at their boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustArtifactFeedbackInput {
    pub package_input: RustCrossLayerInput,
    pub eager_instances: BTreeSet<RustGenericInstance>,
    pub requested_instances: BTreeSet<RustGenericInstance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustArtifactFeedbackPlan {
    pub package_plan: RustCrossLayerPlan,
    pub generic_work_plan: RustGenericWorkPlan,
}

/// One executable identity in the artifact-rooted bridge. Package work and
/// generic-instance work remain typed separately inside each variant, but an
/// executor can compare the complete eager and feedback sets without silently
/// dropping one layer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RustExecutionWork {
    Package(RustWork),
    Generic(RustGenericWork),
}

impl RustArtifactFeedbackPlan {
    pub fn eager_execution_work(&self) -> BTreeSet<RustExecutionWork> {
        self.package_plan
            .eager_work
            .iter()
            .cloned()
            .map(RustExecutionWork::Package)
            .chain(
                self.generic_work_plan
                    .eager_work
                    .iter()
                    .cloned()
                    .map(RustExecutionWork::Generic),
            )
            .collect()
    }

    pub fn feedback_execution_work(&self) -> BTreeSet<RustExecutionWork> {
        self.package_plan
            .feedback_work
            .iter()
            .cloned()
            .map(RustExecutionWork::Package)
            .chain(
                self.generic_work_plan
                    .feedback_work
                    .iter()
                    .cloned()
                    .map(RustExecutionWork::Generic),
            )
            .collect()
    }

    pub fn pruned_execution_work(&self) -> BTreeSet<RustExecutionWork> {
        self.eager_execution_work()
            .difference(&self.feedback_execution_work())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustArtifactFeedbackReject {
    Package(RustCrossLayerReject),
    Generic(RustGenericWorkReject),
    GenericProviderNotCandidate(RustGenericInstance),
    GenericProviderNotSelected(RustGenericInstance),
}

/// Composes artifact-rooted package selection and generic demand. A generic
/// specialization cannot be accepted if its provider package was not selected
/// by the same artifact request.
pub fn plan_rust_artifact_feedback(
    input: &RustArtifactFeedbackInput,
) -> Result<RustArtifactFeedbackPlan, RustArtifactFeedbackReject> {
    let package_plan =
        plan_rust_cross_layer(&input.package_input).map_err(RustArtifactFeedbackReject::Package)?;
    let generic_work_plan =
        plan_rust_generic_work(&input.eager_instances, &input.requested_instances)
            .map_err(RustArtifactFeedbackReject::Generic)?;

    for instance in &input.eager_instances {
        if !input
            .package_input
            .package_candidates
            .contains(&instance.package)
        {
            return Err(RustArtifactFeedbackReject::GenericProviderNotCandidate(
                instance.clone(),
            ));
        }
    }

    for instance in &input.requested_instances {
        if !package_plan.selected_packages.contains(&instance.package) {
            return Err(RustArtifactFeedbackReject::GenericProviderNotSelected(
                instance.clone(),
            ));
        }
    }

    Ok(RustArtifactFeedbackPlan {
        package_plan,
        generic_work_plan,
    })
}

/// Selects only generic instances demanded by the requested artifact from a
/// conservative eager set. Rejecting a non-subset is important: an incomplete
/// eager inventory must not be mistaken for successful pruning.
pub fn plan_rust_generic_work(
    eager_instances: &BTreeSet<RustGenericInstance>,
    requested_instances: &BTreeSet<RustGenericInstance>,
) -> Result<RustGenericWorkPlan, RustGenericWorkReject> {
    if let Some(missing) = requested_instances.difference(eager_instances).next() {
        return Err(RustGenericWorkReject::RequestedInstanceNotInEagerSet(
            missing.clone(),
        ));
    }
    let work_for = |instances: &BTreeSet<RustGenericInstance>| {
        instances
            .iter()
            .flat_map(|instance| {
                [
                    RustGenericWorkStage::SemanticAnalysis,
                    RustGenericWorkStage::LowerToIr,
                    RustGenericWorkStage::Monomorphize,
                    RustGenericWorkStage::Codegen,
                    RustGenericWorkStage::SymbolLiveness,
                ]
                .into_iter()
                .map(move |stage| RustGenericWork {
                    instance: instance.clone(),
                    stage,
                })
            })
            .collect::<BTreeSet<_>>()
    };
    let eager_work = work_for(eager_instances);
    let feedback_work = work_for(requested_instances);
    let pruned_work = eager_work.difference(&feedback_work).cloned().collect();
    Ok(RustGenericWorkPlan {
        eager_work,
        feedback_work,
        pruned_work,
    })
}

fn crate_name(package: &str) -> String {
    package.replace('-', "_")
}

fn work_for(packages: &BTreeSet<String>) -> BTreeSet<RustWork> {
    packages
        .iter()
        .flat_map(|package| {
            [
                RustWorkStage::Parse,
                RustWorkStage::Typecheck,
                RustWorkStage::Lower,
                RustWorkStage::Codegen,
            ]
            .into_iter()
            .map(move |stage| RustWork {
                package: package.clone(),
                stage,
            })
        })
        .collect()
}

/// Plans an eager baseline and the feedback-constrained work set.
///
/// A source reference matches Cargo's Rust crate spelling (`-` in a package
/// name becomes `_` in source). Multiple matching candidates are rejected:
/// selecting one arbitrarily would make early pruning unsound.
pub fn plan_rust_cross_layer(
    input: &RustCrossLayerInput,
) -> Result<RustCrossLayerPlan, RustCrossLayerReject> {
    if !input.package_candidates.contains(&input.entry_package) {
        return Err(RustCrossLayerReject::MissingEntryPackage(
            input.entry_package.clone(),
        ));
    }

    let mut selected_packages = BTreeSet::from([input.entry_package.clone()]);
    for reference in &input.semantic_references {
        let matches: Vec<String> = input
            .package_candidates
            .iter()
            .filter(|candidate| {
                input.declared_dependency_packages.contains(*candidate)
                    && crate_name(candidate) == *reference
            })
            .cloned()
            .collect();
        match matches.as_slice() {
            [] => {
                return Err(RustCrossLayerReject::MissingSemanticProvider(
                    reference.clone(),
                ))
            }
            [package] => {
                selected_packages.insert(package.clone());
            }
            _ => {
                return Err(RustCrossLayerReject::AmbiguousSemanticProvider {
                    reference: reference.clone(),
                    candidates: matches,
                })
            }
        }
    }

    let pruned_packages = input
        .package_candidates
        .difference(&selected_packages)
        .cloned()
        .collect();
    Ok(RustCrossLayerPlan {
        requested_artifact: input.requested_artifact.clone(),
        eager_work: work_for(&input.package_candidates),
        feedback_work: work_for(&selected_packages),
        selected_packages,
        pruned_packages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> RustCrossLayerInput {
        RustCrossLayerInput {
            requested_artifact: "fixture-bin".to_string(),
            entry_package: "fixture-bin".to_string(),
            package_candidates: ["fixture-bin", "used-core", "unused-pkg"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            declared_dependency_packages: ["used-core"].into_iter().map(str::to_string).collect(),
            semantic_references: ["used_core"].into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn source_semantics_prune_an_unreferenced_candidate_before_every_compiler_stage() {
        let plan = plan_rust_cross_layer(&input()).expect("the referenced crate is available");

        assert_eq!(
            plan.selected_packages,
            ["fixture-bin", "used-core"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        assert_eq!(
            plan.pruned_packages,
            BTreeSet::from(["unused-pkg".to_string()])
        );
        assert_eq!(plan.eager_work.len(), 12);
        assert_eq!(plan.feedback_work.len(), 8);
        assert!(
            !plan
                .feedback_work
                .iter()
                .any(|work| work.package == "unused-pkg"),
            "an unreferenced candidate must not reach parse/typecheck/lower/codegen"
        );
    }

    #[test]
    fn absent_source_requirement_is_rejected_without_planning_compiler_work() {
        let mut input = input();
        input.semantic_references.insert("missing".to_string());

        assert_eq!(
            plan_rust_cross_layer(&input),
            Err(RustCrossLayerReject::MissingSemanticProvider(
                "missing".to_string()
            ))
        );
    }

    #[test]
    fn an_unrelated_workspace_member_cannot_become_a_provider() {
        let mut input = input();
        input.semantic_references = ["unused_pkg"].into_iter().map(str::to_string).collect();

        assert_eq!(
            plan_rust_cross_layer(&input),
            Err(RustCrossLayerReject::MissingSemanticProvider(
                "unused_pkg".to_string()
            ))
        );
    }

    #[test]
    fn generic_feedback_rejects_demands_missing_from_eager_inventory() {
        let demanded = RustGenericInstance {
            package: "provider".to_string(),
            function: "convert".to_string(),
            type_arguments: vec!["u8".to_string()],
        };
        assert_eq!(
            plan_rust_generic_work(&BTreeSet::new(), &BTreeSet::from([demanded.clone()])),
            Err(RustGenericWorkReject::RequestedInstanceNotInEagerSet(
                demanded
            ))
        );
    }

    #[test]
    fn artifact_feedback_rejects_a_generic_provider_outside_selected_packages() {
        let instance = RustGenericInstance {
            package: "unused-helper".to_string(),
            function: "convert".to_string(),
            type_arguments: vec!["u8".to_string()],
        };
        let input = RustArtifactFeedbackInput {
            package_input: RustCrossLayerInput {
                requested_artifact: "app".to_string(),
                entry_package: "app".to_string(),
                package_candidates: ["app", "unused-helper"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                declared_dependency_packages: BTreeSet::new(),
                semantic_references: BTreeSet::new(),
            },
            eager_instances: BTreeSet::from([instance.clone()]),
            requested_instances: BTreeSet::from([instance.clone()]),
        };
        assert_eq!(
            plan_rust_artifact_feedback(&input),
            Err(RustArtifactFeedbackReject::GenericProviderNotSelected(
                instance
            ))
        );
    }

    #[test]
    fn artifact_feedback_rejects_a_generic_provider_outside_package_candidates() {
        let instance = RustGenericInstance {
            package: "missing-provider".to_string(),
            function: "convert".to_string(),
            type_arguments: vec!["u8".to_string()],
        };
        let input = RustArtifactFeedbackInput {
            package_input: RustCrossLayerInput {
                requested_artifact: "app".to_string(),
                entry_package: "app".to_string(),
                package_candidates: BTreeSet::from(["app".to_string()]),
                declared_dependency_packages: BTreeSet::new(),
                semantic_references: BTreeSet::new(),
            },
            eager_instances: BTreeSet::from([instance.clone()]),
            requested_instances: BTreeSet::from([instance.clone()]),
        };
        assert_eq!(
            plan_rust_artifact_feedback(&input),
            Err(RustArtifactFeedbackReject::GenericProviderNotCandidate(
                instance
            ))
        );
    }

    #[test]
    fn artifact_feedback_exposes_a_strictly_smaller_combined_execution_set() {
        let instance = RustGenericInstance {
            package: "app".to_string(),
            function: "convert".to_string(),
            type_arguments: vec!["u8".to_string()],
        };
        let input = RustArtifactFeedbackInput {
            package_input: RustCrossLayerInput {
                requested_artifact: "app".to_string(),
                entry_package: "app".to_string(),
                package_candidates: ["app", "unused"].into_iter().map(str::to_string).collect(),
                declared_dependency_packages: BTreeSet::new(),
                semantic_references: BTreeSet::new(),
            },
            eager_instances: BTreeSet::from([instance.clone()]),
            requested_instances: BTreeSet::from([instance]),
        };
        let plan = plan_rust_artifact_feedback(&input).expect("the provider is selected");
        assert!(plan
            .feedback_execution_work()
            .is_subset(&plan.eager_execution_work()));
        assert!(!plan.pruned_execution_work().is_empty());
        assert_eq!(
            plan.eager_execution_work().len() - plan.feedback_execution_work().len(),
            plan.pruned_execution_work().len()
        );
    }
}

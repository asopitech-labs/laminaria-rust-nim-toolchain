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
}

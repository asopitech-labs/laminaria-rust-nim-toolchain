//! The pure P0 selection kernel for issue #50.
//!
//! A caller supplies Cargo-compatible package candidates plus references
//! derived from Rust source.  The latter decide which candidate units may
//! enter parse/typecheck/lower/codegen; this module never reads files or
//! starts a compiler.

use std::collections::{BTreeMap, BTreeSet};

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

    /// Converts executor-owned action metadata into the selected action IDs
    /// for this artifact. The action builder owns the identity mapping; this
    /// method owns the demand decision and never guesses from an action ID.
    pub fn select_action_ids(
        &self,
        action_work: &BTreeMap<String, RustExecutionWork>,
        feedback: bool,
    ) -> BTreeSet<String> {
        let selected = if feedback {
            self.feedback_execution_work()
        } else {
            self.eager_execution_work()
        };
        action_work
            .iter()
            .filter(|(_, work)| selected.contains(*work))
            .map(|(action_id, _)| action_id.clone())
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

/// Caller-independent identity used by the A7 worklist.  The fields are
/// deliberately semantic inputs only; no module, caller, or CGU identity is
/// permitted here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RustInstantiationKey {
    pub declaration: String,
    pub type_arguments: Vec<String>,
    pub const_arguments: Vec<String>,
    pub abi_target: String,
    pub active_features: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RustA7RequestSource {
    SemanticEdge(String),
    Export(String),
    Ffi(String),
    Reflection(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustA7Request {
    pub key: RustInstantiationKey,
    pub source: RustA7RequestSource,
}

/// A semantic definition supplies the transitive generic calls for one key.
/// Its fingerprint makes an inconsistent duplicate an explicit rejection
/// rather than a first-writer-wins race.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustA7Definition {
    pub fingerprint: String,
    pub dependencies: Vec<RustInstantiationKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustA7Input {
    pub roots: Vec<RustA7Request>,
    pub definitions: BTreeMap<RustInstantiationKey, RustA7Definition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustA7WorkItem {
    pub key: RustInstantiationKey,
    pub fingerprint: String,
    pub dependencies: BTreeSet<RustInstantiationKey>,
    pub usage_sources: BTreeSet<RustA7RequestSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustA7Plan {
    pub work_items: Vec<RustA7WorkItem>,
    pub mentioned: BTreeSet<RustInstantiationKey>,
    pub visited: BTreeSet<RustInstantiationKey>,
    pub usage_map: BTreeMap<RustInstantiationKey, BTreeSet<RustA7RequestSource>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustA7Reject {
    MissingDefinition(RustInstantiationKey),
    MissingDependency {
        owner: RustInstantiationKey,
        dependency: RustInstantiationKey,
    },
    ConflictingDefinition {
        key: RustInstantiationKey,
        fingerprints: BTreeSet<String>,
    },
}

/// Collects dependency-closed A7 work with a deterministic BFS.  `mentioned`
/// reserves a key when it enters the queue, while `visited` records completed
/// expansion; keeping those states separate prevents a diamond or recursive
/// edge from losing an unexpanded dependency.
pub fn plan_rust_a7_worklist(
    input: &RustA7Input,
) -> Result<RustA7Plan, Box<RustA7Reject>> {
    use std::collections::VecDeque;

    let mut queue = VecDeque::new();
    let mut mentioned = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut usage_map: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    let mut sources: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();

    let mut roots = input.roots.clone();
    roots.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then(left.source.cmp(&right.source))
    });
    for request in roots {
        usage_map
            .entry(request.key.clone())
            .or_default()
            .insert(request.source.clone());
        sources
            .entry(request.key.clone())
            .or_default()
            .insert(request.source.clone());
        if mentioned.insert(request.key.clone()) {
            queue.push_back(request.key);
        }
    }

    let mut committed: BTreeMap<RustInstantiationKey, RustA7WorkItem> = BTreeMap::new();
    while let Some(key) = queue.pop_front() {
        if visited.contains(&key) {
            continue;
        }
        let definition = input
            .definitions
            .get(&key)
            .ok_or_else(|| Box::new(RustA7Reject::MissingDefinition(key.clone())))?;
        let mut fingerprints = BTreeSet::from([definition.fingerprint.clone()]);
        if let Some(existing) = committed.get(&key) {
            fingerprints.insert(existing.fingerprint.clone());
        }
        if fingerprints.len() > 1 {
            return Err(Box::new(RustA7Reject::ConflictingDefinition { key, fingerprints }));
        }

        let dependencies: BTreeSet<_> = definition.dependencies.iter().cloned().collect();
        for dependency in &dependencies {
            if !input.definitions.contains_key(dependency) {
                return Err(Box::new(RustA7Reject::MissingDependency {
                    owner: key.clone(),
                    dependency: dependency.clone(),
                }));
            }
            usage_map.entry(dependency.clone()).or_default();
            if mentioned.insert(dependency.clone()) {
                queue.push_back(dependency.clone());
            }
        }
        committed.insert(
            key.clone(),
            RustA7WorkItem {
                key: key.clone(),
                fingerprint: definition.fingerprint.clone(),
                dependencies,
                usage_sources: sources.remove(&key).unwrap_or_default(),
            },
        );
        visited.insert(key);
    }

    Ok(RustA7Plan {
        work_items: committed.into_values().collect(),
        mentioned,
        visited,
        usage_map,
    })
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

    fn a7_key(name: &str) -> RustInstantiationKey {
        RustInstantiationKey {
            declaration: name.to_string(),
            type_arguments: vec!["u8".to_string()],
            const_arguments: vec![],
            abi_target: "x86_64-pc-windows-msvc".to_string(),
            active_features: BTreeSet::new(),
        }
    }

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
    fn a7_bfs_deduplicates_a_diamond_and_returns_canonical_items() {
        let (root, left, right, leaf) = (
            a7_key("root"),
            a7_key("left"),
            a7_key("right"),
            a7_key("leaf"),
        );
        let plan = plan_rust_a7_worklist(&RustA7Input {
            roots: vec![RustA7Request {
                key: root.clone(),
                source: RustA7RequestSource::Export("main".into()),
            }],
            definitions: BTreeMap::from([
                (
                    root.clone(),
                    RustA7Definition {
                        fingerprint: "r".into(),
                        dependencies: vec![right.clone(), left.clone()],
                    },
                ),
                (
                    left.clone(),
                    RustA7Definition {
                        fingerprint: "l".into(),
                        dependencies: vec![leaf.clone()],
                    },
                ),
                (
                    right.clone(),
                    RustA7Definition {
                        fingerprint: "q".into(),
                        dependencies: vec![leaf.clone()],
                    },
                ),
                (
                    leaf.clone(),
                    RustA7Definition {
                        fingerprint: "d".into(),
                        dependencies: vec![],
                    },
                ),
            ]),
        })
        .expect("diamond is a valid closed graph");
        assert_eq!(plan.work_items.len(), 4);
        assert_eq!(plan.visited, plan.mentioned);
        assert_eq!(plan.work_items[0].key, leaf);
        assert_eq!(plan.work_items[3].key, root);
    }

    #[test]
    fn a7_bfs_handles_recursive_keys_without_requeueing_forever() {
        let key = a7_key("recursive");
        let plan = plan_rust_a7_worklist(&RustA7Input {
            roots: vec![RustA7Request {
                key: key.clone(),
                source: RustA7RequestSource::Ffi("f".into()),
            }],
            definitions: BTreeMap::from([(
                key.clone(),
                RustA7Definition {
                    fingerprint: "r".into(),
                    dependencies: vec![key.clone()],
                },
            )]),
        })
        .expect("recursive generic is finite after key reservation");
        assert_eq!(plan.work_items.len(), 1);
        assert_eq!(plan.work_items[0].dependencies, BTreeSet::from([key]));
    }

    #[test]
    fn a7_bfs_is_independent_of_root_submission_order() {
        let (a, b) = (a7_key("a"), a7_key("b"));
        let definitions = BTreeMap::from([
            (
                a.clone(),
                RustA7Definition {
                    fingerprint: "a".into(),
                    dependencies: vec![],
                },
            ),
            (
                b.clone(),
                RustA7Definition {
                    fingerprint: "b".into(),
                    dependencies: vec![],
                },
            ),
        ]);
        let make = |roots| {
            plan_rust_a7_worklist(&RustA7Input {
                roots,
                definitions: definitions.clone(),
            })
            .unwrap()
        };
        let first = make(vec![
            RustA7Request {
                key: b.clone(),
                source: RustA7RequestSource::SemanticEdge("z".into()),
            },
            RustA7Request {
                key: a.clone(),
                source: RustA7RequestSource::SemanticEdge("y".into()),
            },
        ]);
        let second = make(vec![
            RustA7Request {
                key: a,
                source: RustA7RequestSource::SemanticEdge("y".into()),
            },
            RustA7Request {
                key: b,
                source: RustA7RequestSource::SemanticEdge("z".into()),
            },
        ]);
        assert_eq!(first, second);
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

    #[test]
    fn artifact_feedback_selects_action_ids_from_typed_work_metadata() {
        let input = RustArtifactFeedbackInput {
            package_input: RustCrossLayerInput {
                requested_artifact: "app".to_string(),
                entry_package: "app".to_string(),
                package_candidates: ["app", "unused"].into_iter().map(str::to_string).collect(),
                declared_dependency_packages: BTreeSet::new(),
                semantic_references: BTreeSet::new(),
            },
            eager_instances: BTreeSet::new(),
            requested_instances: BTreeSet::new(),
        };
        let plan = plan_rust_artifact_feedback(&input).expect("entry package is selected");
        let action_work = BTreeMap::from([
            (
                "selected".to_string(),
                RustExecutionWork::Package(RustWork {
                    package: "app".to_string(),
                    stage: RustWorkStage::Parse,
                }),
            ),
            (
                "pruned".to_string(),
                RustExecutionWork::Package(RustWork {
                    package: "unused".to_string(),
                    stage: RustWorkStage::Parse,
                }),
            ),
        ]);
        assert_eq!(
            plan.select_action_ids(&action_work, true),
            BTreeSet::from(["selected".to_string()])
        );
        assert_eq!(
            plan.select_action_ids(&action_work, false),
            BTreeSet::from(["selected".to_string(), "pruned".to_string()])
        );
    }
}

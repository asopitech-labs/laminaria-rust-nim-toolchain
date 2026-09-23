//! Issue #89 Checkpoint A: the production contract that maps the canonical
//! logical activities onto physical work without turning the whole build into
//! one sequential pipeline.
//!
//! Logical identity and execution grouping are deliberately separate.  A
//! planner may currently group all Rust frontend/backend activities into one
//! `CompileRustObject` invocation, for example, but A2 and A11 remain distinct
//! identities with distinct inputs, outputs and commit boundaries.  A future
//! planner may split or coalesce those identities without changing their
//! semantic dependency graph or cache identity.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Action, ActionKind, ArtifactRef, ExecutionPlan, PlanningInput};

pub const PHYSICAL_WORK_CONTRACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicalActivity {
    A2MemberManifest,
    A3PackageResolution,
    A4SourceUnitGraph,
    A5SourceUnitSelection,
    A6SemanticAnalysis,
    A7Monomorphization,
    A8OptimizationDecision,
    A9CodegenPreparation,
    A10CodeGeneration,
    A11SymbolRegistration,
    A12FinalLink,
    N2DependencyResolution,
    N3BuildInvocation,
    N4ModuleReachability,
    N5SemanticAnalysisAndCGeneration,
    N6Monomorphization,
    N7CCompilation,
    N8NimLink,
    N9ExportRegistration,
    ForeignTargetResolution,
    ForeignArtifactResolution,
    ForeignAdapterGeneration,
    ForeignCompileUnitPreparation,
    ForeignObjectCompilation,
}

impl LogicalActivity {
    pub const ALL: [Self; 24] = [
        Self::A2MemberManifest,
        Self::A3PackageResolution,
        Self::A4SourceUnitGraph,
        Self::A5SourceUnitSelection,
        Self::A6SemanticAnalysis,
        Self::A7Monomorphization,
        Self::A8OptimizationDecision,
        Self::A9CodegenPreparation,
        Self::A10CodeGeneration,
        Self::A11SymbolRegistration,
        Self::A12FinalLink,
        Self::N2DependencyResolution,
        Self::N3BuildInvocation,
        Self::N4ModuleReachability,
        Self::N5SemanticAnalysisAndCGeneration,
        Self::N6Monomorphization,
        Self::N7CCompilation,
        Self::N8NimLink,
        Self::N9ExportRegistration,
        Self::ForeignTargetResolution,
        Self::ForeignArtifactResolution,
        Self::ForeignAdapterGeneration,
        Self::ForeignCompileUnitPreparation,
        Self::ForeignObjectCompilation,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkUnitId {
    Logical(LogicalActivity),
    ArchiveForeignObjects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionGroup {
    RustObjectCompilation,
    NimStaticLibraryCompilation,
    DependencyResolver,
    CObjectCompilation,
    CppAdapterCompilation,
    ForeignArchive,
    NativeExecutableLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataKind {
    WorkspaceManifest,
    MemberManifest,
    PackageResolution,
    SourceUnitGraph,
    SelectedSourceUnit,
    SemanticFact,
    InstantiationKey,
    OptimizationDecision,
    BuildStrategyContext,
    TargetParameters,
    LoweredModule,
    NativeObject,
    LinkSymbol,
    LinkedArtifact,
    NimblePackageManifest,
    NimPackageResolution,
    NimBuildInvocation,
    ReachableNimModule,
    GeneratedCModule,
    NimGenericDeclaration,
    NimInstantiationKey,
    CObjectFile,
    NimLinkArtifact,
    ExportedSymbol,
    ForeignDecl,
    ForeignLibraryTarget,
    ResolvedForeignArtifact,
    AdapterUnit,
    ForeignCompileUnit,
    ForeignNativeObject,
    ForeignArchive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputSource {
    ExternalAuthority,
    ProducedBy(WorkUnitId),
    /// The input may be produced by either language's semantic analysis.  The
    /// concrete identity names exactly one producer; this is not an all-Rust
    /// plus all-Nim barrier.
    ProducedByEither(WorkUnitId, WorkUnitId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkInput {
    pub kind: DataKind,
    pub source: InputSource,
    /// Optional inputs never delay readiness when no matching identity exists.
    /// If one exists, its producer must commit before this work can start.
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadyScope {
    /// Only producers of the concrete identities named by this work unit must
    /// be committed.  Unrelated Rust, Nim and foreign work remains runnable.
    MatchingIdentities,
    /// The sole global convergence rule: every retained final-link input in
    /// the demanded closure must be committed.
    DemandedArtifactClosure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyPredicate {
    pub scope: ReadyScope,
    pub requires_successful_commit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeClass {
    Metadata,
    Semantic,
    Codegen,
    Link,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryClass {
    BoundedMetadata,
    SemanticIr,
    BackendBytes,
    ArtifactSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceClaim {
    pub minimum_cpu_slots: u16,
    pub compute: ComputeClass,
    pub memory: MemoryClass,
    pub external_process_slots: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySpace {
    DependencyClosureInstantiation,
    NimBuildInvocationInstantiation,
    DependencyClosureLinkSymbol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    None,
    KeyedCompareAndCommit(KeySpace),
    StagedArtifact,
    FinalArtifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    DeterministicValue,
    SameKeySameContent,
    ContentAddressedArtifact,
    OptionalDeterministicValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitBoundary {
    PlannerValue,
    KeyedState,
    ArtifactPublicationHandoff,
    FinalArtifactPublicationHandoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerOwnership {
    DeclaresIdentityDependenciesAndGrouping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorOwnership {
    ComputesStagesAndCommits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkOwnership {
    pub planner: PlannerOwnership,
    pub executor: ExecutorOwnership,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalWorkUnit {
    pub id: WorkUnitId,
    pub execution_group: ExecutionGroup,
    pub inputs: Vec<WorkInput>,
    pub outputs: Vec<DataKind>,
    pub ready: ReadyPredicate,
    pub resources: ResourceClaim,
    pub side_effect: SideEffect,
    pub idempotency: Idempotency,
    pub commit_boundary: CommitBoundary,
    pub ownership: WorkOwnership,
    /// An output is terminal only when no downstream work unit consumes it.
    /// This is explicit evidence, never inferred from being unconnected.
    pub terminal_outputs: Vec<DataKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalWorkContract {
    pub contract_version: u32,
    pub units: Vec<PhysicalWorkUnit>,
}

/// The planner-visible reservation for one concrete physical-work identity.
/// This is an accounted request, not an OS-enforced limit. The Rust executor
/// converts it to its live resource accounting type without reinterpreting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedResourceReservation {
    pub cpu_slots: u32,
    pub memory_bytes: u64,
    pub io_slots: u32,
    pub external_process_slots: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedNestedParallelism {
    Disabled,
    Accounted { additional_cpu_slots: u32 },
}

/// One concrete logical identity submitted to the production Nim planner.
/// The work-unit id keeps semantic identity separate from process grouping;
/// dependencies name other concrete identities, never execution order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedPhysicalWork {
    pub instance_id: String,
    pub work_unit: WorkUnitId,
    pub execution_group: ExecutionGroup,
    pub dependencies: BTreeSet<String>,
    pub reservation: PlannedResourceReservation,
    pub nested_parallelism: PlannedNestedParallelism,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhysicalPlanningBridgeError {
    InvalidContract(Vec<PhysicalWorkViolation>),
    DuplicateInstance {
        instance_id: String,
    },
    UnknownWorkUnit {
        instance_id: String,
        work_unit: WorkUnitId,
    },
    UnknownDependency {
        instance_id: String,
        dependency: String,
    },
    InvalidDependency {
        instance_id: String,
        dependency: String,
    },
    MissingDependency {
        instance_id: String,
        producer: WorkUnitId,
    },
    UndemandedInstance {
        instance_id: String,
    },
    InvalidReservation {
        instance_id: String,
        detail: String,
    },
    UnexpectedActionKind {
        action_id: String,
        kind: ActionKind,
    },
    InvalidDescriptor {
        action_id: String,
        detail: String,
    },
    DescriptorMismatch {
        action_id: String,
        detail: String,
    },
    MissingPlannedAction {
        action_id: String,
    },
}

fn physical_result_artifact(instance_id: &str) -> String {
    format!("physical-work-result:{instance_id}")
}

fn validate_planned_instance(
    contract: &PhysicalWorkContract,
    instance: &PlannedPhysicalWork,
) -> Result<(), PhysicalPlanningBridgeError> {
    let Some(unit) = contract
        .units
        .iter()
        .find(|unit| unit.id == instance.work_unit)
    else {
        return Err(PhysicalPlanningBridgeError::UnknownWorkUnit {
            instance_id: instance.instance_id.clone(),
            work_unit: instance.work_unit,
        });
    };
    if instance.execution_group != unit.execution_group {
        return Err(PhysicalPlanningBridgeError::InvalidDescriptor {
            action_id: instance.instance_id.clone(),
            detail: "execution_group differs from the physical-work contract".to_string(),
        });
    }
    let nested_cpu = match instance.nested_parallelism {
        PlannedNestedParallelism::Disabled => 0,
        PlannedNestedParallelism::Accounted {
            additional_cpu_slots,
        } => additional_cpu_slots,
    };
    let invalid = if instance.reservation.cpu_slots == 0 {
        Some("cpu_slots must be non-zero")
    } else if instance.reservation.memory_bytes == 0 {
        Some("memory_bytes must be non-zero")
    } else if instance.reservation.cpu_slots < 1_u32.saturating_add(nested_cpu) {
        Some("nested parallelism is not included in cpu_slots")
    } else if instance.reservation.cpu_slots < u32::from(unit.resources.minimum_cpu_slots) {
        Some("cpu_slots is below the work-unit minimum")
    } else if instance.reservation.external_process_slots
        < u32::from(unit.resources.external_process_slots)
    {
        Some("external_process_slots is below the work-unit minimum")
    } else if instance.reservation.external_process_slots > 0 && instance.reservation.io_slots == 0
    {
        Some("external process work must reserve an I/O slot")
    } else {
        None
    };
    if let Some(detail) = invalid {
        return Err(PhysicalPlanningBridgeError::InvalidReservation {
            instance_id: instance.instance_id.clone(),
            detail: detail.to_string(),
        });
    }
    Ok(())
}

/// Converts concrete physical identities into the existing production Nim
/// planner's artifact graph without translating them into legacy build or
/// compiler-work action kinds. The typed declaration is carried separately
/// from the action's display-only command identity and must round-trip through
/// Nim without reinterpretation.
pub fn physical_planning_input(
    contract: &PhysicalWorkContract,
    instances: Vec<PlannedPhysicalWork>,
    demanded_instances: Vec<String>,
) -> Result<PlanningInput, PhysicalPlanningBridgeError> {
    let violations = validate_physical_work_contract(contract);
    if !violations.is_empty() {
        return Err(PhysicalPlanningBridgeError::InvalidContract(violations));
    }
    let mut by_id = BTreeMap::new();
    for instance in instances {
        validate_planned_instance(contract, &instance)?;
        let instance_id = instance.instance_id.clone();
        if by_id.insert(instance_id.clone(), instance).is_some() {
            return Err(PhysicalPlanningBridgeError::DuplicateInstance { instance_id });
        }
    }
    for instance in by_id.values() {
        for dependency in &instance.dependencies {
            if !by_id.contains_key(dependency) {
                return Err(PhysicalPlanningBridgeError::UnknownDependency {
                    instance_id: instance.instance_id.clone(),
                    dependency: dependency.clone(),
                });
            }
        }
        let unit = contract
            .units
            .iter()
            .find(|unit| unit.id == instance.work_unit)
            .expect("work unit was validated above");
        let dependency_units: BTreeMap<_, _> = instance
            .dependencies
            .iter()
            .map(|dependency| (dependency, by_id[dependency].work_unit))
            .collect();
        for (dependency, producer) in &dependency_units {
            let accepted = unit
                .inputs
                .iter()
                .any(|input| input_producers(input.source).any(|candidate| candidate == *producer));
            if !accepted {
                return Err(PhysicalPlanningBridgeError::InvalidDependency {
                    instance_id: instance.instance_id.clone(),
                    dependency: (*dependency).clone(),
                });
            }
        }
        for input in unit.inputs.iter().filter(|input| !input.optional) {
            let producers: BTreeSet<_> = input_producers(input.source).collect();
            if !producers.is_empty()
                && !dependency_units
                    .values()
                    .any(|producer| producers.contains(producer))
            {
                return Err(PhysicalPlanningBridgeError::MissingDependency {
                    instance_id: instance.instance_id.clone(),
                    producer: *producers.iter().next().expect("non-empty producer set"),
                });
            }
        }
    }
    for demanded in &demanded_instances {
        if !by_id.contains_key(demanded) {
            return Err(PhysicalPlanningBridgeError::UndemandedInstance {
                instance_id: demanded.clone(),
            });
        }
    }

    let actions = by_id
        .values()
        .map(|instance| Action {
            id: instance.instance_id.clone(),
            kind: ActionKind::PhysicalWork,
            command_identity: format!("physical-work:{}", instance.instance_id),
            inputs: instance
                .dependencies
                .iter()
                .map(|dependency| ArtifactRef::declared(physical_result_artifact(dependency)))
                .collect(),
            outputs: vec![ArtifactRef::declared(physical_result_artifact(
                &instance.instance_id,
            ))],
            compiler_work: None,
        })
        .collect();
    let mut input = PlanningInput::new(
        demanded_instances
            .iter()
            .map(|id| physical_result_artifact(id))
            .collect(),
        actions,
    );
    input.physical_work = by_id;
    Ok(input)
}

/// Recovers only physical-work actions selected by the real Nim planner and
/// rejects any descriptor, dependency, or output mutation before execution.
pub fn physical_work_from_execution_plan(
    contract: &PhysicalWorkContract,
    plan: &ExecutionPlan,
) -> Result<Vec<PlannedPhysicalWork>, PhysicalPlanningBridgeError> {
    let ordered_ids: BTreeSet<_> = plan.ordered_actions.iter().cloned().collect();
    let declaration_ids: BTreeSet<_> = plan.physical_work.keys().cloned().collect();
    if ordered_ids != declaration_ids {
        return Err(PhysicalPlanningBridgeError::DescriptorMismatch {
            action_id: "<plan>".to_string(),
            detail: "physical-work declarations do not exactly match ordered_actions".to_string(),
        });
    }
    let mut result = Vec::with_capacity(plan.ordered_actions.len());
    for action_id in &plan.ordered_actions {
        let action = plan.actions.get(action_id).ok_or_else(|| {
            PhysicalPlanningBridgeError::MissingPlannedAction {
                action_id: action_id.clone(),
            }
        })?;
        if action.kind != ActionKind::PhysicalWork {
            return Err(PhysicalPlanningBridgeError::UnexpectedActionKind {
                action_id: action.id.clone(),
                kind: action.kind,
            });
        }
        let instance = plan.physical_work.get(action_id).cloned().ok_or_else(|| {
            PhysicalPlanningBridgeError::InvalidDescriptor {
                action_id: action.id.clone(),
                detail: "missing typed physical-work declaration".to_string(),
            }
        })?;
        validate_planned_instance(contract, &instance)?;
        if instance.instance_id != action.id {
            return Err(PhysicalPlanningBridgeError::DescriptorMismatch {
                action_id: action.id.clone(),
                detail: "descriptor instance_id differs from action id".to_string(),
            });
        }
        let expected_inputs: Vec<_> = instance
            .dependencies
            .iter()
            .map(|dependency| ArtifactRef::declared(physical_result_artifact(dependency)))
            .collect();
        let expected_outputs = vec![ArtifactRef::declared(physical_result_artifact(&action.id))];
        if action.inputs != expected_inputs || action.outputs != expected_outputs {
            return Err(PhysicalPlanningBridgeError::DescriptorMismatch {
                action_id: action.id.clone(),
                detail: "artifact edges differ from the physical descriptor".to_string(),
            });
        }
        result.push(instance);
    }
    physical_planning_input(
        contract,
        result.clone(),
        result
            .iter()
            .map(|instance| instance.instance_id.clone())
            .collect(),
    )?;
    Ok(result)
}

const OWNER: WorkOwnership = WorkOwnership {
    planner: PlannerOwnership::DeclaresIdentityDependenciesAndGrouping,
    executor: ExecutorOwnership::ComputesStagesAndCommits,
};

const MATCHING_READY: ReadyPredicate = ReadyPredicate {
    scope: ReadyScope::MatchingIdentities,
    requires_successful_commit: true,
};

fn input(kind: DataKind, producer: LogicalActivity) -> WorkInput {
    WorkInput {
        kind,
        source: InputSource::ProducedBy(WorkUnitId::Logical(producer)),
        optional: false,
    }
}

fn optional_input(kind: DataKind, producer: LogicalActivity) -> WorkInput {
    WorkInput {
        kind,
        source: InputSource::ProducedBy(WorkUnitId::Logical(producer)),
        optional: true,
    }
}

fn external(kind: DataKind) -> WorkInput {
    WorkInput {
        kind,
        source: InputSource::ExternalAuthority,
        optional: false,
    }
}

fn unit(
    activity: LogicalActivity,
    execution_group: ExecutionGroup,
    inputs: Vec<WorkInput>,
    outputs: Vec<DataKind>,
    compute: ComputeClass,
    memory: MemoryClass,
) -> PhysicalWorkUnit {
    PhysicalWorkUnit {
        id: WorkUnitId::Logical(activity),
        execution_group,
        inputs,
        outputs,
        ready: MATCHING_READY,
        resources: ResourceClaim {
            minimum_cpu_slots: 1,
            compute,
            memory,
            external_process_slots: 0,
        },
        side_effect: SideEffect::None,
        idempotency: Idempotency::DeterministicValue,
        commit_boundary: CommitBoundary::PlannerValue,
        ownership: OWNER,
        terminal_outputs: vec![],
    }
}

/// The single authoritative Checkpoint-A map.  Resolver output carries this
/// exact contract and the integrity gate validates it generically; there is no
/// second YAML fixture or fixture-only behavior model.
pub fn canonical_physical_work_contract() -> PhysicalWorkContract {
    use ComputeClass::*;
    use DataKind::*;
    use ExecutionGroup::*;
    use LogicalActivity::*;
    use MemoryClass::*;

    let mut units = vec![
        unit(
            A2MemberManifest,
            RustObjectCompilation,
            vec![external(WorkspaceManifest)],
            vec![MemberManifest],
            Metadata,
            BoundedMetadata,
        ),
        unit(
            A3PackageResolution,
            RustObjectCompilation,
            vec![input(MemberManifest, A2MemberManifest)],
            vec![PackageResolution],
            Metadata,
            BoundedMetadata,
        ),
        unit(
            A4SourceUnitGraph,
            RustObjectCompilation,
            vec![input(MemberManifest, A2MemberManifest)],
            vec![SourceUnitGraph],
            Semantic,
            SemanticIr,
        ),
        unit(
            A5SourceUnitSelection,
            RustObjectCompilation,
            vec![
                input(PackageResolution, A3PackageResolution),
                input(SourceUnitGraph, A4SourceUnitGraph),
            ],
            vec![SelectedSourceUnit],
            Semantic,
            SemanticIr,
        ),
        unit(
            A6SemanticAnalysis,
            RustObjectCompilation,
            vec![input(SelectedSourceUnit, A5SourceUnitSelection)],
            vec![SemanticFact, ForeignDecl],
            Semantic,
            SemanticIr,
        ),
        unit(
            A7Monomorphization,
            RustObjectCompilation,
            vec![input(SemanticFact, A6SemanticAnalysis)],
            vec![InstantiationKey],
            Semantic,
            SemanticIr,
        ),
        unit(
            A8OptimizationDecision,
            RustObjectCompilation,
            vec![input(InstantiationKey, A7Monomorphization)],
            vec![OptimizationDecision],
            Semantic,
            SemanticIr,
        ),
        unit(
            A9CodegenPreparation,
            RustObjectCompilation,
            vec![
                input(InstantiationKey, A7Monomorphization),
                input(OptimizationDecision, A8OptimizationDecision),
                external(BuildStrategyContext),
                external(TargetParameters),
            ],
            vec![LoweredModule],
            Codegen,
            BackendBytes,
        ),
        unit(
            A10CodeGeneration,
            RustObjectCompilation,
            vec![input(LoweredModule, A9CodegenPreparation)],
            vec![NativeObject],
            Codegen,
            BackendBytes,
        ),
        unit(
            A11SymbolRegistration,
            RustObjectCompilation,
            vec![input(NativeObject, A10CodeGeneration)],
            vec![LinkSymbol],
            Semantic,
            SemanticIr,
        ),
        unit(
            N2DependencyResolution,
            NimStaticLibraryCompilation,
            vec![external(NimblePackageManifest)],
            vec![NimPackageResolution],
            Metadata,
            BoundedMetadata,
        ),
        unit(
            N3BuildInvocation,
            NimStaticLibraryCompilation,
            vec![input(NimPackageResolution, N2DependencyResolution)],
            vec![NimBuildInvocation],
            Metadata,
            BoundedMetadata,
        ),
        unit(
            N4ModuleReachability,
            NimStaticLibraryCompilation,
            vec![input(NimBuildInvocation, N3BuildInvocation)],
            vec![ReachableNimModule],
            Semantic,
            SemanticIr,
        ),
        unit(
            N5SemanticAnalysisAndCGeneration,
            NimStaticLibraryCompilation,
            vec![input(ReachableNimModule, N4ModuleReachability)],
            vec![GeneratedCModule, NimGenericDeclaration, ForeignDecl],
            Codegen,
            BackendBytes,
        ),
        unit(
            N6Monomorphization,
            NimStaticLibraryCompilation,
            vec![input(
                NimGenericDeclaration,
                N5SemanticAnalysisAndCGeneration,
            )],
            vec![NimInstantiationKey],
            Semantic,
            SemanticIr,
        ),
        unit(
            N7CCompilation,
            NimStaticLibraryCompilation,
            vec![
                input(GeneratedCModule, N5SemanticAnalysisAndCGeneration),
                optional_input(NimInstantiationKey, N6Monomorphization),
            ],
            vec![CObjectFile],
            Codegen,
            BackendBytes,
        ),
        unit(
            N8NimLink,
            NimStaticLibraryCompilation,
            vec![input(CObjectFile, N7CCompilation)],
            vec![NimLinkArtifact],
            Link,
            ArtifactSet,
        ),
        unit(
            N9ExportRegistration,
            NimStaticLibraryCompilation,
            vec![input(GeneratedCModule, N5SemanticAnalysisAndCGeneration)],
            vec![ExportedSymbol],
            Semantic,
            SemanticIr,
        ),
        PhysicalWorkUnit {
            id: WorkUnitId::Logical(ForeignTargetResolution),
            execution_group: DependencyResolver,
            inputs: vec![WorkInput {
                kind: ForeignDecl,
                source: InputSource::ProducedByEither(
                    WorkUnitId::Logical(A6SemanticAnalysis),
                    WorkUnitId::Logical(N5SemanticAnalysisAndCGeneration),
                ),
                optional: false,
            }],
            outputs: vec![ForeignLibraryTarget],
            ready: MATCHING_READY,
            resources: ResourceClaim {
                minimum_cpu_slots: 1,
                compute: Metadata,
                memory: BoundedMetadata,
                external_process_slots: 0,
            },
            side_effect: SideEffect::None,
            idempotency: Idempotency::DeterministicValue,
            commit_boundary: CommitBoundary::PlannerValue,
            ownership: OWNER,
            terminal_outputs: vec![],
        },
        unit(
            ForeignArtifactResolution,
            DependencyResolver,
            vec![input(ForeignLibraryTarget, ForeignTargetResolution)],
            vec![ResolvedForeignArtifact],
            Metadata,
            BoundedMetadata,
        ),
        unit(
            ForeignAdapterGeneration,
            CppAdapterCompilation,
            vec![
                WorkInput {
                    kind: ForeignDecl,
                    source: InputSource::ProducedByEither(
                        WorkUnitId::Logical(A6SemanticAnalysis),
                        WorkUnitId::Logical(N5SemanticAnalysisAndCGeneration),
                    ),
                    optional: false,
                },
                input(ResolvedForeignArtifact, ForeignArtifactResolution),
            ],
            vec![AdapterUnit],
            Semantic,
            SemanticIr,
        ),
        unit(
            ForeignCompileUnitPreparation,
            CObjectCompilation,
            vec![
                input(ResolvedForeignArtifact, ForeignArtifactResolution),
                optional_input(AdapterUnit, ForeignAdapterGeneration),
            ],
            vec![ForeignCompileUnit],
            Metadata,
            BoundedMetadata,
        ),
        unit(
            ForeignObjectCompilation,
            CObjectCompilation,
            vec![input(ForeignCompileUnit, ForeignCompileUnitPreparation)],
            vec![ForeignNativeObject],
            Codegen,
            BackendBytes,
        ),
    ];

    let by_id = |activity| WorkUnitId::Logical(activity);
    for activity in [
        A7Monomorphization,
        N6Monomorphization,
        A11SymbolRegistration,
    ] {
        let unit = units
            .iter_mut()
            .find(|unit| unit.id == by_id(activity))
            .unwrap();
        let key_space = match activity {
            A7Monomorphization => KeySpace::DependencyClosureInstantiation,
            N6Monomorphization => KeySpace::NimBuildInvocationInstantiation,
            A11SymbolRegistration => KeySpace::DependencyClosureLinkSymbol,
            _ => unreachable!(),
        };
        unit.side_effect = SideEffect::KeyedCompareAndCommit(key_space);
        unit.idempotency = Idempotency::SameKeySameContent;
        unit.commit_boundary = CommitBoundary::KeyedState;
    }

    for activity in [
        A10CodeGeneration,
        N7CCompilation,
        N8NimLink,
        ForeignObjectCompilation,
    ] {
        let unit = units
            .iter_mut()
            .find(|unit| unit.id == by_id(activity))
            .unwrap();
        unit.side_effect = SideEffect::StagedArtifact;
        unit.idempotency = Idempotency::ContentAddressedArtifact;
        unit.commit_boundary = CommitBoundary::ArtifactPublicationHandoff;
        unit.resources.external_process_slots = u16::from(matches!(
            activity,
            N7CCompilation | ForeignObjectCompilation
        ));
    }
    units
        .iter_mut()
        .find(|unit| unit.id == by_id(ForeignAdapterGeneration))
        .unwrap()
        .idempotency = Idempotency::OptionalDeterministicValue;

    units.push(PhysicalWorkUnit {
        id: WorkUnitId::ArchiveForeignObjects,
        execution_group: ExecutionGroup::ForeignArchive,
        inputs: vec![input(ForeignNativeObject, ForeignObjectCompilation)],
        outputs: vec![DataKind::ForeignArchive],
        ready: MATCHING_READY,
        resources: ResourceClaim {
            minimum_cpu_slots: 1,
            compute: Link,
            memory: ArtifactSet,
            external_process_slots: 1,
        },
        side_effect: SideEffect::StagedArtifact,
        idempotency: Idempotency::ContentAddressedArtifact,
        commit_boundary: CommitBoundary::ArtifactPublicationHandoff,
        ownership: OWNER,
        terminal_outputs: vec![],
    });

    units.push(PhysicalWorkUnit {
        id: by_id(A12FinalLink),
        execution_group: NativeExecutableLink,
        inputs: vec![
            input(LinkSymbol, A11SymbolRegistration),
            input(NimLinkArtifact, N8NimLink),
            input(ExportedSymbol, N9ExportRegistration),
            WorkInput {
                kind: DataKind::ForeignArchive,
                source: InputSource::ProducedBy(WorkUnitId::ArchiveForeignObjects),
                optional: false,
            },
        ],
        outputs: vec![LinkedArtifact],
        ready: ReadyPredicate {
            scope: ReadyScope::DemandedArtifactClosure,
            requires_successful_commit: true,
        },
        resources: ResourceClaim {
            minimum_cpu_slots: 1,
            compute: Link,
            memory: ArtifactSet,
            external_process_slots: 1,
        },
        side_effect: SideEffect::FinalArtifact,
        idempotency: Idempotency::ContentAddressedArtifact,
        commit_boundary: CommitBoundary::FinalArtifactPublicationHandoff,
        ownership: OWNER,
        terminal_outputs: vec![LinkedArtifact],
    });

    PhysicalWorkContract {
        contract_version: PHYSICAL_WORK_CONTRACT_VERSION,
        units,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhysicalWorkViolation {
    UnsupportedVersion {
        got: u32,
    },
    DuplicateUnit {
        id: WorkUnitId,
    },
    MissingLogicalActivity {
        activity: LogicalActivity,
    },
    UnknownProducer {
        consumer: WorkUnitId,
        producer: WorkUnitId,
    },
    DependencyCycle {
        cycle: Vec<WorkUnitId>,
    },
    UnconsumedOutput {
        producer: WorkUnitId,
        output: DataKind,
    },
    NonFinalGlobalBarrier {
        unit: WorkUnitId,
    },
    FinalLinkIsNotGlobalBarrier,
    MissingResourceClaim {
        unit: WorkUnitId,
    },
    CommitSideEffectMismatch {
        unit: WorkUnitId,
    },
}

fn input_producers(source: InputSource) -> impl Iterator<Item = WorkUnitId> {
    let producers = match source {
        InputSource::ExternalAuthority => vec![],
        InputSource::ProducedBy(producer) => vec![producer],
        InputSource::ProducedByEither(a, b) => vec![a, b],
    };
    producers.into_iter()
}

/// Generic structural verification used by the production plan-integrity
/// gate.  It validates relations, not a second copy of the canonical values.
pub fn validate_physical_work_contract(
    contract: &PhysicalWorkContract,
) -> Vec<PhysicalWorkViolation> {
    let mut violations = vec![];
    if contract.contract_version != PHYSICAL_WORK_CONTRACT_VERSION {
        violations.push(PhysicalWorkViolation::UnsupportedVersion {
            got: contract.contract_version,
        });
    }

    let mut by_id = BTreeMap::new();
    for unit in &contract.units {
        if by_id.insert(unit.id, unit).is_some() {
            violations.push(PhysicalWorkViolation::DuplicateUnit { id: unit.id });
        }
        if unit.resources.minimum_cpu_slots == 0 {
            violations.push(PhysicalWorkViolation::MissingResourceClaim { unit: unit.id });
        }
        let commit_matches = matches!(
            (unit.side_effect, unit.commit_boundary),
            (SideEffect::None, CommitBoundary::PlannerValue)
                | (
                    SideEffect::KeyedCompareAndCommit(_),
                    CommitBoundary::KeyedState
                )
                | (
                    SideEffect::StagedArtifact,
                    CommitBoundary::ArtifactPublicationHandoff
                )
                | (
                    SideEffect::FinalArtifact,
                    CommitBoundary::FinalArtifactPublicationHandoff
                )
        );
        if !commit_matches {
            violations.push(PhysicalWorkViolation::CommitSideEffectMismatch { unit: unit.id });
        }
        if unit.ready.scope == ReadyScope::DemandedArtifactClosure
            && unit.id != WorkUnitId::Logical(LogicalActivity::A12FinalLink)
        {
            violations.push(PhysicalWorkViolation::NonFinalGlobalBarrier { unit: unit.id });
        }
    }

    for activity in LogicalActivity::ALL {
        if !by_id.contains_key(&WorkUnitId::Logical(activity)) {
            violations.push(PhysicalWorkViolation::MissingLogicalActivity { activity });
        }
    }
    if by_id
        .get(&WorkUnitId::Logical(LogicalActivity::A12FinalLink))
        .map_or(true, |unit| {
            unit.ready.scope != ReadyScope::DemandedArtifactClosure
        })
    {
        violations.push(PhysicalWorkViolation::FinalLinkIsNotGlobalBarrier);
    }

    for unit in &contract.units {
        for producer in unit
            .inputs
            .iter()
            .flat_map(|input| input_producers(input.source))
        {
            if !by_id.contains_key(&producer) {
                violations.push(PhysicalWorkViolation::UnknownProducer {
                    consumer: unit.id,
                    producer,
                });
            }
        }
        for &output in &unit.outputs {
            let consumed = contract.units.iter().any(|consumer| {
                consumer.inputs.iter().any(|candidate| {
                    candidate.kind == output
                        && input_producers(candidate.source).any(|producer| producer == unit.id)
                })
            });
            if !consumed && !unit.terminal_outputs.contains(&output) {
                violations.push(PhysicalWorkViolation::UnconsumedOutput {
                    producer: unit.id,
                    output,
                });
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Visiting,
        Done,
    }
    fn visit(
        id: WorkUnitId,
        by_id: &BTreeMap<WorkUnitId, &PhysicalWorkUnit>,
        marks: &mut BTreeMap<WorkUnitId, Mark>,
        stack: &mut Vec<WorkUnitId>,
    ) -> Option<Vec<WorkUnitId>> {
        match marks.get(&id) {
            Some(Mark::Done) => return None,
            Some(Mark::Visiting) => {
                let start = stack
                    .iter()
                    .position(|candidate| *candidate == id)
                    .unwrap_or(0);
                let mut cycle = stack[start..].to_vec();
                cycle.push(id);
                return Some(cycle);
            }
            None => {}
        }
        marks.insert(id, Mark::Visiting);
        stack.push(id);
        if let Some(unit) = by_id.get(&id) {
            let mut dependencies = BTreeSet::new();
            for producer in unit
                .inputs
                .iter()
                .flat_map(|input| input_producers(input.source))
            {
                dependencies.insert(producer);
            }
            for producer in dependencies {
                if by_id.contains_key(&producer) {
                    if let Some(cycle) = visit(producer, by_id, marks, stack) {
                        return Some(cycle);
                    }
                }
            }
        }
        stack.pop();
        marks.insert(id, Mark::Done);
        None
    }
    let mut marks = BTreeMap::new();
    let mut stack = vec![];
    for id in by_id.keys().copied() {
        if let Some(cycle) = visit(id, &by_id, &mut marks, &mut stack) {
            violations.push(PhysicalWorkViolation::DependencyCycle { cycle });
            break;
        }
    }

    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planned(
        contract: &PhysicalWorkContract,
        instance_id: &str,
        work_unit: WorkUnitId,
        dependencies: &[&str],
    ) -> PlannedPhysicalWork {
        let unit = contract
            .units
            .iter()
            .find(|unit| unit.id == work_unit)
            .unwrap();
        let external_process_slots = u32::from(unit.resources.external_process_slots);
        PlannedPhysicalWork {
            instance_id: instance_id.to_string(),
            work_unit,
            execution_group: unit.execution_group,
            dependencies: dependencies.iter().map(|id| (*id).to_string()).collect(),
            reservation: PlannedResourceReservation {
                cpu_slots: u32::from(unit.resources.minimum_cpu_slots),
                memory_bytes: 1,
                io_slots: u32::from(external_process_slots > 0),
                external_process_slots,
            },
            nested_parallelism: PlannedNestedParallelism::Disabled,
        }
    }

    #[test]
    fn physical_planning_input_is_derived_from_typed_declarations() {
        let contract = canonical_physical_work_contract();
        let a2 = planned(
            &contract,
            "a2:crate-a",
            WorkUnitId::Logical(LogicalActivity::A2MemberManifest),
            &[],
        );
        let a3 = planned(
            &contract,
            "a3:crate-a",
            WorkUnitId::Logical(LogicalActivity::A3PackageResolution),
            &["a2:crate-a"],
        );

        let input = physical_planning_input(
            &contract,
            vec![a2.clone(), a3.clone()],
            vec![a3.instance_id.clone()],
        )
        .unwrap();

        assert_eq!(input.physical_work[&a2.instance_id], a2);
        assert_eq!(input.physical_work[&a3.instance_id], a3);
        assert_eq!(input.actions[1].kind, ActionKind::PhysicalWork);
        assert_eq!(
            input.actions[1].inputs,
            vec![ArtifactRef::declared("physical-work-result:a2:crate-a")]
        );
    }

    #[test]
    fn physical_planning_input_rejects_dependencies_not_admitted_by_the_contract() {
        let contract = canonical_physical_work_contract();
        let a2 = planned(
            &contract,
            "a2",
            WorkUnitId::Logical(LogicalActivity::A2MemberManifest),
            &[],
        );
        let n2 = planned(
            &contract,
            "n2",
            WorkUnitId::Logical(LogicalActivity::N2DependencyResolution),
            &[],
        );
        let a3 = planned(
            &contract,
            "a3",
            WorkUnitId::Logical(LogicalActivity::A3PackageResolution),
            &["n2"],
        );

        assert!(matches!(
            physical_planning_input(&contract, vec![a2, n2, a3], vec!["a3".to_string()]),
            Err(PhysicalPlanningBridgeError::InvalidDependency { .. })
        ));
    }

    #[test]
    fn canonical_map_is_connected_complete_acyclic_and_has_only_a12_as_global_barrier() {
        let contract = canonical_physical_work_contract();
        assert_eq!(validate_physical_work_contract(&contract), vec![]);
        assert_eq!(
            contract
                .units
                .iter()
                .filter(|unit| unit.ready.scope == ReadyScope::DemandedArtifactClosure)
                .map(|unit| unit.id)
                .collect::<Vec<_>>(),
            vec![WorkUnitId::Logical(LogicalActivity::A12FinalLink)]
        );
    }

    #[test]
    fn rust_nim_and_foreign_fronts_have_no_artificial_cross_chain_dependency() {
        let contract = canonical_physical_work_contract();
        for activity in [
            LogicalActivity::A2MemberManifest,
            LogicalActivity::N2DependencyResolution,
            LogicalActivity::ForeignTargetResolution,
        ] {
            let unit = contract
                .units
                .iter()
                .find(|unit| unit.id == WorkUnitId::Logical(activity))
                .unwrap();
            assert_ne!(unit.ready.scope, ReadyScope::DemandedArtifactClosure);
        }
        let foreign = contract
            .units
            .iter()
            .find(|unit| unit.id == WorkUnitId::Logical(LogicalActivity::ForeignTargetResolution))
            .unwrap();
        assert!(matches!(
            foreign.inputs[0].source,
            InputSource::ProducedByEither(_, _)
        ));
    }

    #[test]
    fn keyed_merges_declare_same_key_conflict_avoidance() {
        let contract = canonical_physical_work_contract();
        let keyed: BTreeMap<_, _> = contract
            .units
            .iter()
            .filter_map(|unit| match unit.side_effect {
                SideEffect::KeyedCompareAndCommit(key_space) => Some((unit.id, key_space)),
                _ => None,
            })
            .collect();
        assert_eq!(keyed.len(), 3);
        assert_eq!(
            keyed[&WorkUnitId::Logical(LogicalActivity::A7Monomorphization)],
            KeySpace::DependencyClosureInstantiation
        );
        assert_eq!(
            keyed[&WorkUnitId::Logical(LogicalActivity::N6Monomorphization)],
            KeySpace::NimBuildInvocationInstantiation
        );
        assert_eq!(
            keyed[&WorkUnitId::Logical(LogicalActivity::A11SymbolRegistration)],
            KeySpace::DependencyClosureLinkSymbol
        );
    }

    #[test]
    fn validator_detects_each_checkpoint_a_structural_failure_class() {
        let mut contract = canonical_physical_work_contract();
        contract
            .units
            .retain(|unit| unit.id != WorkUnitId::Logical(LogicalActivity::A8OptimizationDecision));
        assert!(validate_physical_work_contract(&contract)
            .iter()
            .any(|violation| matches!(
                violation,
                PhysicalWorkViolation::MissingLogicalActivity {
                    activity: LogicalActivity::A8OptimizationDecision
                }
            )));

        let mut contract = canonical_physical_work_contract();
        contract.units.push(contract.units[0].clone());
        assert!(validate_physical_work_contract(&contract)
            .iter()
            .any(|violation| matches!(violation, PhysicalWorkViolation::DuplicateUnit { .. })));

        let mut contract = canonical_physical_work_contract();
        let a2 = contract
            .units
            .iter_mut()
            .find(|unit| unit.id == WorkUnitId::Logical(LogicalActivity::A2MemberManifest))
            .unwrap();
        a2.inputs = vec![input(
            DataKind::LinkedArtifact,
            LogicalActivity::A12FinalLink,
        )];
        assert!(validate_physical_work_contract(&contract)
            .iter()
            .any(|violation| matches!(violation, PhysicalWorkViolation::DependencyCycle { .. })));

        let mut contract = canonical_physical_work_contract();
        let a12 = contract
            .units
            .iter_mut()
            .find(|unit| unit.id == WorkUnitId::Logical(LogicalActivity::A12FinalLink))
            .unwrap();
        a12.terminal_outputs.clear();
        assert!(validate_physical_work_contract(&contract)
            .iter()
            .any(|violation| matches!(
                violation,
                PhysicalWorkViolation::UnconsumedOutput {
                    producer: WorkUnitId::Logical(LogicalActivity::A12FinalLink),
                    ..
                }
            )));
    }
}

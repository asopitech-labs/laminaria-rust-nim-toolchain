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

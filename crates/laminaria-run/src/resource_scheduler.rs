//! Resource admission and lifecycle for issue #89's physical work instances.
//!
//! The Checkpoint-A contract classifies a logical activity's resources, but
//! deliberately does not pretend that every source unit or backend invocation
//! has the same byte count.  This module is the production boundary where a
//! concrete instance supplies its accounted reservation.  Admission is over
//! one global budget, and the reservation remains owned through `committing`:
//! an IR or staged artifact cannot disappear from accounting merely because
//! computation ended before publication did.

use std::collections::{BTreeMap, BTreeSet};

use laminaria_plan::physical_work::{
    physical_work_from_execution_plan, CommitBoundary, DataKind, ExecutionGroup, InputSource,
    KeySpace, PhysicalPlanningBridgeError, PhysicalWorkContract, PhysicalWorkUnit,
    PlannedNestedParallelism, SideEffect, WorkUnitId,
};
use laminaria_plan::ExecutionPlan;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceAmount {
    /// Total CPU slots, including every thread a nested compiler may run.
    pub cpu_slots: u32,
    pub memory_bytes: u64,
    pub io_slots: u32,
    pub external_process_slots: u32,
}

impl ResourceAmount {
    fn checked_add(self, rhs: Self) -> Option<Self> {
        Some(Self {
            cpu_slots: self.cpu_slots.checked_add(rhs.cpu_slots)?,
            memory_bytes: self.memory_bytes.checked_add(rhs.memory_bytes)?,
            io_slots: self.io_slots.checked_add(rhs.io_slots)?,
            external_process_slots: self
                .external_process_slots
                .checked_add(rhs.external_process_slots)?,
        })
    }

    fn checked_sub(self, rhs: Self) -> Option<Self> {
        Some(Self {
            cpu_slots: self.cpu_slots.checked_sub(rhs.cpu_slots)?,
            memory_bytes: self.memory_bytes.checked_sub(rhs.memory_bytes)?,
            io_slots: self.io_slots.checked_sub(rhs.io_slots)?,
            external_process_slots: self
                .external_process_slots
                .checked_sub(rhs.external_process_slots)?,
        })
    }

    fn fits_within(self, limit: Self) -> bool {
        self.cpu_slots <= limit.cpu_slots
            && self.memory_bytes <= limit.memory_bytes
            && self.io_slots <= limit.io_slots
            && self.external_process_slots <= limit.external_process_slots
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalWorkInstance {
    /// Concrete logical identity. Multiple identities may use the same
    /// Checkpoint-A work-unit kind without sharing cache or execution identity.
    pub instance_id: String,
    pub work_unit: WorkUnitId,
    /// Checkpoint-A's physical execution grouping, available to the executor
    /// when binding this identity to a shared process or in-memory stage.
    pub execution_group: ExecutionGroup,
    pub dependencies: BTreeSet<String>,
    pub reservation: ResourceAmount,
    pub nested_parallelism: NestedParallelism,
}

/// Converts the exact physical descriptors echoed by the production Nim
/// planner into executor instances. No legacy `ActionKind` or process command
/// is interpreted as a logical activity here.
pub fn physical_instances_from_execution_plan(
    contract: &PhysicalWorkContract,
    plan: &ExecutionPlan,
) -> Result<Vec<PhysicalWorkInstance>, PhysicalPlanningBridgeError> {
    physical_work_from_execution_plan(contract, plan).map(|instances| {
        instances
            .into_iter()
            .map(|instance| PhysicalWorkInstance {
                instance_id: instance.instance_id,
                work_unit: instance.work_unit,
                execution_group: instance.execution_group,
                dependencies: instance.dependencies,
                reservation: ResourceAmount {
                    cpu_slots: instance.reservation.cpu_slots,
                    memory_bytes: instance.reservation.memory_bytes,
                    io_slots: instance.reservation.io_slots,
                    external_process_slots: instance.reservation.external_process_slots,
                },
                nested_parallelism: match instance.nested_parallelism {
                    PlannedNestedParallelism::Disabled => NestedParallelism::Disabled,
                    PlannedNestedParallelism::Accounted {
                        additional_cpu_slots,
                    } => NestedParallelism::Accounted {
                        additional_cpu_slots,
                    },
                },
            })
            .collect()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NestedParallelism {
    /// The work and any process it launches are configured for one execution
    /// thread. The instance's total CPU reservation still includes that one.
    Disabled,
    /// Additional threads beyond the work's primary execution thread. These
    /// slots must be included in `reservation.cpu_slots` and are therefore
    /// admitted against the same global budget.
    Accounted { additional_cpu_slots: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Ready,
    Running,
    Blocked,
    Committing,
    Completed,
    Failed,
    Cancelled,
}

impl LifecycleState {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn owns_resources(self) -> bool {
        matches!(self, Self::Running | Self::Committing)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDeficit {
    pub cpu_slots: u32,
    pub memory_bytes: u64,
    pub io_slots: u32,
    pub external_process_slots: u32,
}

impl ResourceDeficit {
    fn between(request: ResourceAmount, available: ResourceAmount) -> Self {
        Self {
            cpu_slots: request.cpu_slots.saturating_sub(available.cpu_slots),
            memory_bytes: request.memory_bytes.saturating_sub(available.memory_bytes),
            io_slots: request.io_slots.saturating_sub(available.io_slots),
            external_process_slots: request
                .external_process_slots
                .saturating_sub(available.external_process_slots),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitReason {
    Dependencies { pending: BTreeSet<String> },
    Resources { deficit: ResourceDeficit },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionReason {
    DependenciesSatisfied,
    ResourcesAvailable,
    Admitted,
    ComputationFinished,
    CommitSucceeded,
    ExecutionFailed { detail: String },
    ExplicitCancellation { detail: String },
    ProducerFailed { producer: String },
    UpstreamCancelled { upstream: String },
    Waiting(WaitReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleTransition {
    pub sequence: u64,
    pub instance_id: String,
    pub from: Option<LifecycleState>,
    pub to: LifecycleState,
    pub reason: TransitionReason,
}

/// A concrete output offered by one physical work instance. `logical_key` is
/// semantic identity supplied by the planner; it is deliberately independent
/// of the writer instance, thread, process, or placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitWrite {
    pub output_kind: DataKind,
    pub logical_key: String,
    pub bytes: Vec<u8>,
}

/// The consumer-visible address of a committed value. The Checkpoint-A
/// boundary and key space are part of the address, so a keyed merge cannot
/// alias an artifact publication that happens to use the same text key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CommitTarget {
    pub boundary: CommitBoundary,
    pub key_space: Option<KeySpace>,
    pub output_kind: DataKind,
    pub logical_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedOutput {
    pub target: CommitTarget,
    pub content_sha256: String,
    pub bytes: Vec<u8>,
    /// The first successful writer. Later same-content writers are idempotent
    /// confirmations and cannot rewrite lineage.
    pub writer_instance_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StagedOutput {
    target: CommitTarget,
    content_sha256: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitStoreError {
    EmptyWriteSet {
        instance_id: String,
    },
    MissingDeclaredOutput {
        instance_id: String,
        output_kind: DataKind,
    },
    UndeclaredOutput {
        instance_id: String,
        output_kind: DataKind,
    },
    DuplicateOutput {
        instance_id: String,
        output_kind: DataKind,
    },
    EmptyLogicalKey {
        instance_id: String,
        output_kind: DataKind,
    },
    ConflictingContent {
        target: CommitTarget,
        existing_sha256: String,
        proposed_sha256: String,
    },
    MissingStaging {
        instance_id: String,
    },
}

#[derive(Default)]
struct CommitStore {
    staged_by_writer: BTreeMap<String, Vec<StagedOutput>>,
    committed: BTreeMap<CommitTarget, CommittedOutput>,
    committed_targets_by_writer: BTreeMap<String, Vec<CommitTarget>>,
}

impl CommitStore {
    fn stage(
        &mut self,
        writer: &str,
        unit: &PhysicalWorkUnit,
        writes: Vec<CommitWrite>,
    ) -> Result<Vec<CommitTarget>, CommitStoreError> {
        if writes.is_empty() {
            return Err(CommitStoreError::EmptyWriteSet {
                instance_id: writer.to_string(),
            });
        }
        let declared: BTreeSet<_> = unit.outputs.iter().copied().collect();
        let mut seen = BTreeSet::new();
        let mut staged = Vec::with_capacity(writes.len());
        for write in writes {
            if !declared.contains(&write.output_kind) {
                return Err(CommitStoreError::UndeclaredOutput {
                    instance_id: writer.to_string(),
                    output_kind: write.output_kind,
                });
            }
            if !seen.insert(write.output_kind) {
                return Err(CommitStoreError::DuplicateOutput {
                    instance_id: writer.to_string(),
                    output_kind: write.output_kind,
                });
            }
            if write.logical_key.is_empty() {
                return Err(CommitStoreError::EmptyLogicalKey {
                    instance_id: writer.to_string(),
                    output_kind: write.output_kind,
                });
            }
            let key_space = match unit.side_effect {
                SideEffect::KeyedCompareAndCommit(key_space) => Some(key_space),
                _ => None,
            };
            let target = CommitTarget {
                boundary: unit.commit_boundary,
                key_space,
                output_kind: write.output_kind,
                logical_key: write.logical_key,
            };
            let content_sha256 = sha256_bytes(&write.bytes);
            self.reject_conflicting_content(&target, &content_sha256)?;
            staged.push(StagedOutput {
                target,
                content_sha256,
                bytes: write.bytes,
            });
        }
        if let Some(output_kind) = declared.difference(&seen).next() {
            return Err(CommitStoreError::MissingDeclaredOutput {
                instance_id: writer.to_string(),
                output_kind: *output_kind,
            });
        }
        let targets = staged.iter().map(|output| output.target.clone()).collect();
        self.staged_by_writer.insert(writer.to_string(), staged);
        Ok(targets)
    }

    fn reject_conflicting_content(
        &self,
        target: &CommitTarget,
        proposed_sha256: &str,
    ) -> Result<(), CommitStoreError> {
        let existing = self
            .committed
            .get(target)
            .map(|output| output.content_sha256.as_str())
            .or_else(|| {
                self.staged_by_writer
                    .values()
                    .flatten()
                    .find(|output| output.target == *target)
                    .map(|output| output.content_sha256.as_str())
            });
        if let Some(existing_sha256) = existing {
            if existing_sha256 != proposed_sha256 {
                return Err(CommitStoreError::ConflictingContent {
                    target: target.clone(),
                    existing_sha256: existing_sha256.to_string(),
                    proposed_sha256: proposed_sha256.to_string(),
                });
            }
        }
        Ok(())
    }

    fn commit(&mut self, writer: &str) -> Result<(), CommitStoreError> {
        let staged = self.staged_by_writer.get(writer).cloned().ok_or_else(|| {
            CommitStoreError::MissingStaging {
                instance_id: writer.to_string(),
            }
        })?;
        for output in &staged {
            self.reject_conflicting_content(&output.target, &output.content_sha256)?;
        }

        // Build the complete next visible state first. No consumer can observe
        // a prefix if a future validation step is added and rejects the batch.
        let mut committed = self.committed.clone();
        let targets = staged.iter().map(|output| output.target.clone()).collect();
        for output in staged {
            committed
                .entry(output.target.clone())
                .or_insert_with(|| CommittedOutput {
                    target: output.target,
                    content_sha256: output.content_sha256,
                    bytes: output.bytes,
                    writer_instance_id: writer.to_string(),
                });
        }
        self.committed = committed;
        self.committed_targets_by_writer
            .insert(writer.to_string(), targets);
        self.staged_by_writer.remove(writer);
        Ok(())
    }

    fn discard(&mut self, writer: &str) {
        self.staged_by_writer.remove(writer);
    }

    fn visible(&self, target: &CommitTarget) -> Option<&CommittedOutput> {
        self.committed.get(target)
    }

    fn has_staging(&self, writer: &str) -> bool {
        self.staged_by_writer.contains_key(writer)
    }

    fn outputs_for_writer(&self, writer: &str) -> Vec<CommittedOutput> {
        self.committed_targets_by_writer
            .get(writer)
            .into_iter()
            .flatten()
            .filter_map(|target| self.committed.get(target).cloned())
            .collect()
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerContractError {
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
    InvalidExecutionGroup {
        instance_id: String,
        expected: ExecutionGroup,
        actual: ExecutionGroup,
    },
    InvalidDependencyOutputKind {
        instance_id: String,
        dependency: String,
    },
    MissingInputDependency {
        instance_id: String,
        kind: DataKind,
    },
    DependencyCycle,
    ZeroCpuReservation {
        instance_id: String,
    },
    ZeroMemoryReservation {
        instance_id: String,
    },
    NestedParallelismExceedsCpuReservation {
        instance_id: String,
    },
    ExternalProcessWithoutIoReservation {
        instance_id: String,
    },
    ReservationBelowWorkUnitMinimum {
        instance_id: String,
    },
    ReservationExceedsBudget {
        instance_id: String,
    },
    ResourceAccountingOverflow,
    CommitStore(CommitStoreError),
    UnknownInstance {
        instance_id: String,
    },
    InvalidTransition {
        instance_id: String,
        from: LifecycleState,
        operation: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalWorkExecutionReport {
    pub transitions: Vec<LifecycleTransition>,
    pub states: BTreeMap<String, LifecycleState>,
    pub executed: BTreeSet<String>,
    pub completed: BTreeSet<String>,
    /// Completed work whose complete committed output batch is consumer-visible.
    pub reusable: BTreeSet<String>,
    /// Work whose own computation failed; any staging it owned was discarded.
    pub invalid: BTreeSet<String>,
    /// Failed work plus downstream work cancelled by that failure.
    pub rerun: BTreeSet<String>,
    pub never_started: BTreeSet<String>,
    pub committed_outputs: BTreeMap<CommitTarget, CommittedOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhysicalWorkExecutionError {
    Scheduler(SchedulerContractError),
    Stalled {
        states: BTreeMap<String, LifecycleState>,
    },
}

#[derive(Debug, Clone)]
struct RuntimeWork {
    definition: PhysicalWorkInstance,
    dependency_output_kinds: BTreeMap<String, BTreeSet<DataKind>>,
    state: LifecycleState,
    wait_reason: Option<WaitReason>,
}

/// Deterministic admission controller for concrete instances of the
/// Checkpoint-A physical contract. It owns scheduling state, not computation:
/// callers run the ids returned by [`Self::admit_ready`], then acknowledge
/// computation and commit separately.
pub struct ResourceScheduler {
    budget: ResourceAmount,
    allocated: ResourceAmount,
    units: BTreeMap<WorkUnitId, PhysicalWorkUnit>,
    work: BTreeMap<String, RuntimeWork>,
    dependents: BTreeMap<String, BTreeSet<String>>,
    commit_store: CommitStore,
    transitions: Vec<LifecycleTransition>,
    next_sequence: u64,
}

impl ResourceScheduler {
    pub fn new(
        contract: &PhysicalWorkContract,
        instances: Vec<PhysicalWorkInstance>,
        budget: ResourceAmount,
    ) -> Result<Self, SchedulerContractError> {
        let units: BTreeMap<_, _> = contract
            .units
            .iter()
            .map(|unit| (unit.id, unit.clone()))
            .collect();
        let mut work = BTreeMap::new();
        for instance in instances {
            if work.contains_key(&instance.instance_id) {
                return Err(SchedulerContractError::DuplicateInstance {
                    instance_id: instance.instance_id,
                });
            }
            let Some(unit) = units.get(&instance.work_unit) else {
                return Err(SchedulerContractError::UnknownWorkUnit {
                    instance_id: instance.instance_id,
                    work_unit: instance.work_unit,
                });
            };
            if instance.execution_group != unit.execution_group {
                return Err(SchedulerContractError::InvalidExecutionGroup {
                    instance_id: instance.instance_id,
                    expected: unit.execution_group,
                    actual: instance.execution_group,
                });
            }
            let claim = unit.resources;
            if instance.reservation.cpu_slots == 0 {
                return Err(SchedulerContractError::ZeroCpuReservation {
                    instance_id: instance.instance_id,
                });
            }
            if instance.reservation.memory_bytes == 0 {
                return Err(SchedulerContractError::ZeroMemoryReservation {
                    instance_id: instance.instance_id,
                });
            }
            let nested_cpu_slots = match instance.nested_parallelism {
                NestedParallelism::Disabled => 0,
                NestedParallelism::Accounted {
                    additional_cpu_slots,
                } => additional_cpu_slots,
            };
            if instance.reservation.cpu_slots < 1_u32.saturating_add(nested_cpu_slots) {
                return Err(
                    SchedulerContractError::NestedParallelismExceedsCpuReservation {
                        instance_id: instance.instance_id,
                    },
                );
            }
            if instance.reservation.external_process_slots > 0 && instance.reservation.io_slots == 0
            {
                return Err(
                    SchedulerContractError::ExternalProcessWithoutIoReservation {
                        instance_id: instance.instance_id,
                    },
                );
            }
            if instance.reservation.cpu_slots < u32::from(claim.minimum_cpu_slots)
                || instance.reservation.external_process_slots
                    < u32::from(claim.external_process_slots)
            {
                return Err(SchedulerContractError::ReservationBelowWorkUnitMinimum {
                    instance_id: instance.instance_id,
                });
            }
            if !instance.reservation.fits_within(budget) {
                return Err(SchedulerContractError::ReservationExceedsBudget {
                    instance_id: instance.instance_id,
                });
            }
            work.insert(
                instance.instance_id.clone(),
                RuntimeWork {
                    definition: instance,
                    dependency_output_kinds: BTreeMap::new(),
                    state: LifecycleState::Blocked,
                    wait_reason: None,
                },
            );
        }

        let mut dependents: BTreeMap<String, BTreeSet<String>> = work
            .keys()
            .map(|id| (id.clone(), BTreeSet::new()))
            .collect();
        for (id, runtime) in &work {
            for dependency in &runtime.definition.dependencies {
                let Some(consumers) = dependents.get_mut(dependency) else {
                    return Err(SchedulerContractError::UnknownDependency {
                        instance_id: id.clone(),
                        dependency: dependency.clone(),
                    });
                };
                consumers.insert(id.clone());
            }
        }
        let ids: Vec<_> = work.keys().cloned().collect();
        for id in ids {
            let runtime = &work[&id];
            let consumer = &units[&runtime.definition.work_unit];
            let mut bindings = BTreeMap::new();
            for dependency in &runtime.definition.dependencies {
                let producer_work = &work[dependency];
                let producer = &units[&producer_work.definition.work_unit];
                let kinds: BTreeSet<_> = consumer
                    .inputs
                    .iter()
                    .filter(|input| {
                        input_source_accepts(input.source, producer_work.definition.work_unit)
                            && producer.outputs.contains(&input.kind)
                    })
                    .map(|input| input.kind)
                    .collect();
                if kinds.is_empty() {
                    return Err(SchedulerContractError::InvalidDependencyOutputKind {
                        instance_id: id.clone(),
                        dependency: dependency.clone(),
                    });
                }
                bindings.insert(dependency.clone(), kinds);
            }
            for input in consumer.inputs.iter().filter(|input| !input.optional) {
                if matches!(input.source, InputSource::ExternalAuthority) {
                    continue;
                }
                let supplied = bindings.iter().any(|(dependency, kinds)| {
                    kinds.contains(&input.kind)
                        && input_source_accepts(input.source, work[dependency].definition.work_unit)
                });
                if !supplied {
                    return Err(SchedulerContractError::MissingInputDependency {
                        instance_id: id.clone(),
                        kind: input.kind,
                    });
                }
            }
            work.get_mut(&id)
                .expect("instance exists")
                .dependency_output_kinds = bindings;
        }
        if has_dependency_cycle(&work) {
            return Err(SchedulerContractError::DependencyCycle);
        }

        let mut scheduler = Self {
            budget,
            allocated: ResourceAmount::default(),
            units,
            work,
            dependents,
            commit_store: CommitStore::default(),
            transitions: vec![],
            next_sequence: 0,
        };
        let ids: Vec<_> = scheduler.work.keys().cloned().collect();
        for id in ids {
            let pending = scheduler.pending_dependencies(&id);
            if pending.is_empty() {
                scheduler.set_state(
                    &id,
                    LifecycleState::Ready,
                    None,
                    TransitionReason::DependenciesSatisfied,
                );
            } else {
                let reason = WaitReason::Dependencies { pending };
                scheduler.set_state(
                    &id,
                    LifecycleState::Blocked,
                    Some(reason.clone()),
                    TransitionReason::Waiting(reason),
                );
            }
        }
        Ok(scheduler)
    }

    pub fn state(&self, instance_id: &str) -> Option<LifecycleState> {
        self.work.get(instance_id).map(|work| work.state)
    }

    pub fn wait_reason(&self, instance_id: &str) -> Option<&WaitReason> {
        self.work
            .get(instance_id)
            .and_then(|work| work.wait_reason.as_ref())
    }

    pub fn allocated(&self) -> ResourceAmount {
        self.allocated
    }

    pub fn transitions(&self) -> &[LifecycleTransition] {
        &self.transitions
    }

    pub fn visible_output(&self, target: &CommitTarget) -> Option<&CommittedOutput> {
        self.commit_store.visible(target)
    }

    pub fn has_staged_output(&self, instance_id: &str) -> bool {
        self.commit_store.has_staging(instance_id)
    }

    pub fn declared_outputs(
        &self,
        instance_id: &str,
    ) -> Result<&[DataKind], SchedulerContractError> {
        let runtime =
            self.work
                .get(instance_id)
                .ok_or_else(|| SchedulerContractError::UnknownInstance {
                    instance_id: instance_id.to_string(),
                })?;
        Ok(&self.units[&runtime.definition.work_unit].outputs)
    }

    fn dependency_outputs(
        &self,
        instance_id: &str,
    ) -> Result<Vec<CommittedOutput>, SchedulerContractError> {
        let runtime =
            self.work
                .get(instance_id)
                .ok_or_else(|| SchedulerContractError::UnknownInstance {
                    instance_id: instance_id.to_string(),
                })?;
        Ok(runtime
            .definition
            .dependencies
            .iter()
            .flat_map(|dependency| {
                let kinds = &runtime.dependency_output_kinds[dependency];
                self.commit_store
                    .outputs_for_writer(dependency)
                    .into_iter()
                    .filter(|output| kinds.contains(&output.target.output_kind))
            })
            .collect())
    }

    /// Admits every currently-ready instance that fits, without head-of-line
    /// blocking: one oversized ready sibling cannot hide a smaller independent
    /// sibling that the remaining global budget can run.
    pub fn admit_ready(&mut self) -> Result<Vec<String>, SchedulerContractError> {
        let ids: Vec<_> = self.work.keys().cloned().collect();
        let mut admitted = vec![];
        for id in ids {
            let state = self.work[&id].state;
            let resource_wait = state == LifecycleState::Blocked
                && matches!(
                    self.work[&id].wait_reason,
                    Some(WaitReason::Resources { .. })
                );
            if state != LifecycleState::Ready && !resource_wait {
                continue;
            }
            let reservation = self.work[&id].definition.reservation;
            let available = self
                .budget
                .checked_sub(self.allocated)
                .ok_or(SchedulerContractError::ResourceAccountingOverflow)?;
            if reservation.fits_within(available) {
                if resource_wait {
                    self.set_state(
                        &id,
                        LifecycleState::Ready,
                        None,
                        TransitionReason::ResourcesAvailable,
                    );
                }
                self.allocated = self
                    .allocated
                    .checked_add(reservation)
                    .ok_or(SchedulerContractError::ResourceAccountingOverflow)?;
                self.set_state(
                    &id,
                    LifecycleState::Running,
                    None,
                    TransitionReason::Admitted,
                );
                admitted.push(id);
            } else {
                let reason = WaitReason::Resources {
                    deficit: ResourceDeficit::between(reservation, available),
                };
                if self.work[&id].wait_reason.as_ref() != Some(&reason) {
                    self.set_state(
                        &id,
                        LifecycleState::Blocked,
                        Some(reason.clone()),
                        TransitionReason::Waiting(reason),
                    );
                }
            }
        }
        Ok(admitted)
    }

    /// Atomically stages every declared output and marks computation done.
    /// Staged bytes remain invisible and the full reservation remains owned
    /// until [`Self::commit`] publishes the whole batch.
    pub fn stage_outputs(
        &mut self,
        instance_id: &str,
        writes: Vec<CommitWrite>,
    ) -> Result<Vec<CommitTarget>, SchedulerContractError> {
        self.require_state(instance_id, LifecycleState::Running, "stage_outputs")?;
        let work_unit = self.work[instance_id].definition.work_unit;
        let unit = &self.units[&work_unit];
        let targets = self
            .commit_store
            .stage(instance_id, unit, writes)
            .map_err(SchedulerContractError::CommitStore)?;
        self.set_state(
            instance_id,
            LifecycleState::Committing,
            None,
            TransitionReason::ComputationFinished,
        );
        Ok(targets)
    }

    pub fn commit(&mut self, instance_id: &str) -> Result<(), SchedulerContractError> {
        self.require_state(instance_id, LifecycleState::Committing, "commit")?;
        self.commit_store
            .commit(instance_id)
            .map_err(SchedulerContractError::CommitStore)?;
        self.release(instance_id)?;
        self.set_state(
            instance_id,
            LifecycleState::Completed,
            None,
            TransitionReason::CommitSucceeded,
        );
        self.refresh_dependency_waiters(instance_id);
        Ok(())
    }

    pub fn fail(
        &mut self,
        instance_id: &str,
        detail: impl Into<String>,
    ) -> Result<(), SchedulerContractError> {
        let state = self.known_state(instance_id)?;
        if !matches!(state, LifecycleState::Running | LifecycleState::Committing) {
            return Err(SchedulerContractError::InvalidTransition {
                instance_id: instance_id.to_string(),
                from: state,
                operation: "fail",
            });
        }
        self.commit_store.discard(instance_id);
        self.release(instance_id)?;
        self.set_state(
            instance_id,
            LifecycleState::Failed,
            None,
            TransitionReason::ExecutionFailed {
                detail: detail.into(),
            },
        );
        self.cancel_descendants(instance_id, true);
        Ok(())
    }

    /// Cancels the requested consumer and its consumers. Producers and
    /// independent siblings are intentionally untouched.
    pub fn cancel(
        &mut self,
        instance_id: &str,
        detail: impl Into<String>,
    ) -> Result<(), SchedulerContractError> {
        let state = self.known_state(instance_id)?;
        if state.is_terminal() {
            return Err(SchedulerContractError::InvalidTransition {
                instance_id: instance_id.to_string(),
                from: state,
                operation: "cancel",
            });
        }
        if state.owns_resources() {
            self.commit_store.discard(instance_id);
            self.release(instance_id)?;
        }
        self.set_state(
            instance_id,
            LifecycleState::Cancelled,
            None,
            TransitionReason::ExplicitCancellation {
                detail: detail.into(),
            },
        );
        self.cancel_descendants(instance_id, false);
        Ok(())
    }

    fn known_state(&self, instance_id: &str) -> Result<LifecycleState, SchedulerContractError> {
        self.work
            .get(instance_id)
            .map(|work| work.state)
            .ok_or_else(|| SchedulerContractError::UnknownInstance {
                instance_id: instance_id.to_string(),
            })
    }

    fn require_state(
        &self,
        instance_id: &str,
        expected: LifecycleState,
        operation: &'static str,
    ) -> Result<(), SchedulerContractError> {
        let state = self.known_state(instance_id)?;
        if state == expected {
            Ok(())
        } else {
            Err(SchedulerContractError::InvalidTransition {
                instance_id: instance_id.to_string(),
                from: state,
                operation,
            })
        }
    }

    fn release(&mut self, instance_id: &str) -> Result<(), SchedulerContractError> {
        let reservation = self.work[instance_id].definition.reservation;
        self.allocated = self
            .allocated
            .checked_sub(reservation)
            .ok_or(SchedulerContractError::ResourceAccountingOverflow)?;
        Ok(())
    }

    fn pending_dependencies(&self, instance_id: &str) -> BTreeSet<String> {
        self.work[instance_id]
            .definition
            .dependencies
            .iter()
            .filter(|dependency| self.work[dependency.as_str()].state != LifecycleState::Completed)
            .cloned()
            .collect()
    }

    fn refresh_dependency_waiters(&mut self, producer: &str) {
        let consumers: Vec<_> = self.dependents[producer].iter().cloned().collect();
        for consumer in consumers {
            if self.work[&consumer].state != LifecycleState::Blocked
                || !matches!(
                    self.work[&consumer].wait_reason,
                    Some(WaitReason::Dependencies { .. })
                )
            {
                continue;
            }
            let pending = self.pending_dependencies(&consumer);
            if pending.is_empty() {
                self.set_state(
                    &consumer,
                    LifecycleState::Ready,
                    None,
                    TransitionReason::DependenciesSatisfied,
                );
            } else {
                self.work.get_mut(&consumer).unwrap().wait_reason =
                    Some(WaitReason::Dependencies { pending });
            }
        }
    }

    fn cancel_descendants(&mut self, root: &str, producer_failed: bool) {
        let mut queue: Vec<_> = self.dependents[root].iter().cloned().collect();
        while let Some(id) = queue.pop() {
            let state = self.work[&id].state;
            if state.is_terminal() {
                continue;
            }
            debug_assert!(
                !state.owns_resources(),
                "a consumer cannot run before commit"
            );
            let reason = if producer_failed {
                TransitionReason::ProducerFailed {
                    producer: root.to_string(),
                }
            } else {
                TransitionReason::UpstreamCancelled {
                    upstream: root.to_string(),
                }
            };
            self.set_state(&id, LifecycleState::Cancelled, None, reason);
            queue.extend(self.dependents[&id].iter().cloned());
        }
    }

    fn set_state(
        &mut self,
        instance_id: &str,
        to: LifecycleState,
        wait_reason: Option<WaitReason>,
        reason: TransitionReason,
    ) {
        let runtime = self.work.get_mut(instance_id).unwrap();
        let from = if self
            .transitions
            .iter()
            .any(|event| event.instance_id == instance_id)
        {
            Some(runtime.state)
        } else {
            None
        };
        runtime.state = to;
        runtime.wait_reason = wait_reason;
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.transitions.push(LifecycleTransition {
            sequence,
            instance_id: instance_id.to_string(),
            from,
            to,
            reason,
        });
    }
}

/// Executes a planned physical graph through the production admission,
/// lifecycle, and commit controls. The callback owns computation only: it can
/// read committed dependency outputs and return the complete declared output
/// batch, but cannot publish a partial result or make a consumer ready.
pub fn execute_physical_work_graph<F>(
    contract: &PhysicalWorkContract,
    instances: Vec<PhysicalWorkInstance>,
    budget: ResourceAmount,
    mut compute: F,
) -> Result<PhysicalWorkExecutionReport, PhysicalWorkExecutionError>
where
    F: FnMut(&PhysicalWorkInstance, &[CommittedOutput]) -> Result<Vec<CommitWrite>, String>,
{
    let mut scheduler = ResourceScheduler::new(contract, instances, budget)
        .map_err(PhysicalWorkExecutionError::Scheduler)?;
    let mut executed = BTreeSet::new();

    while scheduler
        .work
        .values()
        .any(|work| !work.state.is_terminal())
    {
        let admitted = scheduler
            .admit_ready()
            .map_err(PhysicalWorkExecutionError::Scheduler)?;
        if admitted.is_empty() {
            return Err(PhysicalWorkExecutionError::Stalled {
                states: scheduler
                    .work
                    .iter()
                    .map(|(id, work)| (id.clone(), work.state))
                    .collect(),
            });
        }

        for instance_id in admitted {
            executed.insert(instance_id.clone());
            let instance = scheduler.work[&instance_id].definition.clone();
            let inputs = scheduler
                .dependency_outputs(&instance_id)
                .map_err(PhysicalWorkExecutionError::Scheduler)?;
            match compute(&instance, &inputs) {
                Ok(writes) => {
                    scheduler
                        .stage_outputs(&instance_id, writes)
                        .map_err(PhysicalWorkExecutionError::Scheduler)?;
                    scheduler
                        .commit(&instance_id)
                        .map_err(PhysicalWorkExecutionError::Scheduler)?;
                }
                Err(detail) => scheduler
                    .fail(&instance_id, detail)
                    .map_err(PhysicalWorkExecutionError::Scheduler)?,
            }
        }
    }

    let states: BTreeMap<_, _> = scheduler
        .work
        .iter()
        .map(|(id, work)| (id.clone(), work.state))
        .collect();
    let completed = states
        .iter()
        .filter(|(_, state)| **state == LifecycleState::Completed)
        .map(|(id, _)| id.clone())
        .collect();
    let reusable = states
        .iter()
        .filter(|(id, state)| {
            **state == LifecycleState::Completed
                && scheduler.commit_store.outputs_for_writer(id).len()
                    == scheduler.units[&scheduler.work[*id].definition.work_unit]
                        .outputs
                        .len()
        })
        .map(|(id, _)| id.clone())
        .collect();
    let invalid: BTreeSet<_> = states
        .iter()
        .filter(|(_, state)| **state == LifecycleState::Failed)
        .map(|(id, _)| id.clone())
        .collect();
    let rerun = states
        .iter()
        .filter(|(_, state)| matches!(state, LifecycleState::Failed | LifecycleState::Cancelled))
        .map(|(id, _)| id.clone())
        .collect();
    let never_started = states
        .iter()
        .filter(|(id, state)| **state == LifecycleState::Cancelled && !executed.contains(*id))
        .map(|(id, _)| id.clone())
        .collect();

    Ok(PhysicalWorkExecutionReport {
        transitions: scheduler.transitions,
        states,
        executed,
        completed,
        reusable,
        invalid,
        rerun,
        never_started,
        committed_outputs: scheduler.commit_store.committed,
    })
}

fn input_source_accepts(source: InputSource, producer: WorkUnitId) -> bool {
    match source {
        InputSource::ExternalAuthority => false,
        InputSource::ProducedBy(expected) => expected == producer,
        InputSource::ProducedByEither(first, second) => first == producer || second == producer,
    }
}

fn has_dependency_cycle(work: &BTreeMap<String, RuntimeWork>) -> bool {
    fn visit(
        id: &str,
        work: &BTreeMap<String, RuntimeWork>,
        visiting: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> bool {
        if done.contains(id) {
            return false;
        }
        if !visiting.insert(id.to_string()) {
            return true;
        }
        for dependency in &work[id].definition.dependencies {
            if visit(dependency, work, visiting, done) {
                return true;
            }
        }
        visiting.remove(id);
        done.insert(id.to_string());
        false
    }

    let mut visiting = BTreeSet::new();
    let mut done = BTreeSet::new();
    work.keys()
        .any(|id| visit(id, work, &mut visiting, &mut done))
}

#[cfg(test)]
mod tests {
    use super::*;
    use laminaria_plan::physical_work::{
        canonical_physical_work_contract, physical_planning_input, ExecutionGroup, InputSource,
        LogicalActivity, PlannedPhysicalWork, PlannedResourceReservation, WorkInput, WorkUnitId,
    };

    fn amount(cpu: u32, memory: u64, io: u32, processes: u32) -> ResourceAmount {
        ResourceAmount {
            cpu_slots: cpu,
            memory_bytes: memory,
            io_slots: io,
            external_process_slots: processes,
        }
    }

    fn planned_instance(
        contract: &PhysicalWorkContract,
        id: &str,
        activity: LogicalActivity,
        dependencies: &[&str],
    ) -> PlannedPhysicalWork {
        let work_unit = WorkUnitId::Logical(activity);
        let unit = contract
            .units
            .iter()
            .find(|unit| unit.id == work_unit)
            .unwrap();
        PlannedPhysicalWork {
            instance_id: id.to_string(),
            work_unit,
            execution_group: unit.execution_group,
            dependencies: dependencies.iter().map(|id| (*id).to_string()).collect(),
            reservation: PlannedResourceReservation {
                cpu_slots: 1,
                memory_bytes: 8,
                io_slots: 1,
                external_process_slots: 0,
            },
            nested_parallelism: PlannedNestedParallelism::Disabled,
        }
    }

    fn instance(
        id: &str,
        activity: LogicalActivity,
        dependencies: &[&str],
        reservation: ResourceAmount,
    ) -> PhysicalWorkInstance {
        PhysicalWorkInstance {
            instance_id: id.to_string(),
            work_unit: WorkUnitId::Logical(activity),
            execution_group: canonical_physical_work_contract()
                .units
                .into_iter()
                .find(|unit| unit.id == WorkUnitId::Logical(activity))
                .unwrap()
                .execution_group,
            dependencies: dependencies.iter().map(|id| (*id).to_string()).collect(),
            reservation,
            nested_parallelism: NestedParallelism::Disabled,
        }
    }

    fn scheduler(
        instances: Vec<PhysicalWorkInstance>,
        budget: ResourceAmount,
    ) -> ResourceScheduler {
        // Lifecycle-focused tests use small synthetic graphs. Give each
        // supplied edge an explicit typed contract input instead of letting
        // the tests depend on unrelated production graph predecessors.
        let mut contract = canonical_physical_work_contract();
        for unit in &mut contract.units {
            unit.inputs.clear();
        }
        for consumer in &instances {
            let inputs = consumer
                .dependencies
                .iter()
                .map(|dependency_id| {
                    let producer = instances
                        .iter()
                        .find(|instance| &instance.instance_id == dependency_id)
                        .expect("test dependency has a producer instance");
                    let producer_unit = contract
                        .units
                        .iter()
                        .find(|unit| unit.id == producer.work_unit)
                        .expect("test producer has a contracted work unit");
                    let kind = *producer_unit
                        .outputs
                        .first()
                        .expect("test producer declares an output");
                    WorkInput {
                        kind,
                        source: InputSource::ProducedBy(producer.work_unit),
                        optional: false,
                    }
                })
                .collect::<Vec<_>>();
            let consumer_unit = contract
                .units
                .iter_mut()
                .find(|unit| unit.id == consumer.work_unit)
                .expect("test instance has a contracted work unit");
            consumer_unit.inputs.extend(inputs);
        }
        ResourceScheduler::new(&contract, instances, budget).unwrap()
    }

    fn writes_for(scheduler: &ResourceScheduler, id: &str, key_prefix: &str) -> Vec<CommitWrite> {
        scheduler
            .declared_outputs(id)
            .unwrap()
            .iter()
            .map(|output_kind| CommitWrite {
                output_kind: *output_kind,
                logical_key: format!("{key_prefix}:{output_kind:?}"),
                bytes: format!("{id}:{output_kind:?}").into_bytes(),
            })
            .collect()
    }

    fn canonical_instance_id(work_unit: WorkUnitId) -> String {
        format!("{work_unit:?}")
    }

    fn canonical_mixed_instances(contract: &PhysicalWorkContract) -> Vec<PhysicalWorkInstance> {
        let known: BTreeSet<_> = contract.units.iter().map(|unit| unit.id).collect();
        contract
            .units
            .iter()
            .map(|unit| {
                let dependencies = unit
                    .inputs
                    .iter()
                    .filter_map(|input| match input.source {
                        InputSource::ExternalAuthority => None,
                        InputSource::ProducedBy(producer) => {
                            (!input.optional || known.contains(&producer)).then_some(producer)
                        }
                        // The concrete identity binds the either/or input to
                        // one producer; it is not a barrier over both lanes.
                        InputSource::ProducedByEither(producer, _) => Some(producer),
                    })
                    .map(canonical_instance_id)
                    .collect();
                let external_process_slots = u32::from(unit.resources.external_process_slots);
                PhysicalWorkInstance {
                    instance_id: canonical_instance_id(unit.id),
                    work_unit: unit.id,
                    execution_group: unit.execution_group,
                    dependencies,
                    reservation: amount(
                        u32::from(unit.resources.minimum_cpu_slots),
                        1,
                        u32::from(external_process_slots > 0),
                        external_process_slots,
                    ),
                    nested_parallelism: NestedParallelism::Disabled,
                }
            })
            .collect()
    }

    fn deterministic_computation(
        contract: &PhysicalWorkContract,
        instance: &PhysicalWorkInstance,
        inputs: &[CommittedOutput],
    ) -> Result<Vec<CommitWrite>, String> {
        let unit = contract
            .units
            .iter()
            .find(|unit| unit.id == instance.work_unit)
            .unwrap();
        let input_digests = inputs
            .iter()
            .map(|input| input.content_sha256.as_str())
            .collect::<Vec<_>>()
            .join(",");
        Ok(unit
            .outputs
            .iter()
            .map(|output_kind| CommitWrite {
                output_kind: *output_kind,
                logical_key: format!("{}:{output_kind:?}", instance.instance_id),
                bytes: format!(
                    "work={:?};output={output_kind:?};inputs={input_digests}",
                    instance.work_unit
                )
                .into_bytes(),
            })
            .collect())
    }

    fn peak_active_work(transitions: &[LifecycleTransition]) -> usize {
        let mut active = 0;
        let mut peak = 0;
        for transition in transitions {
            if transition.to == LifecycleState::Running {
                active += 1;
                peak = peak.max(active);
            } else if transition.to.is_terminal()
                && transition.from.is_some_and(|from| from.owns_resources())
            {
                active -= 1;
            }
        }
        peak
    }

    fn descendants_of(root: &str, instances: &[PhysicalWorkInstance]) -> BTreeSet<String> {
        let mut descendants = BTreeSet::new();
        let mut frontier = vec![root.to_string()];
        while let Some(producer) = frontier.pop() {
            for consumer in instances
                .iter()
                .filter(|instance| instance.dependencies.contains(&producer))
            {
                if descendants.insert(consumer.instance_id.clone()) {
                    frontier.push(consumer.instance_id.clone());
                }
            }
        }
        descendants
    }

    #[test]
    #[cfg(unix)]
    fn real_nim_plan_drives_the_shared_resource_executor_without_legacy_action_mapping() {
        let contract = canonical_physical_work_contract();
        let declarations = vec![
            planned_instance(
                &contract,
                "rust-root",
                LogicalActivity::A2MemberManifest,
                &[],
            ),
            planned_instance(
                &contract,
                "rust-dependent",
                LogicalActivity::A3PackageResolution,
                &["rust-root"],
            ),
            planned_instance(
                &contract,
                "nim-root",
                LogicalActivity::N2DependencyResolution,
                &[],
            ),
            planned_instance(
                &contract,
                "nim-dependent",
                LogicalActivity::N3BuildInvocation,
                &["nim-root"],
            ),
            planned_instance(
                &contract,
                "unused-rust-root",
                LogicalActivity::A2MemberManifest,
                &[],
            ),
        ];
        let input = physical_planning_input(
            &contract,
            declarations,
            vec!["rust-dependent".to_string(), "nim-dependent".to_string()],
        )
        .unwrap();
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let planner = crate::test_support::real_planner_binary(&repo_root);
        let outcome = laminaria_plan::call_planner(&planner, &input).unwrap();
        let laminaria_plan::PlanOutcome::Planned(plan) = outcome else {
            panic!("production Nim planner rejected a valid physical-work graph")
        };
        laminaria_plan::validate(&plan, &input).unwrap();
        assert_eq!(input.physical_work.len(), 5);
        assert_eq!(plan.physical_work.len(), 4);
        assert!(!plan.physical_work.contains_key("unused-rust-root"));

        let instances = physical_instances_from_execution_plan(&contract, &plan).unwrap();
        assert_eq!(instances.len(), 4);
        let report = execute_physical_work_graph(
            &contract,
            instances,
            amount(1, 16, 1, 0),
            |instance, inputs| {
                let expected_group = contract
                    .units
                    .iter()
                    .find(|unit| unit.id == instance.work_unit)
                    .unwrap()
                    .execution_group;
                assert_eq!(instance.execution_group, expected_group);
                deterministic_computation(&contract, instance, inputs)
            },
        )
        .unwrap();

        assert_eq!(
            report.completed,
            BTreeSet::from([
                "nim-dependent".to_string(),
                "nim-root".to_string(),
                "rust-dependent".to_string(),
                "rust-root".to_string(),
            ])
        );
        assert!(report.transitions.iter().any(|transition| matches!(
            transition.reason,
            TransitionReason::Waiting(WaitReason::Resources { .. })
        )));
    }

    #[test]
    fn consumer_receives_only_the_declared_dependency_output_kind() {
        let mut contract = canonical_physical_work_contract();
        let n5 = WorkUnitId::Logical(LogicalActivity::N5SemanticAnalysisAndCGeneration);
        let foreign_target = WorkUnitId::Logical(LogicalActivity::ForeignTargetResolution);
        contract
            .units
            .iter_mut()
            .find(|unit| unit.id == n5)
            .unwrap()
            .inputs
            .clear();
        contract
            .units
            .iter_mut()
            .find(|unit| unit.id == foreign_target)
            .unwrap()
            .inputs = vec![laminaria_plan::physical_work::WorkInput {
            kind: DataKind::ForeignDecl,
            source: InputSource::ProducedBy(n5),
            optional: false,
        }];
        let mut wrong_group = instance(
            "wrong-group",
            LogicalActivity::N5SemanticAnalysisAndCGeneration,
            &[],
            amount(1, 8, 0, 0),
        );
        wrong_group.execution_group = ExecutionGroup::NativeExecutableLink;
        assert!(matches!(
            ResourceScheduler::new(&contract, vec![wrong_group], amount(2, 16, 0, 0)),
            Err(SchedulerContractError::InvalidExecutionGroup { .. })
        ));

        let producer_id = "nim-analysis";
        let consumer_id = "foreign-target";
        let mut scheduler = ResourceScheduler::new(
            &contract,
            vec![
                instance(
                    producer_id,
                    LogicalActivity::N5SemanticAnalysisAndCGeneration,
                    &[],
                    amount(1, 8, 0, 0),
                ),
                instance(
                    consumer_id,
                    LogicalActivity::ForeignTargetResolution,
                    &[producer_id],
                    amount(1, 8, 0, 0),
                ),
            ],
            amount(2, 16, 0, 0),
        )
        .unwrap();

        assert_eq!(
            scheduler.admit_ready().unwrap(),
            vec![producer_id.to_string()]
        );
        let writes = writes_for(&scheduler, producer_id, producer_id);
        scheduler.stage_outputs(producer_id, writes).unwrap();
        scheduler.commit(producer_id).unwrap();

        let inputs = scheduler.dependency_outputs(consumer_id).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].target.output_kind, DataKind::ForeignDecl);
    }

    #[test]
    fn parallel_ready_instances_run_together_and_commit_independently() {
        let mut scheduler = scheduler(
            vec![
                instance(
                    "rust-front",
                    LogicalActivity::A2MemberManifest,
                    &[],
                    amount(1, 10, 0, 0),
                ),
                instance(
                    "nim-front",
                    LogicalActivity::N2DependencyResolution,
                    &[],
                    amount(1, 10, 0, 0),
                ),
            ],
            amount(2, 20, 0, 0),
        );

        assert_eq!(
            scheduler.admit_ready().unwrap(),
            vec!["nim-front".to_string(), "rust-front".to_string()]
        );
        assert_eq!(scheduler.state("rust-front"), Some(LifecycleState::Running));
        assert_eq!(scheduler.state("nim-front"), Some(LifecycleState::Running));
        let writes = writes_for(&scheduler, "rust-front", "rust-front");
        scheduler.stage_outputs("rust-front", writes).unwrap();
        scheduler.commit("rust-front").unwrap();
        assert_eq!(scheduler.state("nim-front"), Some(LifecycleState::Running));
    }

    #[test]
    fn resource_block_records_exact_wait_and_does_not_cancel_sibling() {
        let mut scheduler = scheduler(
            vec![
                instance(
                    "a",
                    LogicalActivity::A2MemberManifest,
                    &[],
                    amount(1, 8, 1, 0),
                ),
                instance(
                    "b",
                    LogicalActivity::N2DependencyResolution,
                    &[],
                    amount(1, 8, 1, 0),
                ),
            ],
            amount(1, 16, 1, 0),
        );

        assert_eq!(scheduler.admit_ready().unwrap(), vec!["a"]);
        assert_eq!(scheduler.state("b"), Some(LifecycleState::Blocked));
        assert_eq!(
            scheduler.wait_reason("b"),
            Some(&WaitReason::Resources {
                deficit: ResourceDeficit {
                    cpu_slots: 1,
                    memory_bytes: 0,
                    io_slots: 1,
                    external_process_slots: 0,
                }
            })
        );
        let writes = writes_for(&scheduler, "a", "a");
        scheduler.stage_outputs("a", writes).unwrap();
        scheduler.commit("a").unwrap();
        assert_eq!(scheduler.admit_ready().unwrap(), vec!["b"]);
    }

    #[test]
    fn failed_producer_cancels_consumers_but_not_independent_running_work() {
        let mut scheduler = scheduler(
            vec![
                instance(
                    "producer",
                    LogicalActivity::A2MemberManifest,
                    &[],
                    amount(1, 1, 0, 0),
                ),
                instance(
                    "consumer",
                    LogicalActivity::A3PackageResolution,
                    &["producer"],
                    amount(1, 1, 0, 0),
                ),
                instance(
                    "sibling",
                    LogicalActivity::N2DependencyResolution,
                    &[],
                    amount(1, 1, 0, 0),
                ),
            ],
            amount(2, 3, 0, 0),
        );

        assert_eq!(
            scheduler.admit_ready().unwrap(),
            vec!["producer".to_string(), "sibling".to_string()]
        );
        scheduler.fail("producer", "bad source").unwrap();
        assert_eq!(scheduler.state("consumer"), Some(LifecycleState::Cancelled));
        assert_eq!(scheduler.state("sibling"), Some(LifecycleState::Running));
        assert!(!scheduler.transitions().iter().any(|event| {
            event.instance_id == "consumer" && event.to == LifecycleState::Running
        }));
    }

    #[test]
    fn consumer_cancellation_does_not_cancel_its_producer_or_sibling() {
        let mut scheduler = scheduler(
            vec![
                instance(
                    "producer",
                    LogicalActivity::A2MemberManifest,
                    &[],
                    amount(1, 1, 0, 0),
                ),
                instance(
                    "consumer",
                    LogicalActivity::A3PackageResolution,
                    &["producer"],
                    amount(1, 1, 0, 0),
                ),
                instance(
                    "downstream",
                    LogicalActivity::A4SourceUnitGraph,
                    &["consumer"],
                    amount(1, 1, 0, 0),
                ),
                instance(
                    "sibling",
                    LogicalActivity::N2DependencyResolution,
                    &[],
                    amount(1, 1, 0, 0),
                ),
            ],
            amount(2, 4, 0, 0),
        );
        scheduler.admit_ready().unwrap();
        scheduler.cancel("consumer", "demand removed").unwrap();
        assert_eq!(scheduler.state("consumer"), Some(LifecycleState::Cancelled));
        assert_eq!(
            scheduler.state("downstream"),
            Some(LifecycleState::Cancelled)
        );
        assert_eq!(scheduler.state("producer"), Some(LifecycleState::Running));
        assert_eq!(scheduler.state("sibling"), Some(LifecycleState::Running));
    }

    #[test]
    fn memory_remains_reserved_through_committing_and_backpressures_next_ir() {
        let mut scheduler = scheduler(
            vec![
                instance(
                    "ir-a",
                    LogicalActivity::A6SemanticAnalysis,
                    &[],
                    amount(1, 8, 0, 0),
                ),
                instance(
                    "ir-b",
                    LogicalActivity::N4ModuleReachability,
                    &[],
                    amount(1, 8, 0, 0),
                ),
            ],
            amount(2, 10, 0, 0),
        );

        assert_eq!(scheduler.admit_ready().unwrap(), vec!["ir-a"]);
        let writes = writes_for(&scheduler, "ir-a", "ir-a");
        scheduler.stage_outputs("ir-a", writes).unwrap();
        assert_eq!(scheduler.allocated().memory_bytes, 8);
        assert!(scheduler.admit_ready().unwrap().is_empty());
        assert_eq!(
            scheduler.wait_reason("ir-b"),
            Some(&WaitReason::Resources {
                deficit: ResourceDeficit {
                    memory_bytes: 6,
                    ..ResourceDeficit::default()
                }
            })
        );
        scheduler.commit("ir-a").unwrap();
        assert_eq!(scheduler.admit_ready().unwrap(), vec!["ir-b"]);
    }

    #[test]
    fn same_key_same_content_is_idempotent_but_different_content_conflicts() {
        let instances = vec![
            instance(
                "writer-a",
                LogicalActivity::A7Monomorphization,
                &[],
                amount(1, 1, 0, 0),
            ),
            instance(
                "writer-b",
                LogicalActivity::A7Monomorphization,
                &[],
                amount(1, 1, 0, 0),
            ),
            instance(
                "writer-other-key",
                LogicalActivity::A7Monomorphization,
                &[],
                amount(1, 1, 0, 0),
            ),
        ];
        let mut idempotent = scheduler(instances, amount(3, 3, 0, 0));
        assert_eq!(idempotent.admit_ready().unwrap().len(), 3);

        let shared = CommitWrite {
            output_kind: DataKind::InstantiationKey,
            logical_key: "dep-closure/function<i32>".to_string(),
            bytes: b"canonical-instantiation".to_vec(),
        };
        let target = idempotent
            .stage_outputs("writer-a", vec![shared.clone()])
            .unwrap()
            .remove(0);
        idempotent
            .stage_outputs("writer-b", vec![shared])
            .expect("same key and content is an idempotent concurrent writer");

        let other_targets = idempotent
            .stage_outputs(
                "writer-other-key",
                vec![CommitWrite {
                    output_kind: DataKind::InstantiationKey,
                    logical_key: "dep-closure/other-function<i32>".to_string(),
                    bytes: b"other-instantiation".to_vec(),
                }],
            )
            .unwrap();
        idempotent.commit("writer-b").unwrap();
        idempotent.commit("writer-a").unwrap();
        idempotent.commit("writer-other-key").unwrap();
        assert_eq!(
            idempotent.visible_output(&target).unwrap().bytes,
            b"canonical-instantiation"
        );
        assert!(idempotent.visible_output(&other_targets[0]).is_some());

        let mut conflict = scheduler(
            vec![
                instance(
                    "first",
                    LogicalActivity::A7Monomorphization,
                    &[],
                    amount(1, 1, 0, 0),
                ),
                instance(
                    "second",
                    LogicalActivity::A7Monomorphization,
                    &[],
                    amount(1, 1, 0, 0),
                ),
            ],
            amount(2, 2, 0, 0),
        );
        conflict.admit_ready().unwrap();
        conflict
            .stage_outputs(
                "first",
                vec![CommitWrite {
                    output_kind: DataKind::InstantiationKey,
                    logical_key: "same-key".to_string(),
                    bytes: b"content-a".to_vec(),
                }],
            )
            .unwrap();
        assert!(matches!(
            conflict.stage_outputs(
                "second",
                vec![CommitWrite {
                    output_kind: DataKind::InstantiationKey,
                    logical_key: "same-key".to_string(),
                    bytes: b"content-b".to_vec(),
                }]
            ),
            Err(SchedulerContractError::CommitStore(
                CommitStoreError::ConflictingContent { .. }
            ))
        ));
        assert_eq!(conflict.state("second"), Some(LifecycleState::Running));
    }

    #[test]
    fn commit_before_failure_is_invisible_and_commit_then_enables_consumer() {
        let graph = || {
            vec![
                instance(
                    "object",
                    LogicalActivity::A10CodeGeneration,
                    &[],
                    amount(1, 4, 1, 0),
                ),
                instance(
                    "symbol-consumer",
                    LogicalActivity::A11SymbolRegistration,
                    &["object"],
                    amount(1, 1, 0, 0),
                ),
            ]
        };
        let object_write = || CommitWrite {
            output_kind: DataKind::NativeObject,
            logical_key: "object:main".to_string(),
            bytes: b"object-bytes".to_vec(),
        };

        let mut failed = scheduler(graph(), amount(1, 5, 1, 0));
        assert_eq!(failed.admit_ready().unwrap(), vec!["object"]);
        let target = failed
            .stage_outputs("object", vec![object_write()])
            .unwrap()
            .remove(0);
        assert!(failed.visible_output(&target).is_none());
        assert!(failed.has_staged_output("object"));
        failed
            .fail("object", "fault immediately before commit")
            .unwrap();
        assert!(failed.visible_output(&target).is_none());
        assert!(!failed.has_staged_output("object"));
        assert_eq!(
            failed.state("symbol-consumer"),
            Some(LifecycleState::Cancelled)
        );

        let mut succeeded = scheduler(graph(), amount(1, 5, 1, 0));
        succeeded.admit_ready().unwrap();
        let target = succeeded
            .stage_outputs("object", vec![object_write()])
            .unwrap()
            .remove(0);
        succeeded.commit("object").unwrap();
        assert_eq!(
            succeeded.visible_output(&target).unwrap().content_sha256,
            sha256_bytes(b"object-bytes")
        );
        assert_eq!(
            succeeded.state("symbol-consumer"),
            Some(LifecycleState::Ready)
        );
        assert_eq!(succeeded.admit_ready().unwrap(), vec!["symbol-consumer"]);
    }

    #[test]
    fn multi_output_and_final_artifact_batches_publish_only_at_their_boundaries() {
        let mut multi = scheduler(
            vec![instance(
                "analysis",
                LogicalActivity::A6SemanticAnalysis,
                &[],
                amount(1, 2, 0, 0),
            )],
            amount(1, 2, 0, 0),
        );
        multi.admit_ready().unwrap();
        assert!(matches!(
            multi.stage_outputs(
                "analysis",
                vec![CommitWrite {
                    output_kind: DataKind::SemanticFact,
                    logical_key: "function:f".to_string(),
                    bytes: b"semantic-fact".to_vec(),
                }]
            ),
            Err(SchedulerContractError::CommitStore(
                CommitStoreError::MissingDeclaredOutput {
                    output_kind: DataKind::ForeignDecl,
                    ..
                }
            ))
        ));
        assert_eq!(multi.state("analysis"), Some(LifecycleState::Running));
        assert!(!multi.has_staged_output("analysis"));
        let targets = multi
            .stage_outputs(
                "analysis",
                vec![
                    CommitWrite {
                        output_kind: DataKind::SemanticFact,
                        logical_key: "function:f".to_string(),
                        bytes: b"semantic-fact".to_vec(),
                    },
                    CommitWrite {
                        output_kind: DataKind::ForeignDecl,
                        logical_key: "extern:c_add".to_string(),
                        bytes: b"foreign-decl".to_vec(),
                    },
                ],
            )
            .unwrap();
        assert!(targets
            .iter()
            .all(|target| multi.visible_output(target).is_none()));
        multi.commit("analysis").unwrap();
        assert!(targets
            .iter()
            .all(|target| multi.visible_output(target).is_some()));

        let mut final_link = scheduler(
            vec![instance(
                "final",
                LogicalActivity::A12FinalLink,
                &[],
                amount(1, 1, 1, 1),
            )],
            amount(1, 1, 1, 1),
        );
        final_link.admit_ready().unwrap();
        let target = final_link
            .stage_outputs(
                "final",
                vec![CommitWrite {
                    output_kind: DataKind::LinkedArtifact,
                    logical_key: "native:app".to_string(),
                    bytes: b"executable".to_vec(),
                }],
            )
            .unwrap()
            .remove(0);
        assert_eq!(
            target.boundary,
            CommitBoundary::FinalArtifactPublicationHandoff
        );
        assert!(final_link.visible_output(&target).is_none());
        final_link.commit("final").unwrap();
        assert!(final_link.visible_output(&target).is_some());
    }

    #[test]
    fn external_compiler_must_reserve_its_process_and_total_nested_cpu() {
        let mut contract = canonical_physical_work_contract();
        contract
            .units
            .iter_mut()
            .find(|unit| unit.id == WorkUnitId::Logical(LogicalActivity::N7CCompilation))
            .unwrap()
            .inputs
            .clear();
        let mut external = instance(
            "cc",
            LogicalActivity::N7CCompilation,
            &[],
            // Parent plus two compiler worker threads: all three are charged.
            amount(3, 1, 1, 1),
        );
        external.nested_parallelism = NestedParallelism::Accounted {
            additional_cpu_slots: 2,
        };
        assert!(matches!(
            ResourceScheduler::new(&contract, vec![external.clone()], amount(2, 1, 1, 1)),
            Err(SchedulerContractError::ReservationExceedsBudget { .. })
        ));
        assert!(ResourceScheduler::new(&contract, vec![external], amount(3, 1, 1, 1)).is_ok());

        let mut missing_process = instance(
            "cc",
            LogicalActivity::N7CCompilation,
            &[],
            amount(3, 1, 1, 0),
        );
        missing_process.nested_parallelism = NestedParallelism::Accounted {
            additional_cpu_slots: 2,
        };
        assert!(matches!(
            ResourceScheduler::new(&contract, vec![missing_process], amount(3, 1, 1, 1)),
            Err(SchedulerContractError::ReservationBelowWorkUnitMinimum { .. })
        ));

        let mut hidden_nested_cpu = instance(
            "cc",
            LogicalActivity::N7CCompilation,
            &[],
            amount(1, 1, 1, 1),
        );
        hidden_nested_cpu.nested_parallelism = NestedParallelism::Accounted {
            additional_cpu_slots: 2,
        };
        assert!(matches!(
            ResourceScheduler::new(&contract, vec![hidden_nested_cpu], amount(3, 1, 1, 1)),
            Err(SchedulerContractError::NestedParallelismExceedsCpuReservation { .. })
        ));

        let no_memory = instance(
            "analysis",
            LogicalActivity::A6SemanticAnalysis,
            &[],
            amount(1, 0, 0, 0),
        );
        assert!(matches!(
            ResourceScheduler::new(&contract, vec![no_memory], amount(3, 1, 1, 1)),
            Err(SchedulerContractError::ZeroMemoryReservation { .. })
        ));

        let no_io = instance(
            "cc",
            LogicalActivity::N7CCompilation,
            &[],
            amount(1, 1, 0, 1),
        );
        assert!(matches!(
            ResourceScheduler::new(&contract, vec![no_io], amount(3, 1, 1, 1)),
            Err(SchedulerContractError::ExternalProcessWithoutIoReservation { .. })
        ));
    }

    #[test]
    fn canonical_mixed_graph_is_budget_invariant_and_exposes_parallel_timeline() {
        let contract = canonical_physical_work_contract();
        let instances = canonical_mixed_instances(&contract);
        let run = |budget| {
            execute_physical_work_graph(&contract, instances.clone(), budget, |instance, inputs| {
                deterministic_computation(&contract, instance, inputs)
            })
            .unwrap()
        };

        let serial = run(amount(1, 64, 1, 1));
        let parallel = run(amount(4, 64, 4, 4));
        let expected_ids: BTreeSet<_> = instances
            .iter()
            .map(|instance| instance.instance_id.clone())
            .collect();

        assert_eq!(serial.completed, expected_ids);
        assert_eq!(parallel.completed, expected_ids);
        assert_eq!(serial.reusable, expected_ids);
        assert_eq!(parallel.reusable, expected_ids);
        assert!(serial.invalid.is_empty());
        assert!(parallel.rerun.is_empty());
        assert_eq!(serial.states, parallel.states);
        assert_eq!(serial.committed_outputs, parallel.committed_outputs);
        assert_eq!(peak_active_work(&serial.transitions), 1);
        assert!(peak_active_work(&parallel.transitions) > 1);

        for group in [
            ExecutionGroup::CObjectCompilation,
            ExecutionGroup::CppAdapterCompilation,
            ExecutionGroup::ForeignArchive,
        ] {
            assert!(contract.units.iter().any(|unit| {
                unit.execution_group == group
                    && parallel.completed.contains(&canonical_instance_id(unit.id))
            }));
        }
        for activity in LogicalActivity::ALL {
            assert!(parallel
                .completed
                .contains(&canonical_instance_id(WorkUnitId::Logical(activity))));
        }
    }

    #[test]
    fn mixed_graph_fault_preserves_independent_commits_and_classifies_recovery() {
        let contract = canonical_physical_work_contract();
        let instances = canonical_mixed_instances(&contract);
        let failed = canonical_instance_id(WorkUnitId::Logical(LogicalActivity::A10CodeGeneration));
        let expected_never_started = descendants_of(&failed, &instances);
        let mut expected_rerun = expected_never_started.clone();
        expected_rerun.insert(failed.clone());
        let all_ids: BTreeSet<_> = instances
            .iter()
            .map(|instance| instance.instance_id.clone())
            .collect();
        let expected_reusable: BTreeSet<_> = all_ids.difference(&expected_rerun).cloned().collect();

        let report = execute_physical_work_graph(
            &contract,
            instances,
            amount(4, 64, 4, 4),
            |instance, inputs| {
                if instance.instance_id == failed {
                    Err("injected code generation fault".to_string())
                } else {
                    deterministic_computation(&contract, instance, inputs)
                }
            },
        )
        .unwrap();

        assert_eq!(report.invalid, BTreeSet::from([failed.clone()]));
        assert_eq!(report.rerun, expected_rerun);
        assert_eq!(report.never_started, expected_never_started);
        assert_eq!(report.completed, expected_reusable);
        assert_eq!(report.reusable, expected_reusable);
        assert!(report.executed.contains(&failed));
        assert!(report.never_started.is_disjoint(&report.executed));
        assert!(report
            .committed_outputs
            .values()
            .all(|output| expected_reusable.contains(&output.writer_instance_id)));

        for activity in [
            LogicalActivity::N8NimLink,
            LogicalActivity::N9ExportRegistration,
        ] {
            assert!(report
                .reusable
                .contains(&canonical_instance_id(WorkUnitId::Logical(activity))));
        }
        assert!(report
            .reusable
            .contains(&canonical_instance_id(WorkUnitId::ArchiveForeignObjects)));
    }
}

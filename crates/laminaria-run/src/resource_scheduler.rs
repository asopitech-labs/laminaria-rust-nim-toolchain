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

use laminaria_plan::physical_work::{PhysicalWorkContract, WorkUnitId};
use serde::{Deserialize, Serialize};

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
    pub dependencies: BTreeSet<String>,
    pub reservation: ResourceAmount,
    pub nested_parallelism: NestedParallelism,
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
    UnknownInstance {
        instance_id: String,
    },
    InvalidTransition {
        instance_id: String,
        from: LifecycleState,
        operation: &'static str,
    },
}

#[derive(Debug, Clone)]
struct RuntimeWork {
    definition: PhysicalWorkInstance,
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
    work: BTreeMap<String, RuntimeWork>,
    dependents: BTreeMap<String, BTreeSet<String>>,
    transitions: Vec<LifecycleTransition>,
    next_sequence: u64,
}

impl ResourceScheduler {
    pub fn new(
        contract: &PhysicalWorkContract,
        instances: Vec<PhysicalWorkInstance>,
        budget: ResourceAmount,
    ) -> Result<Self, SchedulerContractError> {
        let unit_claims: BTreeMap<_, _> = contract
            .units
            .iter()
            .map(|unit| (unit.id, unit.resources))
            .collect();
        let mut work = BTreeMap::new();
        for instance in instances {
            if work.contains_key(&instance.instance_id) {
                return Err(SchedulerContractError::DuplicateInstance {
                    instance_id: instance.instance_id,
                });
            }
            let Some(claim) = unit_claims.get(&instance.work_unit) else {
                return Err(SchedulerContractError::UnknownWorkUnit {
                    instance_id: instance.instance_id,
                    work_unit: instance.work_unit,
                });
            };
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
        if has_dependency_cycle(&work) {
            return Err(SchedulerContractError::DependencyCycle);
        }

        let mut scheduler = Self {
            budget,
            allocated: ResourceAmount::default(),
            work,
            dependents,
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

    /// Marks computation done but deliberately keeps the full reservation.
    /// Publication visibility is Checkpoint C's concern; until `commit`, the
    /// memory and other resources backing staged output remain accounted.
    pub fn finish_computation(&mut self, instance_id: &str) -> Result<(), SchedulerContractError> {
        self.require_state(instance_id, LifecycleState::Running, "finish_computation")?;
        self.set_state(
            instance_id,
            LifecycleState::Committing,
            None,
            TransitionReason::ComputationFinished,
        );
        Ok(())
    }

    pub fn commit(&mut self, instance_id: &str) -> Result<(), SchedulerContractError> {
        self.require_state(instance_id, LifecycleState::Committing, "commit")?;
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
        canonical_physical_work_contract, LogicalActivity, WorkUnitId,
    };

    fn amount(cpu: u32, memory: u64, io: u32, processes: u32) -> ResourceAmount {
        ResourceAmount {
            cpu_slots: cpu,
            memory_bytes: memory,
            io_slots: io,
            external_process_slots: processes,
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
            dependencies: dependencies.iter().map(|id| (*id).to_string()).collect(),
            reservation,
            nested_parallelism: NestedParallelism::Disabled,
        }
    }

    fn scheduler(
        instances: Vec<PhysicalWorkInstance>,
        budget: ResourceAmount,
    ) -> ResourceScheduler {
        ResourceScheduler::new(&canonical_physical_work_contract(), instances, budget).unwrap()
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
        scheduler.finish_computation("rust-front").unwrap();
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
        scheduler.finish_computation("a").unwrap();
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
        scheduler.finish_computation("ir-a").unwrap();
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
    fn external_compiler_must_reserve_its_process_and_total_nested_cpu() {
        let contract = canonical_physical_work_contract();
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
}

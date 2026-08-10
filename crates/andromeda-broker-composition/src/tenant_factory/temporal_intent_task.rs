//! Composition of temporal intent policy with drain runtime mechanics.

use crate::tenant_factory::error::TenantAssemblyError;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::TaskFailure,
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackCadence, WakeHandle},
};
use andromeda_temporal::{
    config::TemporalConfig,
    error::TemporalError,
    intent_drain::TemporalIntentDrain,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_history_sink::TemporalHistorySink,
        temporal_intent_publisher::TemporalIntentPublisher,
        temporal_intent_queue::{TemporalIntentBatchSize, TemporalIntentLeaseOwner, TemporalIntentQueue},
    },
};
use chrono::{DateTime, Utc};
use std::{num::NonZeroU32, sync::Arc, time::Duration};

/// Runtime wake capability exposed through the temporal-owned commit notification port.
struct RuntimeTemporalIntentPublisher {
    /// Drain-specific coalescing wake handle.
    wake: WakeHandle,
}

impl TemporalIntentPublisher for RuntimeTemporalIntentPublisher {
    fn intents_available(&self) {
        self.wake.wake();
    }
}

/// Policy and wake capabilities produced by temporal drain registration.
pub(super) struct TemporalIntentRegistration {
    /// Commit-side wake notification.
    pub(super) publisher: Arc<dyn TemporalIntentPublisher>,
    /// Request-side synchronization before immediate mutations.
    pub(super) mutation_barrier: Arc<dyn TemporalMutationBarrier>,
}

/// Register temporal-intent delivery as the first managed drain operation.
pub(super) fn register_temporal_intent_drain(
    config: &TemporalConfig,
    supervisor: &TaskSupervisor,
    queue: Arc<dyn TemporalIntentQueue>,
    pending: Arc<dyn PendingTemporalIntentReader>,
    sink: Arc<dyn TemporalHistorySink>,
) -> Result<TemporalIntentRegistration, TenantAssemblyError> {
    let fallback_duration = Duration::from_secs(config.intent_fallback_secs);
    let fallback = DrainFallbackCadence::new(fallback_duration).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let batch_size = NonZeroU32::new(config.intent_batch_size)
        .map(TemporalIntentBatchSize::new)
        .ok_or_else(|| TenantAssemblyError::invariant("temporal intent batch size must be non-zero"))?;
    let drain = Arc::new(TemporalIntentDrain::new(
        queue,
        pending,
        sink,
        TemporalIntentLeaseOwner::mint(),
        batch_size,
        Duration::from_secs(config.intent_lease_secs),
        Duration::from_secs(config.intent_retry_secs),
    ));
    let scheduled_drain = Arc::clone(&drain);
    let name = TaskName::try_from("temporal-intent-delivery").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback_duration, fallback_duration),
        ShutdownPolicy::new(Duration::from_secs(config.intent_lease_secs)),
    );
    let task = DrainTask::new(registration, fallback, move |execution| {
        let drain = Arc::clone(&scheduled_drain); // clone: each bounded runtime future owns the shared policy operation.
        async move {
            let now = DateTime::<Utc>::from(execution.cutoff());
            let outcome = drain.execute(now).await.map_err(classify_temporal_drain_failure)?;
            if outcome.work_may_remain {
                Ok(DrainPassOutcome::WorkMayRemain)
            } else {
                Ok(DrainPassOutcome::Idle)
            }
        }
    });
    let wake = supervisor
        .register_drain(task)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    Ok(TemporalIntentRegistration {
        publisher: Arc::new(RuntimeTemporalIntentPublisher { wake }),
        mutation_barrier: drain,
    })
}

/// Exhaustively classify temporal policy failures at composition.
fn classify_temporal_drain_failure(error: TemporalError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        TemporalError::Commit(_) | TemporalError::Persistence { detail: _ } => TaskFailure::transient(summary),
        TemporalError::Internal { detail: _ } => TaskFailure::invariant_violation(summary),
        TemporalError::Validation { detail: _ }
        | TemporalError::NotFound { detail: _ }
        | TemporalError::OperationNotSupported { detail: _ }
        | TemporalError::JsonLd(_)
        | TemporalError::Query(_)
        | TemporalError::Request(_)
        | TemporalError::Rendering { detail: _ } => TaskFailure::permanent(summary),
    }
}

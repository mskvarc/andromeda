//! Composition of temporal intent policy with drain runtime mechanics.

use crate::tenant_factory::{
    assembly_invariant::{AssemblyInvariant, CountSetting},
    background_pacer::BackgroundPacer,
    error::TenantAssemblyError,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::{FailureClass, TaskFailure},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackPolicy, WakeHandle, WakeRequest, wake_channel},
};
use andromeda_temporal::{
    config::TemporalConfig,
    error::TemporalError,
    intent_drain::{TemporalIntentClaim, TemporalIntentDrain},
    intent_retry_policy::TemporalIntentRetryPolicy,
    port::{
        mutation_barrier::TemporalMutationBarrier,
        temporal_intent_application::TemporalIntentApplication,
        temporal_intent_publisher::TemporalIntentPublisher,
        temporal_intent_queue::{TemporalIntentBatchSize, TemporalIntentQueue},
    },
};
use chrono::{DateTime, Utc};
use std::{num::NonZeroU32, sync::Arc};

/// Runtime wake capability exposed through the temporal-owned commit notification port.
struct RuntimeTemporalIntentPublisher {
    /// Drain-specific coalescing wake handle.
    wake: WakeHandle,
}

impl TemporalIntentPublisher for RuntimeTemporalIntentPublisher {
    fn intents_available(&self) {
        match self.wake.wake() {
            // A wake already queued covers this commit too: the drain reads
            // every pending intent in one pass, so coalescing loses nothing.
            WakeRequest::Scheduled | WakeRequest::Coalesced => (),
            // The drain task is gone, so these intents wait for whatever
            // restarts it rather than being drained promptly.
            WakeRequest::Closed => {
                tracing::warn!(event = "temporal_intent_wake_closed", "the temporal drain no longer receives wakes");
            }
        }
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
    application: Arc<dyn TemporalIntentApplication>,
    pacer: Arc<BackgroundPacer>,
) -> Result<TemporalIntentRegistration, TenantAssemblyError> {
    let fallback_floor = config.intent_fallback.duration();
    let fallback_ceiling = config.intent_fallback_max.duration().max(config.intent_fallback.duration());
    let fallback = DrainFallbackPolicy::new(fallback_floor, fallback_ceiling).map_err(AssemblyInvariant::from)?;
    let batch_size = NonZeroU32::new(config.intent_batch_size)
        .map(TemporalIntentBatchSize::new)
        .ok_or(AssemblyInvariant::ZeroCount(CountSetting::TemporalIntentBatchSize))?;
    let stuck_after =
        NonZeroU32::new(config.intent_stuck_after_attempts).ok_or(AssemblyInvariant::ZeroCount(CountSetting::TemporalIntentStuckAfterAttempts))?;
    let retry_policy =
        TemporalIntentRetryPolicy::new(config.intent_retry.duration(), config.intent_retry_max.duration(), stuck_after).map_err(AssemblyInvariant::from)?;
    let drain = Arc::new(TemporalIntentDrain::new(
        queue,
        application,
        TemporalIntentClaim {
            owner: LeaseHolder::mint(),
            batch_size,
            lease_duration: config.intent_lease.duration(),
            retry_policy,
        },
    ));
    let scheduled_drain = Arc::clone(&drain);
    let name = TaskName::try_from("temporal-intent-delivery").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback_floor, fallback_ceiling),
        ShutdownPolicy::new(config.intent_lease.duration()),
    );
    let task = DrainTask::new(registration, fallback, move |execution| {
        let drain = Arc::clone(&scheduled_drain); // clone: each bounded runtime future owns the shared policy operation.
        let pacer = Arc::clone(&pacer); // clone: each pass consults the tenant's one pacer.
        async move {
            pacer
                .run_drain_pass(execution.cause(), async move {
                    let now = DateTime::<Utc>::from(execution.cutoff());
                    let outcome = drain.execute(now).await.map_err(classify_temporal_drain_failure)?;
                    if outcome.work_may_remain {
                        Ok(DrainPassOutcome::WorkMayRemain)
                    } else {
                        Ok(DrainPassOutcome::Idle)
                    }
                })
                .await
        }
    });
    let (wake, receiver) = wake_channel();
    supervisor.register_drain(task, receiver).map_err(AssemblyInvariant::from)?;
    Ok(TemporalIntentRegistration {
        publisher: Arc::new(RuntimeTemporalIntentPublisher { wake }),
        mutation_barrier: drain,
    })
}

/// Exhaustively classify temporal policy failures at composition.
const fn classify_temporal_drain_failure(error: TemporalError) -> TaskFailure<TemporalError> {
    let class = match &error {
        TemporalError::Commit(_)
        | TemporalError::Persistence(_)
        | TemporalError::StoreSaturated
        | TemporalError::StoreUnavailable
        | TemporalError::TransactionContended => FailureClass::Transient,
        TemporalError::Internal(_) => FailureClass::InvariantViolation,
        TemporalError::Validation(_)
        | TemporalError::NotFound(_)
        | TemporalError::OperationNotSupported(_)
        | TemporalError::TooManyResults { .. }
        | TemporalError::PayloadBudget(_)
        | TemporalError::AttributeNesting(_)
        | TemporalError::JsonLd(_)
        | TemporalError::Query(_)
        | TemporalError::Request(_) => FailureClass::Permanent,
    };
    TaskFailure::new(class, error)
}

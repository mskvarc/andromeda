//! Composition of temporal intent policy with drain runtime mechanics.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, background_pacer::BackgroundPacer, error::TenantAssemblyError};
use andromeda_configuration::schema::temporal::history::TemporalHistoryConfig;
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
    configured_policy::intent_retry_schedule,
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
use std::{sync::Arc, time::Duration};

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

/// How long the mutation barrier may spend applying an entity's queued history
/// and waiting for another owner's lease on it before the mutation is answered
/// 503.
///
/// Half the request deadline leaves the mutation itself room to run before
/// the request times out, and a wait past one lease is pointless: the holder's
/// lease has lapsed by then, and the barrier claims the history itself.
fn barrier_wait(intent_lease: Duration, request_timeout: Duration) -> Duration {
    intent_lease.min(request_timeout / 2)
}

/// Register temporal-intent delivery as the first managed drain operation,
/// its mutation barrier bounded by the HTTP `request_timeout`.
pub(super) fn register_temporal_intent_drain(
    config: &TemporalHistoryConfig,
    request_timeout: Duration,
    supervisor: &TaskSupervisor,
    queue: Arc<dyn TemporalIntentQueue>,
    application: Arc<dyn TemporalIntentApplication>,
    pacer: Arc<BackgroundPacer>,
) -> Result<TemporalIntentRegistration, TenantAssemblyError> {
    let queue_config = &config.queue;
    let fallback =
        DrainFallbackPolicy::new(queue_config.poll_interval.duration(), queue_config.max_poll_interval.duration()).map_err(AssemblyInvariant::from)?;
    let batch_size = TemporalIntentBatchSize::new(config.batch_size);
    let retry_policy = TemporalIntentRetryPolicy::new(
        intent_retry_schedule(config).map_err(AssemblyInvariant::IntentRetry)?,
        config.stuck_after_attempts,
    );
    let drain = Arc::new(TemporalIntentDrain::new(
        queue,
        application,
        TemporalIntentClaim {
            owner: LeaseHolder::mint(),
            batch_size,
            lease_duration: queue_config.lease_timeout.duration(),
            retry_policy,
        },
        barrier_wait(queue_config.lease_timeout.duration(), request_timeout),
    ));
    let scheduled_drain = Arc::clone(&drain);
    let name = TaskName::try_from("temporal-intent-delivery").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback.schedule()),
        ShutdownPolicy::new(queue_config.lease_timeout.duration()),
    );
    let task = DrainTask::new(registration, fallback, move |execution| {
        let drain = Arc::clone(&scheduled_drain);
        let pacer = Arc::clone(&pacer);
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
        | TemporalError::TransactionContended
        | TemporalError::HistoryUnsettled(_) => FailureClass::Transient,
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

#[cfg(test)]
mod tests {
    use crate::tenant_factory::temporal_intent_task::barrier_wait;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn the_default_barrier_waits_half_the_request_deadline() {
        assert_eq!(barrier_wait(Duration::from_secs(30), Duration::from_secs(30)), Duration::from_secs(15));
    }

    #[test]
    fn a_lease_shorter_than_half_the_request_deadline_bounds_the_barrier() {
        assert_eq!(barrier_wait(Duration::from_secs(5), Duration::from_secs(30)), Duration::from_secs(5));
    }
}

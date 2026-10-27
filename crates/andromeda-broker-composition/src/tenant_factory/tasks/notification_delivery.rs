//! The drain delivering the notifications queued in a tenant's database, and
//! the cleanup reaping what it finished.
//!
//! Both run under either delivery mode. The delivery mode decides only whether
//! a commit queues its notifications or has them sent at once; the queue
//! itself lives in the tenant database, where a durable broker sharing that
//! database, or this broker before it was switched to live delivery, may have
//! left deliveries that are still due. The pending-work probe reports them due
//! to whichever broker wakes the tenant, so a runtime woken for them has to be
//! able to deliver them.

use crate::tenant_factory::{
    assembly_invariant::{AssemblyInvariant, RetentionSetting},
    background_pacer::BackgroundPacer,
    error::TenantAssemblyError,
    storage::TenantStorage,
    tasks::{notification_schedule::classify_notification_failure, retention::retention_duration},
};
use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
use andromeda_notification::{
    delivery_drain::{NotificationDeliveryDrain, NotificationDeliveryTransport},
    delivery_store::DeliveryStore,
    error::NotificationError,
};
use andromeda_postgresql_context_store::notification_delivery::queue::NotificationDeliveryQueue;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::TaskFailure,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver},
};
use chrono::{DateTime, Utc};
use std::{
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

/// What drives the notification-delivery drain: the wake a durable commit
/// raises when it queues deliveries, and the pacer that rests the drain's
/// fallback timer while the tenant is quiet and nothing is due.
pub(super) struct DeliveryDrainDrive {
    /// The drain's wake receiver; under live delivery no commit wakes it, and
    /// the fallback timer alone finds what the tenant database holds.
    pub(super) wake: DrainWakeReceiver,
    /// The tenant's background pacer.
    pub(super) pacer: Arc<BackgroundPacer>,
}

/// The queue the tenant's database holds, reading the names its queued
/// states hold through the tenant's dictionaries.
pub(super) fn delivery_queue(storage: &TenantStorage) -> Arc<dyn DeliveryStore> {
    Arc::new(NotificationDeliveryQueue::new(
        &storage.pool,
        Arc::clone(&storage.attribute_names),   // clone: the queue shares the tenant's one dictionary cache.
        Arc::clone(&storage.entity_type_names), // clone: the queue shares the tenant's one dictionary cache.
    ))
}

/// Register the drain delivering the notifications queued in the tenant's
/// database `store` through `transport`, and the cleanup reaping what it
/// finished.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the delivery configuration is rejected
/// or when the supervisor refuses a registration.
pub(super) fn register_notification_delivery(
    supervisor: &TaskSupervisor,
    store: Arc<dyn DeliveryStore>,
    config: &DeliveryConfig,
    transport: Arc<dyn NotificationDeliveryTransport>,
    drive: DeliveryDrainDrive,
) -> Result<(), TenantAssemblyError> {
    let drain = Arc::new(NotificationDeliveryDrain::new(store, transport, config).map_err(|error| AssemblyInvariant::NotificationDrain(Box::new(error)))?);
    let DeliveryDrainDrive { wake, pacer } = drive;
    // clone: delivery and cleanup share one stateless delivery operation and the tenant's one pacer.
    register_delivery_drain(supervisor, Arc::clone(&drain), config, wake, Arc::clone(&pacer))?;
    register_delivery_cleanup(supervisor, drain, config, pacer)
}

/// Register the drain itself: woken by a durable commit, and otherwise polled
/// on a fallback timer the tenant's pacer rests while nothing is due.
fn register_delivery_drain(
    supervisor: &TaskSupervisor,
    drain: Arc<NotificationDeliveryDrain>,
    config: &DeliveryConfig,
    wake: DrainWakeReceiver,
    pacer: Arc<BackgroundPacer>,
) -> Result<(), TenantAssemblyError> {
    let fallback_floor = config.queue.poll_interval.duration().max(Duration::from_secs(1));
    let fallback_ceiling = config
        .queue
        .max_poll_interval
        .duration()
        .max(config.queue.poll_interval.duration())
        .max(Duration::from_secs(1));
    let fallback = DrainFallbackPolicy::new(fallback_floor, fallback_ceiling).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("notification-delivery").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback.schedule()),
        ShutdownPolicy::new(fallback_floor),
    );
    let task = DrainTask::new(registration, fallback, move |execution| {
        let drain = Arc::clone(&drain); // clone: each delivery pass owns the policy operation while executing.
        let pacer = Arc::clone(&pacer); // clone: each pass consults the tenant's one pacer.
        async move {
            pacer
                .run_drain_pass(execution.cause(), async move {
                    let now = DateTime::<Utc>::from(execution.cutoff());
                    let outcome = drain.execute(now).await.map_err(classify_notification_failure)?;
                    if outcome.work_may_remain {
                        Ok(DrainPassOutcome::WorkMayRemain)
                    } else {
                        Ok(DrainPassOutcome::Idle)
                    }
                })
                .await
        }
    });
    supervisor.register_drain(task, wake).map_err(|error| AssemblyInvariant::from(error).into())
}

/// How old each kind of finished queue row may grow before cleanup reaps it.
#[derive(Clone, Copy, Debug)]
struct CleanupRetention {
    /// Age after which an acknowledged delivery is reaped.
    delivered: chrono::Duration,
    /// Age after which a dead-lettered delivery is reaped.
    dead_lettered: chrono::Duration,
    /// Age after which an event no delivery references is reaped.
    orphaned: chrono::Duration,
}

impl CleanupRetention {
    /// The retention ages `config` sets.
    fn configured(config: &DeliveryConfig) -> Result<CleanupRetention, TenantAssemblyError> {
        Ok(CleanupRetention {
            delivered: retention_duration(config.retention.acknowledged_after.duration().as_secs(), RetentionSetting::NotificationCleanup)?,
            dead_lettered: retention_duration(
                config.retention.dead_letter_after.duration().as_secs(),
                RetentionSetting::NotificationDeadLetter,
            )?,
            orphaned: retention_duration(
                config.retention.orphan_event_after.duration().as_secs(),
                RetentionSetting::NotificationOrphanEvent,
            )?,
        })
    }
}

/// Register the cleanup reaping acknowledged deliveries, dead letters and
/// orphaned events on its own cadence, paced by the tenant's pacer.
fn register_delivery_cleanup(
    supervisor: &TaskSupervisor,
    drain: Arc<NotificationDeliveryDrain>,
    config: &DeliveryConfig,
    pacer: Arc<BackgroundPacer>,
) -> Result<(), TenantAssemblyError> {
    let cadence = config.retention.cleanup_interval.duration().max(Duration::from_secs(1));
    let retention = CleanupRetention::configured(config)?;
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("notification-delivery-cleanup").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(cadence)),
        ShutdownPolicy::new(cadence),
    );
    // The change count the last cleanup ran under, shared by every tick.
    let observed = Arc::new(AtomicU64::new(0));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let drain = Arc::clone(&drain); // clone: each cleanup execution owns the policy operation while executing.
        let pacer = Arc::clone(&pacer); // clone: each tick consults the tenant's one pacer.
        let observed = Arc::clone(&observed); // clone: every tick compares against the one last-cleanup count.
        async move { paced_cleanup(&pacer, &observed, &drain, retention, DateTime::<Utc>::from(execution.cutoff())).await }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

/// Run one cleanup tick if the tenant may have written since the last cleanup
/// ran, and touch nothing otherwise.
///
/// Every row the cleanup reaps was acknowledged by a drain pass or created by
/// a request, and the pacer counts both as changes; a tenant nobody uses and no
/// pass touches has nothing new to reap, so its cleanup costs no connection.
/// What ages past retention while the tenant rests is reaped by the first
/// cleanup after it next changes. The cleanup is not itself a change: it only
/// removes finished rows, which no obligation read depends on.
async fn paced_cleanup(
    pacer: &BackgroundPacer,
    observed: &AtomicU64,
    drain: &NotificationDeliveryDrain,
    retention: CleanupRetention,
    now: DateTime<Utc>,
) -> Result<(), TaskFailure<NotificationError>> {
    pacer
        .run_when_written(observed, async {
            let purged = drain.purge_delivered(now - retention.delivered).await.map_err(classify_notification_failure)?;
            // Dead letters are reaped before the orphan pass so an event a
            // dead-lettered delivery just released can be swept in the same
            // tick once it also clears the orphan-event horizon.
            let dead_lettered = drain
                .purge_exhausted(now - retention.dead_lettered)
                .await
                .map_err(classify_notification_failure)?;
            let orphaned = drain
                .purge_orphan_events(now - retention.orphaned)
                .await
                .map_err(classify_notification_failure)?;
            tracing::debug!(purged, dead_lettered, orphaned, "notification delivery cleanup completed");
            Ok(())
        })
        .await
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::tenant_factory::{
        background_pacer::{BackgroundPacer, ObligationSource},
        tasks::notification_delivery::{CleanupRetention, paced_cleanup},
    };
    use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_notification::{
        delivery_claim::DeliveryClaim,
        delivery_drain::{DeliveryDisposition, NotificationDeliveryDrain, NotificationDeliveryTransport},
        delivery_entry::{DeliveryEntry, DeliveryKey},
        delivery_lease::{DeliveryLeaseError, LeaseTerm},
        delivery_store::DeliveryStore,
        error::NotificationError,
        held_events::HeldEvents,
    };
    use andromeda_postgresql_context_store::error::ContextStoreError;
    use andromeda_tenancy::state_factory::RuntimeActivity;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use futures::future::BoxFuture;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        time::{Duration, SystemTime},
    };

    /// A tenant with nothing outstanding, counting every obligation read.
    #[derive(Default)]
    struct NothingDue {
        reads: AtomicUsize,
    }

    #[async_trait]
    impl ObligationSource for NothingDue {
        async fn earliest_due(&self) -> Result<Option<SystemTime>, ContextStoreError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    /// A queue that holds nothing and counts every statement cleanup issues.
    #[derive(Default)]
    struct CountingQueue {
        statements: AtomicUsize,
    }

    impl CountingQueue {
        fn reaped(&self) -> u64 {
            self.statements.fetch_add(1, Ordering::Relaxed);
            0
        }
    }

    #[async_trait]
    impl DeliveryStore for CountingQueue {
        async fn claim_pending(
            &self,
            _worker_id: &LeaseHolder,
            _lease: LeaseTerm,
            _batch_size: i32,
            _held: &HeldEvents,
        ) -> Result<Vec<DeliveryClaim>, NotificationError> {
            Ok(Vec::new())
        }

        async fn mark_delivered(&self, _key: DeliveryKey, _worker_id: &LeaseHolder, _target: &ContextTarget) -> Result<(), DeliveryLeaseError> {
            Ok(())
        }

        async fn mark_failed(
            &self,
            _key: DeliveryKey,
            _worker_id: &LeaseHolder,
            _error: &str,
            _next_attempt_at: DateTime<Utc>,
            _target: &ContextTarget,
        ) -> Result<(), DeliveryLeaseError> {
            Ok(())
        }

        async fn mark_terminal(&self, _key: DeliveryKey, _worker_id: &LeaseHolder, _error: &str, _target: &ContextTarget) -> Result<(), DeliveryLeaseError> {
            Ok(())
        }

        async fn delete_delivered_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
            Ok(self.reaped())
        }

        async fn delete_exhausted_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
            Ok(self.reaped())
        }

        async fn delete_orphan_events_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
            Ok(self.reaped())
        }
    }

    /// A transport cleanup never uses.
    struct UnusedTransport;

    impl NotificationDeliveryTransport for UnusedTransport {
        fn deliver<'a>(&'a self, _entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            Box::pin(async { DeliveryDisposition::Obsolete })
        }
    }

    fn retention() -> CleanupRetention {
        CleanupRetention::configured(&DeliveryConfig::default()).expect("the default retention ages are representable")
    }

    /// The cleanup runs for every cached tenant under either delivery mode, so
    /// a tenant nobody uses must not pay a connection for it on every tick:
    /// it reaps once, then rests without reading anything until the tenant
    /// changes again.
    #[tokio::test]
    async fn a_quiet_tenant_skips_its_delivery_cleanup_until_it_changes() {
        let queue = Arc::new(CountingQueue::default());
        let drain = NotificationDeliveryDrain::new(
            Arc::clone(&queue) as Arc<dyn DeliveryStore>,
            Arc::new(UnusedTransport),
            &DeliveryConfig::default(),
        )
        .expect("the default delivery configuration composes a drain");
        let obligations = Arc::new(NothingDue::default());
        let pacer = BackgroundPacer::new(Duration::from_secs(30), Arc::clone(&obligations) as Arc<dyn ObligationSource>);
        let observed = AtomicU64::new(0);

        paced_cleanup(&pacer, &observed, &drain, retention(), Utc::now()).await.expect("first cleanup");
        pretty_assertions::assert_eq!(queue.statements.load(Ordering::Relaxed), 3, "the first tick reaps");

        paced_cleanup(&pacer, &observed, &drain, retention(), Utc::now())
            .await
            .expect("resting cleanup");
        pretty_assertions::assert_eq!(queue.statements.load(Ordering::Relaxed), 3, "nothing changed, so nothing is reaped");

        pacer.record_activity();
        paced_cleanup(&pacer, &observed, &drain, retention(), Utc::now())
            .await
            .expect("cleanup after a use");
        pretty_assertions::assert_eq!(queue.statements.load(Ordering::Relaxed), 6, "a use may have left rows to reap");
        pretty_assertions::assert_eq!(obligations.reads.load(Ordering::Relaxed), 0, "the cleanup reads no obligation");
    }
}

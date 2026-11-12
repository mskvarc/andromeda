//! The clause 5.13.3.5 hit accounting of a tenant's stored `@context`s, flushed
//! by a supervised scheduled task.
//!
//! A flush the store refuses leaves every hit in the counter and is retried
//! after one flush interval, its failure recorded in the task's health.

use crate::{
    context_hit_flush::ContextHitFlush,
    tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError},
};
use andromeda_context_registry::error::ContextRegistryError;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    failure::{FailureClass, TaskFailure},
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
};
use std::sync::Arc;

/// Register the task that moves `flush`'s hits into the store every flush
/// interval, the first one interval after the tenant loads; with no `flush`,
/// the engine counts no hits and nothing is registered.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the interval names no schedule, or the
/// supervisor refuses the registration.
pub(super) fn register_context_hit_flush(supervisor: &TaskSupervisor, flush: Option<&Arc<ContextHitFlush>>) -> Result<(), TenantAssemblyError> {
    let Some(flush) = flush.map(Arc::clone) else {
        return Ok(());
    };
    let interval = flush.interval();
    let schedule = PeriodicSchedule::new(interval, InitialExecution::AfterCadence, MissedTickPolicy::Delay).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("context-hit-flush").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(interval)),
        ShutdownPolicy::new(interval),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let flush = Arc::clone(&flush);
        async move { flush.flush().await.map_err(classify_flush_failure) }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

/// A store that is busy or unreachable may take the hits on a later attempt; a
/// broken registry invariant does not mend itself, and no other refusal comes
/// from a flush.
const fn classify_flush_failure(error: ContextRegistryError) -> TaskFailure<ContextRegistryError> {
    let class = match &error {
        ContextRegistryError::Persistence(_) | ContextRegistryError::StoreSaturated | ContextRegistryError::StoreUnavailable => FailureClass::Transient,
        ContextRegistryError::Invariant(_) => FailureClass::InvariantViolation,
        ContextRegistryError::NoLocalContext(_)
        | ContextRegistryError::NotFound { .. }
        | ContextRegistryError::CachedContextNotServed { .. }
        | ContextRegistryError::ReloadRequiresCached { .. }
        | ContextRegistryError::MissingSourceUrl { .. }
        | ContextRegistryError::ContextUnavailable { .. } => FailureClass::Permanent,
    };
    TaskFailure::new(class, error)
}

#[cfg(test)]
mod tests {
    use crate::{context_hit_flush::ContextHitFlush, tenant_factory::tasks::context_hit_flush::register_context_hit_flush};
    use andromeda_context_registry::{
        error::ContextRegistryError,
        hit_store::{ContextHitStore, ContextHitStoreFuture},
    };
    use andromeda_jsonld::hit_counter::{HitCounter, HitDelta};
    use andromeda_task_runtime::{failure::FailureClass, supervisor::TaskSupervisor, task_name::TaskName};
    use iri_rs::IriBuf;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU32, AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::{
        runtime::Handle,
        time::{sleep, timeout},
    };

    /// A store that refuses its first writes as unreachable and accepts every
    /// later one.
    struct RecoveringStore {
        /// How many writes are still refused.
        refusals: AtomicU32,
        /// How many writes reached the store.
        attempts: AtomicU32,
        /// How many deltas the accepted writes carried.
        landed: AtomicUsize,
    }

    impl ContextHitStore for RecoveringStore {
        fn flush_hits<'a>(&'a self, deltas: &'a [HitDelta]) -> ContextHitStoreFuture<'a> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            let refused = self.refusals.try_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1)).is_ok();
            if !refused {
                self.landed.fetch_add(deltas.len(), Ordering::SeqCst);
            }
            Box::pin(async move { if refused { Err(ContextRegistryError::StoreUnavailable) } else { Ok(()) } })
        }
    }

    #[tokio::test]
    async fn a_flush_the_store_refuses_is_restarted_until_the_hits_land() {
        let supervisor = TaskSupervisor::process(Handle::current());
        let counter = Arc::new(HitCounter::new());
        counter.bump(&IriBuf::new("https://example.com/context.jsonld".to_owned()).expect("valid URL"));
        let store = Arc::new(RecoveringStore {
            refusals: AtomicU32::new(2),
            attempts: AtomicU32::new(0),
            landed: AtomicUsize::new(0),
        });
        let flush = ContextHitFlush::new(Arc::clone(&counter), Arc::clone(&store) as Arc<dyn ContextHitStore>, Duration::from_millis(10));

        register_context_hit_flush(&supervisor, Some(&Arc::new(flush))).expect("the flush registers");
        let name = TaskName::try_from("context-hit-flush").expect("a valid task name");
        let health = timeout(Duration::from_secs(5), async {
            loop {
                match supervisor.health(&name).await {
                    Some(health) if health.last_success_at.is_some() => return health,
                    Some(_) | None => sleep(Duration::from_millis(5)).await,
                }
            }
        })
        .await
        .expect("a flush succeeds once the store accepts the hits");

        pretty_assertions::assert_eq!(store.attempts.load(Ordering::SeqCst), 3);
        pretty_assertions::assert_eq!(store.landed.load(Ordering::SeqCst), 1);
        assert!(!health.state.is_terminal(), "{:?}", health.state);
        pretty_assertions::assert_eq!(health.last_failure_class, Some(FailureClass::Transient));
        pretty_assertions::assert_eq!(health.consecutive_failures, 0);
        supervisor.shutdown().await;
    }
}

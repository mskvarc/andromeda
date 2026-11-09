//! The pass that places again the subscription copies whose placement was cut
//! off before the context source answered (clause 5.8.1.4).

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, background_pacer::BackgroundPacer, error::TenantAssemblyError};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_subscription_forwarding::{error::SubscriptionForwardingError, service::SubscriptionForwardingService};
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
use std::{sync::Arc, time::Duration};

/// How often the pass runs once an abandoned claim is due. An abandoned copy
/// reaches its source at most this long after its claim's lease ends.
const RESUMPTION_INTERVAL: Duration = Duration::from_secs(60);

/// Register the task that, every [`RESUMPTION_INTERVAL`] the pacer admits,
/// places again each copy whose claim outlived its lease.
///
/// The tenant's obligation read holds the earliest end of an unsettled claim's
/// lease, so the pacer rests the pass while no claim is due and wakes a
/// dormant tenant when one is. The task is observed, not tenant-degrading, and
/// a claim a pass could not place stays for the next one.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the schedule or the name is rejected,
/// or when the supervisor refuses the registration.
pub(super) fn register_placement_resumption(
    supervisor: &TaskSupervisor,
    pacer: Arc<BackgroundPacer>,
    forwarding: Arc<SubscriptionForwardingService>,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(RESUMPTION_INTERVAL, InitialExecution::AfterCadence, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("subscription-copy-resumption").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(RESUMPTION_INTERVAL)),
        ShutdownPolicy::new(RESUMPTION_INTERVAL),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let forwarding = Arc::clone(&forwarding);
        let pacer = Arc::clone(&pacer);
        async move {
            pacer
                .run(async move { forwarding.resume_stale_placements().await.map_err(classify_resumption_failure) })
                .await
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

/// How the runtime treats a pass that could not read the abandoned claims. A
/// store that failed, was saturated or was unreachable may answer the next
/// pass, as every other store-backed task is retried, so a database error never
/// stops the pass for good. Every other failure needs a change before a pass
/// can succeed.
const fn classify_resumption_failure(error: SubscriptionForwardingError) -> TaskFailure<SubscriptionForwardingError> {
    let class = match &error {
        SubscriptionForwardingError::Persistence(_) | SubscriptionForwardingError::StoreSaturated | SubscriptionForwardingError::StoreUnavailable => {
            FailureClass::Transient
        }
        SubscriptionForwardingError::Internal(_) => FailureClass::InvariantViolation,
        SubscriptionForwardingError::Subscription(_)
        | SubscriptionForwardingError::Federation(_)
        | SubscriptionForwardingError::Registration(_)
        | SubscriptionForwardingError::Entity(_)
        | SubscriptionForwardingError::JsonLd(_)
        | SubscriptionForwardingError::Representation(_)
        | SubscriptionForwardingError::UnknownSubscription { .. } => FailureClass::Permanent,
    };
    TaskFailure::new(class, error)
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::placement_resumption::classify_resumption_failure;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_subscription_forwarding::error::SubscriptionForwardingError;
    use andromeda_task_runtime::failure::FailureClass;

    /// A store that failed, shed or could not be reached may answer the next
    /// pass, so the runtime retries it, as it retries every other store-backed
    /// task.
    #[test]
    fn a_store_failure_of_any_kind_is_transient() {
        let failures = [
            SubscriptionForwardingError::StoreSaturated,
            SubscriptionForwardingError::StoreUnavailable,
            SubscriptionForwardingError::Persistence(PersistenceFailure::RolledBack),
            SubscriptionForwardingError::Persistence(PersistenceFailure::Undecodable),
        ];
        for failure in failures {
            let rendered = failure.to_string();
            pretty_assertions::assert_eq!(classify_resumption_failure(failure).class(), FailureClass::Transient, "{rendered}");
        }
    }

    /// A failure no retry mends, an identifier naming no forwarded
    /// subscription say, needs a change before a pass can succeed.
    #[test]
    fn a_failure_no_retry_mends_is_permanent() {
        let unknown = SubscriptionForwardingError::UnknownSubscription {
            subscription: "urn:ngsi-ld:Subscription:unknown".parse().expect("a valid identifier"),
        };
        pretty_assertions::assert_eq!(classify_resumption_failure(unknown).class(), FailureClass::Permanent);
    }
}

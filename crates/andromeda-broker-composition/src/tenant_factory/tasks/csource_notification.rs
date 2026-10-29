//! The workers turning registration changes and elapsed intervals into clause
//! 5.11 context source notifications.
//!
//! One tenant runs two of them and they share a delivery half: the resident
//! processor answers a registration or subscription change, the scheduled
//! sweep answers a `timeInterval` that has elapsed. Both address the same
//! stores, and both compact what they notify through the JSON-LD engine, so
//! they are composed together.

use crate::tenant_factory::{
    assembly_invariant::AssemblyInvariant,
    background_pacer::BackgroundPacer,
    error::TenantAssemblyError,
    storage::TenantStorage,
    tasks::{
        notification_schedule::{register_notification_schedule, register_periodic_wake},
        notification_worker_failure::{ChangeChannel, channel_closed, worker_notification_failure},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_notification::{
    csource_change_order::CsourceChangeOrder,
    csource_interval::CsourceIntervalNotificationOperation,
    csource_processor::{CsourceNotificationProcessor, CsourceNotificationProcessorDeps},
    notified_registration_store::NotifiedRegistrationStore,
};
use andromeda_postgresql_context_store::csource_subscription::{notified_registrations::NotifiedRegistrationLedger, revision_ledger::CsourceRevisionLedger};
use andromeda_registration::store::RegistrationStore;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_subscription::{change::CsourceSubscriptionChange, store::SubscriptionStore};
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    registration::TaskRegistration,
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::wake_channel,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};
use tokio::sync::{
    broadcast,
    broadcast::error::{RecvError, TryRecvError},
};

/// What emptying the registration change queue after an overflow found.
///
/// The queued events are older than the reconciliation that follows, which
/// reads every registration as it stands, so they are consumed rather than
/// evaluated: evaluating them as well would report the same change twice.
struct DrainedRegistrations {
    /// Events the channel dropped, including any dropped again while draining.
    lost: u64,
    /// The latest modification time among the consumed events and those
    /// evaluated before them.
    evaluated_through: Option<DateTime<Utc>>,
}

impl DrainedRegistrations {
    /// Consume every event queued on `receiver`.
    fn absorb(&mut self, receiver: &mut broadcast::Receiver<CsrChangeEvent>) {
        loop {
            match receiver.try_recv() {
                Ok(event) => self.evaluated_through = self.evaluated_through.max(event.change.written_at()),
                Err(TryRecvError::Lagged(skipped)) => self.lost = self.lost.saturating_add(skipped),
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
            }
        }
    }
}

/// The change streams a context-source notification worker reacts to.
///
/// The two travel together because one resident worker selects over both:
/// clause 5.11 notifications are produced from registration changes, evaluated
/// against the context-source subscriptions the second stream keeps current.
pub(super) struct CsourceNotificationStreams {
    /// Registration lifecycle changes evaluated against context-source subscriptions.
    pub(super) registrations: broadcast::Sender<CsrChangeEvent>,
    /// Context-source subscription changes keeping the evaluated set current.
    pub(super) subscriptions: broadcast::Sender<CsourceSubscriptionChange>,
}

/// Everything a context source notification worker addresses, gathered so a
/// resident restart can rebuild its processor from owned handles.
///
/// The resident task's closure is re-entered on every restart and cannot
/// borrow the tenant's storage, so each port is cloned once here rather than
/// reached for through a handle to everything.
struct CsourceNotificationPorts {
    /// The tenant branch the subscriptions and registrations are read from.
    target: ContextTarget,
    /// The context source subscriptions evaluated against every change.
    subscriptions: Arc<dyn SubscriptionStore>,
    /// The registrations a notification reports.
    registrations: Arc<dyn RegistrationStore>,
    /// The registrations each subscription was notified of.
    notified: Arc<dyn NotifiedRegistrationStore>,
    /// The order the registrations and subscriptions were written in.
    order: Arc<dyn CsourceChangeOrder>,
    /// The processor compacting a notified registration.
    engine: JsonLdEngine,
    /// The dispatcher every transport binding is reached through.
    transport: Arc<TransportDispatcher>,
    /// The tenant a notification is addressed under.
    tenant: TenantRef,
}

/// Register both context source notification workers for one tenant.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the supervisor refuses a registration.
pub(super) fn register_csource_notification_workers(
    supervisor: &TaskSupervisor,
    storage: &TenantStorage,
    transport: &Arc<TransportDispatcher>,
    csource_subscriptions: Arc<dyn SubscriptionStore>,
    streams: CsourceNotificationStreams,
    interval_max_sleep: Duration,
    pacer: Arc<BackgroundPacer>,
) -> Result<(), TenantAssemblyError> {
    // clone: the periodic sweep subscribes to the same registration and
    // context-source-subscription streams so a change wakes it, while the
    // event-driven processor owns the senders.
    let registration_changes = streams.registrations.clone();
    let subscription_changes = streams.subscriptions.clone();
    register_csource_notification_processor(
        supervisor,
        streams,
        CsourceNotificationPorts {
            target: storage.live_target.clone(),                    // clone: the resident processor owns the live branch it evaluates.
            subscriptions: Arc::clone(&csource_subscriptions),      // clone: the processor and the interval worker share one store.
            registrations: Arc::clone(&storage.registration_store), // clone: every registration worker shares one store.
            notified: Arc::new(NotifiedRegistrationLedger::new(&storage.pool)),
            order: Arc::new(CsourceRevisionLedger::new(&storage.pool)),
            engine: storage.engine.clone(),   // clone: notification policy owns its engine handle.
            transport: Arc::clone(transport), // clone: every worker delivers through one dispatcher.
            tenant: storage.tenant.clone(),   // clone: notification policy owns its tenant partition.
        },
    )?;
    register_csource_interval_notification(
        supervisor,
        Arc::new(CsourceIntervalNotificationOperation::new(
            storage.live_target.clone(), // clone: interval delivery operates on the tenant's live branch.
            csource_subscriptions,
            Arc::clone(&storage.registration_store), // clone: every registration worker shares one store.
            storage.engine.clone(),                  // clone: interval policy owns its engine handle.
            Arc::clone(transport),                   // clone: every worker delivers through one dispatcher.
            storage.tenant.clone(),                  // clone: interval policy owns its tenant partition.
        )),
        registration_changes,
        subscription_changes,
        interval_max_sleep,
        pacer,
    )
}

fn register_csource_notification_processor(
    supervisor: &TaskSupervisor,
    streams: CsourceNotificationStreams,
    ports: CsourceNotificationPorts,
) -> Result<(), TenantAssemblyError> {
    let CsourceNotificationStreams {
        registrations: registration_changes,
        subscriptions: subscription_changes,
    } = streams;
    let CsourceNotificationPorts {
        target,
        subscriptions: subscription_store,
        registrations: registration_store,
        notified,
        order,
        engine,
        transport,
        tenant,
    } = ports;
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("csource-notification-events").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(restart_delay)),
        ShutdownPolicy::new(restart_delay),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let mut registration_receiver = registration_changes.subscribe();
        let mut subscription_receiver = subscription_changes.subscribe();
        let processor = CsourceNotificationProcessor::new(CsourceNotificationProcessorDeps {
            target: target.clone(), // clone: each resident restart owns the same immutable live branch target.
            sub_repo: Arc::clone(&subscription_store),
            csr_repo: Arc::clone(&registration_store),
            notified: Arc::clone(&notified),
            order: Arc::clone(&order),
            engine: engine.clone(), // clone: each resident restart owns the same engine handle.
            transport: Arc::clone(&transport),
            tenant: tenant.clone(), // clone: each resident restart owns the same tenant partition.
        });
        async move {
            processor.reload_cache().await.map_err(worker_notification_failure)?;
            execution.mark_running();
            let mut lost_registration_changes: u64 = 0;
            let mut evaluated_through = Some(Utc::now());
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = subscription_receiver.recv() => match received {
                        Ok(change) => processor.apply_subscription_change(change).await,
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "context-source subscription stream lagged; reloading policy cache");
                            processor.reload_cache().await.map_err(worker_notification_failure)?;
                        }
                        Err(RecvError::Closed) => return Err(channel_closed(ChangeChannel::CsourceSubscription)),
                    },
                    received = registration_receiver.recv() => match received {
                        Ok(event) => {
                            loop {
                                match subscription_receiver.try_recv() {
                                    Ok(change) => processor.apply_subscription_change(change).await,
                                    Err(TryRecvError::Lagged(skipped)) => {
                                        tracing::debug!(skipped, "context-source subscription stream lagged before registration evaluation; reloading policy cache");
                                        processor.reload_cache().await.map_err(worker_notification_failure)?;
                                    }
                                    Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                                }
                            }
                            evaluated_through = evaluated_through.max(event.change.written_at());
                            processor.apply_registration_change(event).await;
                        }
                        Err(RecvError::Lagged(skipped)) => {
                            let mut drained = DrainedRegistrations { lost: skipped, evaluated_through };
                            drained.absorb(&mut registration_receiver);
                            lost_registration_changes = lost_registration_changes.saturating_add(drained.lost);
                            tracing::warn!(skipped = drained.lost, lost_registration_changes, "registration stream lagged; reconciling context source notifications");
                            evaluated_through = processor
                                .reconcile_registrations(drained.evaluated_through)
                                .await
                                .map_err(worker_notification_failure)?;
                        }
                        Err(RecvError::Closed) => return Err(channel_closed(ChangeChannel::Registration)),
                    },
                }
            }
        }
    });
    supervisor
        .register_resident(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

fn register_csource_interval_notification(
    supervisor: &TaskSupervisor,
    operation: Arc<CsourceIntervalNotificationOperation>,
    registration_changes: broadcast::Sender<CsrChangeEvent>,
    subscription_changes: broadcast::Sender<CsourceSubscriptionChange>,
    interval_max_sleep: Duration,
    pacer: Arc<BackgroundPacer>,
) -> Result<(), TenantAssemblyError> {
    let (interval_wake, receiver) = wake_channel();
    // Both streams change what a periodic csource notification would report: the
    // registrations it carries (clause 5.11.7) and the subscription set it is
    // evaluated against. Either waking the sweep is correct; the wake coalesces.
    register_periodic_wake(
        supervisor,
        "csource-notification-interval-registration-wake",
        interval_wake.clone(),
        ChangeChannel::Registration,
        registration_changes,
    )?;
    register_periodic_wake(
        supervisor,
        "csource-notification-interval-subscription-wake",
        interval_wake,
        ChangeChannel::CsourceSubscription,
        subscription_changes,
    )?;
    register_notification_schedule(
        supervisor,
        "csource-notification-interval",
        interval_max_sleep,
        receiver,
        pacer,
        move |cutoff| {
            let operation = Arc::clone(&operation); // clone: each bounded pass owns the shared notification operation.
            async move {
                operation.execute(cutoff).await?;
                operation.next_due(cutoff).await
            }
        },
    )
}

//! The workers turning registration changes and elapsed intervals into clause
//! 5.11 context source notifications.
//!
//! One tenant runs two of them and they share a delivery half: the resident
//! processor answers a registration or subscription change, the scheduled
//! sweep answers a `timeInterval` that has elapsed. Both address the same
//! stores, and both compact what they notify through the JSON-LD engine, so
//! they are composed together.

use crate::{
    event::queued_handoff::settle,
    tenant_factory::{
        assembly_invariant::AssemblyInvariant,
        error::TenantAssemblyError,
        periodic_due::PeriodicSweep,
        storage::TenantStorage,
        tasks::{
            PeriodicSweepDrive,
            notification_schedule::{register_notification_schedule, register_periodic_wake},
            notification_worker_failure::{ChangeChannel, channel_closed, worker_notification_failure},
            periodic_evaluation::periodic_pass,
        },
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
    wake_trigger::{DrainWakeReceiver, WakeHandle, wake_channel},
};
use andromeda_tenancy::tenant_ref::TenantRef;
use std::{sync::Arc, time::Duration};
use tokio::sync::{
    Mutex,
    broadcast,
    broadcast::error::{RecvError, TryRecvError},
    mpsc,
};
use tokio_util::task::TaskTracker;

/// The change streams a context-source notification worker reacts to.
///
/// The two travel together because one resident worker selects over both:
/// clause 5.11 notifications are produced from registration changes, evaluated
/// against the context-source subscriptions the second stream keeps current.
pub(super) struct CsourceNotificationStreams {
    /// The one reader of the queue of registration lifecycle changes evaluated
    /// against context-source subscriptions, held across worker restarts.
    pub(super) registrations: Arc<Mutex<mpsc::Receiver<CsrChangeEvent>>>,
    /// The hand-offs of writes that found the registration queue full, which
    /// the worker waits for when it stops.
    pub(super) handoffs: TaskTracker,
    /// Context-source subscription changes keeping the evaluated set current.
    pub(super) subscriptions: broadcast::Sender<CsourceSubscriptionChange>,
    /// The longest a stop spends evaluating what the tenant holds; it is the
    /// worker's shutdown grace.
    pub(super) shutdown_drain: Duration,
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
    drive: PeriodicSweepDrive,
) -> Result<(), TenantAssemblyError> {
    // Both streams change what a periodic csource notification would report: the
    // registrations it carries (clause 5.11.7) and the subscription set it is
    // evaluated against. The processor, the one reader of the registration
    // queue, wakes the sweep on each registration change; the wake coalesces.
    let (interval_wake, interval_receiver) = wake_channel();
    // clone: the periodic sweep subscribes to the same context-source-subscription
    // stream so a change wakes it, while the event-driven processor owns the sender.
    let subscription_changes = streams.subscriptions.clone();
    register_csource_notification_processor(
        supervisor,
        streams,
        interval_wake.clone(), // clone: the processor and the subscription wake each pulse the sweep.
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
        IntervalWake {
            handle: interval_wake,
            receiver: interval_receiver,
            subscription_changes,
        },
        drive,
    )
}

/// What wakes the periodic context source sweep before its schedule.
struct IntervalWake {
    /// The handle the subscription stream pulses the sweep through.
    handle: WakeHandle,
    /// The receiver the sweep's drain waits on.
    receiver: DrainWakeReceiver,
    /// Context-source subscription changes, each of which wakes the sweep.
    subscription_changes: broadcast::Sender<CsourceSubscriptionChange>,
}

fn register_csource_notification_processor(
    supervisor: &TaskSupervisor,
    streams: CsourceNotificationStreams,
    interval_wake: WakeHandle,
    ports: CsourceNotificationPorts,
) -> Result<(), TenantAssemblyError> {
    let CsourceNotificationStreams {
        registrations: registration_changes,
        handoffs,
        subscriptions: subscription_changes,
        shutdown_drain,
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
        ShutdownPolicy::new(shutdown_drain),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let registration_changes = Arc::clone(&registration_changes); // clone: every resident attempt takes the one queue reader in turn.
        let interval_wake = interval_wake.clone(); // clone: each resident attempt owns a wake handle it pulses.
        let handoffs = handoffs.clone(); // clone: every resident attempt waits for the hand-offs the sink starts.
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
            let mut registration_receiver = registration_changes.lock_owned().await;
            processor.reload_cache().await.map_err(worker_notification_failure)?;
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => {
                        return settle(&handoffs, &mut registration_receiver, |event| async {
                            processor.apply_registration_change(event).await;
                            Ok(())
                        })
                        .await;
                    }
                    received = subscription_receiver.recv() => match received {
                        Ok(change) => processor.apply_subscription_change(change).await,
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "context-source subscription stream lagged; reloading policy cache");
                            processor.reload_cache().await.map_err(worker_notification_failure)?;
                        }
                        Err(RecvError::Closed) => return Err(channel_closed(ChangeChannel::CsourceSubscription)),
                    },
                    received = registration_receiver.recv() => match received {
                        Some(event) => {
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
                            // The sweep reads the store, so it is woken whether or
                            // not this evaluation sends anything.
                            let _ = interval_wake.wake();
                            processor.apply_registration_change(event).await;
                        }
                        None => return Err(channel_closed(ChangeChannel::Registration)),
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
    wake: IntervalWake,
    drive: PeriodicSweepDrive,
) -> Result<(), TenantAssemblyError> {
    let IntervalWake {
        handle,
        receiver,
        subscription_changes,
    } = wake;
    let rerun = handle.clone(); // clone: an evaluation that ends after a pass refused it wakes the sweep.
    register_periodic_wake(
        supervisor,
        "csource-notification-interval-subscription-wake",
        handle,
        ChangeChannel::CsourceSubscription,
        subscription_changes,
    )?;
    let PeriodicSweepDrive { pacing, evaluations } = drive;
    register_notification_schedule(
        supervisor,
        "csource-notification-interval",
        PeriodicSweep::ContextSource,
        receiver,
        pacing,
        move |cutoff| {
            let operation = Arc::clone(&operation); // clone: each pass hands the shared operation to the evaluations it starts.
            let evaluations = evaluations.clone(); // clone: each pass offers its evaluations to the tenant's one set of slots.
            let rerun = rerun.clone(); // clone: each pass hands the sweep's wake to the evaluations it starts.
            async move {
                periodic_pass(
                    &operation,
                    &evaluations,
                    &move || {
                        let _ = rerun.wake();
                    },
                    cutoff,
                )
                .await
            }
        },
    )
}

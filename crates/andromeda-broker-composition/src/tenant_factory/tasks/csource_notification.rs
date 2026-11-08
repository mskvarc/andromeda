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
    pub(super) drain_timeout: Duration,
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
    let subscription_changes = streams.subscriptions.clone();
    register_csource_notification_processor(
        supervisor,
        streams,
        interval_wake.clone(),
        CsourceNotificationPorts {
            target: storage.live_target.clone(),
            subscriptions: Arc::clone(&csource_subscriptions),
            registrations: Arc::clone(&storage.registration_store),
            notified: Arc::new(NotifiedRegistrationLedger::new(&storage.pool)),
            order: Arc::new(CsourceRevisionLedger::new(&storage.pool)),
            engine: storage.engine.clone(),
            transport: Arc::clone(transport),
            tenant: storage.tenant.clone(),
        },
    )?;
    register_csource_interval_notification(
        supervisor,
        Arc::new(CsourceIntervalNotificationOperation::new(
            storage.live_target.clone(),
            csource_subscriptions,
            Arc::clone(&storage.registration_store),
            storage.engine.clone(),
            Arc::clone(transport),
            storage.tenant.clone(),
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

/// Register the resident worker that evaluates committed registration changes
/// against the cached context source Subscriptions (clause 5.11).
///
/// Subscription changes keep the cache current, and pending ones are applied
/// before each registration change is evaluated; a lagged stream reloads the
/// cache. Each registration change also wakes the periodic sweep. On
/// cancellation the queued registration changes are evaluated before the
/// worker stops; a closed channel fails the attempt, retried after one second.
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
        drain_timeout,
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
        ShutdownPolicy::new(drain_timeout),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let registration_changes = Arc::clone(&registration_changes);
        let interval_wake = interval_wake.clone();
        let handoffs = handoffs.clone();
        let mut subscription_receiver = subscription_changes.subscribe();
        let processor = CsourceNotificationProcessor::new(CsourceNotificationProcessorDeps {
            target: target.clone(), // clone: the closure runs once per attempt, so each attempt takes its own target.
            sub_repo: Arc::clone(&subscription_store),
            csr_repo: Arc::clone(&registration_store),
            notified: Arc::clone(&notified),
            order: Arc::clone(&order),
            engine: engine.clone(),
            transport: Arc::clone(&transport),
            tenant: tenant.clone(), // clone: the closure runs once per attempt, so each attempt takes its own tenant.
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

/// Register the periodic context source sweep, woken early by every context
/// source Subscription change and by an evaluation a full pass refused.
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
    let rerun = handle.clone();
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
            let operation = Arc::clone(&operation);
            let evaluations = evaluations.clone();
            let rerun = rerun.clone();
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

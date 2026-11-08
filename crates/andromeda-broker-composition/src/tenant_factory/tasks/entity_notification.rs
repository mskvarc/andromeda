//! The workers turning entity changes and elapsed intervals into clause 5.8
//! notifications.
//!
//! The resident processor registered here is also the transport of the drain delivering what the tenant database queued.
//!
//! Clause 5.11 context source notifications are registered separately: they watch a
//! different change stream, carry registrations rather than entities, and
//! share nothing with these beyond the schedule helper.

use crate::{
    event::queued_handoff::settle,
    linked_forwarding::LinkedForwarding,
    tenant_factory::{
        TenantFactoryConfig,
        assembly_invariant::AssemblyInvariant,
        error::TenantAssemblyError,
        periodic_due::PeriodicSweep,
        storage::TenantStorage,
        tasks::{
            EntityNotificationDelivery,
            PeriodicSweepDrive,
            abandoned_drain::EntityDrain,
            notification_delivery::{DeliveryDrainDrive, register_notification_delivery},
            notification_schedule::{register_notification_schedule, register_periodic_wake},
            notification_worker_failure::{ChangeChannel, channel_closed, evaluation_lane_stopped, worker_notification_failure},
            periodic_evaluation::periodic_pass,
            subscription_cache_reconcile::register_subscription_cache_reconcile,
        },
    },
};
use andromeda_configuration::schema::notifications::NotificationsConfig;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_entity::{
    configured_policy::geometry_census_limits,
    query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource},
};
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_federation::FederationService;
use andromeda_jsonld::operation_hit_scope::run_in_operation_scope;
use andromeda_ngsi_ld_query::{
    filter::complexity::FilterComplexityLimits,
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
};
use andromeda_notification::{
    delivery_store::DeliveryStore,
    interval::{IntervalNotificationDeps, IntervalNotificationOperation},
    pending_change::{PendingChange, PendingCommits},
    processor::{NotificationProcessor, NotificationProcessorDeps},
};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_subscription::{change::SubscriptionChange, store::SubscriptionStore};
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    keyed_lanes::KeyedLanes,
    registration::TaskRegistration,
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::wake_channel,
};
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use tokio::sync::{
    Mutex,
    broadcast,
    broadcast::error::{RecvError, TryRecvError},
    mpsc,
};
use tokio_util::task::TaskTracker;

/// Committed entity changes one live evaluation lane queues behind the change
/// it is evaluating; a full lane holds the rest in the tenant's change queue.
const LIVE_LANE_DEPTH: NonZeroUsize = NonZeroUsize::MIN.saturating_add(31);

/// Compose the query service one tenant's notifications resolve the Linked
/// Entities of their `join` with, change-driven and periodic alike.
///
/// A notification's join resolves Linked Entities exactly as a request's does,
/// so the service is composed like the request-facing one.
pub(super) fn compose_notification_query(
    storage: &TenantStorage,
    forwarding: &Arc<FederationService>,
    config: &TenantFactoryConfig,
) -> Arc<EntityQueryService> {
    Arc::new(
        EntityQueryService::builder()
            .reader(Arc::clone(&storage.entity_ports.reader))
            .query_store(Arc::clone(&storage.entity_ports.query))
            .engine(storage.engine.clone())
            .spatial(storage.spatial.clone())
            .tenant(storage.tenant.clone())
            // A notification's join resolves Linked Entities exactly as a request's
            // does, including the ones clause 4.5.23.1 places outside this broker.
            .remote_linked(Some(
                Arc::new(LinkedForwarding::new(Arc::clone(forwarding), storage.live_target.clone())) as Arc<dyn RemoteLinkedEntitySource>
            ))
            // A notification query only resolves Linked Entities by identifier and
            // compiles no geo-query: a subscription's `geoQ` is matched in memory
            // from its stored form, admitted once when it was written, so neither a
            // lower limit nor the band switched off since refuses a stored
            // subscription.
            .geo_admission(GeoQueryAdmission::new(
                geometry_census_limits(&config.request_limits),
                NearDistanceBand::Admitted,
            ))
            // A subscription's filters read under the broker's live mode.
            .language_targeting(EnabledExtensions::from(&config.extensions).language_targeting())
            // A subscription's `q` was held to the complexity limits when it
            // was written, so a limit lowered since refuses no stored one.
            .filter_complexity(FilterComplexityLimits::PARSED)
            .build(),
    )
}

/// Compose the notification policy operation one tenant evaluates its
/// subscriptions with.
pub(super) fn compose_notification_processor(
    storage: &TenantStorage,
    notification_query: Arc<EntityQueryService>,
    transport: &Arc<TransportDispatcher>,
    subscriptions: &Arc<dyn SubscriptionStore>,
    pending_commits: Arc<PendingCommits>,
) -> Arc<NotificationProcessor> {
    Arc::new(NotificationProcessor::new(NotificationProcessorDeps {
        live_target: storage.live_target.clone(),
        sub_repo: Arc::clone(subscriptions),
        entity_reader: Arc::clone(&storage.entity_ports.reader),
        entity_query: notification_query,
        engine: storage.engine.clone(),
        transport: Arc::clone(transport),
        tenant: storage.tenant.clone(),
        subscription_cache: Arc::clone(&storage.subscription_cache),
        pending_commits,
    }))
}

/// Register the workers driving one tenant's clause 5.8 notification policy:
/// the resident processor, and the drain delivering the notifications queued
/// in the tenant's database.
///
/// The drain claims from `store`, the queue the tenant's database holds.
///
/// The processor evaluates the tenant's committed entity changes only under
/// live delivery, sending each notification at once. Under durable delivery
/// the committer records every notification in the commit that raised it (the
/// entity services' notification write configuration) and the processor
/// evaluates no change of its own. Under either mode the processor is the
/// drain's transport, re-evaluating and sending each queued delivery the drain
/// claims; the drain itself runs under either mode, for deliveries already queued.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the delivery configuration is rejected
/// or when the supervisor refuses a registration.
pub(super) fn register_notification_workers(
    supervisor: &TaskSupervisor,
    store: Arc<dyn DeliveryStore>,
    config: &NotificationsConfig,
    processor: Arc<NotificationProcessor>,
    streams: EntityNotificationStreams,
    drive: DeliveryDrainDrive,
) -> Result<(), TenantAssemblyError> {
    let delivery = if config.delivery.is_durable() {
        EntityNotificationDelivery::Durable
    } else {
        EntityNotificationDelivery::Live
    };
    register_notification_processor(
        supervisor,
        streams,
        Arc::clone(&processor),
        delivery,
        EvaluationLimits {
            lanes: config.evaluation.max_concurrent_changes,
            drain_timeout: config.evaluation.drain_timeout(),
        },
    )?;
    register_subscription_cache_reconcile(supervisor, Arc::clone(&processor))?;
    register_notification_delivery(supervisor, store, &config.delivery, processor, drive)
}

/// What the periodic sweep reads and delivers through beside the tenant's
/// storage.
pub(super) struct IntervalDelivery {
    /// The tenant's entity subscriptions.
    pub(super) subscriptions: Arc<dyn SubscriptionStore>,
    /// The query service the sweep resolves `join` Linked Entities with.
    pub(super) notification_query: Arc<EntityQueryService>,
    /// The dispatcher every worker delivers through.
    pub(super) transport: Arc<TransportDispatcher>,
}

/// Register the sweep delivering one tenant's `timeInterval` entity
/// subscriptions.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the supervisor refuses the registration.
pub(super) fn register_interval_notification(
    supervisor: &TaskSupervisor,
    storage: &TenantStorage,
    delivery: IntervalDelivery,
    subscription_changes: broadcast::Sender<SubscriptionChange>,
    drive: PeriodicSweepDrive,
) -> Result<(), TenantAssemblyError> {
    let operation = Arc::new(IntervalNotificationOperation::new(IntervalNotificationDeps {
        target: storage.live_target.clone(),
        sub_repo: delivery.subscriptions,
        entity_reader: Arc::clone(&storage.entity_ports.reader),
        entity_pages: Arc::clone(&storage.entity_ports.payload_pages),
        entity_query: delivery.notification_query,
        engine: storage.engine.clone(),
        transport: delivery.transport,
        tenant: storage.tenant.clone(),
    }));
    let (interval_wake, receiver) = wake_channel();
    let rerun = interval_wake.clone();
    register_periodic_wake(
        supervisor,
        "notification-interval-wake",
        interval_wake,
        ChangeChannel::Subscription,
        subscription_changes,
    )?;
    let PeriodicSweepDrive { pacing, evaluations } = drive;
    register_notification_schedule(supervisor, "notification-interval", PeriodicSweep::Entity, receiver, pacing, move |cutoff| {
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
    })
}

/// Start the lanes live delivery evaluates committed entity changes on, none
/// under durable delivery.
///
/// Clause 5.8.6 notifies each change of a watched attribute, and the changes of
/// one Entity reach a subscriber in the order they were committed (clause
/// 5.5.11.2 orders the updates of one Entity in a batch chronologically), so a
/// change waits only for the earlier changes of its own Entity.
fn live_evaluation_lanes(
    processor: &Arc<NotificationProcessor>,
    delivery: EntityNotificationDelivery,
    width: NonZeroUsize,
) -> Option<KeyedLanes<PendingChange>> {
    match delivery {
        EntityNotificationDelivery::Durable => None,
        EntityNotificationDelivery::Live => {
            let processor = Arc::clone(processor);
            Some(KeyedLanes::start(width, LIVE_LANE_DEPTH, move |change: PendingChange| {
                let processor = Arc::clone(&processor);
                async move {
                    // Clause 5.13.3.5: counting one hit per context per
                    // operation means the notification's own context
                    // resolutions count once for this change, distinct
                    // from the write that produced it.
                    run_in_operation_scope(processor.apply_entity_change(change)).await;
                }
            }))
        }
    }
}

/// The change streams the resident notification processor reacts to.
pub(super) struct EntityNotificationStreams {
    /// The one reader of the queue of committed entity changes, held across
    /// restarts so a change queued meanwhile is still evaluated.
    pub(super) entity_changes: Arc<Mutex<mpsc::Receiver<PendingChange>>>,
    /// The hand-offs of writes that found the entity queue full, which the
    /// processor waits for when it stops.
    pub(super) handoffs: TaskTracker,
    /// The changes handed to evaluation and not yet evaluated.
    pub(super) commits: Arc<PendingCommits>,
    /// Subscription changes keeping the evaluated set current.
    pub(super) subscription_changes: broadcast::Sender<SubscriptionChange>,
}

/// How wide live evaluation runs and how long one notification may take.
#[derive(Clone, Copy)]
struct EvaluationLimits {
    /// The evaluation lanes running concurrently.
    lanes: NonZeroUsize,
    /// The longest a stop spends evaluating what the tenant holds; it is the
    /// processor's shutdown grace, and the configuration holds it to at least
    /// one notification's delivery deadline.
    drain_timeout: Duration,
}

/// Register the resident entity notification worker. Under live delivery it
/// dispatches each queued change to its entity's lane and settles the queue on
/// cancellation; under durable delivery it follows only Subscription changes.
/// A lagged stream reloads the cache; a closed channel fails the attempt.
fn register_notification_processor(
    supervisor: &TaskSupervisor,
    streams: EntityNotificationStreams,
    processor: Arc<NotificationProcessor>,
    delivery: EntityNotificationDelivery,
    limits: EvaluationLimits,
) -> Result<(), TenantAssemblyError> {
    let EntityNotificationStreams {
        entity_changes,
        handoffs,
        commits,
        subscription_changes,
    } = streams;
    let EvaluationLimits { lanes, drain_timeout } = limits;
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("notification-events").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(restart_delay)),
        ShutdownPolicy::new(drain_timeout),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let commits = Arc::clone(&commits);
        let handoffs = handoffs.clone();
        let entity_changes = Arc::clone(&entity_changes);
        let mut subscription_receiver = subscription_changes.subscribe();
        let processor = Arc::clone(&processor);
        async move {
            let mut entity_receiver = entity_changes.lock_owned().await;
            processor.reload_cache().await.map_err(worker_notification_failure)?;
            let evaluation = live_evaluation_lanes(&processor, delivery, lanes);
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => {
                        if let Some(lanes) = evaluation {
                            let drain = EntityDrain::begin(&commits);
                            settle(&handoffs, &mut entity_receiver, |change| async {
                                lanes.lane(&change.change.event.entity_id).dispatch(change).await.map_err(evaluation_lane_stopped)
                            })
                            .await?;
                            lanes.finish().await;
                            drain.finish();
                        }
                        return Ok(());
                    }
                    received = subscription_receiver.recv() => match received {
                        Ok(_) => processor.release_superseded(),
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "notification subscription stream lagged; reloading policy cache");
                            processor.reload_cache().await.map_err(worker_notification_failure)?;
                        }
                        Err(RecvError::Closed) => return Err(channel_closed(ChangeChannel::Subscription)),
                    },
                    received = entity_receiver.recv(), if evaluation.is_some() => {
                        let Some(change) = received else {
                            // Every sender is gone, as when the tenant is torn down: the lanes still run out what they hold.
                            if let Some(lanes) = evaluation {
                                lanes.finish().await;
                            }
                            return Err(channel_closed(ChangeChannel::Entity));
                        };
                        loop {
                            match subscription_receiver.try_recv() {
                                Ok(_) => processor.release_superseded(),
                                Err(TryRecvError::Lagged(skipped)) => {
                                    tracing::debug!(skipped, "notification subscription stream lagged before entity evaluation; reloading policy cache");
                                    processor.reload_cache().await.map_err(worker_notification_failure)?;
                                }
                                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                            }
                        }
                        if let Some(lanes) = &evaluation {
                            lanes.lane(&change.change.event.entity_id).dispatch(change).await.map_err(evaluation_lane_stopped)?;
                        }
                    }
                }
            }
        }
    });
    supervisor
        .register_resident(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

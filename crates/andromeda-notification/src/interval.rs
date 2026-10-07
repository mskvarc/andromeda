//! Periodic (`timeInterval`) entity notification evaluation (clause 5.8.6).

use crate::{
    claimed_notification_context::claimed_notification_context,
    error::NotificationError,
    interval_candidates::IntervalCandidates,
    interval_match::matching_entities,
    interval_payload::PeriodicPayload,
};
use andromeda_context_space::{context_target::ContextTarget, snapshot_target::SnapshotTarget};
use andromeda_delivery::{dispatch::TransportDispatcher, endpoint::transport::parse_endpoint_transport, request::NotificationDelivery};
use andromeda_entity::{
    port::{payload_pages::EntityPayloadPages, reader::EntityReader},
    query::EntityQueryService,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::subscription::Subscription;
use andromeda_ngsi_ld_render::{geometry_selection::GeometrySelection, notification_data::DataShape};
use andromeda_subscription::{
    notification_slot::{NotificationSlot, NotificationSlotGate},
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use std::sync::Arc;
use tracing::{debug, error};

const INTERVAL_NOTIFICATION_CONCURRENCY: usize = 20;

/// Bounded evaluation of due entity interval subscriptions.
pub struct IntervalNotificationOperation {
    target: ContextTarget,
    sub_repo: Arc<dyn SubscriptionStore>,
    entity_reader: Arc<dyn EntityReader>,
    entity_pages: Arc<dyn EntityPayloadPages>,
    entity_query: Arc<EntityQueryService>,
    transport: Arc<TransportDispatcher>,
    engine: JsonLdEngine,
    tenant: TenantRef,
}

/// Focused capabilities used by periodic entity-notification evaluation.
pub struct IntervalNotificationDeps {
    /// The branch the due subscriptions are evaluated on.
    pub target: ContextTarget,
    /// Entity-subscription persistence.
    pub sub_repo: Arc<dyn SubscriptionStore>,
    /// Entity system-attribute reads.
    pub entity_reader: Arc<dyn EntityReader>,
    /// Paged reads of the entities a subscription matches.
    pub entity_pages: Arc<dyn EntityPayloadPages>,
    /// Linked-entity resolution for `join`, composed as a change-driven
    /// notification's is.
    pub entity_query: Arc<EntityQueryService>,
    /// Tenant JSON-LD processor.
    pub engine: JsonLdEngine,
    /// Selected notification delivery capability.
    pub transport: Arc<TransportDispatcher>,
    /// Tenant partition for delivery metadata.
    pub tenant: TenantRef,
}

impl IntervalNotificationOperation {
    /// Construct interval evaluation over focused entity, subscription, and delivery capabilities.
    #[must_use]
    pub fn new(deps: IntervalNotificationDeps) -> IntervalNotificationOperation {
        IntervalNotificationOperation {
            target: deps.target,
            sub_repo: deps.sub_repo,
            entity_reader: deps.entity_reader,
            entity_pages: deps.entity_pages,
            entity_query: deps.entity_query,
            transport: deps.transport,
            engine: deps.engine,
            tenant: deps.tenant,
        }
    }

    /// Evaluate every interval subscription due at the execution boundary.
    ///
    /// Every due subscription's grid anchor is advanced past `cutoff` before this
    /// returns, whether or not it had anything to send: a subscription that
    /// matched nothing still ticked, so its anchor moves and the sweep stops
    /// re-reading it, and re-running its paged scan, every pass.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Subscription`] when the subscription store
    /// cannot report the due interval subscriptions. Per-subscription failures —
    /// entity lookup, payload assembly, slot claiming, serialization, endpoint
    /// parsing, and delivery outcome bookkeeping — are logged and skipped rather
    /// than aborting the sweep.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<usize, NotificationError> {
        let due_subs = self.sub_repo.find_due_interval_subscriptions(&self.target, cutoff).await?;
        let due_count = due_subs.len();

        let this = &self;
        let _results: Vec<()> = stream::iter(due_subs)
            .map(|sub| async move {
                let entities = match this.candidates().matching(&sub).await {
                    Ok(e) => e,
                    Err(e) => {
                        error!("IntervalWorker failed to fetch entities for {}: {}", sub.id, e);
                        if let Err(e) = this.sub_repo.mark_failure(&this.target, &sub.id).await {
                            debug!("Failed to mark failure for {}: {}", sub.id, e);
                        }
                        return;
                    }
                };

                let entities = match matching_entities(&this.entity_query, &this.target, &sub, entities, Utc::now()).await {
                    Ok(entities) => entities,
                    Err(e) => {
                        error!("IntervalWorker failed to read the entities the filter of {} links to: {}", sub.id, e);
                        return;
                    }
                };

                if entities.is_empty() {
                    // Clause 5.8.6: no matching entities means no notification.
                    // The anchor still advances so the interval is not re-read
                    // every pass until it next comes due.
                    this.advance_anchor(&sub, cutoff).await;
                    return;
                }

                // The slot is claimed only once a notification is certain to be sent.
                // Table 5.2.15-1: a periodic subscription carries no
                // `throttling` (Table 5.2.12-1 makes the pair mutually
                // exclusive), but its endpoint's `cooldown` still suppresses a
                // notification that would follow too soon after a failure. The
                // claim advances the anchor itself, so no separate advance is
                // needed on the send path.
                let gate = NotificationSlotGate::at(cutoff, None, sub.notification.endpoint.cooldown);
                match this.sub_repo.claim_notification_slot(&this.target, &sub.id, gate).await {
                    Ok(NotificationSlot::Claimed) => {}
                    Ok(NotificationSlot::Suppressed) => return,
                    Err(error) => {
                        debug!("IntervalWorker failed to claim slot for {}: {}", sub.id, error);
                        return;
                    }
                }

                let endpoint_uri = sub.notification.endpoint.uri.url();

                let endpoint_transport = match parse_endpoint_transport(endpoint_uri, sub.notification.endpoint.notifier_info.as_deref()) {
                    Ok(t) => t,
                    Err(e) => {
                        debug!("IntervalWorker: invalid endpoint URI for {}: {}", sub.id, e);
                        return;
                    }
                };

                // Clause 5.5.6: a `jsonldContext` that cannot be loaded names no
                // context this notification can be rendered under.
                let Some(context) = claimed_notification_context(
                    &this.engine,
                    sub.delivery.jsonld_context.as_ref(),
                    this.sub_repo.as_ref(),
                    &this.target,
                    &sub.id,
                )
                .await
                else {
                    return;
                };
                let delivery = NotificationDelivery::builder()
                    .endpoint_uri(endpoint_uri)
                    .accept(sub.notification.endpoint.accept.as_ref())
                    .receiver_info(sub.notification.endpoint.receiver_info.as_deref())
                    .context(&context)
                    .tenant(this.tenant.tenant_name())
                    .snapshot(this.target.snapshot().map(SnapshotTarget::snapshot_id))
                    .timeout(sub.notification.endpoint.timeout)
                    .subscription_id(&sub.id)
                    .build();

                // Table 5.3.1-1: an `application/geo+json` endpoint receives the
                // entities as one `FeatureCollection`.
                let shape = DataShape {
                    media_type: delivery.content_type(),
                    selection: GeometrySelection::new(None, sub.filters.dataset_id.as_deref().unwrap_or_default()),
                    context: delivery.body_context(),
                };
                let data = match this.payload().entities(&sub, entities, shape).await {
                    Ok(data) => data,
                    Err(e) => {
                        error!("IntervalWorker failed to build notification payload for {}: {}", sub.id, e);
                        return;
                    }
                };
                let request = match delivery.notification(data) {
                    Ok(request) => request,
                    Err(e) => {
                        error!("Failed to write notification for {}: {}", sub.id, e);
                        return;
                    }
                };

                let success = this.transport.deliver(&endpoint_transport, &request).await.is_delivered();

                if success {
                    if let Err(e) = this.sub_repo.mark_success(&this.target, &sub.id).await {
                        debug!("Failed to mark success for {}: {}", sub.id, e);
                    }
                } else if let Err(e) = this.sub_repo.mark_failure(&this.target, &sub.id).await {
                    debug!("Failed to mark failure for {}: {}", sub.id, e);
                }
            })
            .buffer_unordered(INTERVAL_NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
        Ok(due_count)
    }

    /// The earliest instant some active interval subscription is next due after
    /// `cutoff`, or `None` when this tenant has none.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Subscription`] when the store cannot report
    /// the next due instant.
    pub async fn next_due(&self, cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, NotificationError> {
        self.sub_repo.next_interval_due(&self.target, cutoff).await.map_err(NotificationError::from)
    }

    /// The entities a due subscription's notification considers.
    fn candidates(&self) -> IntervalCandidates<'_> {
        IntervalCandidates {
            target: &self.target,
            pages: self.entity_pages.as_ref(),
        }
    }

    /// What a due subscription's notification carries of the entities it
    /// matched.
    fn payload(&self) -> PeriodicPayload<'_> {
        PeriodicPayload {
            target: &self.target,
            engine: &self.engine,
            entity_reader: self.entity_reader.as_ref(),
            entity_query: &self.entity_query,
        }
    }

    /// Advance a due subscription's grid anchor past `cutoff`, logging a store
    /// failure rather than aborting the sweep.
    async fn advance_anchor(&self, sub: &Subscription, cutoff: DateTime<Utc>) {
        if let Err(error) = self.sub_repo.advance_interval_anchor(&self.target, &sub.id, cutoff).await {
            debug!("Failed to advance interval anchor for {}: {}", sub.id, error);
        }
    }
}

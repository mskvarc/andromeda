//! Periodic (`timeInterval`) entity notification evaluation (clause 5.8.6).

use crate::{
    claimed_notification_context::claimed_notification_context,
    error::NotificationError,
    interval_candidates::IntervalCandidates,
    interval_match::apply_subscription_filters,
    processor::payload::{
        compaction::compact_notified_state,
        entity_instants::{ExpiryStamp, notified_instants},
        notified_entity::rendered_entity,
        selection::{EntityMembers, Selection, SelectionRule},
    },
};
use andromeda_context_space::{context_target::ContextTarget, snapshot_target::SnapshotTarget};
use andromeda_delivery::{
    dispatch::TransportDispatcher,
    endpoint::transport::parse_endpoint_transport,
    request::{NotificationDelivery, notification_media_type},
};
use andromeda_entity::port::{payload_pages::EntityPayloadPages, reader::EntityReader};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::entity_compactor::{EntityCompactor, EntityRead};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::NotificationId};
use andromeda_ngsi_ld_model::{
    notification::{Notification, SysAttrsSnapshot},
    subscription::Subscription,
};
use andromeda_ngsi_ld_render::{
    geometry_selection::GeometrySelection,
    notification_conformance::amend_notification_entities,
    notification_data::notification_data,
    notification_entity::TargetSpelling,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_subscription::{
    compilation::notification_projection,
    notification_slot::{NotificationSlot, NotificationSlotGate},
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tracing::{debug, error};

const INTERVAL_NOTIFICATION_CONCURRENCY: usize = 20;

/// Bounded evaluation of due entity interval subscriptions.
pub struct IntervalNotificationOperation {
    target: ContextTarget,
    sub_repo: Arc<dyn SubscriptionStore>,
    entity_reader: Arc<dyn EntityReader>,
    entity_pages: Arc<dyn EntityPayloadPages>,
    transport: Arc<TransportDispatcher>,
    engine: JsonLdEngine,
    tenant: TenantRef,
}

impl IntervalNotificationOperation {
    /// Construct interval evaluation over focused entity, subscription, and delivery capabilities.
    pub const fn new(
        target: ContextTarget,
        sub_repo: Arc<dyn SubscriptionStore>,
        entity_reader: Arc<dyn EntityReader>,
        entity_pages: Arc<dyn EntityPayloadPages>,
        engine: JsonLdEngine,
        transport: Arc<TransportDispatcher>,
        tenant: TenantRef,
    ) -> Self {
        Self {
            target,
            sub_repo,
            entity_reader,
            entity_pages,
            transport,
            engine,
            tenant,
        }
    }

    /// Evaluate every interval subscription due at the execution boundary.
    ///
    /// Every due subscription's grid anchor is advanced past `cutoff` before this
    /// returns, whether or not it had anything to send: a subscription that
    /// matched nothing still ticked, so its anchor moves and the sweep stops
    /// re-reading it every pass (the clause 5.8.6 defect where a matches-nothing
    /// subscription re-ran a paged scan every second).
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
                // Fetch entities matching the subscription's entity selectors
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

                // Apply q/geoQ/scopeQ filters
                let entities = apply_subscription_filters(&sub, entities, Utc::now());

                if entities.is_empty() {
                    // Clause 5.8.6: no matching entities means no notification.
                    // The anchor still advances so the interval is not re-read
                    // every pass until it next comes due.
                    this.advance_anchor(&sub, cutoff).await;
                    return;
                }

                let data = match this.prepare_notification_entities(&sub, entities).await {
                    Ok(d) => d,
                    Err(e) => {
                        error!("IntervalWorker failed to build notification payload for {}: {}", sub.id, e);
                        return;
                    }
                };

                if data.is_empty() {
                    this.advance_anchor(&sub, cutoff).await;
                    return;
                }

                // Claim slot only once we know we are actually going to send a notification.
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

                let Some(payload) = notification_payload(&sub, data) else {
                    return;
                };

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
                let request = NotificationDelivery::builder()
                    .payload(&payload)
                    .endpoint_uri(sub.notification.endpoint.uri.url())
                    .accept(sub.notification.endpoint.accept.as_ref())
                    .receiver_info(sub.notification.endpoint.receiver_info.as_deref())
                    .context(&context)
                    .tenant(this.tenant.tenant_name())
                    .snapshot(this.target.snapshot().map(SnapshotTarget::snapshot_id))
                    .timeout(sub.notification.endpoint.timeout)
                    .subscription_id(&sub.id)
                    .build()
                    .assemble();

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

    /// Advance a due subscription's grid anchor past `cutoff`, logging a store
    /// failure rather than aborting the sweep.
    async fn advance_anchor(&self, sub: &Subscription, cutoff: DateTime<Utc>) {
        if let Err(error) = self.sub_repo.advance_interval_anchor(&self.target, &sub.id, cutoff).await {
            debug!("Failed to advance interval anchor for {}: {}", sub.id, error);
        }
    }

    /// Render the entities of one periodic notification the way clause 5.8.6
    /// prescribes: narrowed to `notification.attributes`, reduced by `pick`
    /// and `omit`, compacted under the subscription's context and rendered
    /// in its `notification.format`.
    async fn prepare_notification_entities(&self, sub: &Subscription, entities: Vec<EntityState>) -> Result<Vec<Value>, NotificationError> {
        let projection = notification_projection(&sub.notification, sub.id.as_ref());
        let rule = SelectionRule {
            attributes: sub.notification.attributes.as_deref(),
            dataset_ids: None,
            projection: &projection,
        };
        // Table 5.2.14-1: without `sysAttrs` a notification carries no clause
        // 4.8 system attribute, the instances' included.
        let sys_attrs = match sub.notification.sys_attrs {
            Some(true) => SysAttrsInclusion::Include,
            Some(false) | None => SysAttrsInclusion::Strip,
        };
        let instants = self.entity_instants(&entities, sys_attrs, rule.entity_members()).await;
        let context = ContextSpec::for_subscription_context(sub.delivery.jsonld_context.as_ref());
        let context_compactor = self.engine.compactor(&context);
        let compactor = EntityCompactor::new(&context_compactor);
        let read = EntityRead {
            id_override: None,
            sys_attrs,
            now: Utc::now(),
        };
        let format = sub.notification.format.unwrap_or_default();
        let mut results = Vec::with_capacity(entities.len());
        for entity in &entities {
            let selected = Selection::of(entity, rule).apply(entity);
            let compacted = compact_notified_state(&compactor, &self.engine, &selected, &read).await?;
            let stamped = notified_instants(instants.get(entity.id()), ExpiryStamp::Written, None);
            // A periodic notification spells its Relationship targets as the
            // store holds them.
            results.push(rendered_entity(&compacted, &stamped, format, TargetSpelling::AsStored)?);
        }

        // Clause 5.8.6 places the `ngsildConformance` amendment last, after the
        // format rendering: a periodic notification is sent the same way a
        // change-driven one is, so it is amended the same way too.
        Ok(amend_notification_entities(results, sub.delivery.ngsild_conformance, format))
    }

    /// The clause 4.8 system attributes of every entity one periodic
    /// notification carries, read in one round trip when the subscription asks
    /// for them.
    ///
    /// A read that fails leaves the entities without them rather than failing
    /// the notification.
    async fn entity_instants(&self, entities: &[EntityState], sys_attrs: SysAttrsInclusion, members: EntityMembers) -> HashMap<Identifier, SysAttrsSnapshot> {
        match (sys_attrs, members) {
            (SysAttrsInclusion::Include, EntityMembers::Kept) => (),
            (SysAttrsInclusion::Strip, EntityMembers::Kept | EntityMembers::Dropped) | (SysAttrsInclusion::Include, EntityMembers::Dropped) => {
                return HashMap::new();
            }
        }
        // clone: the batch read takes the identifiers as one slice
        let ids: Vec<Identifier> = entities.iter().map(|entity| entity.id().clone()).collect();
        match self.entity_reader.get_sys_attrs_batch(&self.target, &ids).await {
            Ok(instants) => instants,
            Err(error) => {
                debug!("Failed to fetch the system attributes of a periodic notification: {}", error);
                HashMap::new()
            }
        }
    }
}

/// The Notification (Table 5.3.1-1) carrying `data` for `sub`, serialised, or
/// `None` -- logged -- when it cannot be built.
///
/// An `application/geo+json` endpoint receives the entities as one
/// `FeatureCollection`, as Table 5.3.1-1 requires.
fn notification_payload(sub: &Subscription, data: Vec<Value>) -> Option<Value> {
    let Ok(notification_id) = NotificationId::mint().map(Identifier::from) else {
        error!("Failed to construct notification URN for {}", sub.id);
        return None;
    };
    let data = match notification_data(
        data,
        notification_media_type(sub.notification.endpoint.accept.as_ref()),
        GeometrySelection::new(None, sub.filters.dataset_id.as_deref().unwrap_or_default()),
    ) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to render the notification data for {}: {}", sub.id, e);
            return None;
        }
    };
    let notification = Notification::builder()
        .id(notification_id)
        .subscription_id(sub.id.clone()) // clone: the notification owns its subscription id
        .notified_at(Utc::now())
        .data(data)
        .build();
    match serde_json::to_value(&notification) {
        Ok(payload) => Some(payload),
        Err(e) => {
            error!("Failed to serialize notification for {}: {}", sub.id, e);
            None
        }
    }
}

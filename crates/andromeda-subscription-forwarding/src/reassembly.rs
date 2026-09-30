//! Re-assembling an inbound notification for the local subscriber (clause 5.8.6).
//!
//! A notification arriving under a forwarded copy's identifier is copied for
//! the local subscription. When entities may be split, each entity in its
//! `data` is completed from this broker's own copy and from every other
//! registered source that holds it — never the sender, which already supplied
//! its part — and the entities that fail the local subscription's filters are
//! dropped. Whatever remains is delivered to the local endpoint under the local
//! subscription's identifier.

use crate::{error::SubscriptionForwardingError, forwarding_invariant::ForwardingInvariant, service::SubscriptionForwardingService};
use andromeda_delivery::{
    endpoint::transport::parse_endpoint_transport,
    notification_context::NotificationContext,
    request::{NotificationDelivery, notification_media_type},
};
use andromeda_entity::{
    error::EntityError,
    query::{
        attribute_expansion::AttributeExpansions,
        attribute_projection::{filter_entity_attrs, project_entity},
        retrieval::RetrievalNarrowing,
    },
};
use andromeda_federation::{
    config::SplitEntities,
    entity_result::merge_retrieve_results,
    forwarded_query::client_query::ClientQuery,
    outcome::ForwardedEntities,
    request::{CsrModeFilter, EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{
        context_compactor::ContextCompactor,
        expanded_input::ExpandedInput,
        options::{CompactOptions, ContextStripping},
    },
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::NotificationId};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    notification::{Notification, NotificationData, NotificationType},
    registration::forwarding_mode::ForwardingMode,
};
use andromeda_ngsi_ld_render::{
    entity::EntityRenderer,
    geometry_selection::GeometrySelection,
    notification_conformance::amend_notification_entities,
    notification_data::notification_data,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::entity::GetEntityParams;
use andromeda_subscription::{
    compilation::compile_subscription,
    compiled::CompiledSubscription,
    matching::matches_entity_filters,
    notification_slot::{NotificationSlot, NotificationSlotGate},
};
use chrono::Utc;
use iri_rs::IriBuf;
use serde_json::Value;
use tracing::{debug, warn};

/// Mint a notification identifier.
fn notification_id() -> Result<Identifier, SubscriptionForwardingError> {
    NotificationId::mint()
        .map(Identifier::from)
        .map_err(|error| ForwardingInvariant::MintedNotificationId(error).into())
}

impl SubscriptionForwardingService {
    /// Complete one entity of the notification from this broker and from every
    /// other holder (clause 5.8.6 split entities).
    async fn reassemble_entity(&self, received: Entity, sender: &Identifier, context: &ContextSpec) -> Result<Entity, SubscriptionForwardingError> {
        let narrowing = RetrievalNarrowing {
            entity_type: None,
            attrs: &[],
            sys_attrs: SysAttrsInclusion::Strip,
            dataset_ids: &[],
            projection: None,
        };
        let local = match self.entity_query.get_inner(&self.live_target, &received.id, &narrowing, context).await {
            Ok(entity) => Some(entity),
            Err(EntityError::NotFound { .. }) => None,
            Err(error) => return Err(error.into()),
        };
        // The whole entity is wanted from every other holder: no parameter of
        // a retrieval narrows what the re-assembly completes the part with.
        let whole_entity = GetEntityParams::default();
        let remote = self
            .federation
            .forward_retrieve(
                &self.live_target,
                ForwardRetrieveRequest {
                    entity_id: &received.id,
                    exclude_registration: Some(sender),
                    client_query: ClientQuery::EntityRetrieve {
                        id: &received.id,
                        params: &whole_entity,
                    },
                    context,
                    triggering: TriggeringRequest::default(),
                    mode_filter: CsrModeFilter::All,
                    entity_map: EntityMapInUse::None,
                },
            )
            .await?;
        // The received part is the base; the local copy and the other holders
        // are folded in the way a distributed retrieval merges its sources.
        let mut sources = Vec::with_capacity(remote.results.len() + 1);
        if let Some(local) = local {
            sources.push(ForwardedEntities {
                registration_id: None,
                mode: ForwardingMode::Inclusive,
                entities: vec![local],
                warnings: Vec::new(),
                linked_map: None,
            });
        }
        sources.extend(remote.results);
        merge_retrieve_results(Some(received), sources).ok_or_else(|| ForwardingInvariant::MergeProducedNothing.into())
    }

    /// One entity of the inbound notification, re-spelled from the sender's
    /// context (`inbound`) into the local subscription's, the one `local`
    /// compacts against.
    async fn received_under(&self, value: &Value, inbound: &ContextSpec, local: &ContextCompactor<'_>) -> Result<Entity, SubscriptionForwardingError> {
        // clone: expansion consumes the value while the notification stays borrowed
        let expanded = self.engine.expand(value.clone(), inbound).await?;
        let compacted = local
            .compact(
                ExpandedInput::Canonical(expanded),
                CompactOptions {
                    strip_context: ContextStripping::Strip,
                },
            )
            .await?;
        Ok(serde_json::from_value(compacted)?)
    }

    /// Handle a notification arriving under a forwarded copy's identifier.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionForwardingError::UnknownSubscription`] when the
    /// notification's identifier maps to no local subscription. Propagates the
    /// mapping-store, subscription-store, JSON-LD, local entity read and
    /// federation failures raised while re-assembling the entities, and
    /// [`SubscriptionForwardingError::Internal`] when a notification identifier
    /// cannot be minted, an entity cannot be rendered, or the local
    /// subscription's endpoint names no deliverable transport.
    pub async fn handle_notification(&self, notification: &Notification, context: &ContextSpec) -> Result<(), SubscriptionForwardingError> {
        let Some((local, sender)) = self.mappings.local_of_forwarded(&notification.subscription_id).await? else {
            return Err(SubscriptionForwardingError::UnknownSubscription {
                subscription: notification.subscription_id.clone(), // clone: the error owns the identifier
            });
        };
        // Every forwarded copy asks its source for `application/json`, so its
        // notifications carry an Entity array (Table 5.3.1-1); a
        // FeatureCollection names no Entity this copy can be re-assembled from.
        let NotificationData::Entities(received_entities) = &notification.data else {
            warn!(subscription = %local, "inbound notification carried a FeatureCollection its forwarded copy did not ask for");
            return Ok(());
        };
        let Some(stored) = self.subscriptions.get(&self.live_target, &local).await? else {
            debug!(subscription = %local, "notification for a forwarded copy whose local subscription no longer exists");
            return Ok(());
        };
        let compiled = compile_subscription(&stored);
        // Clause 5.2.12 `throttling` and Table 5.2.15-1 `cooldown` gate every
        // notification the local subscription sends, and a re-assembled copy is
        // no less one of them: it goes to the subscriber's own endpoint, under
        // the local subscription's own identifier, and it writes the same
        // `last_failure` and `last_notification` that suppress the
        // subscription's other notifications. Table 5.2.15-1 admits no
        // exception for where the notification came from -- "if requests are
        // received before the cooldown period has expired, no notification is
        // sent" -- so the slot is claimed here, before the re-assembly work,
        // exactly as the locally-triggered path claims it before building.
        let gate = NotificationSlotGate::at(Utc::now(), compiled.throttling_secs, compiled.endpoint_cooldown);
        match self.subscriptions.claim_notification_slot(&self.live_target, &local, gate).await? {
            NotificationSlot::Claimed => {}
            NotificationSlot::Suppressed => {
                debug!(subscription = %local, "re-assembled notification suppressed by the subscription's throttling or its endpoint's cooldown");
                return Ok(());
            }
        }
        let subscription = stored.subscription;
        let split = self.federation.split_entities(subscription.delivery.split_entities);
        // Clause 5.8.6: the copy is delivered under the local subscription's
        // `jsonldContext`. The sender spelled its part under the context of the
        // copy it holds, and the other holders answer under whatever a
        // retrieval asks for, so every part is read under the local context
        // before the parts are merged and the result is rendered.
        let local_context = ContextSpec::for_subscription_context(subscription.delivery.jsonld_context.as_ref());

        let format = subscription.notification.format.unwrap_or_default();
        let local_compactor = self.engine.compactor(&local_context);
        let local_terms = self.engine.prepare_term_context(&local_context);
        let mut local_expansions = AttributeExpansions::new(&self.engine, &local_terms);
        let wanted = notification_attribute_iris(&compiled);
        let mut data = Vec::with_capacity(received_entities.len());
        for value in received_entities {
            let received = match self.received_under(value, context, &local_compactor).await {
                Ok(entity) => entity,
                Err(error) => {
                    warn!(subscription = %local, %error, "inbound notification carried an entity that does not parse");
                    continue;
                }
            };
            let mut entity = match split {
                SplitEntities::Split => self.reassemble_entity(received, &sender, &local_context).await?,
                SplitEntities::Complete => received,
            };
            // Clause 5.8.6: entities failing the local filters leave the copy.
            let expanded = self.engine.expand(serde_json::to_value(&entity)?, &local_context).await?;
            if !matches_entity_filters(&compiled, &expanded) {
                continue;
            }
            // `id` and `type` stay whatever the projection lists: Table 5.2.4-1
            // makes both mandatory, so the copy's `data` carries Entities.
            narrow_entity(&mut entity, &compiled, wanted.as_deref(), &mut local_expansions).await?;
            let rendered = EntityRenderer::new(&entity)
                .with_mode(format)
                .render()
                .map_err(ForwardingInvariant::Rendering)?;
            data.push(rendered);
        }
        // Clause 5.8.6: the copy is sent to the local subscription's endpoint,
        // so it is the local subscription's `ngsildConformance` that the copy's
        // Entities are amended to -- after the format rendering, which is where
        // the clause places the amendment.
        let data = amend_notification_entities(data, subscription.delivery.ngsild_conformance, format);
        if data.is_empty() {
            debug!(subscription = %local, "re-assembled notification carries no entity; nothing delivered");
            return Ok(());
        }

        let endpoint = &subscription.notification.endpoint;
        // Table 5.3.1-1: the local endpoint's `accept` decides the shape of the
        // copy's `data`, whatever shape the source sent.
        let data = notification_data(
            data,
            notification_media_type(endpoint.accept.as_ref()),
            GeometrySelection::new(None, subscription.filters.dataset_id.as_deref().unwrap_or_default()),
        )
        .map_err(ForwardingInvariant::Rendering)?;
        let copy = Notification {
            id: notification_id()?,
            type_: NotificationType,
            subscription_id: local.clone(), // clone: the copy names the local subscription
            notified_at: Utc::now(),
            data,
        };
        let payload = serde_json::to_value(&copy)?;
        let transport = parse_endpoint_transport(endpoint.uri.url(), endpoint.notifier_info.as_deref()).map_err(ForwardingInvariant::LocalEndpoint)?;
        let context = NotificationContext::resolve(subscription.delivery.jsonld_context.as_ref(), &self.engine).await?;
        let request = NotificationDelivery::builder()
            .payload(&payload)
            .endpoint_uri(endpoint.uri.url())
            .accept(endpoint.accept.as_ref())
            .receiver_info(endpoint.receiver_info.as_deref())
            .context(&context)
            .tenant(self.tenant.tenant_name())
            .timeout(endpoint.timeout)
            .subscription_id(&local)
            .build()
            .assemble();
        let result = self.transport.deliver(&transport, &request).await;
        if result.is_delivered() {
            self.subscriptions.mark_success(&self.live_target, &local).await?;
        } else {
            self.subscriptions.mark_failure(&self.live_target, &local).await?;
        }
        Ok(())
    }
}

/// The attributes the local subscription's `notification.attributes` names,
/// as the IRIs a narrowing compares against.
///
/// Every stored name is an absolute IRI because creation expanded it
/// (clause 5.5.7); one that is not names no attribute.
fn notification_attribute_iris(compiled: &CompiledSubscription) -> Option<Vec<IriBuf>> {
    compiled
        .notification_attributes
        .as_deref()
        .map(|attributes| attributes.iter().filter_map(|name| IriBuf::new(name.as_str().to_owned()).ok()).collect())
}

/// Narrow one re-assembled entity to what the local subscription delivers
/// (clause 5.8.6 `notification.attributes`, `pick` and `omit`) -- the same
/// clause 4.21 projection a query applies, so a subscriber sees the entity a
/// query under the same selection would show.
async fn narrow_entity(
    entity: &mut Entity,
    compiled: &CompiledSubscription,
    wanted: Option<&[IriBuf]>,
    expansions: &mut AttributeExpansions<'_>,
) -> Result<(), SubscriptionForwardingError> {
    if let Some(wanted) = wanted {
        filter_entity_attrs(entity, wanted, expansions).await?;
    }
    project_entity(entity, &compiled.notification_projection, expansions).await?;
    Ok(())
}

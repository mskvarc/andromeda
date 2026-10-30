//! Re-assembling an inbound notification for the local subscriber (clause 5.8.6).
//!
//! A notification arriving under a forwarded copy's identifier is copied for
//! the local subscription. Each entity in its `data` is read into the model
//! under the context it was written in; when entities may be split, it is
//! completed on the model from this broker's own copy and from every other
//! registered source that holds it -- never the sender, which already supplied
//! its part -- and the entities that fail the local subscription's filters are
//! dropped. Whatever remains is written the way a notification of the local
//! subscription writes a local entity, with the previous values the sender
//! reported when the local subscription asks for them (`showChanges`), and
//! delivered to the local endpoint under the local subscription's identifier.

use crate::{
    error::SubscriptionForwardingError,
    forwarding_invariant::ForwardingInvariant,
    inbound::{InboundData, InboundNotification},
    notification_narrowing::narrow_entity,
    received_entity::{NotifiedPart, read_notified},
    relayed_entities::RelayedEntities,
    service::SubscriptionForwardingService,
};
use andromeda_delivery::{endpoint::transport::parse_endpoint_transport, notification_context::NotificationContext, request::NotificationDelivery};
use andromeda_entity::{
    error::EntityError,
    query::{attribute_expansion::AttributeExpansions, retrieval::RetrievalNarrowing},
};
use andromeda_federation::{
    entity_result::merge_retrieve_results,
    forwarded_query::client_query::ClientQuery,
    outcome::ForwardedEntities,
    request::{CsrModeFilter, EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
    source_reach::SourceReach,
    split_entities::SplitEntities,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_entity::{document::EntityDocument, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, received_state::ReceivedDocument};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    entity::Entity,
    received::{ContextualEntity, ReceivedElement},
    registration::forwarding_mode::ForwardingMode,
};
use andromeda_ngsi_ld_render::{geometry_selection::GeometrySelection, notification_data::DataShape};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::entity::GetEntityParams;
use andromeda_subscription::{
    compilation::compile_subscription,
    compiled::CompiledSubscription,
    matching::matches_entity_filters,
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate},
};
use chrono::Utc;
use tracing::{debug, warn};

impl SubscriptionForwardingService {
    /// Complete one entity of the notification from this broker and from every
    /// other holder (clause 5.8.6 split entities), on the model.
    async fn reassemble_entity(
        &self,
        part: ReceivedDocument,
        sender: &Identifier,
        context: &ContextSpec,
    ) -> Result<Option<ReceivedDocument>, SubscriptionForwardingError> {
        let NotifiedPart { version, deletions, previous } = match NotifiedPart::of(part) {
            Ok(part) => part,
            Err(refusal) => {
                warn!(%refusal, "inbound notification carried a part that cannot be completed");
                return Ok(None);
            }
        };
        let id = version.state.id();
        let narrowing = RetrievalNarrowing {
            entity_type: None,
            attrs: &[],
            sys_attrs: SysAttrsInclusion::Strip,
            dataset_ids: &[],
            projection: None,
        };
        let local = match self.entity_query.read_held(&self.live_target, id, &narrowing, context).await {
            Ok(held) => Some(held),
            Err(EntityError::NotFound { .. }) => None,
            Err(error) => return Err(error.into()),
        };
        // The whole entity is wanted from every other holder: no parameter of
        // a retrieval narrows what the re-assembly completes the part with.
        let whole_entity = GetEntityParams::default();
        let request = ForwardRetrieveRequest {
            entity_id: id,
            reach: SourceReach::AllBut(sender),
            client_query: ClientQuery::EntityRetrieve { id, params: &whole_entity },
            context,
            triggering: TriggeringRequest::default(),
            mode_filter: CsrModeFilter::All,
            entity_map: EntityMapInUse::None,
        };
        let matched = self.federation.match_retrieve(&self.live_target, &request).await?;
        let remote = self
            .federation
            .forward_retrieve(&self.live_target, request, matched.into_registrations())
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
        let merged = merge_retrieve_results(Some(version), sources).ok_or(ForwardingInvariant::MergeProducedNothing)?;
        Ok(Some(NotifiedPart::completed(merged, deletions, previous)))
    }

    /// Whether the local subscription's filters admit `document`, the entities
    /// a `rel{…}` hop crosses into read from this broker's store (clause 4.9).
    async fn admits(&self, compiled: &CompiledSubscription, document: &EntityDocument) -> Result<bool, SubscriptionForwardingError> {
        let filters: Vec<_> = compiled.hopping_filters().collect();
        let linked = if filters.is_empty() {
            LinkedStates::new()
        } else {
            self.entity_query
                .linked_filter_states(&self.live_target, &[document], &filters, Utc::now())
                .await?
        };
        Ok(matches_entity_filters(compiled, document, &linked))
    }

    /// Every entity of an inbound notification re-assembled for the local
    /// subscription, in the local subscription's terms; an entity that cannot
    /// be read, or that the local filters drop, leaves the copy (clause 5.8.6).
    async fn reassembled_entities(
        &self,
        received: &[ReceivedElement<ContextualEntity>],
        reading: &ReassemblyReading<'_, '_>,
        sender: &Identifier,
        local: &Identifier,
    ) -> Result<RelayedEntities, SubscriptionForwardingError> {
        let local_terms = self.engine.prepare_term_context(reading.local_context);
        let mut local_expansions = AttributeExpansions::new(&self.engine, &local_terms);
        let mut data = RelayedEntities::with_capacity(received.len());
        for element in received {
            let contextual = match element {
                ReceivedElement::Read(contextual) => contextual,
                ReceivedElement::Refused(refusal) => {
                    warn!(subscription = %local, %refusal, "inbound notification carried an entity that is refused");
                    continue;
                }
            };
            let part = match read_notified(&self.engine, reading.inbound, contextual).await {
                Ok(part) => part,
                Err(refusal) => {
                    warn!(subscription = %local, %refusal, "inbound notification carried an entity that does not read");
                    continue;
                }
            };
            let completed = match reading.completion {
                SplitEntities::Complete => Some(part),
                SplitEntities::Split => self.reassemble_entity(part, sender, reading.local_context).await?,
            };
            // Clause 5.8.6: entities failing the local filters leave the copy.
            let Some(ReceivedDocument {
                document,
                system_times,
                previous,
            }) = completed
            else {
                continue;
            };
            if !self.admits(reading.compiled, &document).await? {
                continue;
            }
            let mut entity = match reading.local_compactor.document_entity(&document).await {
                Ok(entity) => entity,
                Err(error) => {
                    warn!(subscription = %local, %error, "a re-assembled entity does not compact under the local context");
                    continue;
                }
            };
            system_times.restore(&mut entity);
            // `id` and `type` stay whatever the projection lists: Table 5.2.4-1
            // makes both mandatory, so the copy's `data` carries Entities.
            narrow_entity(&mut entity, reading.compiled, &mut local_expansions).await?;
            let previous = self.relayed_previous(previous.as_ref(), reading, local).await;
            data.push(entity, previous);
        }
        Ok(data)
    }

    /// The instances a relayed entity held before the change, as its source
    /// reported them, in the local subscription's terms: only when the local
    /// subscription asks for them (Table 5.2.14-1 `showChanges`). Previous
    /// values that do not compact under the local context leave the entity
    /// without them, as an entity whose previous values do not exist.
    async fn relayed_previous(&self, previous: Option<&EntityDocument>, reading: &ReassemblyReading<'_, '_>, local: &Identifier) -> Option<Entity> {
        if !reading.compiled.notification_show_changes {
            return None;
        }
        match reading.local_compactor.document_entity(previous?).await {
            Ok(previous) => Some(previous),
            Err(error) => {
                warn!(subscription = %local, %error, "the previous values of a relayed entity do not compact under the local context");
                None
            }
        }
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
    pub async fn handle_notification(&self, notification: &InboundNotification, context: &ContextSpec) -> Result<(), SubscriptionForwardingError> {
        let Some((local, sender)) = self.mappings.local_of_forwarded(&notification.subscription_id).await? else {
            return Err(SubscriptionForwardingError::UnknownSubscription {
                subscription: notification.subscription_id.clone(), // clone: the error owns the identifier
            });
        };
        // Every forwarded copy asks its source for `application/json`, so its
        // notifications carry an Entity array (Table 5.3.1-1); a
        // FeatureCollection names no Entity this copy can be re-assembled from.
        let InboundData::Entities(received) = &notification.data else {
            warn!(subscription = %local, sender = %sender, "inbound notification carried a FeatureCollection its forwarded copy did not ask for");
            return Ok(());
        };
        let Some(stored) = self.subscriptions.get(&self.live_target, &local).await? else {
            debug!(subscription = %local, "notification for a forwarded copy whose local subscription no longer exists");
            return Ok(());
        };
        let compiled = compile_subscription(&stored, self.entity_query.language_targeting().matching());
        let subscription = stored.subscription;
        // Clause 5.8.6: the copy is delivered under the local subscription's
        // `jsonldContext`. The sender spelled its part under the context of the
        // copy it holds, and every part is read into the model under the
        // context it was written in before the parts are merged and the result
        // is written under the local context.
        let local_context = ContextSpec::for_subscription_context(subscription.delivery.jsonld_context.as_ref());
        let inbound = match EntityExpansion::prepare(&self.engine, context).await {
            Ok(inbound) => inbound,
            Err(error) => {
                warn!(subscription = %local, %error, "inbound notification names a context its entities cannot be read under");
                return Ok(());
            }
        };
        let context_compactor = self.engine.compactor(&local_context);
        let local_compactor = EntityCompactor::new(&context_compactor);
        let reading = ReassemblyReading {
            completion: self.federation.split_entities(subscription.delivery.split_entities),
            compiled: &compiled,
            inbound: &inbound,
            local_context: &local_context,
            local_compactor: &local_compactor,
        };
        let data = self.reassembled_entities(&received.0, &reading, &sender, &local).await?;
        if data.is_empty() {
            debug!(subscription = %local, "re-assembled notification carries no entity; nothing delivered");
            return Ok(());
        }

        let endpoint = &subscription.notification.endpoint;
        let transport = parse_endpoint_transport(endpoint.uri.url(), endpoint.notifier_info.as_deref()).map_err(ForwardingInvariant::LocalEndpoint)?;
        let context = NotificationContext::resolve(subscription.delivery.jsonld_context.as_ref(), &self.engine).await?;
        let delivery = NotificationDelivery::builder()
            .endpoint_uri(endpoint.uri.url())
            .accept(endpoint.accept)
            .receiver_info(endpoint.receiver_info.as_deref())
            .context(&context)
            .tenant(self.tenant.tenant_name())
            .timeout(endpoint.timeout)
            .subscription_id(&local)
            .build();
        let shape = DataShape {
            media_type: delivery.content_type(),
            selection: GeometrySelection::new(None, subscription.filters.dataset_id.as_deref().unwrap_or_default()),
            context: delivery.body_context(),
        };
        let written = self.written_relay(&compiled, &subscription, &local_context, &data, shape).await?;
        let request = delivery.notification(written).map_err(ForwardingInvariant::WrittenNotification)?;
        // Clause 5.2.12 `throttling` and Table 5.2.15-1 `cooldown` gate every
        // notification the local subscription sends, and a re-assembled copy is
        // no less one of them: it goes to the subscriber's own endpoint, under
        // the local subscription's own identifier, and it writes the same
        // `last_failure` and `last_notification` that suppress the
        // subscription's other notifications. The slot is claimed only once
        // the copy is ready to send, so a copy that failed before it was sent
        // spends no slot and the sender's retry is delivered.
        let gate = NotificationSlotGate::at(Utc::now(), compiled.cadence(), compiled.endpoint_cooldown);
        let record = match self.subscriptions.claim_notification_slot(&self.live_target, &local, gate).await? {
            NotificationSlot::Claimed(record) => record,
            NotificationSlot::Suppressed => {
                debug!(subscription = %local, "re-assembled notification suppressed by the subscription's throttling or its endpoint's cooldown");
                return Ok(());
            }
        };
        let outcome = if self.transport.deliver(&transport, &request).await.is_delivered() {
            NotificationOutcome::Success
        } else {
            NotificationOutcome::Failure
        };
        self.subscriptions.record_outcome(&self.live_target, &local, outcome, record).await?;
        Ok(())
    }
}

/// How the entities of one inbound notification are read for the local
/// subscription.
struct ReassemblyReading<'r, 'c> {
    /// Whether the entities are completed from their other holders.
    completion: SplitEntities,
    /// The local subscription, whose filters decide what the copy carries.
    compiled: &'r CompiledSubscription,
    /// The notification's context, its entities read under it.
    inbound: &'r EntityExpansion<'c>,
    /// The local subscription's context.
    local_context: &'r ContextSpec,
    /// The compactor of the local subscription's context.
    local_compactor: &'r EntityCompactor<'c>,
}

//! Entity event notification evaluation: which compiled subscriptions a
//! committed change reaches, and the delivery of one notification to each.

/// The entity document a change event is notified with.
mod change_payload;
/// Delivery of a durably queued event through the processor.
mod durable_delivery;
pub(crate) mod payload;

use crate::{error::NotificationError, processor::change_payload::entity_data_for};
use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{
    dispatch::{DeliveryResult, TransportDispatcher},
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
    request::NotificationDelivery,
};
use andromeda_entity::{port::reader::EntityReader, query::EntityQueryService, sys_attrs::inject_sys_attrs};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_ngsi_ld_model::notification::{Notification, NotificationTrigger, NotificationType};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change::SubscriptionChange,
    compiled::CompiledSubscription,
    matching::matches_entity_filters,
    spatial_narrowing::narrow_by_spatial_index,
    store::SubscriptionStore,
    throttle::ceiling_for_throttle,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use futures::{StreamExt, stream};
use payload::{NotificationBuildParams, build_notification_data, cache::NotificationCompactionCache};
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;

const NOTIFICATION_CONCURRENCY: usize = 20;

fn compiled_transport(subscription: &CompiledSubscription) -> Option<EndpointTransport> {
    match parse_endpoint_transport(&subscription.endpoint_uri, subscription.endpoint_notifier_info.as_deref()) {
        Ok(transport) => Some(transport),
        Err(error) => {
            debug!(subscription = %subscription.id, %error, "Compiled subscription endpoint is no longer deliverable");
            None
        }
    }
}

/// Applies subscription and entity changes to entity-notification policy.
pub struct NotificationProcessor {
    live_target: ContextTarget,
    sub_repo: Arc<dyn SubscriptionStore>,
    entity_reader: Arc<dyn EntityReader>,
    entity_query: Arc<EntityQueryService>,
    cache: Arc<SubscriptionCache>,
    transport: Arc<TransportDispatcher>,
    engine: JsonLdEngine,
    tenant: TenantRef,
}

#[derive(Clone)]
/// Focused capabilities used by entity-notification evaluation.
pub struct NotificationProcessorDeps {
    /// Tenant live branch loaded during worker startup.
    pub live_target: ContextTarget,
    /// Entity-subscription persistence.
    pub sub_repo: Arc<dyn SubscriptionStore>,
    /// Current entity reads used to assemble notification data.
    pub entity_reader: Arc<dyn EntityReader>,
    /// Linked-entity query policy used by join projection.
    pub entity_query: Arc<EntityQueryService>,
    /// Tenant JSON-LD processor.
    pub engine: JsonLdEngine,
    /// Selected notification delivery capability.
    pub transport: Arc<TransportDispatcher>,
    /// Tenant partition for spatial matching and delivery metadata.
    pub tenant: TenantRef,
    /// Compiled subscription matching state.
    pub subscription_cache: Arc<SubscriptionCache>,
}

/// Aggregate endpoint outcomes from evaluating one committed entity change.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NotificationEvaluationOutcome {
    /// Endpoints that accepted a notification.
    pub delivered: usize,
    /// Endpoint failures worth retrying.
    pub retryable_failures: usize,
    /// Endpoint or payload failures that retry cannot repair.
    pub terminal_failures: usize,
}

impl NotificationProcessor {
    /// Construct event evaluation from its focused capabilities.
    #[must_use]
    pub fn new(deps: NotificationProcessorDeps) -> NotificationProcessor {
        NotificationProcessor {
            live_target: deps.live_target,
            sub_repo: deps.sub_repo,
            entity_reader: deps.entity_reader,
            entity_query: deps.entity_query,
            cache: deps.subscription_cache,
            transport: deps.transport,
            engine: deps.engine,
            tenant: deps.tenant,
        }
    }

    /// Reload compiled subscriptions after startup or an event-stream gap.
    pub async fn reload_cache(&self) -> Result<(), NotificationError> {
        self.cache.reload(&self.live_target, self.sub_repo.as_ref()).await.map_err(Into::into)
    }

    /// Apply one committed subscription lifecycle change to matching state.
    pub async fn apply_subscription_change(&self, change: SubscriptionChange) {
        // Close any active SSE/WebSocket connections when a subscription is deleted.
        if let SubscriptionChange::Deleted { ref id, .. } = change {
            self.transport.sse_registry().close(id);
            self.transport.ws_registry().close(id);
        }
        self.cache.handle_change(&change, self.sub_repo.as_ref()).await;

        // After cache update, close lingering SSE/WebSocket connections if
        // transport was changed away from the streaming type.
        if let SubscriptionChange::Upserted { ref target, ref id } = change
            && let Some(compiled) = self.cache.get(target, id)
        {
            let endpoint_transport = compiled_transport(&compiled);
            if !matches!(endpoint_transport, Some(EndpointTransport::Sse)) {
                self.transport.sse_registry().close(id);
            }
            if !matches!(endpoint_transport, Some(EndpointTransport::WebSocket)) {
                self.transport.ws_registry().close(id);
            }
        }
    }

    /// Evaluate and deliver notifications for one committed entity change.
    pub async fn apply_entity_change(&self, change: ContextChangeEvent) -> NotificationEvaluationOutcome {
        self.apply_entity_change_for(change.target, change.event, None).await
    }

    async fn apply_entity_change_for(&self, target: ContextTarget, event: ChangeEvent, subscription_id: Option<&Uri>) -> NotificationEvaluationOutcome {
        debug!(
            "Change event received: id={}, trigger={}, types={:?}, attrs={:?}",
            event.entity_id, event.trigger_reason, event.entity_types, event.changed_attributes
        );
        // A durably queued delivery may be drained by another instance of this
        // broker, which shares the store but not the in-memory cache the
        // subscription's own lifecycle changes refreshed. The subscription it
        // names is therefore re-read before matching, so a stale cache neither
        // drops a subscription created elsewhere nor notifies one deleted there.
        if let Some(subscription_id) = subscription_id {
            self.cache
                .handle_change(
                    &SubscriptionChange::Upserted {
                        target: target.clone(),      // clone: the cache refresh and delivery share the branch target.
                        id: subscription_id.clone(), // clone: the change names the identifier the delivery carries.
                    },
                    self.sub_repo.as_ref(),
                )
                .await;
        }
        let outcome = self.cache.find_matching(&target, &event);
        debug!(
            entity = %event.entity_id,
            trigger = %event.trigger_reason,
            active = outcome.funnel.active,
            routed = outcome.funnel.routed,
            watched = outcome.funnel.watched,
            evaluated = outcome.funnel.evaluated,
            matched = outcome.funnel.matched,
            "Subscription candidate funnel"
        );
        outcome.funnel.record();
        let mut matching = outcome.matched;
        if let Some(subscription_id) = subscription_id {
            matching.retain(|subscription| &subscription.id == subscription_id);
        }

        if matching.is_empty() {
            debug!("No matching subscriptions for event {} (trigger={})", event.entity_id, event.trigger_reason);
            return NotificationEvaluationOutcome::default();
        }
        debug!("Found {} matching subscriptions for event {}", matching.len(), event.entity_id);

        // Fetch entity data once for all matching subscriptions
        let entity_data = entity_data_for(self.entity_reader.as_ref(), &target, &event).await;

        let filter_data = if matches!(event.trigger_reason, NotificationTrigger::EntityDeleted | NotificationTrigger::AttributeDeleted) {
            event.previous_attributes.as_ref().or(event.entity_snapshot.as_ref()).map(AsRef::as_ref)
        } else {
            entity_data.as_ref()
        };
        let matching: Vec<Arc<CompiledSubscription>> = if let Some(data) = filter_data {
            let geo_filtered = narrow_by_spatial_index(&self.cache, &target, matching, data, &event.entity_types);
            geo_filtered.into_iter().filter(|sub| matches_entity_filters(sub, data)).collect()
        } else {
            matching
        };

        if matching.is_empty() {
            return NotificationEvaluationOutcome::default();
        }

        let need_sys_attrs = matching.iter().any(|sub| sub.notification_sys_attrs == SysAttrsInclusion::Include);

        let sys_attrs = if need_sys_attrs {
            let fallback_sys_attrs = event.sys_attrs_snapshot.clone();
            if matches!(event.trigger_reason, NotificationTrigger::EntityDeleted) {
                fallback_sys_attrs
            } else {
                match self.entity_reader.get_sys_attrs(&target, &event.entity_id).await {
                    Ok(Some(attrs)) => Some(attrs),
                    Ok(None) => fallback_sys_attrs,
                    Err(e) => {
                        debug!("Failed to fetch sysAttrs for {}: {}", event.entity_id, e);
                        fallback_sys_attrs
                    }
                }
            }
        } else {
            None
        };

        let event = Arc::new(event);
        let compaction_cache = NotificationCompactionCache::default();
        let results: Vec<Option<DeliveryResult>> = stream::iter(matching)
            .map(|compiled_sub| {
                let event = Arc::clone(&event);
                let target = target.clone(); // clone: every concurrent notification resolves linked state in the event's branch.
                let mut entity_data = entity_data.clone();
                let sys_attrs = sys_attrs.clone();
                let compaction_cache = &compaction_cache;
                async move {
                    let sub_id = compiled_sub.id.clone();

                    match self
                        .sub_repo
                        .claim_notification_slot(&target, &sub_id, ceiling_for_throttle(Utc::now(), compiled_sub.throttling_secs))
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(e) => {
                            debug!("Failed to claim notification slot for {}: {}", sub_id, e);
                            return Some(DeliveryResult::RetryableFailure);
                        }
                    }

                    if compiled_sub.notification_sys_attrs == SysAttrsInclusion::Include
                        && let (Some(ref mut data), Some(sa)) = (entity_data.as_mut(), &sys_attrs)
                    {
                        inject_sys_attrs(data, sa.created_at, sa.modified_at, sa.expires_at);
                    }

                    let data = match entity_data {
                        Some(ref d) => {
                            build_notification_data(NotificationBuildParams {
                                target: &target,
                                engine: &self.engine,
                                entity_data: d,
                                previous_attributes: event.previous_attributes.as_deref(),
                                sub: &compiled_sub,
                                trigger_reason: &event.trigger_reason,
                                changed_attributes: &event.changed_attributes,
                                changed_dataset_id: event.changed_dataset_id.as_ref(),
                                occurred_at: event.occurred_at,
                                entity_query: &self.entity_query,
                                compaction_cache: Some(compaction_cache),
                            })
                            .await
                        }
                        None => return Some(DeliveryResult::TerminalFailure),
                    };
                    if data.is_empty() {
                        debug!("Subscription {} produced empty notification data for entity {}", sub_id, event.entity_id);
                        return Some(DeliveryResult::TerminalFailure);
                    }

                    let notification_id = if let Ok(u) = format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Uri>() { u } else {
                        error!("Failed to construct notification URN for {}", sub_id);
                        return Some(DeliveryResult::TerminalFailure);
                    };
                    // clone: the builder takes the id by value and `sub_id` is still needed for delivery
                    let sub_uri = sub_id.clone();
                    let notification = Notification::builder()
                        .id(notification_id)
                        .type_(NotificationType)
                        .subscription_id(sub_uri)
                        .notified_at(Utc::now())
                        .data(data)
                        .build();

                    let notification_payload = match serde_json::to_value(&notification) {
                        Ok(v) => v,
                        Err(e) => {
                            error!("Failed to serialize notification for {}: {}", sub_id, e);
                            return Some(DeliveryResult::TerminalFailure);
                        }
                    };

                    let Some(endpoint_transport) = compiled_transport(&compiled_sub) else {
                        return Some(DeliveryResult::TerminalFailure);
                    };
                    let request = (NotificationDelivery {
                        payload: &notification_payload,
                        endpoint_uri: &compiled_sub.endpoint_uri,
                        accept: compiled_sub.endpoint_accept.as_ref(),
                        receiver_info: compiled_sub.endpoint_receiver_info.as_deref(),
                        jsonld_context: compiled_sub.jsonld_context.as_ref(),
                        tenant: self.tenant.tenant_name(),
                        snapshot: target.snapshot().map(andromeda_context_space::snapshot_target::SnapshotTarget::snapshot_id),
                        subscription_id: &sub_id,
                    })
                    .assemble();
                    let delivery = self.transport.deliver(&endpoint_transport, &request).await;

                    if delivery.is_delivered() {
                        if let Err(e) = self.sub_repo.mark_success(&target, &sub_id).await {
                            debug!("Failed to mark notification success for {}: {}", sub_id, e);
                        }
                    } else if let Err(e) = self.sub_repo.mark_failure(&target, &sub_id).await {
                        debug!("Failed to mark notification failure for {}: {}", sub_id, e);
                    }
                    Some(delivery)
                }
            })
            .buffer_unordered(NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
        results
            .into_iter()
            .flatten()
            .fold(NotificationEvaluationOutcome::default(), |mut outcome, delivery| {
                if delivery.is_delivered() {
                    outcome.delivered += 1;
                } else if delivery.should_retry() {
                    outcome.retryable_failures += 1;
                } else {
                    outcome.terminal_failures += 1;
                }
                outcome
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::payload::filtering::filter_entity_attributes;
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_ngsi_ld_model::{
        notification::ChangeOrigin,
        subscription::{EntitySelector, SubscriptionStatus},
    };
    use andromeda_ngsi_ld_query::{filter::parser, projection::compiled::Projection, scope::query::ScopeQuery};
    use andromeda_subscription::{compilation::compile_geo_q, matching::matches_subscription};
    use chrono::DateTime;
    use serde_json::Value;
    use std::num::NonZeroU32;
    use url::Url;

    fn make_event(entity_id: &str, types: Vec<&str>, trigger: NotificationTrigger, attrs: Vec<&str>) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id(entity_id.parse().expect("valid test URI"))
            .entity_types(types.into_iter().map(|t| t.parse().expect("valid test type name")).collect())
            .trigger_reason(trigger)
            .changed_attributes(attrs.into_iter().map(|a| a.parse().expect("valid test attribute name")).collect())
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build()
    }

    fn make_sub(id: &str, selectors: Vec<EntitySelector>, triggers: Vec<&str>, watched: Option<Vec<&str>>) -> CompiledSubscription {
        CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{id}").parse().expect("a valid subscription URI"),
            key: SubscriptionKey::try_from(1).expect("a valid subscription key"),
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").expect("a valid endpoint URI"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            triggers: triggers.into_iter().map(|t| t.parse().expect("valid trigger")).collect(),
            watched_attrs: watched.map(|attributes| {
                attributes
                    .into_iter()
                    .map(|attribute| attribute.parse().expect("valid test attribute name"))
                    .collect()
            }),
            selectors,
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: None,
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            expires_at: None,
            created_at: DateTime::<Utc>::MIN_UTC,
            compiled_q: None,
            compiled_geo_q: None,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            notification_projection: Projection::default(),
        }
    }

    #[test]
    fn matches_by_type_and_trigger() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_wrong_type() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Parking".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_wrong_trigger() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["entityCreated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_watched_attributes() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            Some(vec!["speed"]),
        );
        let event = make_event(
            "urn:ngsi-ld:Vehicle:A1",
            vec!["Vehicle"],
            NotificationTrigger::AttributeUpdated,
            vec!["speed", "location"],
        );
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_unwatched_attribute() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            Some(vec!["speed"]),
        );
        let event = make_event(
            "urn:ngsi-ld:Vehicle:A1",
            vec!["Vehicle"],
            NotificationTrigger::AttributeUpdated,
            vec!["location"],
        );
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_with_id_pattern() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: Some("urn:ngsi-ld:Vehicle:.*".parse().expect("valid pattern")),
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_id_pattern_mismatch() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: Some("urn:ngsi-ld:Parking:.*".parse().expect("valid pattern")),
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_specific_entity_id() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: Some("urn:ngsi-ld:Vehicle:A1".parse().expect("valid uri")),
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_different_entity_id() {
        let sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: Some("urn:ngsi-ld:Vehicle:B2".parse().expect("valid uri")),
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn expired_subscription_does_not_match() {
        let mut sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        sub.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn no_selectors_matches_all_entities() {
        let sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn paused_subscription_does_not_match() {
        let mut sub = make_sub(
            "sub1",
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".parse().expect("valid type expression"),
            }],
            vec!["attributeUpdated"],
            None,
        );
        sub.status = SubscriptionStatus::Paused;
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], NotificationTrigger::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn filter_entity_attributes_keeps_structural_keys() {
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["Vehicle"],
            "https://example.com/speed": [{"@value": 50}],
            "https://example.com/location": [{"@value": "Madrid"}]
        });
        let filtered = filter_entity_attributes(&entity, &["https://example.com/speed".parse().expect("valid test attribute name")]);
        assert!(filtered.get("@id").is_some());
        assert!(filtered.get("@type").is_some());
        assert!(filtered.get("https://example.com/speed").is_some());
        assert!(filtered.get("https://example.com/location").is_none());
    }

    fn entity_with_geo_and_scope() -> Value {
        serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}]}],
            "https://uri.etsi.org/ngsi-ld/location": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [8.684783, 49.406132]}}]}],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid/Gardens"}, {"@value": "/Spain"}]
        }])
    }

    #[test]
    fn matches_entity_filters_q_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/speed\">50").unwrap());
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_q_no_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/speed\">100").unwrap());
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_exact() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/Spain").expect("valid scope query"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_no_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/France").expect("valid scope query"));
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_hash_wildcard() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/Madrid/#").expect("valid scope query"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_and() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/Spain;/Madrid/Gardens").expect("valid scope query"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_or() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_scope_q = Some(ScopeQuery::parse("/France,/Spain").expect("valid scope query"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_geo_near() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_geo_q = compile_geo_q(
            &serde_json::json!({"georel": "near;maxDistance==100000", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"}),
            "test",
        );
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_geo_near_too_far() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_geo_q = compile_geo_q(
            &serde_json::json!({"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"}),
            "test",
        );
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_no_filters() {
        let sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }
}

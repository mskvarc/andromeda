//! Entity event notification evaluation: which compiled subscriptions a
//! committed change reaches, and the delivery of one notification to each.

/// The entity document a change event is notified with.
mod change_payload;
/// Delivery of a durably queued event through the processor.
mod durable_delivery;
pub(crate) mod payload;
/// Notifying one subscription of one committed entity change.
mod subscription_notification;

use crate::{
    error::NotificationError,
    processor::{change_payload::entity_data_for, payload::cache::NotificationCompactionCache, subscription_notification::NotifiedChange},
};
use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{
    dispatch::{DeliveryResult, TransportDispatcher},
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
};
use andromeda_entity::{port::reader::EntityReader, query::EntityQueryService};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, notification::SysAttrsSnapshot};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change::SubscriptionChange,
    change_filters::{ChangedStates, narrow_by_change_filters},
    compiled::CompiledSubscription,
    error::SubscriptionError,
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use futures::{StreamExt, stream};
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

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

/// Fold the endpoint results of one committed change into its aggregate
/// outcome; a subscription whose throttling window swallowed the notification
/// contributes nothing.
fn aggregate_delivery_outcomes(deliveries: Vec<Option<DeliveryResult>>) -> NotificationEvaluationOutcome {
    deliveries
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
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Subscription`] when the subscription store
    /// cannot list the tenant's subscriptions; the cache is then left as it was.
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

    async fn apply_entity_change_for(&self, target: ContextTarget, event: ChangeEvent, subscription_id: Option<&Identifier>) -> NotificationEvaluationOutcome {
        debug!(
            entity = %event.entity_id,
            lifecycle = ?event.change.lifecycle,
            types = ?event.entity_types,
            attributes = ?event.change.attributes(),
            "Change event received"
        );
        let candidates = self.candidate_subscriptions(&target, &event, subscription_id);
        if candidates.is_empty() {
            debug!(entity = %event.entity_id, "No matching subscriptions for the change");
            return NotificationEvaluationOutcome::default();
        }
        debug!("Found {} matching subscriptions for event {}", candidates.len(), event.entity_id);

        // The entity is read once and shared by every notification the change produces.
        let entity_data = entity_data_for(self.entity_reader.as_ref(), &target, &event).await;
        let matching = self.narrow_by_entity_filters(&target, &event, candidates, entity_data.as_ref());
        if matching.is_empty() {
            return NotificationEvaluationOutcome::default();
        }

        let sys_attrs = self.sys_attrs_for(&target, &event, &matching).await;
        let compaction_cache = NotificationCompactionCache::default();
        let change = NotifiedChange {
            target: &target,
            event: &event,
            sys_attrs: sys_attrs.as_ref(),
            compaction_cache: &compaction_cache,
        };
        let deliveries: Vec<Option<DeliveryResult>> = stream::iter(matching)
            .map(|compiled_sub| {
                // clone: each concurrent notification owns the document it injects system attributes into
                let entity_data = entity_data.clone();
                let change = &change;
                async move { self.notify_subscription(&compiled_sub, change, entity_data).await }
            })
            .buffer_unordered(NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
        aggregate_delivery_outcomes(deliveries)
    }

    /// Re-read one subscription from the store before the change is matched
    /// against it.
    ///
    /// A durably queued delivery may be drained by another instance of this
    /// broker, which shares the store but not the in-memory cache the
    /// subscription's own lifecycle changes refreshed. Reconciling first means
    /// a stale cache neither drops a subscription created elsewhere nor
    /// notifies one deleted there (clause 5.8.6).
    ///
    /// # Errors
    ///
    /// Returns the store's failure when the subscription cannot be read back;
    /// the cache is then left as it was.
    async fn refresh_cached_subscription(&self, target: &ContextTarget, subscription_id: &Identifier) -> Result<(), SubscriptionError> {
        self.cache.refresh(target, subscription_id, self.sub_repo.as_ref()).await
    }

    /// The compiled subscriptions the change reaches before entity filters are
    /// evaluated, narrowed to `subscription_id` alone when one durably queued
    /// delivery is being redelivered.
    fn candidate_subscriptions(&self, target: &ContextTarget, event: &ChangeEvent, subscription_id: Option<&Identifier>) -> Vec<Arc<CompiledSubscription>> {
        let outcome = self.cache.find_matching(target, event);
        debug!(
            entity = %event.entity_id,
            triggers = ?event.change.triggers(),
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
        matching
    }

    /// Narrow the candidates to those whose `q`, `geoQ` and `scopeQ` the
    /// changed entity satisfies (clause 5.8.6), exactly as the durable path
    /// decides it at commit.
    ///
    /// A deletion is evaluated against what the entity was, because what it is
    /// now is nothing; the state before the write is what the event carries as
    /// its previous attributes, or, failing that, the removed snapshot. Every
    /// other change is evaluated against the entity the write left behind, and
    /// against the state before it as well when it removed instances.
    fn narrow_by_entity_filters(
        &self,
        target: &ContextTarget,
        event: &ChangeEvent,
        candidates: Vec<Arc<CompiledSubscription>>,
        entity_data: Option<&Value>,
    ) -> Vec<Arc<CompiledSubscription>> {
        let previous = event.previous_attributes.as_deref();
        let states = match event.change.lifecycle {
            EntityLifecycle::Deleted => ChangedStates {
                current: None,
                previous: previous.or(event.entity_snapshot.as_deref()),
            },
            EntityLifecycle::Created | EntityLifecycle::Modified => ChangedStates {
                current: entity_data,
                previous,
            },
        };
        narrow_by_change_filters(&self.cache, target, candidates, &event.change, states, &event.entity_types)
    }

    /// The entity's clause 4.8 system attributes, resolved once when any
    /// notified subscription asks for them.
    ///
    /// A deleted entity cannot be read back, so the snapshot the change
    /// carries is what the notification is stamped with; a read that finds
    /// nothing, or fails, falls back to that same snapshot.
    async fn sys_attrs_for(&self, target: &ContextTarget, event: &ChangeEvent, matching: &[Arc<CompiledSubscription>]) -> Option<SysAttrsSnapshot> {
        if !matching.iter().any(|sub| sub.notification_sys_attrs == SysAttrsInclusion::Include) {
            return None;
        }
        // clone: the notification stamps its own copy while the event keeps its snapshot
        let recorded = event.sys_attrs_snapshot.clone();
        match event.change.lifecycle {
            EntityLifecycle::Deleted => return recorded,
            EntityLifecycle::Created | EntityLifecycle::Modified => (),
        }
        match self.entity_reader.get_sys_attrs(target, &event.entity_id).await {
            Ok(Some(attrs)) => Some(attrs),
            Ok(None) => recorded,
            Err(error) => {
                debug!("Failed to fetch sysAttrs for {}: {}", event.entity_id, error);
                recorded
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::payload::filtering::filter_entity_attributes;
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_ngsi_ld_model::{
        entity_change::EntityChange,
        expanded::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            instance_key::AttributeInstanceKey,
        },
        notification::ChangeOrigin,
        subscription::{EntitySelector, SubscriptionStatus},
    };
    use andromeda_ngsi_ld_query::{filter::parser, projection::compiled::Projection, scope::query::ScopeQuery};
    use andromeda_subscription::{
        geo_filter::SubscriptionGeoFilter,
        matching::{matches_entity_filters, matches_subscription},
    };
    use chrono::{DateTime, Utc};
    use serde_json::Value;
    use std::num::NonZeroU32;
    use url::Url;

    /// A modification that `kind`s the default instance of every attribute in
    /// `attrs`.
    fn make_event(entity_id: &str, types: Vec<&str>, kind: AttributeChangeKind, attrs: Vec<&str>) -> ChangeEvent {
        let instances = attrs
            .into_iter()
            .map(|attribute| AttributeChange {
                key: AttributeInstanceKey {
                    attr: attribute.parse().expect("valid test attribute name"),
                    dataset_id: None,
                },
                kind,
            })
            .collect();
        ChangeEvent::builder()
            .entity_id(entity_id.parse().expect("valid test URI"))
            .entity_types(types.into_iter().map(|t| t.parse().expect("valid test type name")).collect())
            .change(EntityChange::new(EntityLifecycle::Modified, instances))
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
            endpoint_cooldown: None,
            endpoint_timeout: None,
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
            compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            ngsild_conformance: None,
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
            AttributeChangeKind::Updated,
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["location"]);
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn no_selectors_matches_all_entities() {
        let sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], AttributeChangeKind::Updated, vec!["speed"]);
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
            "https://uri.etsi.org/ngsi-ld/location": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [8.684_783, 49.406_132]}}]}],
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
        sub.compiled_geo_q = SubscriptionGeoFilter::from_stored(
            Some(&serde_json::json!({"georel": "near;maxDistance==100000", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"})),
            "test",
        );
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_geo_near_too_far() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_geo_q = SubscriptionGeoFilter::from_stored(
            Some(&serde_json::json!({"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"})),
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

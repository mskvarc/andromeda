//! Entity event notification evaluation: which compiled subscriptions a
//! committed change reaches, and the delivery of one notification to each.

/// The entity state a change event is notified with.
mod change_payload;
/// Delivery of a durably queued event through the processor.
mod durable_delivery;
pub(crate) mod payload;
/// Notifying one subscription of one committed entity change.
mod subscription_notification;

use crate::{
    error::NotificationError,
    processor::{change_payload::notified_state, payload::cache::NotificationCompactionCache, subscription_notification::NotifiedChange},
};
use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{
    dispatch::{DeliveryResult, TransportDispatcher},
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
};
use andromeda_entity::{port::reader::EntityReader, query::EntityQueryService};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
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
use chrono::Utc;
use futures::{StreamExt, stream};
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
        self.apply_entity_change_for(change.target, change.event, ChangeAudience::Live).await
    }

    async fn apply_entity_change_for(&self, target: ContextTarget, event: ChangeEvent, audience: ChangeAudience<'_>) -> NotificationEvaluationOutcome {
        debug!(
            entity = %event.entity_id,
            lifecycle = ?event.change.lifecycle,
            types = ?event.entity_types,
            attributes = ?event.change.attributes(),
            "Change event received"
        );
        let candidates = self.candidate_subscriptions(&target, &event, audience);
        if candidates.is_empty() {
            debug!(entity = %event.entity_id, "No matching subscriptions for the change");
            return NotificationEvaluationOutcome::default();
        }
        debug!("Found {} matching subscriptions for event {}", candidates.len(), event.entity_id);

        // The entity is read once and shared by every notification the change produces.
        let notified = notified_state(self.entity_reader.as_ref(), &target, &event).await;
        let matching = admitted_by_entity_filters(audience, &self.cache, &target, &event, candidates, notified.as_deref());
        if matching.is_empty() {
            return NotificationEvaluationOutcome::default();
        }

        let sys_attrs = self.sys_attrs_for(&target, &event, &matching).await;
        let compaction_cache = NotificationCompactionCache::default();
        let change = NotifiedChange {
            target: &target,
            event: &event,
            notified: notified.as_ref(),
            now: Utc::now(),
            sys_attrs: sys_attrs.as_ref(),
            compaction_cache: &compaction_cache,
        };
        let deliveries: Vec<Option<DeliveryResult>> = stream::iter(matching)
            .map(|compiled_sub| {
                let change = &change;
                async move { self.notify_subscription(&compiled_sub, change).await }
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
    /// evaluated, narrowed to the one subscription a durably queued delivery
    /// is addressed to.
    ///
    /// The subscription's own state is read as it stands now -- a subscription
    /// paused, expired or re-targeted since the change was queued is not
    /// notified of it -- while nothing here reads the entity.
    fn candidate_subscriptions(&self, target: &ContextTarget, event: &ChangeEvent, audience: ChangeAudience<'_>) -> Vec<Arc<CompiledSubscription>> {
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
        if let ChangeAudience::Queued { subscription, .. } = audience {
            matching.retain(|candidate| &candidate.id == subscription);
        }
        matching
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

/// Who one committed change is evaluated for.
#[derive(Clone, Copy, Debug)]
enum ChangeAudience<'a> {
    /// Every subscription the change reaches, its entity filters decided now.
    Live,
    /// The one subscription a durable delivery is addressed to.
    Queued {
        /// The subscription the delivery is addressed to.
        subscription: &'a Identifier,
        /// Whether the commit already decided its entity filters.
        filters: CommitFilters,
    },
}

/// Whether the commit that queued a delivery decided the subscription's
/// entity filters.
///
/// A delivery is queued because the commit found the change matching the
/// subscription's `q`, `geoQ`, `scopeQ` and `csf` on the very states queued
/// with it, and clause 5.8.6 decides a notification by the change and the
/// state it left, both fixed at commit, so those filters are not evaluated a
/// second time. What may have moved since is the subscription itself, which is
/// read again, and the instants of the entity's instances, which the rendering
/// reads as they stand at delivery (clause 4.22).
#[derive(Clone, Copy, Debug)]
enum CommitFilters {
    /// The commit evaluated the filters on a state queued with the delivery.
    Decided,
    /// The commit had no state to evaluate them on, so it admitted the change
    /// and delivery evaluates them on the entity it reads.
    Undecided,
}

/// The candidates the change's entity filters admit for `audience`: decided
/// now for a live change, as the commit decided them for a queued delivery.
fn admitted_by_entity_filters(
    audience: ChangeAudience<'_>,
    cache: &SubscriptionCache,
    target: &ContextTarget,
    event: &ChangeEvent,
    candidates: Vec<Arc<CompiledSubscription>>,
    notified: Option<&EntityState>,
) -> Vec<Arc<CompiledSubscription>> {
    match audience {
        ChangeAudience::Live
        | ChangeAudience::Queued {
            filters: CommitFilters::Undecided,
            ..
        } => narrow_by_entity_filters(cache, target, event, candidates, notified),
        ChangeAudience::Queued {
            filters: CommitFilters::Decided,
            ..
        } => candidates,
    }
}

/// Narrow the candidates to those whose `q`, `geoQ` and `scopeQ` the
/// changed entity satisfies (clause 5.8.6), exactly as the durable path
/// decides it at commit.
///
/// A deletion is evaluated against what the entity was, because what it is
/// now is nothing; the state before the write is what the event carries as
/// its previous attributes, or, failing that, the removed state. Every other
/// change is evaluated against the entity the write left behind, and against
/// the state before it as well when it removed instances.
fn narrow_by_entity_filters(
    cache: &SubscriptionCache,
    target: &ContextTarget,
    event: &ChangeEvent,
    candidates: Vec<Arc<CompiledSubscription>>,
    notified: Option<&EntityState>,
) -> Vec<Arc<CompiledSubscription>> {
    let previous = event.previous_attributes.as_deref();
    let states = match event.change.lifecycle {
        EntityLifecycle::Deleted => ChangedStates {
            current: None,
            previous: previous.or(event.entity_snapshot.as_deref()),
        },
        EntityLifecycle::Created | EntityLifecycle::Modified => ChangedStates { current: notified, previous },
    };
    narrow_by_change_filters(cache, target, candidates, &event.change, states, &event.entity_types)
}

#[cfg(test)]
mod tests {
    use crate::processor::{ChangeAudience, CommitFilters, admitted_by_entity_filters};
    use andromeda_context_commit::{change_event::ChangeEvent, subscription_key::SubscriptionKey};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
        subscription::{EntitySelector, SubscriptionStatus},
    };
    use andromeda_ngsi_ld_query::{filter::parser, projection::compiled::Projection, scope::query::ScopeQuery};
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_subscription::{
        cache::SubscriptionCache,
        compiled::CompiledSubscription,
        geo_filter::SubscriptionGeoFilter,
        matching::{matches_entity_filters, matches_subscription},
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::{DateTime, Utc};
    use std::{num::NonZeroU32, sync::Arc};
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

    /// A Vehicle with a `speed` of 80, a `location` and two scopes.
    fn entity_with_geo_and_scope() -> EntityState {
        let document = serde_json::json!([{
            "@id": "urn:ngsi-ld:Vehicle:A1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}]
            }],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 8.684_783}, {"@value": 49.406_132}]}]
                }]
            }],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid/Gardens"}, {"@value": "/Spain"}]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    #[test]
    fn matches_entity_filters_q_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/speed\">50").expect("valid q"));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_q_no_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/speed\">100").expect("valid q"));
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

    /// Design: the commit matched a durable delivery's `q`, `geoQ`, `scopeQ`
    /// and `csf` on the state queued with it, so delivery does not evaluate
    /// them again; a live change is filtered now.
    #[test]
    fn durable_delivery_does_not_re_evaluate_a_matched_filter() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(parser::parse("\"https://example.com/speed\">100").expect("valid q"));
        let subscription_id = sub.id.clone(); // clone: the audience names the subscription the candidates own
        let candidates = vec![Arc::new(sub)];
        let state = entity_with_geo_and_scope();
        let event = make_event(
            "urn:ngsi-ld:Vehicle:A1",
            vec!["https://example.com/Vehicle"],
            AttributeChangeKind::Updated,
            vec!["https://example.com/speed"],
        );

        // clone: the same candidates are offered to both audiences
        let live = admitted_by_entity_filters(ChangeAudience::Live, &cache, &ContextTarget::Live, &event, candidates.clone(), Some(&state));
        assert!(live.is_empty(), "a live change is filtered by the q it fails");

        let decided = ChangeAudience::Queued {
            subscription: &subscription_id,
            filters: CommitFilters::Decided,
        };
        // clone: same
        let queued = admitted_by_entity_filters(decided, &cache, &ContextTarget::Live, &event, candidates.clone(), Some(&state));
        pretty_assertions::assert_eq!(queued.iter().map(|sub| &sub.id).collect::<Vec<_>>(), vec![&subscription_id]);

        // A commit that had no state to read admitted the change undecided, so
        // its delivery decides the filters on the entity it reads.
        let undecided = ChangeAudience::Queued {
            subscription: &subscription_id,
            filters: CommitFilters::Undecided,
        };
        assert!(admitted_by_entity_filters(undecided, &cache, &ContextTarget::Live, &event, candidates, Some(&state)).is_empty());
    }
}

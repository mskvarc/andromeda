pub(crate) mod matching;
pub(crate) mod payload;

use crate::{
    domain::{
        notification::{ChangeEvent, CompiledSelector, CompiledSubscription, Notification, SubscriptionChange, TriggerReason},
        subscription::{EntitySelector, Subscription, SubscriptionStatus},
    },
    jsonld::JsonLdEngine,
    query::q,
    repository::{entity::EntityRepository, subscription::SubscriptionRepository, subscription_store::SubscriptionStore},
    service::entity::{inject_sys_attrs, query::EntityQueryService},
    transport::{
        dispatch::{DeliveryResult, TransportDispatcher, build_delivery_request},
        endpoint::parse_endpoint_transport,
    },
};
use chrono::{SecondsFormat, Utc};
use futures::{StreamExt, stream};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, info};
use uuid::Uuid;

use matching::{find_matching_subscriptions, matches_entity_filters};
use payload::{NotificationBuildParams, NotificationCompactionCache, build_notification_data};

const NOTIFICATION_CONCURRENCY: usize = 20;

#[async_trait::async_trait]
impl super::Worker for NotificationWorker {
    async fn run(self) {
        // Delegates to the inherent run method below
        NotificationWorker::run(self).await
    }
}

pub struct NotificationWorker {
    change_rx: broadcast::Receiver<ChangeEvent>,
    sub_change_rx: broadcast::Receiver<SubscriptionChange>,
    sub_repo: SubscriptionRepository,
    entity_repo: EntityRepository,
    entity_query: EntityQueryService,
    cache: Arc<RwLock<HashMap<String, Arc<CompiledSubscription>>>>,
    transport: Arc<TransportDispatcher>,
    engine: JsonLdEngine,
    tenant: crate::tenancy::TenantRef,
}

pub struct NotificationWorkerDeps {
    pub sub_repo: SubscriptionRepository,
    pub entity_repo: EntityRepository,
    pub entity_query: EntityQueryService,
    pub engine: JsonLdEngine,
    pub transport: Arc<TransportDispatcher>,
    pub tenant: crate::tenancy::TenantRef,
}

impl NotificationWorker {
    pub fn new(change_rx: broadcast::Receiver<ChangeEvent>, sub_change_rx: broadcast::Receiver<SubscriptionChange>, deps: NotificationWorkerDeps) -> Self {
        Self {
            change_rx,
            sub_change_rx,
            sub_repo: deps.sub_repo,
            entity_repo: deps.entity_repo,
            entity_query: deps.entity_query,
            cache: Arc::new(RwLock::new(HashMap::new())),
            transport: deps.transport,
            engine: deps.engine,
            tenant: deps.tenant,
        }
    }

    pub async fn run(mut self) {
        info!("NotificationWorker starting, loading active subscriptions...");

        if let Err(e) = self.reload_cache().await {
            error!("Failed to load subscriptions into cache: {}", e);
        }

        let cache_count = self.cache.read().await.len();
        info!("NotificationWorker loaded {} active subscriptions", cache_count);

        loop {
            tokio::select! {
                result = self.change_rx.recv() => {
                    match result {
                        Ok(event) => {
                            self.handle_change_event(event).await;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            debug!("NotificationWorker lagged, missed {} events", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Change event channel closed, shutting down NotificationWorker");
                            return;
                        }
                    }
                }
                result = self.sub_change_rx.recv() => {
                    match result {
                        Ok(change) => {
                            self.handle_subscription_change(change).await;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            debug!("NotificationWorker lagged on subscription changes, missed {}", n);
                            if let Err(e) = self.reload_cache().await {
                                error!("Failed to reload subscription cache: {}", e);
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Subscription change channel closed, shutting down NotificationWorker");
                            return;
                        }
                    }
                }
            }
        }
    }

    async fn reload_cache(&self) -> Result<(), crate::error::AndromedaError> {
        let subscriptions = self.sub_repo.list_active().await?;
        let mut cache = self.cache.write().await;
        cache.clear();
        for sub in subscriptions {
            if let Some(compiled) = compile_subscription(&sub) {
                cache.insert(sub.id.clone(), Arc::new(compiled));
            }
        }
        Ok(())
    }

    async fn handle_subscription_change(&self, change: SubscriptionChange) {
        match change {
            SubscriptionChange::Upserted(id) => match self.sub_repo.get(&id).await {
                Ok(Some(sub)) => {
                    let mut cache = self.cache.write().await;
                    if sub.status == SubscriptionStatus::Active {
                        if let Some(compiled) = compile_subscription(&sub) {
                            cache.insert(id, Arc::new(compiled));
                        }
                    } else {
                        cache.remove(&id);
                    }
                }
                Ok(None) => {
                    self.cache.write().await.remove(&id);
                }
                Err(e) => {
                    debug!("Failed to load subscription {} for cache update: {}", id, e);
                }
            },
            SubscriptionChange::Deleted(id) => {
                self.cache.write().await.remove(&id);
            }
        }
    }

    async fn handle_change_event(&self, event: ChangeEvent) {
        debug!(
            "Change event received: id={}, trigger={}, types={:?}, attrs={:?}",
            event.entity_id, event.trigger_reason, event.entity_types, event.changed_attributes
        );
        let matching = {
            let cache = self.cache.read().await;
            find_matching_subscriptions(&cache, &event)
        };

        if matching.is_empty() {
            debug!("No matching subscriptions for event {} (trigger={})", event.entity_id, event.trigger_reason);
            return;
        }
        debug!("Found {} matching subscriptions for event {}", matching.len(), event.entity_id);

        // Fetch entity data once for all matching subscriptions
        let entity_data = if !matches!(event.trigger_reason, TriggerReason::EntityDeleted | TriggerReason::AttributeDeleted) {
            match self.entity_repo.get(&event.entity_id).await {
                Ok(Some(data)) => Some(data),
                Ok(None) => {
                    debug!("Entity {} no longer exists, skipping notification", event.entity_id);
                    None
                }
                Err(e) => {
                    error!("Failed to fetch entity {} for notification: {}", event.entity_id, e);
                    None
                }
            }
        } else if matches!(event.trigger_reason, TriggerReason::AttributeDeleted) {
            match self.entity_repo.get(&event.entity_id).await {
                Ok(Some(data)) => Some(data),
                _ => {
                    if let Some(ref snapshot) = event.entity_snapshot {
                        let mut bare = payload::strip_to_structural_keys(
                            &match snapshot.as_ref() {
                                Value::Array(arr) => arr.first().cloned().unwrap_or_else(|| snapshot.as_ref().clone()),
                                other => other.clone(),
                            },
                            false,
                        );
                        if let Some(obj) = bare.as_object_mut() {
                            obj.insert(
                                "https://uri.etsi.org/ngsi-ld/deletedAt".to_string(),
                                serde_json::json!([{
                                    "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                                    "@value": event.occurred_at.to_rfc3339_opts(SecondsFormat::Millis, true)
                                }]),
                            );
                        }
                        Some(serde_json::json!([bare]))
                    } else {
                        None
                    }
                }
            }
        } else if let Some(snapshot) = event.entity_snapshot.as_deref() {
            let deleted_at_value = serde_json::json!([{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": event.occurred_at.to_rfc3339_opts(SecondsFormat::Millis, true)
            }]);
            let mut data = match snapshot {
                Value::Array(arr) => arr.first().cloned().unwrap_or_else(|| snapshot.clone()),
                other => other.clone(),
            };
            if let Some(obj) = data.as_object_mut() {
                obj.insert("https://uri.etsi.org/ngsi-ld/deletedAt".to_string(), deleted_at_value);
            }
            Some(serde_json::json!([data]))
        } else {
            Some(serde_json::json!([{
                "@id": event.entity_id,
                "@type": event.entity_types,
                "https://uri.etsi.org/ngsi-ld/deletedAt": [{
                    "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                    "@value": event.occurred_at.to_rfc3339_opts(SecondsFormat::Millis, true)
                }]
            }]))
        };

        let matching: Vec<Arc<CompiledSubscription>> = if !matches!(event.trigger_reason, TriggerReason::EntityDeleted) {
            if let Some(ref data) = entity_data {
                matching.into_iter().filter(|sub| matches_entity_filters(sub, data)).collect()
            } else {
                matching
            }
        } else {
            matching
        };

        if matching.is_empty() {
            return;
        }

        let need_sys_attrs = matching.iter().any(|sub| sub.notification_sys_attrs);

        let sys_attrs = if need_sys_attrs {
            let fallback_sys_attrs = event.sys_attrs_snapshot.clone();
            if matches!(event.trigger_reason, TriggerReason::EntityDeleted) {
                fallback_sys_attrs
            } else {
                match self.entity_repo.get_sys_attrs(&event.entity_id).await {
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
        let _results: Vec<()> = stream::iter(matching)
            .map(|compiled_sub| {
                let event = Arc::clone(&event);
                let mut entity_data = entity_data.clone();
                let sys_attrs = sys_attrs.clone();
                let compaction_cache = &compaction_cache;
                async move {
                    let sub_id = compiled_sub.id.clone();

                    match self.sub_repo.claim_notification_slot(&sub_id, compiled_sub.throttling_secs).await {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(e) => {
                            debug!("Failed to claim notification slot for {}: {}", sub_id, e);
                            return;
                        }
                    }

                    if compiled_sub.notification_sys_attrs
                        && let (Some(ref mut data), Some(sa)) = (entity_data.as_mut(), &sys_attrs)
                    {
                        inject_sys_attrs(data, sa.created_at, sa.modified_at, sa.expires_at);
                    }

                    let data = match entity_data {
                        Some(ref d) => {
                            build_notification_data(NotificationBuildParams {
                                engine: &self.engine,
                                entity_data: d,
                                previous_attributes: event.previous_attributes.as_deref(),
                                sub: &compiled_sub,
                                trigger_reason: &event.trigger_reason,
                                changed_attributes: &event.changed_attributes,
                                occurred_at: event.occurred_at,
                                entity_query: &self.entity_query,
                                compaction_cache: Some(compaction_cache),
                            })
                            .await
                        }
                        None => return,
                    };
                    if data.is_empty() {
                        debug!("Subscription {} produced empty notification data for entity {}", sub_id, event.entity_id);
                        return;
                    }

                    let notification = Notification {
                        id: format!("urn:ngsi-ld:Notification:{}", Uuid::new_v4()),
                        type_: "Notification".to_string(),
                        subscription_id: sub_id.clone(),
                        notified_at: Utc::now(),
                        data,
                    };

                    let notification_payload = match serde_json::to_value(&notification) {
                        Ok(v) => v,
                        Err(e) => {
                            error!("Failed to serialize notification for {}: {}", sub_id, e);
                            return;
                        }
                    };

                    let request = match build_delivery_request(
                        &notification_payload,
                        &compiled_sub.endpoint_uri,
                        compiled_sub.endpoint_accept.as_deref(),
                        compiled_sub.endpoint_receiver_info.as_deref(),
                        compiled_sub.jsonld_context.as_deref(),
                        self.tenant.response_header_value(),
                        &sub_id,
                    ) {
                        Ok(r) => r,
                        Err(e) => {
                            debug!("Failed to build delivery request for {}: {}", sub_id, e);
                            return;
                        }
                    };
                    let success = matches!(
                        self.transport.deliver(&compiled_sub.endpoint_transport, &request).await,
                        DeliveryResult::Delivered
                    );

                    if success {
                        if let Err(e) = self.sub_repo.mark_success(&sub_id).await {
                            debug!("Failed to mark notification success for {}: {}", sub_id, e);
                        }
                    } else if let Err(e) = self.sub_repo.mark_failure(&sub_id).await {
                        debug!("Failed to mark notification failure for {}: {}", sub_id, e);
                    }
                }
            })
            .buffer_unordered(NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
    }
}

fn compile_subscription(sub: &Subscription) -> Option<CompiledSubscription> {
    let selectors: Vec<CompiledSelector> = sub
        .entities
        .as_ref()
        .map(|entities| entities.iter().filter_map(compile_selector).collect())
        .unwrap_or_default();

    let triggers: Vec<String> = sub
        .notification_trigger
        .as_ref()
        .map(|t| t.iter().map(|nt| nt.to_string()).collect())
        .unwrap_or_else(|| TriggerReason::defaults().iter().map(|s| String::from(*s)).collect());

    let watched_attrs = sub.watched_attributes.as_ref().map(|attrs| attrs.iter().cloned().collect::<HashSet<String>>());

    let compiled_q = sub.q.as_ref().and_then(|q_str| match q::parse(q_str) {
        Ok(expr) => Some(expr),
        Err(e) => {
            debug!("Failed to parse q filter '{}' for subscription {}: {}", q_str, sub.id, e);
            None
        }
    });

    let compiled_csf = sub.csf.as_ref().and_then(|csf_str| match q::parse(csf_str) {
        Ok(expr) => Some(expr),
        Err(e) => {
            debug!("Failed to parse csf filter '{}' for subscription {}: {}", csf_str, sub.id, e);
            None
        }
    });

    let endpoint_transport = match parse_endpoint_transport(&sub.notification.endpoint.uri, sub.notification.endpoint.notifier_info.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            debug!("Failed to parse endpoint transport for subscription {}: {}", sub.id, e);
            return None;
        }
    };

    Some(CompiledSubscription {
        id: sub.id.clone(),
        status: sub.status,
        endpoint_uri: sub.notification.endpoint.uri.clone(),
        endpoint_accept: sub.notification.endpoint.accept.clone(),
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(),
        endpoint_transport,
        triggers,
        watched_attrs,
        selectors,
        throttling_secs: sub.throttling,
        time_interval_secs: sub.time_interval,
        notification_attributes: sub.notification.attributes.clone(),
        notification_format: sub.notification.format.map(|f| f.to_string()),
        notification_sys_attrs: sub.notification.sys_attrs.unwrap_or(false),
        notification_show_changes: sub.notification.show_changes.unwrap_or(false),
        jsonld_context: sub.jsonld_context.clone(),
        expires_at: sub.expires_at,
        compiled_q,
        geo_q: sub.geo_q.clone(),
        scope_q: sub.scope_q.clone(),
        dataset_id: sub.dataset_id.clone(),
        notification_join: sub.notification.join.map(|j| j.to_string()),
        notification_join_level: sub.notification.join_level.unwrap_or(1).min(5),
        compiled_csf,
    })
}

fn compile_selector(selector: &EntitySelector) -> Option<CompiledSelector> {
    let id_pattern = selector.id_pattern.as_ref().and_then(|p| match Regex::new(p) {
        Ok(r) => Some(r),
        Err(e) => {
            debug!("Invalid idPattern regex '{}': {}", p, e);
            None
        }
    });

    let type_pattern = if selector.type_.contains('|') || (selector.type_.contains('(') && selector.type_.contains(')')) {
        let raw = selector.type_.trim();
        let inner = raw.trim_start_matches('(').trim_end_matches(')');
        let alternatives: Vec<&str> = inner.split('|').collect();
        let pattern_parts: Vec<String> = alternatives.iter().map(|alt| regex::escape(alt.trim())).collect();
        let regex_str = format!("({})", pattern_parts.join("|"));
        match Regex::new(&regex_str) {
            Ok(r) => Some(r),
            Err(e) => {
                debug!("Invalid type regex pattern '{}': {}", selector.type_, e);
                None
            }
        }
    } else {
        None
    };

    Some(CompiledSelector {
        id: selector.id.clone(),
        id_pattern,
        type_: selector.type_.clone(),
        type_pattern,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::endpoint::EndpointTransport;
    use matching::matches_subscription;
    use payload::filter_entity_attributes;

    fn make_event(entity_id: &str, types: Vec<&str>, trigger: TriggerReason, attrs: Vec<&str>) -> ChangeEvent {
        ChangeEvent {
            entity_id: entity_id.to_string(),
            entity_types: types.into_iter().map(String::from).collect(),
            trigger_reason: trigger,
            changed_attributes: attrs.into_iter().map(String::from).collect(),
            occurred_at: Utc::now(),
            entity_snapshot: None,
            previous_attributes: None,
            sys_attrs_snapshot: None,
        }
    }

    fn make_sub(id: &str, selectors: Vec<CompiledSelector>, triggers: Vec<&str>, watched: Option<Vec<&str>>) -> CompiledSubscription {
        CompiledSubscription {
            id: id.to_string(),
            status: SubscriptionStatus::Active,
            endpoint_uri: "http://example.com/notify".to_string(),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_transport: EndpointTransport::Http,
            triggers: triggers.into_iter().map(String::from).collect(),
            watched_attrs: watched.map(|w| w.into_iter().map(String::from).collect()),
            selectors,
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: None,
            notification_format: None,
            notification_sys_attrs: false,
            notification_show_changes: false,
            jsonld_context: None,
            expires_at: None,
            compiled_q: None,
            geo_q: None,
            scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: 1,
            compiled_csf: None,
        }
    }

    #[test]
    fn matches_by_type_and_trigger() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_wrong_type() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: None,
                type_: "Parking".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_wrong_trigger() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["entityCreated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_watched_attributes() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            Some(vec!["speed"]),
        );
        let event = make_event(
            "urn:ngsi-ld:Vehicle:A1",
            vec!["Vehicle"],
            TriggerReason::AttributeUpdated,
            vec!["speed", "location"],
        );
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_unwatched_attribute() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            Some(vec!["speed"]),
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["location"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_with_id_pattern() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: Some(Regex::new("urn:ngsi-ld:Vehicle:.*").unwrap()),
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_id_pattern_mismatch() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: Some(Regex::new("urn:ngsi-ld:Parking:.*").unwrap()),
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn matches_specific_entity_id() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: Some("urn:ngsi-ld:Vehicle:A1".to_string()),
                id_pattern: None,
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn no_match_different_entity_id() {
        let sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: Some("urn:ngsi-ld:Vehicle:B2".to_string()),
                id_pattern: None,
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            None,
        );
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn expired_subscription_does_not_match() {
        let mut sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            None,
        );
        sub.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(!matches_subscription(&sub, &event));
    }

    #[test]
    fn no_selectors_matches_all_entities() {
        let sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
        assert!(matches_subscription(&sub, &event));
    }

    #[test]
    fn paused_subscription_does_not_match() {
        let mut sub = make_sub(
            "sub1",
            vec![CompiledSelector {
                id: None,
                id_pattern: None,
                type_: "Vehicle".to_string(),
                type_pattern: None,
            }],
            vec!["attributeUpdated"],
            None,
        );
        sub.status = SubscriptionStatus::Paused;
        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::AttributeUpdated, vec!["speed"]);
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
        let filtered = filter_entity_attributes(&entity, &["https://example.com/speed".to_string()]);
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
        sub.compiled_q = Some(crate::query::q::parse("\"https://example.com/speed\">50").unwrap());
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_q_no_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_q = Some(crate::query::q::parse("\"https://example.com/speed\">100").unwrap());
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_exact() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.scope_q = Some("/Spain".to_string());
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_no_match() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.scope_q = Some("/France".to_string());
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_hash_wildcard() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.scope_q = Some("/Madrid/#".to_string());
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_and() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.scope_q = Some("/Spain;/Madrid/Gardens".to_string());
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_scope_or() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.scope_q = Some("/France,/Spain".to_string());
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_geo_near() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.geo_q = Some(serde_json::json!({"georel": "near;maxDistance==100000", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"}));
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_geo_near_too_far() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.geo_q = Some(serde_json::json!({"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"}));
        assert!(!matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_no_filters() {
        let sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }
}

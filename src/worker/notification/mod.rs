pub(crate) mod payload;

use crate::service::entity::{query::EntityQueryService, store::EntityStore, sys_attrs::inject_sys_attrs};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_delivery::{
    dispatch::TransportDispatcher,
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
    request::NotificationDelivery,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    entity_type_name::EntityTypeName,
    rfc3339::whole_seconds,
    uri::{NGSI_LD_URN_NID, Uri},
};
use andromeda_ngsi_ld_model::{
    expanded::geo_property::extract_geo_property_geometry,
    notification::{Notification, NotificationTrigger, NotificationType},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use andromeda_postgresql_context_store::subscription::repo::SubscriptionRepository;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change::SubscriptionChange,
    compiled::CompiledSubscription,
    matching::matches_entity_filters,
    store::SubscriptionStore,
    throttle::ceiling_for_throttle,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use futures::{StreamExt, stream};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};
use uuid::Uuid;

use andromeda_spatial_index::key::SpatialKey;
use iri_rs::IriBuf;
use payload::{NotificationBuildParams, build_notification_data, cache::NotificationCompactionCache};
use std::collections::{HashMap, HashSet};
use tokio::sync::broadcast::error::TryRecvError;

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

pub struct NotificationWorker {
    change_rx: broadcast::Receiver<ChangeEvent>,
    sub_change_rx: broadcast::Receiver<SubscriptionChange>,
    sub_repo: SubscriptionRepository,
    entity_repo: Arc<dyn EntityStore>,
    entity_query: EntityQueryService,
    cache: Arc<SubscriptionCache>,
    transport: Arc<TransportDispatcher>,
    engine: JsonLdEngine,
    tenant: TenantRef,
}

pub struct NotificationWorkerDeps {
    pub sub_repo: SubscriptionRepository,
    pub entity_repo: Arc<dyn EntityStore>,
    pub entity_query: EntityQueryService,
    pub engine: JsonLdEngine,
    pub transport: Arc<TransportDispatcher>,
    pub tenant: TenantRef,
    pub subscription_cache: Arc<SubscriptionCache>,
}

impl NotificationWorker {
    pub fn new(
        change_rx: broadcast::Receiver<ChangeEvent>,
        sub_change_rx: broadcast::Receiver<SubscriptionChange>,
        deps: NotificationWorkerDeps,
    ) -> NotificationWorker {
        NotificationWorker {
            change_rx,
            sub_change_rx,
            sub_repo: deps.sub_repo,
            entity_repo: deps.entity_repo,
            entity_query: deps.entity_query,
            cache: deps.subscription_cache,
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

        let cache_count = self.cache.len();
        info!("NotificationWorker loaded {} active subscriptions", cache_count);

        loop {
            tokio::select! {
                result = self.change_rx.recv() => {
                    match result {
                        Ok(event) => {
                            self.drain_subscription_changes().await;
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
        self.cache.reload(&self.sub_repo).await.map_err(Into::into)
    }

    /// Apply every already-published subscription change before matching an entity change.
    ///
    /// Subscription and entity mutations use separate broadcast channels. A later entity
    /// event can therefore win `select!` while an earlier committed subscription mutation
    /// is still queued. Draining the subscription queue restores request order at the
    /// matching seam.
    async fn drain_subscription_changes(&mut self) {
        loop {
            match self.sub_change_rx.try_recv() {
                Ok(change) => self.handle_subscription_change(change).await,
                Err(TryRecvError::Empty | TryRecvError::Closed) => return,
                Err(TryRecvError::Lagged(skipped)) => {
                    debug!(skipped, "NotificationWorker lagged while ordering subscription changes before an entity event");
                    if let Err(error) = self.reload_cache().await {
                        error!(%error, "Failed to reload subscription cache after lag");
                    }
                }
            }
        }
    }

    async fn handle_subscription_change(&self, change: SubscriptionChange) {
        // Close any active SSE/WebSocket connections when a subscription is deleted.
        if let SubscriptionChange::Deleted(ref id) = change {
            self.transport.sse_registry().close(id);
            self.transport.ws_registry().close(id);
        }
        self.cache.handle_change(&change, &self.sub_repo).await;

        // After cache update, close lingering SSE/WebSocket connections if
        // transport was changed away from the streaming type.
        if let SubscriptionChange::Upserted(ref id) = change
            && let Some(compiled) = self.cache.get(id)
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

    async fn handle_change_event(&self, event: ChangeEvent) {
        debug!(
            "Change event received: id={}, trigger={}, types={:?}, attrs={:?}",
            event.entity_id, event.trigger_reason, event.entity_types, event.changed_attributes
        );
        let matching = self.cache.find_matching(&event);

        if matching.is_empty() {
            debug!("No matching subscriptions for event {} (trigger={})", event.entity_id, event.trigger_reason);
            return;
        }
        debug!("Found {} matching subscriptions for event {}", matching.len(), event.entity_id);

        // Fetch entity data once for all matching subscriptions
        let entity_data = if !matches!(event.trigger_reason, NotificationTrigger::EntityDeleted | NotificationTrigger::AttributeDeleted) {
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
        } else if matches!(event.trigger_reason, NotificationTrigger::AttributeDeleted) {
            match self.entity_repo.get(&event.entity_id).await {
                Ok(Some(data)) => Some(data),
                _ => {
                    if let Some(ref snapshot) = event.entity_snapshot {
                        let mut bare = payload::context::strip_to_structural_keys(
                            &match snapshot.as_ref() {
                                Value::Array(arr) => arr.first().cloned().unwrap_or_else(|| snapshot.as_ref().clone()),
                                other => other.clone(),
                            },
                            SysAttrsInclusion::Strip,
                        );
                        if let Some(obj) = bare.as_object_mut() {
                            obj.insert(
                                expanded_properties::DELETED_AT.as_str().to_string(),
                                serde_json::json!([{
                                    "@type": expanded_classes::DATE_TIME.as_str(),
                                    "@value": whole_seconds(event.occurred_at)
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
                "@type": expanded_classes::DATE_TIME.as_str(),
                "@value": whole_seconds(event.occurred_at)
            }]);
            let mut data = match snapshot {
                Value::Array(arr) => arr.first().cloned().unwrap_or_else(|| snapshot.clone()),
                other => other.clone(),
            };
            if let Some(obj) = data.as_object_mut() {
                obj.insert(expanded_properties::DELETED_AT.as_str().to_string(), deleted_at_value);
            }
            Some(serde_json::json!([data]))
        } else {
            Some(serde_json::json!([{
                "@id": event.entity_id,
                "@type": event.entity_types,
                (expanded_properties::DELETED_AT.as_str()): [{
                    "@type": expanded_classes::DATE_TIME.as_str(),
                    "@value": whole_seconds(event.occurred_at)
                }]
            }]))
        };

        let matching: Vec<Arc<CompiledSubscription>> = if !matches!(event.trigger_reason, NotificationTrigger::EntityDeleted) {
            if let Some(ref data) = entity_data {
                let geo_filtered = self.spatial_narrow(matching, data, &event.entity_types);
                geo_filtered.into_iter().filter(|sub| matches_entity_filters(sub, data)).collect()
            } else {
                matching
            }
        } else {
            matching
        };

        if matching.is_empty() {
            return;
        }

        let need_sys_attrs = matching.iter().any(|sub| sub.notification_sys_attrs == SysAttrsInclusion::Include);

        let sys_attrs = if need_sys_attrs {
            let fallback_sys_attrs = event.sys_attrs_snapshot.clone();
            if matches!(event.trigger_reason, NotificationTrigger::EntityDeleted) {
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

                    match self
                        .sub_repo
                        .claim_notification_slot(&sub_id, ceiling_for_throttle(Utc::now(), compiled_sub.throttling_secs))
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(e) => {
                            debug!("Failed to claim notification slot for {}: {}", sub_id, e);
                            return;
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

                    let notification_id = match format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Uri>() {
                        Ok(u) => u,
                        Err(_) => {
                            error!("Failed to construct notification URN for {}", sub_id);
                            return;
                        }
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
                            return;
                        }
                    };

                    let Some(endpoint_transport) = compiled_transport(&compiled_sub) else {
                        return;
                    };
                    let request = (NotificationDelivery {
                        payload: &notification_payload,
                        endpoint_uri: &compiled_sub.endpoint_uri,
                        accept: compiled_sub.endpoint_accept.as_ref(),
                        receiver_info: compiled_sub.endpoint_receiver_info.as_deref(),
                        jsonld_context: compiled_sub.jsonld_context.as_ref(),
                        tenant: self.tenant.tenant_name(),
                        subscription_id: &sub_id,
                    })
                    .assemble();
                    let success = self.transport.deliver(&endpoint_transport, &request).await.is_delivered();

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

    /// Narrow `matching` to subscriptions whose pre-compiled `geoQ` actually
    /// has the entity in scope, by consulting the spatial subscription index.
    ///
    /// Subscriptions without a `compiled_geo_q` are kept verbatim. The index
    /// returns the same answer the exact predicate would, so the downstream
    /// `matches_entity_filters` call still re-checks for safety but evaluates
    /// far fewer geometries when many geo-enabled subscriptions exist but
    /// few overlap any given entity.
    fn spatial_narrow(&self, matching: Vec<Arc<CompiledSubscription>>, entity_data: &Value, entity_types: &[EntityTypeName]) -> Vec<Arc<CompiledSubscription>> {
        let spatial = self.cache.spatial();
        let tenant = self.cache.tenant().clone();

        // One read of each distinct geoproperty, cached across the
        // subscriptions that target it. A property the entity does not carry is
        // cached as absent too, so a miss is not re-read either.
        let mut geometries: HashMap<IriBuf, Option<Geometry>> = HashMap::new();
        let mut survivors: HashSet<Uri> = HashSet::new();
        let mut non_geo: Vec<Arc<CompiledSubscription>> = Vec::new();

        for sub in matching {
            let Some(ref geo_q) = sub.compiled_geo_q else {
                non_geo.push(sub);
                continue;
            };

            let geoproperty_iri = &geo_q.geoproperty_iri;
            if !geometries.contains_key(geoproperty_iri) {
                // clone: the cache owns its key, which the subscription keeps
                let geometry = extract_geo_property_geometry(entity_data, geoproperty_iri.as_str());
                geometries.insert(geoproperty_iri.clone(), geometry);
            }
            // The entity carries no instance of this geoproperty, so no
            // subscription filtering on it can match (clause 4.10).
            let Some(Some(entity_geometry)) = geometries.get(geoproperty_iri) else {
                continue;
            };

            let mut matched_any = false;
            for type_name in entity_types {
                let Ok(type_iri) = IriBuf::new(type_name.clone()) else {
                    continue;
                };
                let key = SpatialKey::new(tenant.clone(), type_iri, geoproperty_iri.clone());
                let candidates = spatial.candidates(&key, entity_geometry.as_ref());
                if candidates.iter().any(|candidate| candidate == &sub.id) {
                    matched_any = true;
                    break;
                }
            }
            // An untyped or boolean-typed geo subscription is indexed under the
            // wildcard partition, which no concrete-type key reaches; query it
            // too, or such a subscription is never evaluated and silently
            // dropped (clause 4.10 still applies to it).
            if !matched_any {
                let key = SpatialKey::any_type(tenant.clone(), geoproperty_iri.clone());
                if spatial.candidates(&key, entity_geometry.as_ref()).iter().any(|candidate| candidate == &sub.id) {
                    matched_any = true;
                }
            }
            if matched_any && survivors.insert(sub.id.clone()) {
                non_geo.push(sub);
            }
        }

        non_geo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::notification::payload::filtering::filter_entity_attributes;
    use andromeda_ngsi_ld_model::{
        notification::ChangeOrigin,
        subscription::{EntitySelector, SubscriptionStatus},
    };
    use andromeda_ngsi_ld_query::{filter::parser, scope::query::ScopeQuery};
    use andromeda_subscription::matching::matches_subscription;
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
            compiled_q: None,
            compiled_geo_q: None,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: 1,
            compiled_csf: None,
            local_only: false,
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
        sub.compiled_geo_q = andromeda_subscription::compilation::compile_geo_q(
            &serde_json::json!({"georel": "near;maxDistance==100000", "geometry": "Point", "coordinates": "[8.6724, 49.3988]"}),
            "test",
        );
        assert!(matches_entity_filters(&sub, &entity_with_geo_and_scope()));
    }

    #[test]
    fn matches_entity_filters_geo_near_too_far() {
        let mut sub = make_sub("sub1", vec![], vec!["attributeUpdated"], None);
        sub.compiled_geo_q = andromeda_subscription::compilation::compile_geo_q(
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

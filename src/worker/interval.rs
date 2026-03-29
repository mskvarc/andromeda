use crate::{
    domain::{
        entity::Entity,
        notification::{EndpointTransport, parse_endpoint_transport},
    },
    jsonld::{CompactOptions, ContextSpec, JsonLdEngine},
    query::q,
    repository::{entity::EntityRepository, subscription::SubscriptionRepository, subscription_store::SubscriptionStore},
    service::entity::inject_sys_attrs,
    transport::mqtt::MqttConnectionManager,
};
use chrono::Utc;
use ngsi_ld_types::{notification::Notification, subscription::Subscription};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, info};
use uuid::Uuid;

use super::notification::{
    matching::{matches_geo_filter, matches_scope_filter},
    payload::{context_for_subscription_raw, filter_entity_attributes, normalize_compacted_entity},
};

#[async_trait::async_trait]
impl super::Worker for IntervalWorker {
    async fn run(self) {
        IntervalWorker::run(self).await
    }
}

pub struct IntervalWorker {
    sub_repo: SubscriptionRepository,
    entity_repo: EntityRepository,
    http_client: reqwest::Client,
    mqtt_manager: Arc<MqttConnectionManager>,
    poll_interval: Duration,
    engine: JsonLdEngine,
    tenant: crate::tenancy::TenantRef,
}

impl IntervalWorker {
    pub fn new(
        sub_repo: SubscriptionRepository,
        entity_repo: EntityRepository,
        engine: JsonLdEngine,
        mqtt_manager: Arc<MqttConnectionManager>,
        tenant: crate::tenancy::TenantRef,
    ) -> Self {
        Self {
            sub_repo,
            entity_repo,
            http_client: reqwest::ClientBuilder::new().http1_title_case_headers().build().unwrap(),
            mqtt_manager,
            poll_interval: Duration::from_secs(1),
            engine,
            tenant,
        }
    }

    pub async fn run(self) {
        info!("IntervalWorker starting (poll interval: {:?})", self.poll_interval);

        let mut ticker = tokio::time::interval(self.poll_interval);

        loop {
            ticker.tick().await;

            let due_subs = match self.sub_repo.find_due_interval_subscriptions().await {
                Ok(subs) => subs,
                Err(e) => {
                    error!("IntervalWorker failed to find due subscriptions: {}", e);
                    continue;
                }
            };

            for sub in due_subs {
                // Fetch entities matching the subscription's entity selectors
                let entities = match self.fetch_matching_entities(&sub).await {
                    Ok(e) => e,
                    Err(e) => {
                        error!("IntervalWorker failed to fetch entities for {}: {}", sub.id, e);
                        if let Err(e) = self.sub_repo.mark_failure(&sub.id).await {
                            debug!("Failed to mark failure for {}: {}", sub.id, e);
                        }
                        continue;
                    }
                };

                // Apply q/geoQ/scopeQ filters
                let entities = apply_subscription_filters(&sub, entities);

                if entities.is_empty() {
                    // Per spec: if no matching entities, no notification is sent
                    continue;
                }

                let data = match self.prepare_notification_entities(&sub, entities).await {
                    Ok(d) => d,
                    Err(e) => {
                        error!("IntervalWorker failed to build notification payload for {}: {}", sub.id, e);
                        continue;
                    }
                };

                if data.is_empty() {
                    continue;
                }

                // Claim slot only once we know we are actually going to send a notification.
                let claimed = match self.sub_repo.claim_notification_slot(&sub.id, None).await {
                    Ok(c) => c,
                    Err(e) => {
                        debug!("IntervalWorker failed to claim slot for {}: {}", sub.id, e);
                        continue;
                    }
                };

                if !claimed {
                    continue;
                }

                let notification = Notification {
                    id: format!("urn:ngsi-ld:Notification:{}", Uuid::new_v4()),
                    type_: "Notification".to_string(),
                    subscription_id: sub.id.clone(),
                    notified_at: Utc::now(),
                    data,
                };

                let payload = match serde_json::to_value(&notification) {
                    Ok(v) => v,
                    Err(e) => {
                        error!("Failed to serialize notification for {}: {}", sub.id, e);
                        continue;
                    }
                };

                let endpoint_uri = &sub.notification.endpoint.uri;
                let accept = sub.notification.endpoint.accept.as_deref();
                let jsonld_context = sub.jsonld_context.as_deref();

                let transport = parse_endpoint_transport(endpoint_uri, sub.notification.endpoint.notifier_info.as_deref());

                let success = match transport {
                    Ok(EndpointTransport::Mqtt(ref config)) => {
                        let content_type = match accept {
                            Some("application/ld+json") => "application/ld+json",
                            Some("application/geo+json") => "application/geo+json",
                            _ => "application/json",
                        };
                        let body = match serde_json::to_vec(&payload) {
                            Ok(b) => b,
                            Err(e) => {
                                debug!("Failed to serialize MQTT payload: {}", e);
                                continue;
                            }
                        };
                        self.mqtt_manager
                            .publish(
                                config,
                                &body,
                                content_type,
                                sub.notification.endpoint.receiver_info.as_deref(),
                                jsonld_context,
                                self.tenant.response_header_value(),
                            )
                            .await
                    }
                    _ => {
                        crate::transport::http::deliver_http(
                            &self.http_client,
                            endpoint_uri,
                            accept,
                            None,
                            jsonld_context,
                            self.tenant.response_header_value(),
                            &payload,
                        )
                        .await
                    }
                };

                if success {
                    if let Err(e) = self.sub_repo.mark_success(&sub.id).await {
                        debug!("Failed to mark success for {}: {}", sub.id, e);
                    }
                } else {
                    if let Err(e) = self.sub_repo.mark_failure(&sub.id).await {
                        debug!("Failed to mark failure for {}: {}", sub.id, e);
                    }
                }
            }
        }
    }

    async fn fetch_matching_entities(&self, sub: &Subscription) -> Result<Vec<Value>, crate::error::AndromedaError> {
        let mut all_entities = Vec::new();

        if let Some(ref selectors) = sub.entities {
            for selector in selectors {
                let type_ = &selector.type_;
                let entities = self.entity_repo.get_entities_by_type(type_, None, None).await?;

                for entity in entities {
                    // Apply id / idPattern filtering
                    let entity_id = entity
                        .as_array()
                        .and_then(|arr| arr.first())
                        .or(Some(&entity))
                        .and_then(|e| e.get("@id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if let Some(ref id) = selector.id {
                        if entity_id != id {
                            continue;
                        }
                    }

                    if let Some(ref pattern) = selector.id_pattern {
                        if let Ok(re) = regex::Regex::new(pattern) {
                            if !re.is_match(entity_id) {
                                continue;
                            }
                        }
                    }

                    all_entities.push(entity);
                }
            }
        } else {
            // No entity selectors — all entities
            all_entities = self.entity_repo.get_all_entities(None, None).await?;
        }

        Ok(all_entities)
    }

    async fn prepare_notification_entities(&self, sub: &Subscription, entities: Vec<Value>) -> Result<Vec<Value>, crate::error::AndromedaError> {
        let mut results = Vec::new();
        for mut entity in entities {
            if sub.notification.sys_attrs.unwrap_or(false) {
                if let Some(id) = extract_entity_id(&entity) {
                    if let Ok(Some((created_at, modified_at, expires_at))) = self.entity_repo.get_sys_attrs(&id).await {
                        inject_sys_attrs(&mut entity, created_at, modified_at, expires_at);
                    }
                }
            }

            let filtered = if let Some(ref attrs) = sub.notification.attributes {
                filter_entity_attributes(&entity, attrs)
            } else {
                entity
            };

            let context = context_for_subscription_raw(sub.jsonld_context.as_deref());
            let include_context = sub.notification.endpoint.accept.as_deref().is_some_and(|v| v == "application/ld+json");
            let compacted = match self
                .engine
                .compact(
                    filtered.clone(),
                    &context,
                    CompactOptions {
                        strip_context: !include_context,
                    },
                )
                .await
            {
                Ok(value) => value,
                Err(e) => {
                    debug!("Compaction failed with subscription context, falling back to core: {}", e);
                    self.engine
                        .compact(
                            filtered,
                            &ContextSpec::core_only(),
                            CompactOptions {
                                strip_context: !include_context,
                            },
                        )
                        .await
                        .map_err(|e| crate::error::AndromedaError::internal(format!("Compaction failed: {}", e)))?
                }
            };

            let compacted = normalize_compacted_entity(compacted);

            let formatted = if let Some(fmt) = sub.notification.format {
                if fmt.is_simplified() {
                    match serde_json::from_value::<Entity>(compacted.clone()) {
                        Ok(entity) => entity.simplify(),
                        Err(_) => compacted,
                    }
                } else {
                    compacted
                }
            } else {
                compacted
            };

            results.push(formatted);
        }

        Ok(results)
    }
}

fn extract_entity_id(entity: &Value) -> Option<String> {
    match entity {
        Value::Array(arr) => arr.first().and_then(|v| v.get("@id").and_then(|v| v.as_str()).map(String::from)),
        Value::Object(obj) => obj.get("@id").and_then(|v| v.as_str()).map(String::from),
        _ => None,
    }
}

/// Apply q, geoQ, and scopeQ filters from a subscription to a list of expanded entities.
fn apply_subscription_filters(sub: &Subscription, entities: Vec<Value>) -> Vec<Value> {
    let compiled_q = sub.q.as_ref().and_then(|q_str| match q::parse(q_str) {
        Ok(expr) => Some(expr),
        Err(e) => {
            debug!("IntervalWorker: failed to parse q filter '{}' for subscription {}: {}", q_str, sub.id, e);
            None
        }
    });

    let has_filters = compiled_q.is_some() || sub.geo_q.is_some() || sub.scope_q.is_some();
    if !has_filters {
        return entities;
    }

    entities
        .into_iter()
        .filter(|entity| {
            if let Some(ref expr) = compiled_q {
                if !q::evaluate(expr, entity) {
                    return false;
                }
            }
            if let Some(ref geo_q) = sub.geo_q {
                if !matches_geo_filter(geo_q, entity) {
                    return false;
                }
            }
            if let Some(ref scope_q) = sub.scope_q {
                if !matches_scope_filter(scope_q, entity) {
                    return false;
                }
            }
            true
        })
        .collect()
}

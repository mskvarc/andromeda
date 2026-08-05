use crate::{
    http::model::response_format::APPLICATION_LD_JSON,
    jsonld::{CompactOptions, ContextSpec, ContextStripping, JsonLdEngine},
    renderer::simplify::simplify_entity,
    repository::{entity::repo::EntityRepository, subscription::repo::SubscriptionRepository, subscription_store::repo::SubscriptionStore},
    service::entity::inject_sys_attrs,
    transport::{
        dispatch::{DeliveryResult, TransportDispatcher, build_delivery_request},
        endpoint::parse_endpoint_transport,
    },
};
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_ngsi_ld_model::{entity::Entity, notification::Notification, subscription::Subscription};
use andromeda_ngsi_ld_query::filter::eval;
use chrono::Utc;
use futures::{StreamExt, stream};
use mediatype::MediaTypeBuf;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, info};
use url::Url;
use uuid::Uuid;

use super::notification::payload::{context_for_subscription_raw, filter_entity_attributes, normalize_compacted_entity};
use crate::service::{
    subscription_cache::compile_geo_q,
    subscription_matching::{matches_compiled_geo_query, matches_scope_query},
};

const INTERVAL_NOTIFICATION_CONCURRENCY: usize = 20;

pub struct IntervalWorker {
    sub_repo: SubscriptionRepository,
    entity_repo: EntityRepository,
    transport: Arc<TransportDispatcher>,
    poll_interval: Duration,
    engine: JsonLdEngine,
    tenant: crate::tenancy::types::TenantRef,
}

impl IntervalWorker {
    pub const fn new(
        sub_repo: SubscriptionRepository,
        entity_repo: EntityRepository,
        engine: JsonLdEngine,
        transport: Arc<TransportDispatcher>,
        tenant: crate::tenancy::types::TenantRef,
    ) -> Self {
        Self {
            sub_repo,
            entity_repo,
            transport,
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

            let this = &self;
            let _results: Vec<()> = stream::iter(due_subs)
                .map(|sub| async move {
                    // Fetch entities matching the subscription's entity selectors
                    let entities = match this.fetch_matching_entities(&sub).await {
                        Ok(e) => e,
                        Err(e) => {
                            error!("IntervalWorker failed to fetch entities for {}: {}", sub.id, e);
                            if let Err(e) = this.sub_repo.mark_failure(sub.id.as_ref()).await {
                                debug!("Failed to mark failure for {}: {}", sub.id, e);
                            }
                            return;
                        }
                    };

                    // Apply q/geoQ/scopeQ filters
                    let entities = apply_subscription_filters(&sub, entities);

                    if entities.is_empty() {
                        // Per spec: if no matching entities, no notification is sent
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
                        return;
                    }

                    // Claim slot only once we know we are actually going to send a notification.
                    let claimed = match this.sub_repo.claim_notification_slot(sub.id.as_ref(), None).await {
                        Ok(c) => c,
                        Err(e) => {
                            debug!("IntervalWorker failed to claim slot for {}: {}", sub.id, e);
                            return;
                        }
                    };

                    if !claimed {
                        return;
                    }

                    let notification_id = match format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Uri>() {
                        Ok(u) => u,
                        Err(_) => {
                            error!("Failed to construct notification URN for {}", sub.id);
                            return;
                        }
                    };
                    // clone: the notification owns its subscription id and the
                    // subscription is still needed below
                    let notification = Notification::builder()
                        .id(notification_id)
                        .subscription_id(sub.id.clone())
                        .notified_at(Utc::now())
                        .data(data)
                        .build();

                    let payload = match serde_json::to_value(&notification) {
                        Ok(v) => v,
                        Err(e) => {
                            error!("Failed to serialize notification for {}: {}", sub.id, e);
                            return;
                        }
                    };

                    let endpoint_uri = sub.notification.endpoint.uri.as_str();
                    let accept = sub.notification.endpoint.accept.as_ref().map(MediaTypeBuf::as_str);
                    let jsonld_context = sub.delivery.jsonld_context.as_ref().map(Url::as_str);

                    let endpoint_transport = match parse_endpoint_transport(endpoint_uri, sub.notification.endpoint.notifier_info.as_deref()) {
                        Ok(t) => t,
                        Err(e) => {
                            debug!("IntervalWorker: invalid endpoint URI for {}: {}", sub.id, e);
                            return;
                        }
                    };

                    let request = match build_delivery_request(
                        &payload,
                        endpoint_uri,
                        accept,
                        sub.notification.endpoint.receiver_info.as_deref(),
                        jsonld_context,
                        this.tenant.tenant_name(),
                        sub.id.as_ref(),
                    ) {
                        Ok(r) => r,
                        Err(e) => {
                            debug!("IntervalWorker: failed to build delivery request for {}: {}", sub.id, e);
                            return;
                        }
                    };

                    let success = matches!(this.transport.deliver(&endpoint_transport, &request).await, DeliveryResult::Delivered);

                    if success {
                        if let Err(e) = this.sub_repo.mark_success(sub.id.as_ref()).await {
                            debug!("Failed to mark success for {}: {}", sub.id, e);
                        }
                    } else if let Err(e) = this.sub_repo.mark_failure(sub.id.as_ref()).await {
                        debug!("Failed to mark failure for {}: {}", sub.id, e);
                    }
                })
                .buffer_unordered(INTERVAL_NOTIFICATION_CONCURRENCY)
                .collect()
                .await;
        }
    }

    async fn fetch_matching_entities(&self, sub: &Subscription) -> Result<Vec<Value>, crate::core::error::AndromedaError> {
        let mut all_entities = Vec::new();

        if let Some(ref selectors) = sub.selection.entities {
            for selector in selectors {
                // A by-type lookup takes one type, so only a leaf selector can
                // be pushed down; a boolean expression reads the candidates and
                // is applied in memory. Passing the expression text as a type
                // name, which is what happened before it was parsed, matched
                // nothing at all.
                let entities = match selector.type_.as_single_type() {
                    Some(type_iri) => self.entity_repo.get_entities_by_type(type_iri, None, None).await?,
                    None => self.entity_repo.get_all_entities(None, None).await?,
                };

                for entity in entities {
                    if selector.type_.as_single_type().is_none() && !selector.type_.matches(&entity_types(&entity)) {
                        continue;
                    }

                    // Apply id / idPattern filtering
                    let entity_id = entity
                        .as_array()
                        .and_then(|arr| arr.first())
                        .or(Some(&entity))
                        .and_then(|e| e.get("@id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if let Some(ref id) = selector.id
                        && entity_id != id.to_string()
                    {
                        continue;
                    }

                    if let Some(ref pattern) = selector.id_pattern
                        && !pattern.is_match(entity_id)
                    {
                        continue;
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

    async fn prepare_notification_entities(&self, sub: &Subscription, entities: Vec<Value>) -> Result<Vec<Value>, crate::core::error::AndromedaError> {
        let mut results = Vec::new();
        for mut entity in entities {
            if sub.notification.sys_attrs.unwrap_or(false)
                && let Some(id) = extract_entity_id(&entity)
                && let Ok(Some(sa)) = self.entity_repo.get_sys_attrs(&id).await
            {
                inject_sys_attrs(&mut entity, sa.created_at, sa.modified_at, sa.expires_at);
            }

            let filtered = if let Some(ref attrs) = sub.notification.attributes {
                filter_entity_attributes(&entity, attrs)
            } else {
                entity
            };

            let context = context_for_subscription_raw(sub.delivery.jsonld_context.as_ref().map(Url::as_str));
            let include_context = sub
                .notification
                .endpoint
                .accept
                .as_ref()
                .is_some_and(|accept| accept.essence() == APPLICATION_LD_JSON);
            let compacted = match self
                .engine
                .compact(
                    filtered.clone(),
                    &context,
                    CompactOptions {
                        strip_context: if include_context { ContextStripping::Keep } else { ContextStripping::Strip },
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
                                strip_context: if include_context { ContextStripping::Keep } else { ContextStripping::Strip },
                            },
                        )
                        .await
                        .map_err(|e| crate::core::error::AndromedaError::internal(format!("Compaction failed: {}", e)))?
                }
            };

            let compacted = normalize_compacted_entity(compacted);

            let formatted = if let Some(fmt) = sub.notification.format {
                if fmt.is_simplified() {
                    match serde_json::from_value::<Entity>(compacted.clone()) {
                        Ok(entity) => simplify_entity(&entity),
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

/// The expanded type IRIs an entity payload declares.
fn entity_types(entity: &Value) -> Vec<&str> {
    let node = match entity {
        Value::Array(arr) => arr.first(),
        object @ Value::Object(_) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    };
    node.and_then(|node| node.get("@type"))
        .and_then(Value::as_array)
        .map(|types| types.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
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
    let compiled_q = sub.filters.q.as_ref();

    let compiled_geo = sub.filters.geo_q.as_ref().and_then(|v| compile_geo_q(v, sub.id.as_ref()));
    let compiled_scope = sub.filters.scope_q.as_ref();

    let has_filters = compiled_q.is_some() || compiled_geo.is_some() || compiled_scope.is_some();
    if !has_filters {
        return entities;
    }

    entities
        .into_iter()
        .filter(|entity| {
            if let Some(expr) = compiled_q
                && !eval::evaluate(expr, entity)
            {
                return false;
            }
            if let Some(ref geo_q) = compiled_geo
                && !matches_compiled_geo_query(geo_q, entity)
            {
                return false;
            }
            if let Some(scope_q) = compiled_scope
                && !matches_scope_query(scope_q, entity)
            {
                return false;
            }
            true
        })
        .collect()
}

use crate::worker::notification::payload::{
    context::{context_from_url, normalize_compacted_entity},
    filtering::filter_entity_attributes,
};
use andromeda_delivery::{dispatch::TransportDispatcher, endpoint::transport::parse_endpoint_transport, request::NotificationDelivery};
use andromeda_entity::{
    port::{discovery::EntityDiscoveryStore, reader::EntityReader},
    sys_attrs::inject_sys_attrs,
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
};
use andromeda_ngsi_ld_http_headers::media_type::APPLICATION_LD_JSON;
use andromeda_ngsi_ld_lexicon::{
    entity_type_name::EntityTypeName,
    uri::{NGSI_LD_URN_NID, Uri},
};
use andromeda_ngsi_ld_model::{entity::Entity, notification::Notification, subscription::Subscription};
use andromeda_ngsi_ld_query::filter::eval;
use andromeda_ngsi_ld_render::simplify::simplify_entity;
use andromeda_postgresql_context_store::subscription::repo::SubscriptionRepository;
use andromeda_subscription::{
    compilation::compile_geo_q,
    matching::{matches_geo_query, matches_scope_query},
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, info};
use uuid::Uuid;

const INTERVAL_NOTIFICATION_CONCURRENCY: usize = 20;

/// Rows read per round trip while a periodic notification assembles its
/// candidate set.
///
/// A selector whose type is a boolean expression cannot be pushed into a
/// `WHERE`, so the broker reads every entity in the tenant and decides in
/// memory. Reading that in pages bounds one round trip, and discarding
/// non-matching rows as each page arrives is what bounds what is held.
const INTERVAL_ENTITY_PAGE: u16 = 500;

pub struct IntervalWorker {
    sub_repo: SubscriptionRepository,
    entity_reader: Arc<dyn EntityReader>,
    entity_discovery: Arc<dyn EntityDiscoveryStore>,
    transport: Arc<TransportDispatcher>,
    poll_interval: Duration,
    engine: JsonLdEngine,
    tenant: TenantRef,
}

impl IntervalWorker {
    pub const fn new(
        sub_repo: SubscriptionRepository,
        entity_reader: Arc<dyn EntityReader>,
        entity_discovery: Arc<dyn EntityDiscoveryStore>,
        engine: JsonLdEngine,
        transport: Arc<TransportDispatcher>,
        tenant: TenantRef,
    ) -> Self {
        Self {
            sub_repo,
            entity_reader,
            entity_discovery,
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
                            if let Err(e) = this.sub_repo.mark_failure(&sub.id).await {
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
                    let claimed = match this.sub_repo.claim_notification_slot(&sub.id, None).await {
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

                    let endpoint_uri = &sub.notification.endpoint.uri;

                    let endpoint_transport = match parse_endpoint_transport(endpoint_uri, sub.notification.endpoint.notifier_info.as_deref()) {
                        Ok(t) => t,
                        Err(e) => {
                            debug!("IntervalWorker: invalid endpoint URI for {}: {}", sub.id, e);
                            return;
                        }
                    };

                    let request = (NotificationDelivery {
                        payload: &payload,
                        endpoint_uri: &sub.notification.endpoint.uri,
                        accept: sub.notification.endpoint.accept.as_ref(),
                        receiver_info: sub.notification.endpoint.receiver_info.as_deref(),
                        jsonld_context: sub.delivery.jsonld_context.as_ref(),
                        tenant: this.tenant.tenant_name(),
                        subscription_id: &sub.id,
                    })
                    .assemble();

                    let success = this.transport.deliver(&endpoint_transport, &request).await.is_delivered();

                    if success {
                        if let Err(e) = this.sub_repo.mark_success(&sub.id).await {
                            debug!("Failed to mark success for {}: {}", sub.id, e);
                        }
                    } else if let Err(e) = this.sub_repo.mark_failure(&sub.id).await {
                        debug!("Failed to mark failure for {}: {}", sub.id, e);
                    }
                })
                .buffer_unordered(INTERVAL_NOTIFICATION_CONCURRENCY)
                .collect()
                .await;
        }
    }

    async fn fetch_matching_entities(&self, sub: &Subscription) -> Result<Vec<Value>, crate::error::AndromedaError> {
        let Some(selectors) = sub.selection.entities.as_ref() else {
            // No entity selectors — every entity qualifies.
            return self.collect_pages(None, |_| true).await;
        };

        let mut all_entities = Vec::new();
        for selector in selectors {
            // A by-type lookup takes one type, so only a leaf selector can be
            // pushed down; a boolean expression reads every entity and is
            // applied in memory. Passing the expression text as a type name,
            // which is what happened before it was parsed, matched nothing at
            // all.
            let single_type = selector.type_.as_single_type();
            let matched = self
                .collect_pages(single_type, |entity| {
                    if single_type.is_none() && !selector.type_.matches(&entity_types(entity)) {
                        return false;
                    }
                    let Some(entity_id) = entity_id_of(entity) else {
                        return false;
                    };
                    if let Some(id) = selector.id.as_ref()
                        && &entity_id != id
                    {
                        return false;
                    }
                    selector.id_pattern.as_ref().is_none_or(|pattern| pattern.is_match(entity_id.as_str()))
                })
                .await?;
            all_entities.extend(matched);
        }

        Ok(all_entities)
    }

    /// Read entities a page at a time, keeping only those `keep` accepts.
    ///
    /// `type_iri` narrows the read to one entity type when the selector names
    /// exactly one; otherwise every entity is a candidate and the predicate is
    /// the only filter. See [`INTERVAL_ENTITY_PAGE`] for why this pages.
    async fn collect_pages(
        &self,
        entity_type: Option<&EntityTypeName>,
        mut keep: impl FnMut(&Value) -> bool,
    ) -> Result<Vec<Value>, crate::error::AndromedaError> {
        let limit = i64::from(INTERVAL_ENTITY_PAGE);
        let mut kept = Vec::new();
        let mut offset = 0_i64;
        loop {
            let page = match entity_type {
                Some(type_name) => self.entity_discovery.get_entities_by_type(type_name, Some(limit), Some(offset)).await?,
                None => self.entity_discovery.get_all_entities(Some(limit), Some(offset)).await?,
            };
            let exhausted = page.len() < usize::from(INTERVAL_ENTITY_PAGE);
            kept.extend(page.into_iter().filter(|entity| keep(entity)));
            if exhausted {
                return Ok(kept);
            }
            offset += limit;
        }
    }

    async fn prepare_notification_entities(&self, sub: &Subscription, entities: Vec<Value>) -> Result<Vec<Value>, crate::error::AndromedaError> {
        let mut results = Vec::new();
        for mut entity in entities {
            if sub.notification.sys_attrs.unwrap_or(false)
                && let Some(id) = entity_id_of(&entity)
                && let Ok(Some(sa)) = self.entity_reader.get_sys_attrs(&id).await
            {
                inject_sys_attrs(&mut entity, sa.created_at, sa.modified_at, sa.expires_at);
            }

            let filtered = if let Some(ref attrs) = sub.notification.attributes {
                filter_entity_attributes(&entity, attrs)
            } else {
                entity
            };

            let context = context_from_url(sub.delivery.jsonld_context.as_ref());
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
                        .map_err(|e| crate::error::AndromedaError::internal(format!("Compaction failed: {}", e)))?
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

/// The node of an expanded entity payload, which the store hands back either
/// bare or wrapped in a one-element array.
fn entity_node(entity: &Value) -> Option<&Value> {
    match entity {
        Value::Array(arr) => arr.first(),
        object @ Value::Object(_) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

/// The expanded type IRIs an entity payload declares.
fn entity_types(entity: &Value) -> Vec<EntityTypeName> {
    entity_node(entity)
        .and_then(|node| node.get("@type"))
        .and_then(Value::as_array)
        .map(|types| {
            types
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|raw| raw.parse::<EntityTypeName>().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The validated `@id` an expanded entity payload carries.
fn entity_id_of(entity: &Value) -> Option<Uri> {
    entity_node(entity)?.get("@id")?.as_str()?.parse().ok()
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
                && !matches_geo_query(geo_q, entity)
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

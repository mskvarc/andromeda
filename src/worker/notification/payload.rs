//! Notification payload building: showChanges, previous values, datasetId
//! filtering, nullification, and format simplification.

use crate::{
    service::{
        compiled_subscription::CompiledSubscription,
        entity::{
            query::EntityQueryService,
            sys_attrs::{expanded_entity_object_mut, inject_attr_sys_attrs},
        },
    },
    worker::notification::payload::{
        context::{context_for_subscription, normalize_compacted_entity, strip_to_structural_keys},
        deletion::{fix_null_language_maps, inject_deleted_attributes, nullify_entity_attributes},
        filtering::{filter_entity_attributes, filter_entity_by_dataset_id, inject_previous_values},
    },
};
use andromeda_delivery::request::notification_media_type;
use andromeda_jsonld::{
    context_spec::{ContextSpec, CoreContextPolicy},
    engine::{
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, rfc3339::whole_seconds};
use andromeda_ngsi_ld_model::{entity::Entity, notification::NotificationTrigger};
use andromeda_ngsi_ld_render::{simplify::simplify_entity, snapshot::SnapshotRenderer, uri_decoding::decode_percent_encoded_uris};
use andromeda_ngsi_ld_representation::{response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_vocabulary::{
    expanded::{classes as expanded_classes, properties as expanded_properties},
    prefix,
};
use chrono::{DateTime, Utc};
use iri_rs::IriRefBuf;
use serde_json::Value;
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::debug;
use url::Url;

/// Cached output of `resolve_linked_entities` + per-entity JSON
/// serialization. Shared across subscriptions that issue the same
/// `(root_entity, join_level, sys_attrs, context)` resolution during a
/// single event fan-out so the DB round-trips and serialization work run
/// only once per distinct resolution shape.
#[derive(Debug)]
pub struct ResolvedLinkedEntities {
    /// Entities keyed by URI, as returned by `resolve_linked_entities`.
    pub by_id: Arc<std::collections::HashMap<String, Entity>>,
    /// `Vec<Value>` matching `by_id.values()` pre-serialized once. Used by the
    /// `join=flat` notification path to append linked entities to the
    /// notification data array.
    pub as_values: Arc<Vec<Value>>,
}

pub struct NotificationBuildParams<'a> {
    pub engine: &'a JsonLdEngine,
    pub entity_data: &'a Value,
    pub previous_attributes: Option<&'a Value>,
    pub sub: &'a CompiledSubscription,
    pub trigger_reason: &'a NotificationTrigger,
    pub changed_attributes: &'a [AttributeName],
    pub occurred_at: DateTime<Utc>,
    pub entity_query: &'a EntityQueryService,
    pub compaction_cache: Option<&'a NotificationCompactionCache>,
}

#[derive(Default)]
pub struct NotificationCompactionCache {
    entries: Mutex<std::collections::HashMap<u64, Value>>,
    linked_entities: Mutex<std::collections::HashMap<u64, ResolvedLinkedEntities>>,
}

pub async fn build_notification_data(params: NotificationBuildParams<'_>) -> Vec<Value> {
    let NotificationBuildParams {
        engine,
        entity_data,
        previous_attributes,
        sub,
        trigger_reason,
        changed_attributes,
        occurred_at,
        entity_query,
        compaction_cache,
    } = params;

    let mut entity = match entity_data {
        Value::Array(arr) => arr.first().cloned().unwrap_or(Value::Null),
        other => other.clone(),
    };

    if entity.is_null() {
        return vec![];
    }

    let is_entity_deleted = matches!(trigger_reason, NotificationTrigger::EntityDeleted);
    let is_attr_deleted = matches!(trigger_reason, NotificationTrigger::AttributeDeleted);

    if is_entity_deleted && !sub.notification_show_changes {
        entity = strip_to_structural_keys(&entity, sub.notification_sys_attrs);
    }

    if is_entity_deleted && sub.notification_show_changes {
        nullify_entity_attributes(&mut entity);
    }

    if is_attr_deleted {
        let deleted_at = if sub.notification_sys_attrs == SysAttrsInclusion::Include {
            Some(occurred_at)
        } else {
            None
        };
        let filtered_changed: Vec<AttributeName>;
        let effective_changed = if let Some(ref watched) = sub.watched_attrs {
            filtered_changed = changed_attributes.iter().filter(|a| watched.contains(a.as_str())).cloned().collect();
            &filtered_changed
        } else {
            changed_attributes
        };
        inject_deleted_attributes(&mut entity, effective_changed, previous_attributes, deleted_at);

        if sub.notification_sys_attrs == SysAttrsInclusion::Include
            && let Some(obj) = expanded_entity_object_mut(&mut entity)
        {
            let ca = obj.get(expanded_properties::CREATED_AT.as_str()).cloned();
            let ma = obj.get(expanded_properties::MODIFIED_AT.as_str()).cloned();
            if let (Some(ca), Some(ma)) = (ca, ma) {
                let default_context_prefix = format!("{}default-context/", prefix::NGSI_LD.as_str());
                inject_attr_sys_attrs(obj, &ca, &ma, &default_context_prefix);
            }
        }
    }

    if let Some(ref dataset_ids) = sub.dataset_id {
        filter_entity_by_dataset_id(&mut entity, dataset_ids);
    }

    let entity = if let Some(ref attrs) = sub.notification_attributes {
        filter_entity_attributes(&entity, attrs)
    } else {
        entity
    };

    let context = context_for_subscription(sub);
    // The same reading the transport makes: a body carries its own `@context`
    // only when the notification goes out as `application/ld+json`.
    let include_context = notification_media_type(sub.endpoint_accept.as_ref()).include_context_in_body();

    let compacted = match compact_entity_with_cache(
        engine,
        &entity,
        &context,
        if include_context { ContextStripping::Keep } else { ContextStripping::Strip },
        compaction_cache,
    )
    .await
    {
        Ok(value) => value,
        Err(e) => {
            debug!("Failed to compact notification entity with subscription context: {}", e);
            match compact_entity_with_cache(
                engine,
                &entity,
                &ContextSpec::core_only(),
                if include_context { ContextStripping::Keep } else { ContextStripping::Strip },
                compaction_cache,
            )
            .await
            {
                Ok(value) => value,
                Err(e) => {
                    debug!("Failed to compact notification entity with core context: {}", e);
                    return vec![];
                }
            }
        }
    };

    let compacted = if include_context {
        if let Some(ref ctx_url) = sub.jsonld_context {
            let mut c = compacted;
            if let Some(obj) = c.as_object_mut() {
                obj.insert("@context".to_string(), Value::String(ctx_url.as_str().to_owned()));
            }
            c
        } else {
            compacted
        }
    } else {
        compacted
    };

    let mut compacted = normalize_compacted_entity(compacted);

    if is_entity_deleted && sub.notification_show_changes {
        fix_null_language_maps(&mut compacted);
    }

    if sub.notification_show_changes
        && let Some(prev) = previous_attributes
    {
        let prev_entity = match prev {
            Value::Array(arr) => arr.first().cloned().unwrap_or(Value::Null),
            other => other.clone(),
        };
        if let Ok(prev_compacted) = engine
            .compact(
                prev_entity,
                &context,
                CompactOptions {
                    strip_context: ContextStripping::Strip,
                },
            )
            .await
        {
            let prev_compacted = normalize_compacted_entity(prev_compacted);
            inject_previous_values(&mut compacted, &prev_compacted, changed_attributes, engine, &context).await;
        }
    }

    let mut all_compacted = vec![compacted];
    if let Some(ref join) = sub.notification_join {
        if join == "flat"
            && let Ok(main_entity) = serde_json::from_value::<Entity>(all_compacted[0].clone())
        {
            match resolve_linked_cached(
                entity_query,
                &main_entity,
                sub.notification_join_level,
                sub.notification_sys_attrs,
                &context,
                compaction_cache,
            )
            .await
            {
                Ok(resolved) => {
                    all_compacted.reserve(resolved.as_values.len());
                    for linked_val in resolved.as_values.iter() {
                        all_compacted.push(linked_val.clone());
                    }
                }
                Err(e) => {
                    debug!("Failed to resolve linked entities for join=flat: {}", e);
                }
            }
        } else if join == "inline"
            && let Ok(main_entity) = serde_json::from_value::<Entity>(all_compacted[0].clone())
        {
            match resolve_linked_cached(
                entity_query,
                &main_entity,
                sub.notification_join_level,
                sub.notification_sys_attrs,
                &context,
                compaction_cache,
            )
            .await
            {
                Ok(resolved) => {
                    let payload = std::mem::take(&mut all_compacted[0]);
                    match SnapshotRenderer::new(payload).with_inline_linked_entities(&resolved.by_id).render() {
                        Ok(rendered) => all_compacted[0] = rendered,
                        Err(error) => {
                            debug!("Failed to apply join=inline to notification payload: {}", error);
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to resolve linked entities for join=inline: {}", e);
                }
            }
        }
    }

    let is_simplified = sub.notification_format.as_deref().is_some_and(|f| f == "keyValues" || f == "simplified");
    let has_inline_join = sub.notification_join.as_deref() == Some("inline");
    let mut result = Vec::with_capacity(all_compacted.len());
    for item in all_compacted {
        let formatted = if is_simplified {
            if has_inline_join {
                match SnapshotRenderer::new(item.clone()).with_mode(ResponseFormat::Simplified).render() {
                    Ok(simplified) => simplified,
                    Err(error) => {
                        debug!("Failed to simplify inline-joined notification payload: {}", error);
                        item
                    }
                }
            } else {
                match serde_json::from_value::<Entity>(item.clone()) {
                    Ok(entity) => simplify_entity(&entity),
                    Err(e) => {
                        debug!("Failed to simplify entity for keyValues: {}", e);
                        item
                    }
                }
            }
        } else {
            item
        };
        let mut formatted = formatted;
        // The same targeted walk the query response uses (clause 4.5.1): only a
        // Relationship's `object` and a ListRelationship's `objectList` are
        // percent-encoded entity ids to decode, so a Property value carrying a
        // `%` is not corrupted, and a notification decodes exactly as a GET of
        // the same entity does.
        decode_percent_encoded_uris(&mut formatted);
        result.push(formatted);
    }

    result
}

/// Resolve and pre-serialize linked entities, caching by
/// `(root_entity_id, join_level, sys_attrs, context_digest)` on the shared
/// [`NotificationCompactionCache`]. The first subscription in a fan-out pays
/// the cost; subsequent subscriptions with the same resolution shape read
/// from the cache.
async fn resolve_linked_cached(
    entity_query: &EntityQueryService,
    main_entity: &Entity,
    join_level: u32,
    sys_attrs: SysAttrsInclusion,
    context: &ContextSpec,
    cache: Option<&NotificationCompactionCache>,
) -> Result<ResolvedLinkedEntities, crate::error::AndromedaError> {
    // Compute cache key even when no cache is provided -- cheap and keeps the
    // code path uniform with the cached branch.
    let key = linked_resolution_cache_key(main_entity, join_level, sys_attrs, context);
    if let Some(cache) = cache {
        let cached = cache.linked_entities.lock().await;
        if let Some(hit) = cached.get(&key) {
            return Ok(ResolvedLinkedEntities {
                by_id: Arc::clone(&hit.by_id),
                as_values: Arc::clone(&hit.as_values),
            });
        }
    }

    let linked_map = entity_query
        .resolve_linked_entities(std::slice::from_ref(main_entity), join_level, sys_attrs, context, &HashSet::new())
        .await?;
    let values: Vec<Value> = linked_map
        .values()
        .filter_map(|entity| match serde_json::to_value(entity) {
            Ok(v) => Some(v),
            Err(e) => {
                debug!("Failed to serialize linked entity: {}", e);
                None
            }
        })
        .collect();
    let resolved = ResolvedLinkedEntities {
        by_id: Arc::new(linked_map),
        as_values: Arc::new(values),
    };

    if let Some(cache) = cache {
        let mut cached = cache.linked_entities.lock().await;
        cached.entry(key).or_insert_with(|| ResolvedLinkedEntities {
            by_id: Arc::clone(&resolved.by_id),
            as_values: Arc::clone(&resolved.as_values),
        });
    }

    Ok(resolved)
}

fn linked_resolution_cache_key(main_entity: &Entity, join_level: u32, sys_attrs: SysAttrsInclusion, context: &ContextSpec) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    main_entity.id.to_string().hash(&mut hasher);
    hasher.write_u32(join_level);
    hasher.write_u8(match sys_attrs {
        SysAttrsInclusion::Include => 1,
        SysAttrsInclusion::Strip => 0,
    });
    match context.to_context_value() {
        Some(ctx_val) => {
            hasher.write_u8(1);
            hash_json_value(&ctx_val, &mut hasher);
        }
        None => hasher.write_u8(0),
    }
    hasher.finish()
}

async fn compact_entity_with_cache(
    engine: &JsonLdEngine,
    entity: &Value,
    context: &ContextSpec,
    strip_context: ContextStripping,
    cache: Option<&NotificationCompactionCache>,
) -> Result<Value, andromeda_jsonld::error::JsonLdError> {
    let Some(cache) = cache else {
        return engine.compact(entity.clone(), context, CompactOptions { strip_context }).await;
    };

    let key = compaction_cache_key(entity, context, strip_context);
    {
        let cache_guard = cache.entries.lock().await;
        if let Some(hit) = cache_guard.get(&key) {
            return Ok(hit.clone());
        }
    }

    let compacted = engine.compact(entity.clone(), context, CompactOptions { strip_context }).await?;
    let mut entries = cache.entries.lock().await;
    let cached = entries.entry(key).or_insert(compacted);
    Ok(cached.clone())
}

fn compaction_cache_key(entity: &Value, context: &ContextSpec, strip_context: ContextStripping) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hash_json_value(entity, &mut hasher);
    match context.to_context_value() {
        Some(ctx_val) => {
            hasher.write_u8(1);
            hash_json_value(&ctx_val, &mut hasher);
        }
        None => hasher.write_u8(0),
    }
    strip_context.hash(&mut hasher);
    hasher.finish()
}

/// Structural hash of a `serde_json::Value` without allocating a string representation.
fn hash_json_value(value: &Value, hasher: &mut impl Hasher) {
    match value {
        Value::Null => hasher.write_u8(0),
        Value::Bool(b) => {
            hasher.write_u8(1);
            hasher.write_u8(*b as u8);
        }
        Value::Number(n) => {
            hasher.write_u8(2);
            if let Some(i) = n.as_i64() {
                hasher.write_i64(i);
            } else if let Some(u) = n.as_u64() {
                hasher.write_u64(u);
            } else if let Some(f) = n.as_f64() {
                hasher.write_u64(f.to_bits());
            }
        }
        Value::String(s) => {
            hasher.write_u8(3);
            hasher.write(s.as_bytes());
            hasher.write_u8(0xFF);
        }
        Value::Array(arr) => {
            hasher.write_u8(4);
            hasher.write_usize(arr.len());
            for v in arr {
                hash_json_value(v, hasher);
            }
        }
        Value::Object(obj) => {
            hasher.write_u8(5);
            hasher.write_usize(obj.len());
            for (k, v) in obj {
                hasher.write(k.as_bytes());
                hasher.write_u8(0xFF);
                hash_json_value(v, hasher);
            }
        }
    }
}

/// Notification context selection and compacted-entity normalization.
pub mod context;
/// Deleted attribute and instance materialization.
mod deletion;
/// Notification filtering and previous-value materialization.
pub mod filtering;

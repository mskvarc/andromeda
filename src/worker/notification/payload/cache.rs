//! Per-event memoization for notification compaction and linked-entity resolution.

use crate::{error::AndromedaError, service::entity::query::EntityQueryService};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
    error::JsonLdError,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::debug;

/// Cached linked entities in both indexed and serialized form.
#[derive(Debug)]
pub(super) struct ResolvedLinkedEntities {
    pub(super) by_id: Arc<HashMap<Uri, Entity>>,
    pub(super) as_values: Arc<Vec<Value>>,
}

/// Cache shared across every notification produced for one change event.
#[derive(Default)]
pub(in crate::worker::notification) struct NotificationCompactionCache {
    entries: Mutex<HashMap<u64, Value>>,
    linked_entities: Mutex<HashMap<u64, ResolvedLinkedEntities>>,
}

pub(super) async fn resolve_linked_cached(
    entity_query: &EntityQueryService,
    main_entity: &Entity,
    join_level: u32,
    sys_attrs: SysAttrsInclusion,
    context: &ContextSpec,
    cache: Option<&NotificationCompactionCache>,
) -> Result<ResolvedLinkedEntities, AndromedaError> {
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
    let values = linked_map
        .values()
        .filter_map(|entity| match serde_json::to_value(entity) {
            Ok(value) => Some(value),
            Err(error) => {
                debug!(%error, "failed to serialize linked notification entity");
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

pub(super) async fn compact_entity_with_cache(
    engine: &JsonLdEngine,
    entity: &Value,
    context: &ContextSpec,
    strip_context: ContextStripping,
    cache: Option<&NotificationCompactionCache>,
) -> Result<Value, JsonLdError> {
    let Some(cache) = cache else {
        return engine.compact(entity.clone(), context, CompactOptions { strip_context }).await;
    };
    let key = compaction_cache_key(entity, context, strip_context);
    {
        let entries = cache.entries.lock().await;
        if let Some(hit) = entries.get(&key) {
            return Ok(hit.clone());
        }
    }
    let compacted = engine.compact(entity.clone(), context, CompactOptions { strip_context }).await?;
    let mut entries = cache.entries.lock().await;
    Ok(entries.entry(key).or_insert(compacted).clone())
}

fn linked_resolution_cache_key(main_entity: &Entity, join_level: u32, sys_attrs: SysAttrsInclusion, context: &ContextSpec) -> u64 {
    let mut hasher = DefaultHasher::new();
    main_entity.id.hash(&mut hasher);
    hasher.write_u32(join_level);
    hasher.write_u8(match sys_attrs {
        SysAttrsInclusion::Include => 1,
        SysAttrsInclusion::Strip => 0,
    });
    hash_optional_context(context, &mut hasher);
    hasher.finish()
}

fn compaction_cache_key(entity: &Value, context: &ContextSpec, strip_context: ContextStripping) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_json_value(entity, &mut hasher);
    hash_optional_context(context, &mut hasher);
    strip_context.hash(&mut hasher);
    hasher.finish()
}

fn hash_optional_context(context: &ContextSpec, hasher: &mut impl Hasher) {
    match context.to_context_value() {
        Some(value) => {
            hasher.write_u8(1);
            hash_json_value(&value, hasher);
        }
        None => hasher.write_u8(0),
    }
}

fn hash_json_value(value: &Value, hasher: &mut impl Hasher) {
    match value {
        Value::Null => hasher.write_u8(0),
        Value::Bool(value) => {
            hasher.write_u8(1);
            hasher.write_u8(u8::from(*value));
        }
        Value::Number(value) => {
            hasher.write_u8(2);
            if let Some(integer) = value.as_i64() {
                hasher.write_i64(integer);
            } else if let Some(integer) = value.as_u64() {
                hasher.write_u64(integer);
            } else if let Some(float) = value.as_f64() {
                hasher.write_u64(float.to_bits());
            }
        }
        Value::String(value) => {
            hasher.write_u8(3);
            hasher.write(value.as_bytes());
        }
        Value::Array(values) => {
            hasher.write_u8(4);
            hasher.write_usize(values.len());
            values.iter().for_each(|value| hash_json_value(value, hasher));
        }
        Value::Object(object) => {
            hasher.write_u8(5);
            hasher.write_usize(object.len());
            object.iter().for_each(|(key, value)| {
                hasher.write(key.as_bytes());
                hash_json_value(value, hasher);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::worker::notification::payload::cache::hash_json_value;
    use serde_json::json;
    use std::{collections::hash_map::DefaultHasher, hash::Hasher};

    #[test]
    fn structural_hash_is_stable_for_equal_values() {
        let value = json!({"a": [1, true, null]});
        let mut first = DefaultHasher::new();
        let mut second = DefaultHasher::new();
        hash_json_value(&value, &mut first);
        hash_json_value(&value, &mut second);
        pretty_assertions::assert_eq!(first.finish(), second.finish());
    }
}

//! Per-event memoization for notification compaction and linked-entity resolution.

use crate::error::NotificationError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
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
use dashmap::DashMap;
use indexmap::IndexMap;
use serde_json::Value;
use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    num::NonZeroU32,
    slice::from_ref,
    sync::Arc,
};
use tracing::{debug, warn};

/// Cached linked entities in both indexed and serialized form.
#[derive(Debug)]
pub(super) struct ResolvedLinkedEntities {
    pub(super) by_id: Arc<IndexMap<Uri, Entity>>,
    pub(super) as_values: Arc<Vec<Value>>,
}

/// Cache shared across every notification produced for one change event.
///
/// Both maps are keyed by a 64-bit structural hash rather than by the inputs
/// themselves: the compaction key covers a whole entity document plus its
/// context, which would be far more expensive to store and compare than the
/// work it saves. A collision would therefore serve one subscriber a payload
/// computed for another, which is why the hash is over every discriminating
/// input; at the handful of distinct payloads one change event produces, the
/// probability is negligible against the cost of keying by the documents.
#[derive(Default)]
pub(crate) struct NotificationCompactionCache {
    entries: DashMap<u64, Value>,
    linked_entities: DashMap<u64, ResolvedLinkedEntities>,
}

pub(super) async fn resolve_linked_cached(
    target: &ContextTarget,
    entity_query: &EntityQueryService,
    main_entity: &Entity,
    join_level: NonZeroU32,
    sys_attrs: SysAttrsInclusion,
    context: &ContextSpec,
    cache: Option<&NotificationCompactionCache>,
) -> Result<ResolvedLinkedEntities, NotificationError> {
    let key = linked_resolution_cache_key(main_entity, join_level, sys_attrs, context);
    // The map reference is confined to this block: no entry guard may be held
    // across the resolution await below, which is why a miss computes outside
    // the map and races another notification to insert rather than holding the
    // shard while it works.
    if let Some(cache) = cache
        && let Some(hit) = cache.linked_entities.get(&key)
    {
        return Ok(ResolvedLinkedEntities {
            by_id: Arc::clone(&hit.by_id),
            as_values: Arc::clone(&hit.as_values),
        });
    }
    let resolution = entity_query
        .resolve_linked_entities(target, from_ref(main_entity), join_level, sys_attrs, context, &HashSet::new())
        .await
        .map_err(NotificationError::from)?;
    // A notification has no response to carry a clause 6.3.17 warning on, so an
    // unresolvable Linked Entity is reported to the operator instead of the
    // subscriber, and the notification goes out with the entity simply absent.
    for failure in &resolution.failures {
        warn!(
            entity = %failure.id,
            condition = %failure.kind,
            detail = %failure.detail,
            "linked entity left unresolved in a notification payload"
        );
    }
    let linked_map = resolution.entities;
    let values = linked_entities_as_values(&linked_map);
    let resolved = ResolvedLinkedEntities {
        by_id: Arc::new(linked_map),
        as_values: Arc::new(values),
    };
    if let Some(cache) = cache {
        cache.linked_entities.entry(key).or_insert_with(|| ResolvedLinkedEntities {
            by_id: Arc::clone(&resolved.by_id),
            as_values: Arc::clone(&resolved.as_values),
        });
    }
    Ok(resolved)
}

/// Serialize a resolved linked-entity map to its `join=flat` append order.
///
/// [`EntityQueryService::resolve_linked_entities`] guarantees `linked_map`'s
/// iteration order is the traversal's first-encounter order (clause 4.5.23.3),
/// and `IndexMap::values` walks a map in that same insertion order, so the
/// values collected here carry that order forward unchanged. An entity that
/// fails to serialize is dropped rather than failing the whole notification.
fn linked_entities_as_values(linked_map: &IndexMap<Uri, Entity>) -> Vec<Value> {
    linked_map
        .values()
        .filter_map(|entity| match serde_json::to_value(entity) {
            Ok(value) => Some(value),
            Err(error) => {
                debug!(%error, "failed to serialize linked notification entity");
                None
            }
        })
        .collect()
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
    // As in the linked resolution above, the entry guard never spans the
    // compaction await: a miss compacts outside the map, and two notifications
    // that miss together each compact and one insert wins.
    if let Some(hit) = cache.entries.get(&key) {
        return Ok(hit.clone()); // clone: the caller owns its payload, the cache keeps serving the entry
    }
    let compacted = engine.compact(entity.clone(), context, CompactOptions { strip_context }).await?;
    // clone: the cached entry stays behind for the notifications still to come
    Ok(cache.entries.entry(key).or_insert(compacted).clone())
}

fn linked_resolution_cache_key(main_entity: &Entity, join_level: NonZeroU32, sys_attrs: SysAttrsInclusion, context: &ContextSpec) -> u64 {
    let mut hasher = DefaultHasher::new();
    main_entity.id.hash(&mut hasher);
    hasher.write_u32(join_level.get());
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
    use crate::processor::payload::cache::{hash_json_value, linked_entities_as_values};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::entity::Entity;
    use indexmap::IndexMap;
    use serde_json::json;
    use std::{collections::hash_map::DefaultHasher, hash::Hasher};

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    fn entity(id: &str) -> Entity {
        serde_json::from_value(json!({"id": id, "type": "Thing"})).expect("entity")
    }

    #[test]
    fn structural_hash_is_stable_for_equal_values() {
        let value = json!({"a": [1, true, null]});
        let mut first = DefaultHasher::new();
        let mut second = DefaultHasher::new();
        hash_json_value(&value, &mut first);
        hash_json_value(&value, &mut second);
        pretty_assertions::assert_eq!(first.finish(), second.finish());
    }

    /// `linked_entities_as_values` must not reorder its input: the map's
    /// iteration order is the traversal's first-encounter order (clause
    /// 4.5.23.3), and the `join=flat` append in
    /// `crate::processor::payload::build_notification_data` relies on
    /// `as_values` carrying that order forward unchanged.
    #[test]
    fn as_values_preserves_the_indexmaps_traversal_encounter_order() {
        let mut linked_map = IndexMap::new();
        linked_map.insert(uri("urn:ngsi-ld:T:C"), entity("urn:ngsi-ld:T:C"));
        linked_map.insert(uri("urn:ngsi-ld:T:B"), entity("urn:ngsi-ld:T:B"));
        linked_map.insert(uri("urn:ngsi-ld:T:D"), entity("urn:ngsi-ld:T:D"));

        let values = linked_entities_as_values(&linked_map);

        let ids: Vec<&str> = values.iter().map(|value| value["id"].as_str().expect("id present")).collect();
        pretty_assertions::assert_eq!(ids, vec!["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B", "urn:ngsi-ld:T:D"]);
    }
}

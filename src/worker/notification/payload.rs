//! Notification payload building: showChanges, previous values, datasetId
//! filtering, nullification, and format simplification.

use crate::{
    domain::{
        entity::Entity,
        notification::{CompiledSubscription, TriggerReason},
    },
    jsonld::{CompactOptions, ContextSpec, ContextStripping, JsonLdEngine, vocab},
    renderer::{
        SysAttrsInclusion,
        entity::{apply_inline_join, simplify_value},
    },
    service::entity::{expanded_entity_object_mut, inject_attr_sys_attrs, query::EntityQueryService},
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
};
use tokio::sync::Mutex;
use tracing::debug;

pub struct NotificationBuildParams<'a> {
    pub engine: &'a JsonLdEngine,
    pub entity_data: &'a Value,
    pub previous_attributes: Option<&'a Value>,
    pub sub: &'a CompiledSubscription,
    pub trigger_reason: &'a TriggerReason,
    pub changed_attributes: &'a [String],
    pub occurred_at: DateTime<Utc>,
    pub entity_query: &'a EntityQueryService,
    pub compaction_cache: Option<&'a NotificationCompactionCache>,
}

#[derive(Default)]
pub struct NotificationCompactionCache {
    entries: Mutex<std::collections::HashMap<u64, Value>>,
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

    let is_entity_deleted = matches!(trigger_reason, TriggerReason::EntityDeleted);
    let is_attr_deleted = matches!(trigger_reason, TriggerReason::AttributeDeleted);

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
        let filtered_changed: Vec<String>;
        let effective_changed = if let Some(ref watched) = sub.watched_attrs {
            filtered_changed = changed_attributes.iter().filter(|a| watched.contains(*a)).cloned().collect();
            &filtered_changed
        } else {
            changed_attributes
        };
        inject_deleted_attributes(&mut entity, effective_changed, previous_attributes, deleted_at);

        if sub.notification_sys_attrs == SysAttrsInclusion::Include
            && let Some(obj) = expanded_entity_object_mut(&mut entity)
        {
            let ca = obj.get(vocab::expanded::CREATED_AT.as_str()).cloned();
            let ma = obj.get(vocab::expanded::MODIFIED_AT.as_str()).cloned();
            if let (Some(ca), Some(ma)) = (ca, ma) {
                let default_context_prefix = format!("{}default-context/", vocab::prefix::NGSI_LD.as_str());
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
    let include_context = sub.endpoint_accept.as_deref().is_some_and(|v| v == "application/ld+json");

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
                obj.insert("@context".to_string(), Value::String(ctx_url.clone()));
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
        if join == "flat" {
            if let Ok(main_entity) = serde_json::from_value::<Entity>(all_compacted[0].clone()) {
                let sys_attrs = sub.notification_sys_attrs;
                match entity_query
                    .resolve_linked_entities(
                        &[main_entity],
                        sub.notification_join_level,
                        sys_attrs,
                        &context,
                        &std::collections::HashSet::new(),
                    )
                    .await
                {
                    Ok(linked_map) => {
                        for linked_entity in linked_map.values() {
                            if let Ok(linked_val) = serde_json::to_value(linked_entity) {
                                all_compacted.push(linked_val);
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Failed to resolve linked entities for join=flat: {}", e);
                    }
                }
            }
        } else if join == "inline"
            && let Ok(main_entity) = serde_json::from_value::<Entity>(all_compacted[0].clone())
        {
            let sys_attrs = sub.notification_sys_attrs;
            match entity_query
                .resolve_linked_entities(
                    &[main_entity],
                    sub.notification_join_level,
                    sys_attrs,
                    &context,
                    &std::collections::HashSet::new(),
                )
                .await
            {
                Ok(linked_map) => {
                    if let Err(error) = apply_inline_join(&mut all_compacted[0], &linked_map, None) {
                        debug!("Failed to apply join=inline to notification payload: {}", error);
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
                simplify_value(&item)
            } else {
                match serde_json::from_value::<Entity>(item.clone()) {
                    Ok(entity) => entity.simplify(),
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
        decode_percent_encoded_strings(&mut formatted);
        result.push(formatted);
    }

    result
}

async fn compact_entity_with_cache(
    engine: &JsonLdEngine,
    entity: &Value,
    context: &ContextSpec,
    strip_context: ContextStripping,
    cache: Option<&NotificationCompactionCache>,
) -> Result<Value, crate::jsonld::Error> {
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

pub fn context_for_subscription(sub: &CompiledSubscription) -> ContextSpec {
    context_for_subscription_raw(sub.jsonld_context.as_deref())
}

/// Build a ContextSpec from an optional jsonldContext string.
/// Shared by both notification_worker and interval_worker.
pub fn context_for_subscription_raw(jsonld_context: Option<&str>) -> ContextSpec {
    if let Some(url) = jsonld_context {
        let trimmed = url.trim();
        if ((trimmed.starts_with('{') && trimmed.ends_with('}')) || (trimmed.starts_with('[') && trimmed.ends_with(']')))
            && let Ok(value) = serde_json::from_str::<Value>(trimmed)
        {
            return ContextSpec::none()
                .with_body_context(Some(value))
                .with_core_policy(crate::jsonld::CoreContextPolicy::Append);
        }
        ContextSpec::from_links(vec![url.to_string()]).with_core_policy(crate::jsonld::CoreContextPolicy::Append)
    } else {
        ContextSpec::none().with_core_policy(crate::jsonld::CoreContextPolicy::Append)
    }
}

pub fn normalize_compacted_entity(mut entity: Value) -> Value {
    if let Value::Object(obj) = &mut entity {
        if !obj.contains_key("id")
            && let Some(id) = obj.remove("@id")
        {
            obj.insert("id".to_string(), id);
        }
        if !obj.contains_key("type")
            && let Some(t) = obj.remove("@type")
        {
            let normalized = match t {
                Value::Array(arr) if arr.len() == 1 => arr.into_iter().next().unwrap_or(Value::Null),
                other => other,
            };
            obj.insert("type".to_string(), normalized);
        }
    }
    entity
}

fn decode_percent_encoded_strings(value: &mut Value) {
    match value {
        Value::String(s) => {
            if s.contains('%')
                && let Ok(decoded) = percent_encoding::percent_decode_str(s).decode_utf8()
            {
                *s = decoded.into_owned();
            }
        }
        Value::Object(obj) => {
            for v in obj.values_mut() {
                decode_percent_encoded_strings(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                decode_percent_encoded_strings(v);
            }
        }
        _ => {}
    }
}

pub fn strip_to_structural_keys(entity: &Value, sys_attrs: SysAttrsInclusion) -> Value {
    let Some(obj) = entity.as_object() else {
        return entity.clone();
    };
    let mut stripped = serde_json::Map::new();
    for (key, value) in obj {
        let keep = key == "@id"
            || key == "@type"
            || key == vocab::expanded::DELETED_AT.as_str()
            || (sys_attrs == SysAttrsInclusion::Include && (key == vocab::expanded::CREATED_AT.as_str() || key == vocab::expanded::MODIFIED_AT.as_str()));
        if keep {
            stripped.insert(key.clone(), value.clone());
        }
    }
    Value::Object(stripped)
}

fn nullify_entity_attributes(entity: &mut Value) {
    let Some(obj) = entity.as_object_mut() else { return };
    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| {
            !k.starts_with('@')
                && *k != vocab::expanded::CREATED_AT.as_str()
                && *k != vocab::expanded::MODIFIED_AT.as_str()
                && *k != vocab::expanded::DELETED_AT.as_str()
                && *k != vocab::expanded::SCOPE.as_str()
        })
        .cloned()
        .collect();
    for key in attr_keys {
        if let Some(attr_arr) = obj.get_mut(&key).and_then(|v| v.as_array_mut()) {
            for instance in attr_arr.iter_mut() {
                nullify_attribute_instance(instance);
            }
        }
    }
}

fn nullify_attribute_instance(instance: &mut Value) {
    let Some(obj) = instance.as_object_mut() else { return };

    let null_value = serde_json::json!([{"@value": vocab::expanded::NULL_URI}]);
    let null_object = serde_json::json!([{"@id": vocab::expanded::NULL_URI}]);
    let null_json = serde_json::json!([{"@value": vocab::expanded::NULL_URI, "@type": "@json"}]);
    let null_vocab = serde_json::json!([{"@id": vocab::expanded::NULL_URI}]);

    if obj.contains_key(vocab::expanded::OBJECT.as_str()) {
        obj.insert(vocab::expanded::OBJECT.as_str().to_string(), null_object);
    } else if obj.contains_key(vocab::expanded::LANGUAGE_MAP.as_str()) {
        obj.insert(vocab::expanded::LANGUAGE_MAP.as_str().to_string(), null_value);
    } else if obj.contains_key(vocab::expanded::JSON.as_str()) {
        obj.insert(vocab::expanded::JSON.as_str().to_string(), null_json);
    } else if obj.contains_key(vocab::expanded::VOCAB.as_str()) {
        obj.insert(vocab::expanded::VOCAB.as_str().to_string(), null_vocab);
    } else {
        obj.insert(vocab::expanded::VALUE.as_str().to_string(), null_value);
    }
}

fn fix_null_language_maps(entity: &mut Value) {
    let Some(obj) = entity.as_object_mut() else { return };
    for value in obj.values_mut() {
        let is_null_lang_map = value
            .as_object()
            .and_then(|attr| attr.get("languageMap"))
            .and_then(|lm| lm.as_object())
            .and_then(|lm_obj| {
                if lm_obj.len() == 1 {
                    lm_obj.get("@none").and_then(|v| v.as_str())
                } else {
                    None
                }
            })
            == Some(vocab::expanded::NULL_URI);
        if is_null_lang_map && let Some(attr) = value.as_object_mut() {
            attr.insert("languageMap".to_string(), Value::String(vocab::expanded::NULL_URI.as_str().to_string()));
        }
    }
}

fn inject_deleted_attributes(entity: &mut Value, changed_attributes: &[String], previous_attributes: Option<&Value>, deleted_at: Option<DateTime<Utc>>) {
    let Some(prev) = previous_attributes else { return };
    let prev_obj = match prev {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _ => None,
    };
    let Some(prev_obj) = prev_obj else { return };
    let Some(entity_obj) = entity.as_object_mut() else { return };

    for attr_iri in changed_attributes {
        if entity_obj.contains_key(attr_iri) {
            inject_deleted_instances(entity_obj, attr_iri, prev_obj, deleted_at);
            continue;
        }
        if let Some(prev_attr) = prev_obj.get(attr_iri) {
            let first_instance = match prev_attr {
                Value::Array(arr) => arr.first().cloned(),
                other => Some(other.clone()),
            };
            if let Some(mut instance) = first_instance {
                nullify_attribute_instance(&mut instance);
                if let Some(obj) = instance.as_object_mut() {
                    obj.remove(vocab::expanded::DATASET_ID.as_str());
                }
                if let Some(ts) = deleted_at {
                    inject_deleted_at_on_instance(&mut instance, ts);
                }
                entity_obj.insert(attr_iri.clone(), serde_json::json!([instance]));
            }
        }
    }
}

fn inject_deleted_instances(
    entity_obj: &mut serde_json::Map<String, Value>,
    attr_iri: &str,
    prev_obj: &serde_json::Map<String, Value>,
    deleted_at: Option<DateTime<Utc>>,
) {
    let Some(prev_attr) = prev_obj.get(attr_iri) else { return };
    let Some(prev_arr) = prev_attr.as_array() else { return };
    let Some(cur_arr) = entity_obj.get(attr_iri).and_then(|v| v.as_array()) else {
        return;
    };

    let current_dataset_ids: HashSet<Option<String>> = cur_arr
        .iter()
        .map(|inst| {
            inst.as_object()
                .and_then(|o| o.get(vocab::expanded::DATASET_ID.as_str()))
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_object())
                .and_then(|o| o.get("@id"))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();

    let mut deleted_instances: Vec<Value> = Vec::new();
    for prev_inst in prev_arr {
        let prev_ds_id = prev_inst
            .as_object()
            .and_then(|o| o.get(vocab::expanded::DATASET_ID.as_str()))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_object())
            .and_then(|o| o.get("@id"))
            .and_then(|v| v.as_str())
            .map(String::from);

        if !current_dataset_ids.contains(&prev_ds_id) {
            let mut nullified = prev_inst.clone();
            nullify_attribute_instance(&mut nullified);
            if let Some(ts) = deleted_at {
                inject_deleted_at_on_instance(&mut nullified, ts);
            }
            deleted_instances.push(nullified);
        }
    }

    if !deleted_instances.is_empty()
        && let Some(cur_arr) = entity_obj.get_mut(attr_iri).and_then(|v| v.as_array_mut())
    {
        cur_arr.extend(deleted_instances);
    }
}

fn inject_deleted_at_on_instance(instance: &mut Value, ts: DateTime<Utc>) {
    if let Some(obj) = instance.as_object_mut() {
        obj.insert(
            vocab::expanded::DELETED_AT.as_str().to_string(),
            serde_json::json!([{
                "@type": vocab::expanded::DATE_TIME.as_str(),
                "@value": ts.to_rfc3339_opts(SecondsFormat::Millis, true)
            }]),
        );
    }
}

pub fn filter_entity_attributes(entity: &Value, attrs: &[String]) -> Value {
    let Some(obj) = entity.as_object() else { return entity.clone() };

    let mut filtered = serde_json::Map::new();
    for (key, value) in obj {
        if key.starts_with('@') || attrs.iter().any(|a| a == key) {
            filtered.insert(key.clone(), value.clone());
        }
    }
    Value::Object(filtered)
}

fn filter_entity_by_dataset_id(entity: &mut Value, dataset_ids: &[String]) {
    let Some(obj) = expanded_entity_object_mut(entity) else { return };
    let default_context_prefix = format!("{}default-context/", vocab::prefix::NGSI_LD.as_str());

    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| !k.starts_with('@') && (!k.starts_with(vocab::prefix::NGSI_LD.as_str()) || k.starts_with(&default_context_prefix)))
        .cloned()
        .collect();

    for key in attr_keys {
        let Some(arr) = obj.get_mut(&key).and_then(|v| v.as_array_mut()) else {
            continue;
        };

        let filtered: Vec<Value> = arr
            .drain(..)
            .filter(|instance| {
                let instance_dataset_id = instance
                    .as_object()
                    .and_then(|o| o.get(vocab::expanded::DATASET_ID.as_str()))
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_object())
                    .and_then(|o| o.get("@id"))
                    .and_then(|v| v.as_str());

                match instance_dataset_id {
                    Some(id) => dataset_ids.iter().any(|d| d == id),
                    None => dataset_ids.is_empty(),
                }
            })
            .collect();

        if filtered.is_empty() {
            obj.remove(&key);
        } else if let Some(entry) = obj.get_mut(&key) {
            *entry = Value::Array(filtered);
        }
    }
}

async fn inject_previous_values(current: &mut Value, previous: &Value, changed_attributes: &[String], engine: &JsonLdEngine, context: &ContextSpec) {
    let (Some(curr_obj), Some(prev_obj)) = (current.as_object_mut(), previous.as_object()) else {
        return;
    };

    let changed_set: HashSet<&str> = changed_attributes.iter().map(String::as_str).collect();
    let keys: Vec<String> = curr_obj.keys().filter(|k| !k.starts_with('@') && *k != "id" && *k != "type").cloned().collect();

    for key in keys {
        if !changed_attributes.is_empty() {
            let expanded_key = engine.expand_term(&key, context).await.unwrap_or_else(|_| key.clone());
            if !changed_set.contains(expanded_key.as_str()) && !changed_set.contains(key.as_str()) {
                continue;
            }
        }

        if let Some(prev_val) = prev_obj.get(&key) {
            let curr_attr = curr_obj.get(&key).cloned();
            let previous_key = previous_key_for_attr(prev_val);

            if let Some(Value::Object(attr_obj)) = curr_attr.as_ref().and_then(|v| if v.is_object() { Some(v) } else { None }) {
                let mut updated_attr = attr_obj.clone();
                let prev_value = extract_previous_value(prev_val, previous_key);
                updated_attr.insert(previous_key.to_string(), prev_value);
                curr_obj.insert(key, Value::Object(updated_attr));
            }
        }
    }
}

fn previous_key_for_attr(val: &Value) -> &'static str {
    val.as_object()
        .and_then(|o| o.get("type"))
        .and_then(|t| t.as_str())
        .and_then(|s| s.parse::<crate::domain::attribute_type::AttributeType>().ok())
        .map_or("previousValue", |t| t.previous_key())
}

fn extract_previous_value(val: &Value, previous_key: &str) -> Value {
    if let Some(obj) = val.as_object() {
        match previous_key {
            "previousObject" => {
                if let Some(v) = obj.get("object") {
                    return v.clone();
                }
            }
            "previousLanguageMap" => {
                if let Some(v) = obj.get("languageMap") {
                    return v.clone();
                }
            }
            "previousJson" => {
                if let Some(v) = obj.get("json") {
                    return v.clone();
                }
            }
            "previousVocab" => {
                if let Some(v) = obj.get("vocab") {
                    return v.clone();
                }
            }
            _ => {
                if let Some(v) = obj.get("value") {
                    return v.clone();
                }
            }
        }
    }
    val.clone()
}

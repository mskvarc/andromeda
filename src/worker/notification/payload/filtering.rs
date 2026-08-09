//! Notification attribute, dataset, and previous-value filtering.

use crate::service::entity::sys_attrs::expanded_entity_object_mut;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_vocabulary::{expanded::properties as expanded_properties, prefix};
use serde_json::Value;

pub fn filter_entity_attributes(entity: &Value, attrs: &[AttributeName]) -> Value {
    let Some(obj) = entity.as_object() else { return entity.clone() };

    let mut filtered = serde_json::Map::new();
    for (key, value) in obj {
        if key.starts_with('@') || attrs.iter().any(|a| a.as_str() == key) {
            filtered.insert(key.clone(), value.clone());
        }
    }
    Value::Object(filtered)
}

pub(super) fn filter_entity_by_dataset_id(entity: &mut Value, dataset_ids: &[Uri]) {
    let Some(obj) = expanded_entity_object_mut(entity) else { return };
    let default_context_prefix = format!("{}default-context/", prefix::NGSI_LD.as_str());

    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| !k.starts_with('@') && (!k.starts_with(prefix::NGSI_LD.as_str()) || k.starts_with(&default_context_prefix)))
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
                    .and_then(|o| o.get(expanded_properties::DATASET_ID.as_str()))
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_object())
                    .and_then(|o| o.get("@id"))
                    .and_then(|v| v.as_str());

                match instance_dataset_id {
                    Some(id) => dataset_ids.iter().any(|dataset_id| dataset_id.as_str() == id),
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

pub(super) async fn inject_previous_values(
    current: &mut Value,
    previous: &Value,
    changed_attributes: &[AttributeName],
    engine: &JsonLdEngine,
    context: &ContextSpec,
) {
    let (Some(curr_obj), Some(prev_obj)) = (current.as_object_mut(), previous.as_object()) else {
        return;
    };

    let keys: Vec<String> = curr_obj.keys().filter(|k| !k.starts_with('@') && *k != "id" && *k != "type").cloned().collect();

    let prepared_context = engine.prepare_term_context(context);
    for key in keys {
        if !changed_attributes.is_empty() {
            let expanded_matches = engine
                .expand_term(&key, &prepared_context)
                .await
                .ok()
                .and_then(|iri| iri.as_str().parse::<AttributeName>().ok())
                .is_some_and(|name| changed_attributes.contains(&name));
            let compact_matches = key.parse::<AttributeName>().ok().is_some_and(|name| changed_attributes.contains(&name));
            if !expanded_matches && !compact_matches {
                continue;
            }
        }

        if let Some(prev_val) = prev_obj.get(&key) {
            let curr_attr = curr_obj.get(&key).cloned();
            let previous_key = previous_key_for_attr(prev_val);

            if let Some(Value::Object(attr_obj)) = curr_attr.as_ref().filter(|&v| v.is_object()) {
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
        .and_then(|s| s.parse::<AttributeType>().ok())
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

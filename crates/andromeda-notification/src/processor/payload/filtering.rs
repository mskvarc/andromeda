//! Notification attribute, dataset, and previous-value filtering.

use andromeda_entity::sys_attrs::expanded_entity_object_mut;
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
    changed_dataset_id: Option<&Uri>,
    exact_attribute_instance: bool,
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

        if let Some(prev_val) = prev_obj.get(&key)
            && let Some(curr_attr) = curr_obj.get_mut(&key)
        {
            inject_previous_for_dataset(curr_attr, prev_val, changed_dataset_id, exact_attribute_instance);
        }
    }
}

fn inject_previous_for_dataset(current: &mut Value, previous: &Value, changed_dataset_id: Option<&Uri>, exact_attribute_instance: bool) {
    match current {
        Value::Object(_) => {
            let target_dataset_id = if exact_attribute_instance {
                changed_dataset_id.map(Uri::as_str)
            } else {
                compact_dataset_id(current)
            };
            let previous_instance = match previous {
                Value::Array(instances) => instances.iter().find(|instance| compact_dataset_id(instance) == target_dataset_id),
                Value::Object(_) if compact_dataset_id(previous) == target_dataset_id => Some(previous),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => None,
            };
            if let Some(previous_instance) = previous_instance {
                inject_previous_into_instance(current, previous_instance);
            }
        }
        Value::Array(current_instances) => {
            let previous_instances: Vec<&Value> = match previous {
                Value::Array(instances) => instances.iter().collect(),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => vec![previous],
            };
            for current_instance in current_instances {
                let current_dataset_id = compact_dataset_id(current_instance);
                if exact_attribute_instance && current_dataset_id != changed_dataset_id.map(Uri::as_str) {
                    continue;
                }
                if let Some(previous_instance) = previous_instances.iter().find(|instance| compact_dataset_id(instance) == current_dataset_id) {
                    inject_previous_into_instance(current_instance, previous_instance);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn inject_previous_into_instance(current: &mut Value, previous: &Value) {
    let Some(current_object) = current.as_object_mut() else { return };
    let previous_key = previous_key_for_attr(previous);
    current_object.insert(previous_key.to_owned(), extract_previous_value(previous, previous_key));
}

fn compact_dataset_id(instance: &Value) -> Option<&str> {
    let object = instance.as_object()?;
    let value = object.get("datasetId").or_else(|| object.get(expanded_properties::DATASET_ID.as_str()))?;
    match value {
        Value::String(raw) => Some(raw),
        Value::Object(object) => object.get("id").or_else(|| object.get("@id")).and_then(Value::as_str),
        Value::Array(values) => values.first().and_then(|value| match value {
            Value::String(raw) => Some(raw.as_str()),
            Value::Object(object) => object.get("id").or_else(|| object.get("@id")).and_then(Value::as_str),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => None,
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
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

#[cfg(test)]
mod tests {
    use super::inject_previous_for_dataset;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;

    #[test]
    fn show_changes_marks_only_the_changed_dataset_instance() {
        let dataset = "urn:dataset:named".parse::<Uri>().expect("valid dataset URI");
        let mut current = json!([
            {"type": "Property", "value": 10},
            {"type": "Property", "value": 20, "datasetId": dataset.as_str()}
        ]);
        let previous = json!([
            {"type": "Property", "value": 1},
            {"type": "Property", "value": 2, "datasetId": dataset.as_str()}
        ]);

        inject_previous_for_dataset(&mut current, &previous, Some(&dataset), true);

        assert!(current[0].get("previousValue").is_none());
        assert_eq!(current[1]["previousValue"], json!(2));
    }

    #[test]
    fn an_entity_lifecycle_event_marks_every_dataset_instance() {
        let dataset = "urn:dataset:named".parse::<Uri>().expect("valid dataset URI");
        let mut current = json!([
            {"type": "Property", "value": null},
            {"type": "Property", "value": null, "datasetId": dataset.as_str()}
        ]);
        let previous = json!([
            {"type": "Property", "value": 1},
            {"type": "Property", "value": 2, "datasetId": dataset.as_str()}
        ]);

        inject_previous_for_dataset(&mut current, &previous, None, false);

        assert_eq!(current[0]["previousValue"], json!(1));
        assert_eq!(current[1]["previousValue"], json!(2));
    }
}

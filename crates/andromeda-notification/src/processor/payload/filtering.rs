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
            let compact_matches = key.parse::<AttributeName>().is_ok_and(|name| changed_attributes.contains(&name));
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

/// Add the `showChanges` previous member to one notified instance.
///
/// The member's name is the family of the instance being notified, not of the
/// one it replaced: clause 5.6.19 lets a Replace Attribute change an
/// attribute's family, and each family's prohibitions are stated on itself --
/// clause 4.5.21.2 forbids `previousValue` on a `ListProperty` and clause
/// 4.5.2.2 forbids `previousValueList` on a Property. Taking the name from the
/// previous instance would write the departed family's member onto the new one,
/// which is prohibited on whichever of the two the change ended at.
///
/// When the previous instance carries no member of the current family -- which
/// is what a family change leaves behind -- there is no previous value in the
/// representation the clause requires, so none is reported rather than one
/// invented from the previous instance as a whole.
fn inject_previous_into_instance(current: &mut Value, previous: &Value) {
    let Some(current_object) = current.as_object_mut() else { return };
    let attribute_type = attribute_type_of(current_object.get("type"));
    let Some(previous_value) = extract_previous_value(previous, attribute_type) else {
        return;
    };
    current_object.insert(attribute_type.previous_key().to_owned(), previous_value);
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

/// The attribute family a compacted previous instance belongs to.
///
/// A previous representation reaches this after compaction from the expanded
/// form, so it declares its clause 5.2.4 type. An instance that declares none
/// is a bare value, which is the Property clause 4.5.2.3 abbreviates.
fn attribute_type_of(declared: Option<&Value>) -> AttributeType {
    declared
        .and_then(Value::as_str)
        .and_then(|token| token.parse::<AttributeType>().ok())
        .unwrap_or(AttributeType::Property)
}

/// The value the `showChanges` previous member carries for one instance.
///
/// Every family's previous member is "the same representation as" the value
/// member it shadows — clause 4.5.21.2 says so of `previousValueList` and
/// `valueList`, clause 4.5.22.2 of `previousObjectList` and `objectList` — so
/// the member to lift is the family's own, read through the one table that
/// holds them all. A `ListProperty` carries `valueList` and a
/// `ListRelationship` `objectList`, never the `value` those clauses prohibit,
/// so nothing here may fall back to a Property's member and emit the whole
/// previous instance in place of the previous array.
///
/// An instance carrying no value member at all is a concise bare value
/// (clause 4.5.2.3), which is its own previous value.
fn extract_previous_value(previous: &Value, attribute_type: AttributeType) -> Option<Value> {
    let Some(object) = previous.as_object() else {
        // A concise bare value is its own previous value (clause 4.5.2.3).
        return Some(previous.clone()); // clone: the notified instance owns the value the stored representation keeps.
    };
    // clone: same.
    object.get(attribute_type.compact_value_key()).cloned()
}

#[cfg(test)]
mod tests {
    use super::inject_previous_for_dataset;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use pretty_assertions::assert_eq;
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

    /// Clause 4.5.21.2: `previousValueList` "represents the previous
    /// `ListProperty` `valueList` ... The representation is the same as that of
    /// `valueList`", and `value` / `previousValue` "shall never be present, as
    /// value is a generalization of `valueList`".
    #[test]
    fn show_changes_on_a_list_property_carries_the_previous_value_list_array() {
        let mut current = json!({"type": "ListProperty", "valueList": [3, 4]});
        let previous = json!({"type": "ListProperty", "valueList": [1, 2]});

        inject_previous_for_dataset(&mut current, &previous, None, false);

        assert_eq!(current["previousValueList"], json!([1, 2]));
        assert!(current.get("previousValue").is_none(), "{current}");
        assert!(current.get("value").is_none(), "{current}");
    }

    /// Clause 4.5.22.2: `previousObjectList` "represents the previous
    /// `ListRelationship` `objectList` ... The representation is the same as that
    /// of `objectList`", and `object` / `previousObject` "shall never be
    /// present as object is a generalization of `objectList`". The same clause
    /// makes that representation "an ordered array of Relationship Objects each
    /// consisting of a JSON object containing a single Attribute with a key
    /// called `object`", so the array lifted out of the previous instance is
    /// the array the previous instance held, member shapes included.
    #[test]
    fn show_changes_on_a_list_relationship_carries_the_previous_object_list_array() {
        let mut current = json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:3"}]});
        let previous = json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}]});

        inject_previous_for_dataset(&mut current, &previous, None, false);

        assert_eq!(
            current["previousObjectList"],
            json!([{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}])
        );
        assert!(current.get("previousObject").is_none(), "{current}");
        assert!(current.get("object").is_none(), "{current}");
    }

    /// Every family lifts its own value member into its own previous member
    /// (clauses 4.5.2.2, 4.5.3.2, 4.5.18.2, 4.5.20.2, 4.5.24.2), so no family
    /// falls back to another's and delivers the whole previous instance.
    #[test]
    fn every_family_lifts_its_own_value_member_into_its_own_previous_member() {
        let cases = [
            (json!({"type": "Property", "value": 1}), "previousValue", json!(1)),
            (
                json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}),
                "previousValue",
                json!({"type": "Point", "coordinates": [1.0, 2.0]}),
            ),
            (
                json!({"type": "Relationship", "object": "urn:ngsi-ld:Room:1"}),
                "previousObject",
                json!("urn:ngsi-ld:Room:1"),
            ),
            (
                json!({"type": "LanguageProperty", "languageMap": {"en": "yes"}}),
                "previousLanguageMap",
                json!({"en": "yes"}),
            ),
            (json!({"type": "VocabProperty", "vocab": "Male"}), "previousVocab", json!("Male")),
            (json!({"type": "JsonProperty", "json": {"id": "1"}}), "previousJson", json!({"id": "1"})),
            (json!({"type": "ListProperty", "valueList": [1, 2]}), "previousValueList", json!([1, 2])),
            (
                json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:1"}]}),
                "previousObjectList",
                json!([{"object": "urn:ngsi-ld:Room:1"}]),
            ),
        ];
        for (previous, previous_member, expected) in cases {
            // clone: the case's previous instance is both the source and the shape the current one takes
            let mut current = previous.clone();
            inject_previous_for_dataset(&mut current, &previous, None, false);
            assert_eq!(current[previous_member], expected, "{previous}");
        }
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

//! Deleted attribute and instance materialization.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, rfc3339::sub_second, uri::Uri};
use andromeda_ngsi_ld_model::expanded::null_sentinel::expanded_null_value;
use andromeda_ngsi_ld_vocabulary::{
    NULL_URI,
    expanded::{classes as expanded_classes, properties as expanded_properties},
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashSet;

pub(super) fn nullify_entity_attributes(entity: &mut Value) {
    let Some(obj) = entity.as_object_mut() else { return };
    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| {
            !k.starts_with('@')
                && *k != expanded_properties::CREATED_AT.as_str()
                && *k != expanded_properties::MODIFIED_AT.as_str()
                && *k != expanded_properties::DELETED_AT.as_str()
                && *k != expanded_properties::SCOPE.as_str()
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

/// Replace an instance's value with the NGSI-LD Null its own attribute family
/// is deleted by (clause 4.5.0).
///
/// The null goes into the family's own value member, which is what keeps a
/// deleted `ListProperty` or `ListRelationship` free of the `value` /
/// `previousValue` and `object` / `previousObject` members clauses 4.5.21.2 and
/// 4.5.22.2 prohibit on them: writing a Property's `value` onto a list instance
/// would emit a prohibited member beside a stale list.
pub(super) fn nullify_attribute_instance(instance: &mut Value) {
    let Some(obj) = instance.as_object_mut() else { return };
    let attribute_type = attribute_family_of(obj);
    obj.insert(attribute_type.value_key_str().to_string(), expanded_null_value(attribute_type));
}

/// The attribute family an expanded instance belongs to.
///
/// A stored instance declares its clause 5.2.4 type, and that is what decides
/// the member a deletion is written into. An instance that declares none is
/// placed by the value member it carries instead, and a Property — the family
/// of a bare value — when it carries none of those either.
fn attribute_family_of(instance: &serde_json::Map<String, Value>) -> AttributeType {
    let declared = instance
        .get("@type")
        .and_then(|types| match types {
            Value::Array(names) => names.first().and_then(Value::as_str),
            Value::String(name) => Some(name.as_str()),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
        })
        .and_then(AttributeType::from_expanded_iri);
    match declared {
        Some(attribute_type) => attribute_type,
        None => AttributeType::ALL
            .into_iter()
            .find(|candidate| instance.contains_key(candidate.value_key_str()))
            .unwrap_or(AttributeType::Property),
    }
}

pub(super) fn fix_null_language_maps(entity: &mut Value) {
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
            == Some(NULL_URI.as_str());
        if is_null_lang_map && let Some(attr) = value.as_object_mut() {
            attr.insert("languageMap".to_string(), Value::String(NULL_URI.as_str().to_string()));
        }
    }
}

pub(super) fn inject_deleted_attributes(
    entity: &mut Value,
    changed_attributes: &[AttributeName],
    changed_dataset_id: Option<&Uri>,
    previous_attributes: Option<&Value>,
    deleted_at: Option<DateTime<Utc>>,
) {
    let Some(prev) = previous_attributes else { return };
    let prev_obj = match prev {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    };
    let Some(prev_obj) = prev_obj else { return };
    let Some(entity_obj) = entity.as_object_mut() else { return };

    for attr_name in changed_attributes {
        let attr_iri = attr_name.as_str();
        if entity_obj.contains_key(attr_iri) {
            inject_deleted_instances(entity_obj, attr_iri, prev_obj, changed_dataset_id, deleted_at);
            continue;
        }
        if let Some(prev_attr) = prev_obj.get(attr_iri) {
            let deleted_instance = match prev_attr {
                Value::Array(arr) => arr.iter().find(|instance| dataset_id_of(instance).as_ref() == changed_dataset_id).cloned(),
                other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => {
                    (changed_dataset_id.is_none()).then(|| other.clone())
                }
            };
            if let Some(mut instance) = deleted_instance {
                nullify_attribute_instance(&mut instance);
                if let Some(ts) = deleted_at {
                    inject_deleted_at_on_instance(&mut instance, ts);
                }
                entity_obj.insert(attr_iri.to_string(), serde_json::json!([instance]));
            }
        }
    }
}

pub(super) fn inject_deleted_instances(
    entity_obj: &mut serde_json::Map<String, Value>,
    attr_iri: &str,
    prev_obj: &serde_json::Map<String, Value>,
    changed_dataset_id: Option<&Uri>,
    deleted_at: Option<DateTime<Utc>>,
) {
    let Some(prev_attr) = prev_obj.get(attr_iri) else { return };
    let Some(prev_arr) = prev_attr.as_array() else { return };
    let Some(cur_arr) = entity_obj.get(attr_iri).and_then(|v| v.as_array()) else {
        return;
    };

    let current_dataset_ids: HashSet<Option<Uri>> = cur_arr.iter().map(dataset_id_of).collect();

    let mut deleted_instances: Vec<Value> = Vec::new();
    for prev_inst in prev_arr {
        let prev_ds_id = dataset_id_of(prev_inst);

        if prev_ds_id.as_ref() == changed_dataset_id && !current_dataset_ids.contains(&prev_ds_id) {
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

fn dataset_id_of(instance: &Value) -> Option<Uri> {
    instance
        .as_object()
        .and_then(|object| object.get(expanded_properties::DATASET_ID.as_str()))
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_object)
        .and_then(|object| object.get("@id"))
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uri>().ok())
}

pub(super) fn inject_deleted_at_on_instance(instance: &mut Value, ts: DateTime<Utc>) {
    if let Some(obj) = instance.as_object_mut() {
        obj.insert(
            expanded_properties::DELETED_AT.as_str().to_string(),
            serde_json::json!([{
                "@type": expanded_classes::DATE_TIME.as_str(),
                "@value": sub_second(ts)
            }]),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{inject_deleted_attributes, nullify_attribute_instance};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::slice;

    #[test]
    fn a_dataset_specific_event_materializes_only_that_deleted_instance() {
        let attribute = "https://example.com/speed".parse::<AttributeName>().expect("valid attribute name");
        let dataset = "urn:dataset:named".parse::<Uri>().expect("valid dataset URI");
        let previous = json!({
            "@id": "urn:ngsi-ld:Vehicle:1",
            attribute.as_str(): [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], expanded_properties::VALUE.as_str(): [{"@value": 1}]},
                {
                    "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                    expanded_properties::VALUE.as_str(): [{"@value": 2}],
                    expanded_properties::DATASET_ID.as_str(): [{"@id": dataset.as_str()}]
                }
            ]
        });
        let mut current = json!({"@id": "urn:ngsi-ld:Vehicle:1"});

        inject_deleted_attributes(&mut current, slice::from_ref(&attribute), Some(&dataset), Some(&previous), None);

        let instances = current[attribute.as_str()].as_array().expect("deleted attribute instances");
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0][expanded_properties::DATASET_ID.as_str()][0]["@id"], json!(dataset.as_str()));
    }

    /// Clauses 4.5.0 and 4.5.21.2: a deleted `ListProperty` is nullified in its
    /// own `valueList`, as the array holding one single NGSI-LD Null. The
    /// `value` clause 4.5.21.2 prohibits is never written, and the list it
    /// replaces does not survive as a stale value beside it.
    #[test]
    fn a_deleted_list_property_is_nullified_in_its_own_value_list() {
        let mut instance = json!({
            "@type": [AttributeType::ListProperty.iri().as_str()],
            expanded_properties::VALUE_LIST.as_str(): [{"@list": [{"@value": 1}, {"@value": 2}]}]
        });

        nullify_attribute_instance(&mut instance);

        assert_eq!(
            instance[expanded_properties::VALUE_LIST.as_str()],
            json!([{"@list": [{"@value": "urn:ngsi-ld:null"}]}])
        );
        assert!(instance.get(expanded_properties::VALUE.as_str()).is_none(), "{instance}");
    }

    /// Clauses 4.5.0 and 4.5.22.2: the same for a `ListRelationship`, whose
    /// `objectList` takes the single-null array and whose prohibited `object`
    /// is never written.
    #[test]
    fn a_deleted_list_relationship_is_nullified_in_its_own_object_list() {
        let mut instance = json!({
            "@type": [AttributeType::ListRelationship.iri().as_str()],
            expanded_properties::OBJECT_LIST.as_str(): [{"@list": [{"@value": "urn:ngsi-ld:Room:1"}]}]
        });

        nullify_attribute_instance(&mut instance);

        assert_eq!(
            instance[expanded_properties::OBJECT_LIST.as_str()],
            json!([{"@list": [{"@value": "urn:ngsi-ld:null"}]}])
        );
        assert!(instance.get(expanded_properties::OBJECT.as_str()).is_none(), "{instance}");
    }

    /// An instance that declares no type is placed by the value member it
    /// carries, so a Relationship written without one is still nullified in
    /// its `object` (clause 4.5.3.2) rather than gaining a Property value.
    #[test]
    fn a_typeless_instance_is_nullified_in_the_value_member_it_carries() {
        let mut instance = json!({expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Room:1"}]});

        nullify_attribute_instance(&mut instance);

        assert_eq!(instance[expanded_properties::OBJECT.as_str()], json!([{"@id": "urn:ngsi-ld:null"}]));
        assert!(instance.get(expanded_properties::VALUE.as_str()).is_none(), "{instance}");
    }
}

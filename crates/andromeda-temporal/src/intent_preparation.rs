//! Semantic preparation of expanded history consequences for durable intents.

use crate::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent, TemporalIntentId},
    error::TemporalError,
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, instance_id::InstanceId, scope::grammar::ScopeBuf, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{geo_property::expanded_geo_node_to_geometry, instance_key::is_system_attribute},
    temporal::{TemporalHistoryRow, TemporalValue},
};
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// Prepare one typed intent from the history consequences of an atomic write.
pub(crate) fn prepare(entity_id: &Uri, history: &[HistoryRecord], recorded_at: DateTime<Utc>) -> Result<Option<TemporalAppendIntent>, TemporalError> {
    if history.is_empty() {
        return Ok(None);
    }
    let id = TemporalIntentId::mint();
    let mut entries = Vec::new();
    let mut selection_value = None;
    for record in history {
        match record {
            HistoryRecord::Observations(payload) => {
                append_payload_entries(id, payload, recorded_at, &mut entries)?;
                selection_value = Some(Value::clone(payload));
            }
            HistoryRecord::Tombstone(key) => {
                let ordinal = ordinal(entries.len())?;
                entries.push(TemporalAppendEntry::Tombstone {
                    attribute_name: key.attr.clone(),   // clone: the durable intent owns the instance identity.
                    dataset_id: key.dataset_id.clone(), // clone: the durable intent owns the dataset identity.
                    instance_id: generated_instance_id(id, ordinal, "attribute")?,
                    recorded_at,
                });
            }
        }
    }
    Ok(Some(TemporalAppendIntent {
        id,
        entity_id: entity_id.clone(), // clone: the durable intent owns the validated identifier.
        entries,
        selection_value,
    }))
}

/// Append neutral attribute and scope entries from one expanded entity payload.
fn append_payload_entries(
    intent_id: TemporalIntentId,
    expanded: &Value,
    recorded_at: DateTime<Utc>,
    entries: &mut Vec<TemporalAppendEntry>,
) -> Result<(), TemporalError> {
    let object = match expanded {
        Value::Array(values) => values.first().and_then(Value::as_object),
        Value::Object(object) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    };
    let Some(object) = object else {
        return Ok(());
    };
    for (raw_name, value) in object {
        if raw_name.starts_with('@') {
            continue;
        }
        let attribute_name = raw_name
            .parse::<AttributeName>()
            .map_err(|error| TemporalError::validation(format!("invalid expanded attribute name '{raw_name}': {error}")))?;
        if is_system_attribute(&attribute_name) {
            continue;
        }
        if attribute_name.as_str() == expanded_properties::SCOPE.as_str() {
            append_scopes(intent_id, value, recorded_at, entries)?;
            continue;
        }
        let Some(instances) = value.as_array() else {
            continue;
        };
        for instance in instances {
            let Some(instance_object) = instance.as_object() else {
                continue;
            };
            if !is_ngsi_ld_attribute_instance(instance_object) {
                continue;
            }
            let attribute_type = detect_attribute_type(instance);
            let value = extract_temporal_value(instance, attribute_type)?;
            let dataset_id = extract_dataset_id(instance)?;
            let observed_at = extract_observed_at(instance)?;
            let ordinal = ordinal(entries.len())?;
            let (instance_id, client_supplied_instance_id) = extract_instance_id(instance)?.map_or_else(
                || generated_instance_id(intent_id, ordinal, "attribute").map(|generated| (generated, false)),
                |provided| Ok((provided, true)),
            )?;
            entries.push(TemporalAppendEntry::Observation {
                row: TemporalHistoryRow {
                    attribute_name: attribute_name.clone(), // clone: each observation owns its attribute name.
                    attribute_type,
                    dataset_id,
                    value,
                    observed_at,
                    time: observed_at.unwrap_or(recorded_at),
                    created_at: recorded_at,
                    modified_at: recorded_at,
                    instance_id,
                    metadata: collect_metadata(instance),
                    partition_total: None,
                },
                client_supplied_instance_id,
                recorded_at,
            });
        }
    }
    Ok(())
}

/// Append validated scope observations found in the expanded scope member.
fn append_scopes(intent_id: TemporalIntentId, value: &Value, recorded_at: DateTime<Utc>, entries: &mut Vec<TemporalAppendEntry>) -> Result<(), TemporalError> {
    let Some(scopes) = value.as_array() else {
        return Ok(());
    };
    for scope in scopes {
        let Some(path) = scope
            .as_object()
            .and_then(|object| object.get("@value"))
            .and_then(Value::as_str)
            .and_then(|raw| ScopeBuf::new(raw.to_owned()).ok())
        else {
            continue;
        };
        let entry_ordinal = ordinal(entries.len())?;
        entries.push(TemporalAppendEntry::Scope {
            path,
            instance_id: generated_instance_id(intent_id, entry_ordinal, "scope")?,
            recorded_at,
        });
    }
    Ok(())
}

/// Convert a vector position into the stable persisted intent ordinal.
fn ordinal(position: usize) -> Result<u32, TemporalError> {
    u32::try_from(position).map_err(|error| TemporalError::internal(format!("temporal intent has too many entries: {error}")))
}

/// Generate an instance identifier that is deterministic within an intent.
fn generated_instance_id(intent_id: TemporalIntentId, ordinal: u32, family: &str) -> Result<InstanceId, TemporalError> {
    format!("urn:ngsi-ld:{family}:instance:{intent_id}:{ordinal}")
        .parse::<InstanceId>()
        .map_err(|error| TemporalError::internal(format!("generated invalid temporal instance identifier: {error}")))
}

/// Return whether an expanded object declares an NGSI-LD attribute type.
fn is_ngsi_ld_attribute_instance(object: &Map<String, Value>) -> bool {
    object.get("@type").and_then(Value::as_array).is_some_and(|types| {
        types
            .iter()
            .filter_map(Value::as_str)
            .any(|value| AttributeType::from_expanded_iri(value).is_some())
    })
}

/// Detect the declared attribute type, using Property for legacy untyped input.
fn detect_attribute_type(instance: &Value) -> AttributeType {
    instance
        .get("@type")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(AttributeType::from_expanded_iri)
        .unwrap_or(AttributeType::Property)
}

/// Decode the optional expanded dataset identifier.
fn extract_dataset_id(instance: &Value) -> Result<Option<Uri>, TemporalError> {
    instance
        .get(expanded_properties::DATASET_ID.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@id"))
        .and_then(Value::as_str)
        .map(|raw| {
            raw.parse::<Uri>()
                .map_err(|error| TemporalError::validation(format!("invalid datasetId '{raw}': {error}")))
        })
        .transpose()
}

/// Decode the optional expanded observed-at timestamp.
fn extract_observed_at(instance: &Value) -> Result<Option<DateTime<Utc>>, TemporalError> {
    instance
        .get(expanded_properties::OBSERVED_AT.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@value"))
        .and_then(Value::as_str)
        .map(|raw| {
            DateTime::parse_from_rfc3339(raw)
                .map(|timestamp| timestamp.with_timezone(&Utc))
                .map_err(|error| TemporalError::validation(format!("invalid observedAt '{raw}': {error}")))
        })
        .transpose()
}

/// Decode the optional client-supplied expanded instance identifier.
fn extract_instance_id(instance: &Value) -> Result<Option<InstanceId>, TemporalError> {
    instance
        .get(expanded_properties::INSTANCE_ID.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@id"))
        .and_then(Value::as_str)
        .map(|raw| {
            raw.parse::<InstanceId>()
                .map_err(|error| TemporalError::validation(format!("invalid instanceId '{raw}': {error}")))
        })
        .transpose()
}

/// Retain expanded members that are metadata rather than dedicated value fields.
fn collect_metadata(instance: &Value) -> Option<Value> {
    let object = instance.as_object()?;
    let metadata = object
        .iter()
        .filter(|(key, _)| !is_dedicated_key(key))
        .map(|(key, value)| (key.clone(), value.clone())) // clone: metadata owns the retained expanded members.
        .collect::<Map<String, Value>>();
    (!metadata.is_empty()).then_some(Value::Object(metadata))
}

/// Return whether a member is persisted in a dedicated temporal column.
fn is_dedicated_key(key: &str) -> bool {
    key == "@id"
        || key == "@type"
        || key == expanded_properties::DATASET_ID.as_str()
        || key == expanded_properties::JSON.as_str()
        || key == expanded_properties::LANGUAGE_MAP.as_str()
        || key == expanded_properties::OBJECT.as_str()
        || key == expanded_properties::VALUE.as_str()
        || key == expanded_properties::VOCAB.as_str()
        || key == expanded_properties::INSTANCE_ID.as_str()
        || key == expanded_properties::OBSERVED_AT.as_str()
}

/// Decode one expanded attribute value into its neutral temporal form.
fn extract_temporal_value(instance: &Value, attribute_type: AttributeType) -> Result<TemporalValue, TemporalError> {
    match attribute_type {
        AttributeType::Relationship => relationship_value(instance),
        AttributeType::GeoProperty => geometry_value(instance),
        AttributeType::VocabProperty => vocab_value(instance),
        AttributeType::LanguageProperty => owned_member(instance, expanded_properties::LANGUAGE_MAP.as_str(), "LanguageProperty missing languageMap"),
        AttributeType::JsonProperty => nested_value(instance, expanded_properties::JSON.as_str(), "JsonProperty missing json value"),
        AttributeType::ListProperty => list_value(instance, expanded_properties::VALUE_LIST.as_str(), "ListProperty missing valueList"),
        AttributeType::ListRelationship => list_relationship_value(instance),
        AttributeType::Property => property_value(instance),
    }
}

/// Decode a Relationship object IRI.
fn relationship_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let raw = first_member(instance, expanded_properties::OBJECT.as_str())
        .and_then(|value| value.get("@id"))
        .and_then(Value::as_str)
        .ok_or_else(|| TemporalError::validation("Relationship missing object"))?;
    raw.parse::<Uri>()
        .map(TemporalValue::Uri)
        .map_err(|error| TemporalError::validation(format!("Relationship object is not a valid URI: {error}")))
}

/// Decode an expanded or encoded GeoProperty value.
fn geometry_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let node = first_member(instance, expanded_properties::VALUE.as_str()).ok_or_else(|| TemporalError::validation("GeoProperty missing value"))?;
    let geometry = if let Some(value) = node.get("@value") {
        match value.as_str() {
            Some(raw) => serde_json::from_str(raw).map_err(|error| TemporalError::validation(format!("invalid GeoProperty GeoJSON string: {error}")))?,
            None => Value::clone(value), // clone: the intent owns the geometry value.
        }
    } else {
        let geometry = expanded_geo_node_to_geometry(node).ok_or_else(|| TemporalError::validation("invalid expanded GeoProperty geometry"))?;
        serde_json::to_value(geometry.to_geojson()).map_err(|error| TemporalError::internal(format!("failed to serialize GeoProperty geometry: {error}")))?
    };
    Ok(TemporalValue::Geometry(geometry))
}

/// Decode one or many VocabProperty identifiers.
fn vocab_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let values = instance
        .get(expanded_properties::VOCAB.as_str())
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::validation("VocabProperty missing vocab value"))?;
    if values.len() > 1 {
        Ok(TemporalValue::Object(Value::Array(
            values
                .iter()
                .filter_map(|value| value.get("@id").or_else(|| value.get("@value")).cloned())
                .collect(),
        )))
    } else {
        values
            .first()
            .and_then(|value| value.get("@id").or_else(|| value.get("@value")))
            .and_then(Value::as_str)
            .map(|value| TemporalValue::String(value.to_owned()))
            .ok_or_else(|| TemporalError::validation("VocabProperty missing vocab value"))
    }
}

/// Own an expanded member used directly as a temporal object value.
fn owned_member(instance: &Value, key: &str, error: &'static str) -> Result<TemporalValue, TemporalError> {
    instance
        .get(key)
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(|| TemporalError::validation(error))
}

/// Own the `@value` nested in the first expanded member.
fn nested_value(instance: &Value, key: &str, error: &'static str) -> Result<TemporalValue, TemporalError> {
    first_member(instance, key)
        .and_then(|value| value.get("@value"))
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(|| TemporalError::validation(error))
}

/// Own the `@list` nested in the first expanded member.
fn list_value(instance: &Value, key: &str, error: &'static str) -> Result<TemporalValue, TemporalError> {
    first_member(instance, key)
        .and_then(|value| value.get("@list"))
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(|| TemporalError::validation(error))
}

/// Decode a ListRelationship object list.
fn list_relationship_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let list = first_member(instance, expanded_properties::OBJECT_LIST.as_str())
        .and_then(|value| value.get("@list"))
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::validation("ListRelationship missing objectList"))?;
    Ok(TemporalValue::Object(Value::Array(
        list.iter().filter_map(|value| value.get("@id").cloned()).collect(),
    )))
}

/// Decode a Property scalar, structured value, or typed date-time.
fn property_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let value_node = first_member(instance, expanded_properties::VALUE.as_str()).ok_or_else(|| TemporalError::validation("Property missing value"))?;
    let Some(value) = value_node.get("@value") else {
        return Ok(TemporalValue::Object(Value::clone(value_node))); // clone: the intent owns the expanded node.
    };
    let type_hint = value_node.get("@type").and_then(Value::as_str);
    if (type_hint == Some(expanded_classes::DATE_TIME.as_str()) || type_hint == Some(expanded_properties::XSD_DATETIME.as_str()))
        && let Some(raw) = value.as_str()
    {
        return DateTime::parse_from_rfc3339(raw)
            .map(|timestamp| TemporalValue::DateTime(timestamp.with_timezone(&Utc)))
            .map_err(|error| TemporalError::validation(format!("invalid datetime property '{raw}': {error}")));
    }
    match value {
        Value::Number(number) => number
            .as_f64()
            .map(TemporalValue::Number)
            .ok_or_else(|| TemporalError::validation("Property number cannot be represented as f64")),
        Value::String(text) => Ok(TemporalValue::String(text.clone())), // clone: the intent owns the text.
        Value::Bool(flag) => Ok(TemporalValue::Boolean(*flag)),
        Value::Array(_) | Value::Object(_) => Ok(TemporalValue::Object(Value::clone(value))), // clone: the intent owns the structured value.
        Value::Null => Ok(TemporalValue::String("null".to_owned())),
    }
}

/// Borrow the first value of one expanded array member.
fn first_member<'a>(instance: &'a Value, key: &str) -> Option<&'a Value> {
    instance.get(key).and_then(Value::as_array).and_then(|values| values.first())
}

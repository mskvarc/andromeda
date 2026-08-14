//! Semantic preparation of expanded history consequences for durable intents.

use crate::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    error::TemporalError,
    scope_observation::ScopeObservation,
};
use andromeda_context_commit::{history_record::HistoryRecord, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
    ngsi_ld_date::NgsiLdDate,
    ngsi_ld_date_time::NgsiLdDateTime,
    ngsi_ld_time::NgsiLdTime,
    scope::grammar::ScopeBuf,
    uri::Uri,
};
use andromeda_ngsi_ld_model::{
    attribute::json_number::JsonNumber,
    expanded::{geo_property::expanded_geo_node_to_geometry, instance_key::is_system_attribute, null_sentinel::instance_is_null_sentinel},
    temporal::{TemporalHistoryRow, TemporalValue},
};
use andromeda_ngsi_ld_render::uri_decoding::decode_percent_encoded_uri;
use andromeda_ngsi_ld_vocabulary::{
    NULL_URI,
    expanded::{classes as expanded_classes, properties as expanded_properties},
};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// Prepare one typed intent from the history consequences of an atomic write.
///
/// `record` and `base` are the commit's, not minted here: every entity a
/// commit lands shares the commit's single write identity, and this intent's
/// entries are numbered starting at `base` in the commit's one running
/// ordinal counter rather than each entity restarting its own count at zero.
pub(crate) fn prepare(
    entity_id: &Uri,
    history: &[HistoryRecord],
    record: WriteRecord,
    base: WriteOrdinal,
) -> Result<Option<TemporalAppendIntent>, TemporalError> {
    if history.is_empty() {
        return Ok(None);
    }
    let recorded_at = record.recorded_at();
    let mut entries = Vec::new();
    let mut selection_value = None;
    for consequence in history {
        match consequence {
            HistoryRecord::Observations(payload) => {
                append_payload_entries(payload, recorded_at, &mut entries)?;
                selection_value = Some(Value::clone(payload));
            }
            HistoryRecord::Tombstone(removed) => entries.push(TemporalAppendEntry::Tombstone {
                attribute_name: removed.key.attr.clone(),   // clone: the durable intent owns the instance identity.
                dataset_id: removed.key.dataset_id.clone(), // clone: the durable intent owns the dataset identity.
                attribute_type: removed.attribute_type,
                instance: InstanceIdentity::generate(),
            }),
        }
    }
    Ok(Some(TemporalAppendIntent {
        record,
        base,
        entity_id: entity_id.clone(), // clone: the durable intent owns the validated identifier.
        entries,
        selection_value,
    }))
}

/// Append neutral attribute and scope entries from one expanded entity payload.
fn append_payload_entries(expanded: &Value, recorded_at: DateTime<Utc>, entries: &mut Vec<TemporalAppendEntry>) -> Result<(), TemporalError> {
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
            append_scopes(value, entries);
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
            if instance_is_null_sentinel(instance) {
                continue;
            }
            let attribute_type = detect_attribute_type(instance);
            if let Some(observations) = temporal_observations(instance, attribute_type) {
                for observation in observations {
                    let observation = merge_temporal_observation(instance_object, observation, attribute_type);
                    append_observation(&attribute_name, attribute_type, &observation, recorded_at, entries)?;
                }
            } else {
                append_observation(&attribute_name, attribute_type, instance, recorded_at, entries)?;
            }
        }
    }
    Ok(())
}

/// Append one expanded observation after its temporal envelope has been removed.
fn append_observation(
    attribute_name: &AttributeName,
    attribute_type: AttributeType,
    instance: &Value,
    recorded_at: DateTime<Utc>,
    entries: &mut Vec<TemporalAppendEntry>,
) -> Result<(), TemporalError> {
    let value = extract_temporal_value(instance, attribute_type)?;
    let dataset_id = extract_dataset_id(instance)?;
    let observed_at = extract_observed_at(instance)?;
    let instance_identity = extract_instance_id(instance)?.map_or_else(InstanceIdentity::generate, |provided| {
        InstanceIdentity::from_uri(provided, InstanceFamily::Attribute)
    });
    entries.push(TemporalAppendEntry::Observation {
        row: TemporalHistoryRow {
            attribute_name: attribute_name.clone(), // clone: each observation owns its attribute name.
            attribute_type,
            dataset_id,
            value,
            // An `observedAt` the client did not declare stays absent: clause
            // 5.2.21 makes it a Temporal Property of the observation, so
            // substituting the write clock would invent one.
            observed_at,
            recorded_at,
            // Clause 4.8 has the broker assign `createdAt` and `modifiedAt`
            // itself, and a newly appended instance is created and last
            // modified at the instant the write was accepted.
            created_at: Some(recorded_at),
            modified_at: Some(recorded_at),
            instance: instance_identity,
            metadata: collect_metadata(instance),
            partition_total: None,
        },
    });
    Ok(())
}

/// Borrow the observations nested below a plural temporal value member.
fn temporal_observations(instance: &Value, attribute_type: AttributeType) -> Option<&[Value]> {
    instance
        .get(temporal_values_key(attribute_type))
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@list"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

/// Combine one temporal observation with its attribute-instance envelope.
fn merge_temporal_observation(envelope: &Map<String, Value>, observation: &Value, attribute_type: AttributeType) -> Value {
    let mut merged = envelope
        .iter()
        .filter(|(key, _)| key.as_str() != temporal_values_key(attribute_type))
        .map(|(key, value)| (key.clone(), value.clone())) // clone: each prepared observation owns inherited envelope members.
        .collect::<Map<String, Value>>();
    if let Some(observation) = observation.as_object() {
        merged.extend(observation.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the durable observation owns its expanded members.
    }
    Value::Object(merged)
}

/// Return the expanded plural temporal-value member for an attribute kind.
fn temporal_values_key(attribute_type: AttributeType) -> &'static str {
    match attribute_type {
        AttributeType::Relationship | AttributeType::ListRelationship => expanded_properties::OBJECTS.as_str(),
        AttributeType::LanguageProperty => expanded_properties::LANGUAGE_MAPS.as_str(),
        AttributeType::JsonProperty => expanded_properties::JSONS.as_str(),
        AttributeType::VocabProperty => expanded_properties::VOCABS.as_str(),
        AttributeType::Property | AttributeType::GeoProperty | AttributeType::ListProperty => expanded_properties::VALUES.as_str(),
    }
}

/// Append validated scope observations found in the expanded scope member.
///
/// The scope grammar of clause 4.18 admits `urn:ngsi-ld:null` alongside the
/// hierarchical paths and reserves it for a deleted scope, so that spelling
/// records the end of the entity's scope rather than a path named after it.
fn append_scopes(value: &Value, entries: &mut Vec<TemporalAppendEntry>) {
    let Some(scopes) = value.as_array() else {
        return;
    };
    for scope in scopes {
        let Some(raw) = scope.as_object().and_then(|object| object.get("@value")).and_then(Value::as_str) else {
            continue;
        };
        let observation = if raw == NULL_URI.as_str() {
            ScopeObservation::Deleted
        } else {
            match ScopeBuf::new(raw.to_owned()) {
                Ok(path) => ScopeObservation::Path(path),
                Err(_) => continue,
            }
        };
        entries.push(TemporalAppendEntry::Scope {
            observation,
            instance: InstanceIdentity::generate(),
        });
    }
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
///
/// Parsing is routed through [`NgsiLdDateTime`] so `observedAt` is held to the
/// same clause 4.6.3 strict-UTC, six-fractional-digit policy as every other
/// client-supplied `DateTime`, rather than accepting whatever `chrono`'s bare
/// RFC 3339 parser admits.
fn extract_observed_at(instance: &Value) -> Result<Option<DateTime<Utc>>, TemporalError> {
    instance
        .get(expanded_properties::OBSERVED_AT.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@value"))
        .and_then(Value::as_str)
        .map(|raw| {
            raw.parse::<NgsiLdDateTime>()
                .map(NgsiLdDateTime::get)
                .map_err(|error| TemporalError::validation(format!("invalid observedAt '{raw}': {error}")))
        })
        .transpose()
}

/// Decode the optional client-supplied expanded instance identifier.
fn extract_instance_id(instance: &Value) -> Result<Option<Uri>, TemporalError> {
    instance
        .get(expanded_properties::INSTANCE_ID.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@id"))
        .and_then(Value::as_str)
        .map(|raw| {
            raw.parse::<Uri>()
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
        || key == expanded_properties::OBJECTS.as_str()
        || key == expanded_properties::VALUE.as_str()
        || key == expanded_properties::VALUES.as_str()
        || key == expanded_properties::VOCAB.as_str()
        || key == expanded_properties::VOCABS.as_str()
        || key == expanded_properties::INSTANCE_ID.as_str()
        || key == expanded_properties::OBSERVED_AT.as_str()
        || key == expanded_properties::LANGUAGE_MAPS.as_str()
        || key == expanded_properties::JSONS.as_str()
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
    decode_percent_encoded_uri(raw)
        .parse::<Uri>()
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
///
/// Each member names a target Entity URI (clause 4.5.22), but which JSON-LD
/// keyword carries it depends on the context the client wrote the Entity under.
/// The normative core context defines `objectList` with `@container: "@list"`
/// and no `@type: "@id"` coercion — unlike the singular `object` — so an
/// `objectList` written under it expands to `@value` nodes, while a context that
/// does coerce, and every document written in expanded form, gives `@id` nodes.
/// Both spell the same target, so both are read; a member that spells neither is
/// not a target at all and refuses the write rather than silently shortening the
/// list.
fn list_relationship_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let list = first_member(instance, expanded_properties::OBJECT_LIST.as_str())
        .and_then(|value| value.get("@list"))
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::validation("ListRelationship missing objectList"))?;
    let targets = list
        .iter()
        .map(|member| {
            member
                .get("@id")
                .or_else(|| member.get("@value"))
                .filter(|target| target.is_string())
                .cloned()
                .ok_or_else(|| TemporalError::validation("ListRelationship objectList member names no target URI"))
        })
        .collect::<Result<Vec<Value>, TemporalError>>()?;
    Ok(TemporalValue::Object(Value::Array(targets)))
}

/// Decode a Property scalar, structured value, or typed temporal literal.
fn property_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let value_node = first_member(instance, expanded_properties::VALUE.as_str()).ok_or_else(|| TemporalError::validation("Property missing value"))?;
    let Some(value) = value_node.get("@value") else {
        return Ok(TemporalValue::Object(Value::clone(value_node))); // clone: the intent owns the expanded node.
    };
    let type_hint = value_node.get("@type").and_then(Value::as_str);
    if let Some(raw) = value.as_str()
        && let Some(typed) = temporal_property_value(type_hint, raw)
    {
        return typed;
    }
    match value {
        // The number keeps the shape it was parsed in all the way to the
        // arbitrary-precision storage column: flattening it to a double here
        // would round an integer beyond 2^53 before anything could refuse it.
        Value::Number(number) => JsonNumber::try_from(number)
            .map(TemporalValue::Number)
            .map_err(|error| TemporalError::validation(format!("Property number cannot be carried: {error}"))),
        Value::String(text) => Ok(TemporalValue::String(text.clone())), // clone: the intent owns the text.
        Value::Bool(flag) => Ok(TemporalValue::Boolean(*flag)),
        Value::Array(_) | Value::Object(_) => Ok(TemporalValue::Object(Value::clone(value))), // clone: the intent owns the structured value.
        Value::Null => Ok(TemporalValue::String("null".to_owned())),
    }
}

/// Decode a Property value the client typed as one of the three clause 4.6.3
/// temporal forms, or `None` when the declared type is not one of them.
///
/// Each form is parsed rather than taken verbatim, and the two that are stored as
/// text are stored in the canonical rendering their type defines. Clause 4.6.3
/// admits many spellings of one `Time` — any offset, any fractional width — and
/// clause 4.9 orders `Time` values chronologically; the string column those values
/// land in orders them byte-wise, so only a fixed-width UTC rendering makes the
/// two orders the same. `Date` is validated for the same reason: its canonical
/// `YYYY-MM-DD` form already sorts chronologically, and a lenient spelling would
/// not. `DateTime` is routed through [`NgsiLdDateTime`] rather than a bare
/// `chrono` RFC 3339 parse for the same reason [`extract_observed_at`] routes
/// `observedAt` through it: a non-`Z` offset or more than six fractional digits
/// must be refused rather than silently reinterpreted into the `timestamptz`
/// column, on every axis that carries a client-supplied instant, not only on
/// `observedAt`.
fn temporal_property_value(type_hint: Option<&str>, raw: &str) -> Option<Result<TemporalValue, TemporalError>> {
    let hint = type_hint?;
    if hint == expanded_classes::DATE_TIME.as_str() {
        return Some(
            raw.parse::<NgsiLdDateTime>()
                .map(NgsiLdDateTime::get)
                .map(TemporalValue::DateTime)
                .map_err(|error| TemporalError::validation(format!("invalid datetime property '{raw}': {error}"))),
        );
    }
    if hint == expanded_classes::TIME.as_str() {
        return Some(
            raw.parse::<NgsiLdTime>()
                .map(|time| TemporalValue::String(time.to_string()))
                .map_err(|error| TemporalError::validation(format!("invalid Time property '{raw}': {error}"))),
        );
    }
    if hint == expanded_classes::DATE.as_str() {
        return Some(
            raw.parse::<NgsiLdDate>()
                .map(|date| TemporalValue::String(date.to_string()))
                .map_err(|error| TemporalError::validation(format!("invalid Date property '{raw}': {error}"))),
        );
    }
    None
}

/// Borrow the first value of one expanded array member.
fn first_member<'a>(instance: &'a Value, key: &str) -> Option<&'a Value> {
    instance.get(key).and_then(Value::as_array).and_then(|values| values.first())
}

#[cfg(test)]
mod tests {
    use crate::{append_intent::TemporalAppendEntry, error::TemporalError, intent_preparation::prepare};
    use andromeda_context_commit::{
        history_record::{HistoryRecord, tombstone},
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
    use andromeda_ngsi_ld_model::attribute::json_number::JsonNumber;
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::{Value, json};
    use std::sync::Arc;

    /// Build a restored record so tests can hand `prepare` a fixed identity.
    fn record(recorded_at: DateTime<Utc>) -> WriteRecord {
        WriteRecord::restored(WriteId::mint(), recorded_at)
    }

    #[test]
    fn null_sentinel_is_not_decoded_as_a_geoproperty_observation() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity identifier");
        let attribute_name = "https://example.com/location".parse::<AttributeName>().expect("valid attribute name");
        let dataset_id = "urn:ngsi-ld:Dataset:a".parse::<Uri>().expect("valid dataset identifier");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
            }]
        }]));
        let history = [
            HistoryRecord::Observations(payload),
            tombstone(&attribute_name, Some(&dataset_id), AttributeType::GeoProperty),
        ];
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        let intent = prepare(&entity_id, &history, record(recorded_at), WriteOrdinal::at(0))
            .expect("null sentinel must not fail temporal preparation")
            .expect("the tombstone produces an intent");

        pretty_assertions::assert_eq!(intent.entries.len(), 1);
        assert!(matches!(
            &intent.entries[0],
            TemporalAppendEntry::Tombstone {
                attribute_name: actual_attribute,
                dataset_id: Some(actual_dataset),
                ..
            } if actual_attribute == &attribute_name && actual_dataset == &dataset_id
        ));
    }

    #[test]
    fn plural_temporal_values_are_prepared_as_individual_observations() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity identifier");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:sensor"}],
                "https://uri.etsi.org/ngsi-ld/hasValues": [{"@list": [
                    {
                        "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 55}],
                        "https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": "2020-08-01T12:00:00Z"}]
                    },
                    {
                        "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 61}],
                        "https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": "2020-08-01T13:00:00Z"}]
                    }
                ]}]
            }]
        }]));
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        let intent = prepare(&entity_id, &[HistoryRecord::Observations(payload)], record(recorded_at), WriteOrdinal::at(0))
            .expect("plural temporal values must be valid")
            .expect("observations produce an intent");

        pretty_assertions::assert_eq!(intent.entries.len(), 2);
        let rows = intent
            .entries
            .iter()
            .filter_map(|entry| match entry {
                TemporalAppendEntry::Observation { row } => Some(row),
                TemporalAppendEntry::Tombstone { .. } | TemporalAppendEntry::Scope { .. } => None,
            })
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(rows.len(), 2);
        pretty_assertions::assert_eq!(rows[0].value, andromeda_ngsi_ld_model::temporal::TemporalValue::Number(JsonNumber::Signed(55)));
        pretty_assertions::assert_eq!(rows[1].value, andromeda_ngsi_ld_model::temporal::TemporalValue::Number(JsonNumber::Signed(61)));
        assert!(rows.iter().all(|row| {
            row.dataset_id
                .as_ref()
                .is_some_and(|dataset_id| dataset_id.as_str() == "urn:ngsi-ld:Dataset:sensor")
        }));
    }

    #[test]
    fn percent_encoded_relationship_target_is_validated_after_decoding() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity identifier");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/ref": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn%3Angsi-ld%3ABuilding%3A1"}]
            }]
        }]));
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        let intent = prepare(&entity_id, &[HistoryRecord::Observations(payload)], record(recorded_at), WriteOrdinal::at(0))
            .expect("encoded relationship target must be valid")
            .expect("the relationship produces an intent");

        assert!(matches!(
            &intent.entries[0],
            TemporalAppendEntry::Observation {
                row: andromeda_ngsi_ld_model::temporal::TemporalHistoryRow {
                    value: andromeda_ngsi_ld_model::temporal::TemporalValue::Uri(uri),
                    ..
                },
                ..
            } if uri.as_str() == "urn:ngsi-ld:Building:1"
        ));
    }

    /// Clause 4.5.22: every `objectList` member names a target Entity. The
    /// normative core context coerces `object` to `@id` but leaves `objectList`
    /// uncoerced, so the same list arrives as `@value` nodes under it and as
    /// `@id` nodes under a coercing context. Both are the same list of targets.
    #[test]
    fn an_object_list_keeps_its_targets_however_the_context_expanded_them() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity identifier");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        for member in [json!({"@id": "urn:ngsi-ld:Part:1"}), json!({"@value": "urn:ngsi-ld:Part:1"})] {
            let payload = Arc::new(json!([{
                "@id": entity_id,
                "https://example.com/parts": [{
                    "@type": ["https://uri.etsi.org/ngsi-ld/ListRelationship"],
                    "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [member]}]
                }]
            }]));

            let intent = prepare(&entity_id, &[HistoryRecord::Observations(payload)], record(recorded_at), WriteOrdinal::at(0))
                .expect("an object list must be valid")
                .expect("the list relationship produces an intent");

            assert!(
                matches!(
                    &intent.entries[0],
                    TemporalAppendEntry::Observation {
                        row: andromeda_ngsi_ld_model::temporal::TemporalHistoryRow {
                            value: andromeda_ngsi_ld_model::temporal::TemporalValue::Object(list),
                            ..
                        },
                        ..
                    } if list == &json!(["urn:ngsi-ld:Part:1"])
                ),
                "{:?}",
                intent.entries[0]
            );
        }
    }

    /// A member that spells no target is a malformed list, not a shorter one:
    /// silently dropping it would store a ListRelationship that names fewer
    /// Entities than the client wrote.
    #[test]
    fn an_object_list_member_without_a_target_refuses_the_write() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity identifier");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/parts": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/ListRelationship"],
                "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [{"@type": "urn:ngsi-ld:Part:1"}]}]
            }]
        }]));
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        assert!(prepare(&entity_id, &[HistoryRecord::Observations(payload)], record(recorded_at), WriteOrdinal::at(0)).is_err());
    }

    /// Build the expanded payload of one `DateTime`-typed Property value.
    fn datetime_property_payload(entity_id: &Uri, raw: &str) -> Arc<Value> {
        Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/installedAt": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@value": raw,
                    "@type": "https://uri.etsi.org/ngsi-ld/DateTime"
                }]
            }]
        }]))
    }

    /// Clause 4.6.3 mandates the literal `Z` UTC designator; a `DateTime`
    /// Property value carrying a numeric offset must be refused on the primary
    /// write path exactly as it already is on instance PATCH, rather than
    /// silently reinterpreted into the `timestamptz` column.
    #[test]
    fn a_datetime_property_value_with_a_non_utc_offset_is_refused() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity identifier");
        let payload = datetime_property_payload(&entity_id, "2026-07-25T12:00:00+02:00");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        let error = prepare(&entity_id, &[HistoryRecord::Observations(payload)], record(recorded_at), WriteOrdinal::at(0))
            .expect_err("a non-Z offset must be refused, not silently reinterpreted");
        assert!(matches!(error, TemporalError::Validation { .. }), "{error:?}");
    }

    /// Clause 4.6.3 admits at most six fractional-second digits; a seventh
    /// digit must be refused on the primary write path exactly as it already
    /// is on instance PATCH, rather than rounded by the storage column.
    #[test]
    fn a_datetime_property_value_with_seven_fractional_digits_is_refused() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity identifier");
        let payload = datetime_property_payload(&entity_id, "2026-07-25T12:00:00.1234567Z");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        let error = prepare(&entity_id, &[HistoryRecord::Observations(payload)], record(recorded_at), WriteOrdinal::at(0))
            .expect_err("seven fractional digits must be refused, not rounded");
        assert!(matches!(error, TemporalError::Validation { .. }), "{error:?}");
    }
}

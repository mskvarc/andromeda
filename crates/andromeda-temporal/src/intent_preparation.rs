//! Semantic preparation of expanded history consequences for durable intents.

use crate::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    attribute_instance_value::{collect_metadata, extract_temporal_value},
    error::TemporalError,
    scope_observation::ScopeObservation,
    temporal_validation_reason::TemporalValidationReason,
};
use andromeda_context_commit::{history_record::HistoryRecord, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
    ngsi_ld_date_time::NgsiLdDateTime,
    scope::path::ScopePath,
};
use andromeda_ngsi_ld_model::{
    expanded::{instance_key::is_system_attribute, null_sentinel::instance_is_null_sentinel},
    temporal::TemporalHistoryRow,
};
use andromeda_ngsi_ld_vocabulary::{NULL_URI, expanded::properties as expanded_properties};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::slice::from_ref;

/// Prepare one typed intent from the history consequences of an atomic write.
///
/// `record` and `base_ordinal` are the commit's, not minted here: every entity a
/// commit lands shares the commit's single write identity, and this intent's
/// entries are numbered starting at `base_ordinal` in the commit's one running
/// ordinal counter rather than each entity restarting its own count at zero.
///
/// `entity_types` is likewise the write's, not the history's: clause 4.5.6
/// makes the Temporal Evolution "the Entity [...] as mandated by clause 4.5.1",
/// and the observed payload of an ordinary attribute write is an Entity
/// Fragment whose `type` clauses 5.6.2.4, 5.6.3.4 and 5.6.17.4 leave optional.
pub(crate) fn prepare(
    entity_id: &Identifier,
    entity_types: &[EntityTypeName],
    history: &[HistoryRecord],
    record: WriteRecord,
    base_ordinal: WriteOrdinal,
) -> Result<Option<TemporalAppendIntent>, TemporalError> {
    if history.is_empty() {
        return Ok(None);
    }
    let recorded_at = record.recorded_at();
    let mut entries = Vec::new();
    let mut selection_value = None;
    for consequence in history {
        match consequence {
            HistoryRecord::Observations(payload) | HistoryRecord::EvolutionInstances(payload) => {
                append_payload_entries(payload, recorded_at, &mut entries)?;
                selection_value = Some(Value::clone(payload));
            }
            HistoryRecord::Tombstone(removed) => entries.push(TemporalAppendEntry::Tombstone {
                attribute_name: removed.key.attr.clone(),   // clone: the durable intent owns the instance identity.
                dataset_id: removed.key.dataset_id.clone(), // clone: the durable intent owns the dataset identity.
                attribute_type: removed.attribute_type,
                instance: InstanceIdentity::generate(),
            }),
            // Clause 4.18 ends a Scope's validity when it is "explicitly
            // removed by an update or delete operation", and the write that
            // removed it declared no instant of its own, so the removal is
            // dated at the write — which is also what clause 4.5.6 asks of a
            // Scope changed through the Core API, "the observedAt sub-Property
            // should be set as a copy of the modifiedAt sub-Property".
            HistoryRecord::ScopeRemoval => entries.push(TemporalAppendEntry::Scope {
                observation: ScopeObservation::Deleted,
                observed_at: Some(recorded_at),
                instance: InstanceIdentity::generate(),
            }),
        }
    }
    Ok(Some(TemporalAppendIntent {
        record,
        base_ordinal,
        entity_id: entity_id.clone(), // clone: the durable intent owns the validated identifier.
        entity_types: entity_types.to_vec(),
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
        let attribute_name = raw_name.parse::<AttributeName>().map_err(|source| {
            TemporalError::Validation(TemporalValidationReason::ExpandedAttributeName {
                name: raw_name.clone(),
                source,
            })
        })?;
        if is_system_attribute(&attribute_name) {
            continue;
        }
        if attribute_name.as_str() == expanded_properties::SCOPE.as_str() {
            append_scopes(value, recorded_at, entries)?;
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
        InstanceIdentity::from_identifier(provided, InstanceFamily::Attribute)
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
///
/// The table lives in the lexicon beside its compacted twin: clause 4.5.9 gives
/// the two list families `valueLists` / `objectLists` rather than the
/// generalized `values` / `objects`, and a second copy of that mapping here is
/// how the temporal write side came to store a `ListProperty` under `hasValues`
/// while the renderer read it from `hasValueLists`.
fn temporal_values_key(attribute_type: AttributeType) -> &'static str {
    attribute_type.expanded_temporal_values_key_str()
}

/// Append validated scope observations found in the expanded scope member.
///
/// The scope arrives non-reified from the Core API, as bare values, or
/// reified as the temporal representation of a Property (clause 4.5.6),
/// whose `hasValue` holds the paths and whose `observedAt` dates them. An
/// observation the client did not date is dated at the write, which is what
/// clause 4.5.6 asks for a scope changed through the Core API ("the
/// observedAt sub-Property should be set as a copy of the modifiedAt").
///
/// The scope grammar of clause 4.18 admits `urn:ngsi-ld:null` alongside the
/// hierarchical paths and reserves it for a deleted scope, so that spelling
/// records the end of the entity's scope rather than a path named after it.
fn append_scopes(value: &Value, recorded_at: DateTime<Utc>, entries: &mut Vec<TemporalAppendEntry>) -> Result<(), TemporalError> {
    let Some(scopes) = value.as_array() else {
        return Ok(());
    };
    for scope in scopes {
        let Some(object) = scope.as_object() else {
            continue;
        };
        let (paths, observed_at) = match object.get(expanded_properties::VALUE.as_str()).and_then(Value::as_array) {
            Some(reified) => (reified.as_slice(), extract_observed_at(scope)?.unwrap_or(recorded_at)),
            None => (from_ref(scope), recorded_at),
        };
        for path in paths {
            let Some(raw) = path.get("@value").and_then(Value::as_str) else {
                continue;
            };
            let observation = if raw == NULL_URI.as_str() {
                ScopeObservation::Deleted
            } else {
                match raw.parse::<ScopePath>() {
                    Ok(path) => ScopeObservation::Path(path),
                    Err(_) => continue,
                }
            };
            entries.push(TemporalAppendEntry::Scope {
                observation,
                observed_at: Some(observed_at),
                instance: InstanceIdentity::generate(),
            });
        }
    }
    Ok(())
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
fn extract_dataset_id(instance: &Value) -> Result<Option<Identifier>, TemporalError> {
    instance
        .get(expanded_properties::DATASET_ID.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@id"))
        .and_then(Value::as_str)
        .map(|raw| {
            raw.parse::<Identifier>()
                .map_err(|source| TemporalError::Validation(TemporalValidationReason::DatasetId { raw: raw.to_string(), source }))
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
                .map_err(|source| TemporalError::Validation(TemporalValidationReason::ObservedAt { raw: raw.to_string(), source }))
        })
        .transpose()
}

/// Decode the optional client-supplied expanded instance identifier.
fn extract_instance_id(instance: &Value) -> Result<Option<Identifier>, TemporalError> {
    instance
        .get(expanded_properties::INSTANCE_ID.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@id"))
        .and_then(Value::as_str)
        .map(|raw| {
            raw.parse::<Identifier>()
                .map_err(|source| TemporalError::Validation(TemporalValidationReason::InstanceId { raw: raw.to_string(), source }))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::{append_intent::TemporalAppendEntry, error::TemporalError, intent_preparation::prepare, scope_observation::ScopeObservation};
    use andromeda_context_commit::{
        history_record::{HistoryRecord, tombstone},
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, scope::path::ScopePath};
    use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::{Value, json};
    use std::sync::Arc;

    /// Build a restored record so tests can hand `prepare` a fixed identity.
    fn record(recorded_at: DateTime<Utc>) -> WriteRecord {
        WriteRecord::restored(WriteId::mint(), recorded_at)
    }

    /// Clause 4.5.6: the temporal scope is the reified Property whose
    /// `observedAt` dates it; a non-reified scope written through the Core
    /// API is dated at the write ("a copy of the modifiedAt").
    #[test]
    fn scopes_are_prepared_with_their_observed_at_or_dated_at_the_write() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://uri.etsi.org/ngsi-ld/scope": [
                {"@value": "/Madrid"},
                {
                    "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "/Barcelona"}, {"@value": "urn:ngsi-ld:null"}],
                    "https://uri.etsi.org/ngsi-ld/observedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2020-06-01T00:00:00Z"}]
                }
            ]
        }]));
        let history = [HistoryRecord::Observations(payload)];
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let observed = Utc.with_ymd_and_hms(2020, 6, 1, 0, 0, 0).single().expect("valid instant");

        let intent = prepare(&entity_id, &[], &history, record(recorded_at), WriteOrdinal::at(0))
            .expect("scopes prepare")
            .expect("the scopes produce an intent");

        let scopes: Vec<(ScopeObservation, Option<DateTime<Utc>>)> = intent
            .entries
            .iter()
            .filter_map(|entry| match entry {
                TemporalAppendEntry::Scope { observation, observed_at, .. } => Some((observation.clone(), *observed_at)),
                TemporalAppendEntry::Observation { .. } | TemporalAppendEntry::Tombstone { .. } => None,
            })
            .collect();
        let path = |raw: &str| ScopeObservation::Path(raw.parse::<ScopePath>().expect("a valid scope path"));
        pretty_assertions::assert_eq!(
            scopes,
            vec![
                (path("/Madrid"), Some(recorded_at)),
                (path("/Barcelona"), Some(observed)),
                (ScopeObservation::Deleted, Some(observed)),
            ]
        );
    }

    #[test]
    fn null_sentinel_is_not_decoded_as_a_geoproperty_observation() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier");
        let attribute_name = "https://example.com/location".parse::<AttributeName>().expect("valid attribute name");
        let dataset_id = "urn:ngsi-ld:Dataset:a".parse::<Identifier>().expect("valid dataset identifier");
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

        let intent = prepare(&entity_id, &[], &history, record(recorded_at), WriteOrdinal::at(0))
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
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier");
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

        let intent = prepare(
            &entity_id,
            &[],
            &[HistoryRecord::Observations(payload)],
            record(recorded_at),
            WriteOrdinal::at(0),
        )
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
        pretty_assertions::assert_eq!(rows[0].value, TemporalValue::Number(JsonNumber::from(55)));
        pretty_assertions::assert_eq!(rows[1].value, TemporalValue::Number(JsonNumber::from(61)));
        assert!(rows.iter().all(|row| {
            row.dataset_id
                .as_ref()
                .is_some_and(|dataset_id| dataset_id.as_str() == "urn:ngsi-ld:Dataset:sensor")
        }));
    }

    #[test]
    fn percent_encoded_relationship_target_is_validated_after_decoding() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/ref": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn%3Angsi-ld%3ABuilding%3A1"}]
            }]
        }]));
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        let intent = prepare(
            &entity_id,
            &[],
            &[HistoryRecord::Observations(payload)],
            record(recorded_at),
            WriteOrdinal::at(0),
        )
        .expect("encoded relationship target must be valid")
        .expect("the relationship produces an intent");

        assert!(matches!(
            &intent.entries[0],
            TemporalAppendEntry::Observation {
                row: TemporalHistoryRow {
                    value: TemporalValue::Uri(uri),
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
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        for member in [json!({"@id": "urn:ngsi-ld:Part:1"}), json!({"@value": "urn:ngsi-ld:Part:1"})] {
            let payload = Arc::new(json!([{
                "@id": entity_id,
                "https://example.com/parts": [{
                    "@type": ["https://uri.etsi.org/ngsi-ld/ListRelationship"],
                    "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [member]}]
                }]
            }]));

            let intent = prepare(
                &entity_id,
                &[],
                &[HistoryRecord::Observations(payload)],
                record(recorded_at),
                WriteOrdinal::at(0),
            )
            .expect("an object list must be valid")
            .expect("the list relationship produces an intent");

            assert!(
                matches!(
                    &intent.entries[0],
                    TemporalAppendEntry::Observation {
                        row: TemporalHistoryRow {
                            value: TemporalValue::Object(list),
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
    /// silently dropping it would store a `ListRelationship` that names fewer
    /// Entities than the client wrote.
    #[test]
    fn an_object_list_member_without_a_target_refuses_the_write() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/parts": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/ListRelationship"],
                "https://uri.etsi.org/ngsi-ld/hasObjectList": [{"@list": [{"@type": "urn:ngsi-ld:Part:1"}]}]
            }]
        }]));
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        assert!(
            prepare(
                &entity_id,
                &[],
                &[HistoryRecord::Observations(payload)],
                record(recorded_at),
                WriteOrdinal::at(0)
            )
            .is_err()
        );
    }

    /// Build the expanded payload of one `DateTime`-typed Property value.
    fn datetime_property_payload(entity_id: &Identifier, raw: &str) -> Arc<Value> {
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
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier");
        let payload = datetime_property_payload(&entity_id, "2026-07-25T12:00:00+02:00");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        let error = prepare(
            &entity_id,
            &[],
            &[HistoryRecord::Observations(payload)],
            record(recorded_at),
            WriteOrdinal::at(0),
        )
        .expect_err("a non-Z offset must be refused, not silently reinterpreted");
        assert!(matches!(error, TemporalError::Validation { .. }), "{error:?}");
    }

    /// Clause 4.6.3 admits at most six fractional-second digits; a seventh
    /// digit must be refused on the primary write path exactly as it already
    /// is on instance PATCH, rather than rounded by the storage column.
    #[test]
    fn a_datetime_property_value_with_seven_fractional_digits_is_refused() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity identifier");
        let payload = datetime_property_payload(&entity_id, "2026-07-25T12:00:00.1234567Z");
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");

        let error = prepare(
            &entity_id,
            &[],
            &[HistoryRecord::Observations(payload)],
            record(recorded_at),
            WriteOrdinal::at(0),
        )
        .expect_err("seven fractional digits must be refused, not rounded");
        assert!(matches!(error, TemporalError::Validation { .. }), "{error:?}");
    }
}

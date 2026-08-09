//! Turning expanded entity payloads into the rows one temporal write commits.
//!
//! Preparation is deliberately separate from the write: it touches no
//! database, so the whole translation from JSON-LD to `temporal_entity_attribute`
//! and `attribute_history` rows is testable without one.

use crate::{
    attribute_instance::{
        collect_metadata,
        detect_attribute_type,
        extract_dataset_id,
        extract_observed_at,
        extract_or_generate_instance_id,
        generated_scope_instance_id,
        is_ngsi_ld_attribute_instance,
    },
    attribute_value::{HistoryValueColumns, extract_attribute_value},
    params::{ScopeHistoryEntry, TeaBatchEntry},
    rows::TeaBatchResultRow,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_id::InstanceId, scope::grammar::ScopeBuf, uri::Uri};
use andromeda_ngsi_ld_model::expanded::instance_key::is_system_attribute;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

/// Rows collected from expanded entities, not yet paired with their TEA UUIDs.
///
/// The catalogue upsert is what assigns those UUIDs, so history rows cannot be
/// finished until it has run.
pub struct PreparedObservations {
    /// Catalogue rows to upsert, one per distinct attribute instance grain.
    pub tea_entries: Vec<TeaBatchEntry>,
    /// Per-observation history data awaiting a TEA UUID.
    pub pending_histories: Vec<PendingHistory>,
    /// For each pending history, the index of its entry in `tea_entries`.
    pub tea_index: Vec<usize>,
    /// Scope observations, which have no catalogue row of their own.
    pub scope_entries: Vec<ScopeHistoryEntry>,
}

/// Per-observation history data that only needs a TEA UUID to become a
/// [`HistoryBatchEntry`].
pub struct PendingHistory {
    /// The observation's position on the temporal axis.
    pub time: DateTime<Utc>,
    /// The typed value columns this observation writes.
    pub columns: HistoryValueColumns,
    /// The instance identifier (clause 4.5.7).
    pub instance_id: InstanceId,
    /// Whether the client supplied `instance_id` rather than the broker minting it.
    pub instance_id_client_supplied: bool,
    /// The declared `observedAt`, absent when the instance declared none.
    pub observed_at: Option<DateTime<Utc>>,
    /// Everything on the instance without a dedicated column.
    pub metadata: Option<Value>,
}

/// Prepare observations from entity payloads held behind `Arc`s.
pub fn prepare_from_shared(entities: &[(Arc<Uri>, Arc<Value>)]) -> Result<PreparedObservations, crate::error::TemporalStoreError> {
    let refs: Vec<(&Uri, &Value)> = entities.iter().map(|(id, value)| (id.as_ref(), value.as_ref())).collect();
    prepare(&refs)
}

/// Prepare observations from borrowed entity payloads.
///
/// An attribute the broker cannot read is skipped with a warning rather than
/// failing the whole write: clause 5.6.7 lets a batch partially succeed, and a
/// single unreadable attribute is not grounds to reject an entity outright.
pub fn prepare(entities: &[(&Uri, &Value)]) -> Result<PreparedObservations, crate::error::TemporalStoreError> {
    let mut tea_entries = Vec::new();
    let mut pending_histories: Vec<PendingHistory> = Vec::new();
    let mut tea_index: Vec<usize> = Vec::new();
    let mut scope_entries: Vec<ScopeHistoryEntry> = Vec::new();

    for (entity_id, expanded) in entities {
        let obj = match *expanded {
            Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
            Value::Object(obj) => Some(obj),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
        };
        let Some(obj) = obj else {
            continue;
        };

        for (attr_name, attr_value) in obj {
            if attr_name.starts_with('@') {
                continue;
            }
            let attr_name = attr_name
                .parse::<AttributeName>()
                .map_err(|error| crate::error::TemporalStoreError::Decode(format!("invalid expanded attribute name '{attr_name}': {error}")))?;
            if is_system_attribute(&attr_name) {
                continue;
            }

            if attr_name.as_str() == expanded_properties::SCOPE.as_str() {
                collect_scope_entries(entity_id, attr_value, &mut scope_entries)?;
                continue;
            }

            let Value::Array(instances) = attr_value else {
                continue;
            };

            for instance in instances {
                let Some(inst_obj) = instance.as_object() else {
                    continue;
                };
                if !is_ngsi_ld_attribute_instance(inst_obj) {
                    continue;
                }

                let attribute_type = detect_attribute_type(instance);
                let attribute_value = match extract_attribute_value(instance, attribute_type) {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::warn!(error = %error, entity_id = %entity_id, attr = %attr_name, event = "temporal_batch_extract_failed");
                        continue;
                    }
                };
                let value_type = attribute_value.value_type();
                let dataset_id = extract_dataset_id(instance)?;
                let observed_at = extract_observed_at(instance)?;
                let (instance_id, instance_id_client_supplied) = extract_or_generate_instance_id(instance)?;
                let metadata = collect_metadata(instance);

                let time = observed_at.unwrap_or_else(Utc::now);

                let tea_idx = tea_entries.len();
                tea_entries.push(TeaBatchEntry {
                    entity_id: (*entity_id).clone(),   // clone: the catalogue row owns the entity URI.
                    attribute_name: attr_name.clone(), // clone: one catalogue row owns each instance's attribute name.
                    attribute_type,
                    dataset_id,
                    value_type,
                });
                pending_histories.push(PendingHistory {
                    time,
                    columns: attribute_value.into_columns(),
                    instance_id,
                    instance_id_client_supplied,
                    observed_at,
                    metadata,
                });
                tea_index.push(tea_idx);
            }
        }
    }

    Ok(PreparedObservations {
        tea_entries,
        pending_histories,
        tea_index,
        scope_entries,
    })
}

/// Record one history row per scope the entity declares.
///
/// A value outside the clause 4.18 grammar cannot name a scope, so no history
/// row is recorded for it.
fn collect_scope_entries(entity_id: &Uri, attr_value: &Value, scope_entries: &mut Vec<ScopeHistoryEntry>) -> Result<(), crate::error::TemporalStoreError> {
    let Some(scope_arr) = attr_value.as_array() else {
        return Ok(());
    };
    for scope_entry in scope_arr {
        if let Some(scope) = scope_entry
            .as_object()
            .and_then(|o| o.get("@value"))
            .and_then(|v| v.as_str())
            .and_then(|raw| ScopeBuf::new(raw.to_owned()).ok())
        {
            scope_entries.push(ScopeHistoryEntry {
                time: Utc::now(),
                entity_id: entity_id.clone(), // clone: each persisted scope row owns its entity URI.
                scope_path: scope,
                instance_id: generated_scope_instance_id()?,
            });
        }
    }
    Ok(())
}

/// Pair prepared observations with the TEA UUIDs the catalogue upsert assigned.
///
/// `recorded_at` is stamped once for the whole batch rather than per row:
/// every row in one atomic write is written by the same commit, so they share
/// a write-order position. A per-row `Utc::now()` would impose an arbitrary
/// intra-batch ordering on rows that are in fact simultaneous.
pub fn build_history_entries(prepared: &PreparedObservations, tea_results: &[TeaBatchResultRow]) -> Vec<HistoryBatchEntry> {
    let recorded_at = Utc::now();
    let mut tea_id_map: HashMap<(&Uri, &AttributeName, Option<&Uri>), Uuid> = HashMap::with_capacity(tea_results.len());
    for row in tea_results {
        tea_id_map.insert((&row.entity_id, &row.attribute_name, row.dataset_id.as_ref()), row.id);
    }

    let mut history_entries = Vec::with_capacity(prepared.pending_histories.len());
    for (i, pending) in prepared.pending_histories.iter().enumerate() {
        let tea_entry = &prepared.tea_entries[prepared.tea_index[i]];
        let key = (&tea_entry.entity_id, &tea_entry.attribute_name, tea_entry.dataset_id.as_ref());
        let Some(&tea_id) = tea_id_map.get(&key) else {
            tracing::warn!(
                entity_id = %tea_entry.entity_id,
                attr = %tea_entry.attribute_name,
                event = "tea_id_not_found_in_batch_result"
            );
            continue;
        };
        history_entries.push(HistoryBatchEntry {
            time: pending.time,
            tea_id,
            value_number: pending.columns.value_number,
            value_string: pending.columns.value_string.clone(), // clone: the row owns its value; the prepared batch is reused for the next entry
            value_bool: pending.columns.value_bool,
            value_geo: pending.columns.value_geo.clone(),       // clone: the row owns its geometry
            value_object: pending.columns.value_object.clone(), // clone: the row owns its JSON value
            value_datetime: pending.columns.value_datetime,
            value_uri: pending.columns.value_uri.clone(), // clone: the row owns its URI
            instance_id: pending.instance_id.clone(),     // clone: the row owns its instance identifier
            observed_at: pending.observed_at,
            metadata: pending.metadata.clone(), // clone: the row owns its metadata blob
            recorded_at,
            instance_id_client_supplied: pending.instance_id_client_supplied,
        });
    }
    history_entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

    fn prepare_entity(entity: &Value) -> PreparedObservations {
        let entity_id = "urn:ngsi-ld:Entity:e1".parse::<Uri>().expect("valid entity URI");
        prepare(&[(&entity_id, entity)]).expect("valid observation fixture")
    }

    fn tea_results_for(prepared: &PreparedObservations) -> Vec<TeaBatchResultRow> {
        prepared
            .tea_entries
            .iter()
            .map(|t| TeaBatchResultRow {
                id: Uuid::new_v4(),
                entity_id: t.entity_id.clone(),
                attribute_name: t.attribute_name.clone(),
                dataset_id: t.dataset_id.clone(),
            })
            .collect()
    }

    #[test]
    fn prepare_propagates_the_client_supplied_instance_id_flag() {
        let attr = expanded_properties::VALUE.as_str();
        let property_type = AttributeType::Property.iri_str();
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Entity:e1",
            "@type": ["urn:ngsi-ld:Type:T"],
            "https://example.com/withId": [{
                "@type": [property_type],
                attr: [{"@value": 1}],
                expanded_properties::INSTANCE_ID.as_str(): [
                    {"@id": "urn:ngsi-ld:attribute:instance:client"}
                ]
            }],
            "https://example.com/noId": [{
                "@type": [property_type],
                attr: [{"@value": 2}]
            }],
        });
        let prepared = prepare_entity(&entity);
        pretty_assertions::assert_eq!(prepared.pending_histories.len(), 2);
        let with_id = prepared
            .pending_histories
            .iter()
            .find(|pending| pending.instance_id.as_str() == "urn:ngsi-ld:attribute:instance:client")
            .expect("client-supplied entry");
        let without_id = prepared
            .pending_histories
            .iter()
            .find(|pending| pending.instance_id.as_str() != "urn:ngsi-ld:attribute:instance:client")
            .expect("generated entry");
        assert!(with_id.instance_id_client_supplied);
        assert!(!without_id.instance_id_client_supplied);
    }

    #[test]
    fn build_history_entries_carries_the_flag_to_the_history_row() {
        let attr = expanded_properties::VALUE.as_str();
        let property_type = AttributeType::Property.iri_str();
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Entity:e1",
            "@type": ["urn:ngsi-ld:Type:T"],
            "https://example.com/withId": [{
                "@type": [property_type],
                attr: [{"@value": 1}],
                expanded_properties::INSTANCE_ID.as_str(): [
                    {"@id": "urn:ngsi-ld:attribute:instance:client"}
                ]
            }],
        });
        let prepared = prepare_entity(&entity);
        let history = build_history_entries(&prepared, &tea_results_for(&prepared));
        pretty_assertions::assert_eq!(history.len(), 1);
        pretty_assertions::assert_eq!(history[0].instance_id.as_str(), "urn:ngsi-ld:attribute:instance:client");
        assert!(history[0].instance_id_client_supplied);
    }

    #[test]
    fn two_instances_sharing_one_observed_at_are_both_prepared() {
        // NGSI-LD 5.6.12.4: the instances "shall be added" to the target
        // Temporal Evolution -- pure addition, with no notion of replacing an
        // instance that happens to share a timestamp. 4.5.7 makes instanceId the
        // instance identity, so collapsing two client-supplied instances would
        // destroy one and leave it unaddressable.
        let attr = expanded_properties::VALUE.as_str();
        let observed_at = expanded_properties::OBSERVED_AT.as_str();
        let instance_id = expanded_properties::INSTANCE_ID.as_str();
        let property_type = AttributeType::Property.iri_str();
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Entity:e1",
            "https://example.com/a": [
                {
                    "@type": [property_type],
                    attr: [{"@value": 1}],
                    observed_at: [{"@value": "2026-07-25T10:00:00Z"}],
                    instance_id: [{"@id": "urn:ngsi-ld:attribute:instance:first"}],
                },
                {
                    "@type": [property_type],
                    attr: [{"@value": 2}],
                    observed_at: [{"@value": "2026-07-25T10:00:00Z"}],
                    instance_id: [{"@id": "urn:ngsi-ld:attribute:instance:second"}],
                },
            ],
        });
        let prepared = prepare_entity(&entity);
        pretty_assertions::assert_eq!(prepared.pending_histories.len(), 2);
        let ids: Vec<&str> = prepared.pending_histories.iter().map(|p| p.instance_id.as_str()).collect();
        pretty_assertions::assert_eq!(ids, vec!["urn:ngsi-ld:attribute:instance:first", "urn:ngsi-ld:attribute:instance:second"]);
        let observed: Vec<Option<DateTime<Utc>>> = prepared.pending_histories.iter().map(|p| p.observed_at).collect();
        pretty_assertions::assert_eq!(observed[0], observed[1]);
        // Both share the attribute, so both resolve to one TEA.
        pretty_assertions::assert_eq!(prepared.tea_index, vec![0, 1]);
        pretty_assertions::assert_eq!(prepared.tea_entries[0].attribute_name, prepared.tea_entries[1].attribute_name);
    }

    #[test]
    fn history_entries_share_one_recorded_at_per_batch() {
        // Every row in one atomic write is written by the same commit, so they
        // share a write-order position; a per-row clock read would impose an
        // arbitrary ordering on simultaneous rows.
        let attr = expanded_properties::VALUE.as_str();
        let property_type = AttributeType::Property.iri_str();
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Entity:e1",
            "https://example.com/a": [{"@type": [property_type], attr: [{"@value": 1}]}],
            "https://example.com/b": [{"@type": [property_type], attr: [{"@value": 2}]}],
        });
        let prepared = prepare_entity(&entity);
        let history = build_history_entries(&prepared, &tea_results_for(&prepared));
        pretty_assertions::assert_eq!(history.len(), 2);
        pretty_assertions::assert_eq!(history[0].recorded_at, history[1].recorded_at);
    }

    #[test]
    fn a_scope_outside_the_clause_4_18_grammar_records_no_history() {
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Entity:e1",
            expanded_properties::SCOPE.as_str(): [
                {"@value": "/valid/path"},
                {"@value": "not a scope at all"},
            ],
        });
        let prepared = prepare_entity(&entity);
        pretty_assertions::assert_eq!(prepared.scope_entries.len(), 1);
        pretty_assertions::assert_eq!(prepared.scope_entries[0].entity_id.as_str(), "urn:ngsi-ld:Entity:e1");
    }

    #[test]
    fn a_system_attribute_records_no_observation() {
        let property_type = AttributeType::Property.iri_str();
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Entity:e1",
            expanded_properties::CREATED_AT.as_str(): [{"@value": "2026-07-25T10:00:00Z"}],
            "https://example.com/a": [{"@type": [property_type], expanded_properties::VALUE.as_str(): [{"@value": 1}]}],
        });
        let prepared = prepare_entity(&entity);
        pretty_assertions::assert_eq!(prepared.tea_entries.len(), 1);
        pretty_assertions::assert_eq!(prepared.tea_entries[0].attribute_name.as_str(), "https://example.com/a");
    }
}

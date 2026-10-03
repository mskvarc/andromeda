//! Deterministic merging of context-source temporal entity responses.

use crate::{outcome::ForwardedTemporalEntities, temporal_instances::pair_member};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
use indexmap::IndexMap;
use serde_json::Value;
use std::{mem::replace, slice::from_ref};

/// Merge local and forwarded temporal query entities by entity identifier.
#[must_use]
pub fn merge_temporal_query_results(local_entities: Vec<Value>, remote_results: Vec<ForwardedTemporalEntities>) -> Vec<Value> {
    if remote_results.is_empty() {
        return local_entities;
    }

    let mut entity_map: IndexMap<Identifier, Value> = IndexMap::new();

    for entity in local_entities {
        if let Some(id) = entity.get("id").and_then(|v| v.as_str()).and_then(|id| id.parse().ok()) {
            entity_map.insert(id, entity);
        }
    }

    for forwarded in remote_results {
        match forwarded.mode {
            ForwardingMode::Auxiliary => {
                for remote_entity in forwarded.entities {
                    if let Some(remote_id) = remote_entity.get("id").and_then(|v| v.as_str()).and_then(|id| id.parse::<Identifier>().ok())
                        && let Some(local_entity) = entity_map.get_mut(&remote_id)
                    {
                        merge_temporal_entity_attributes(local_entity, &remote_entity);
                    }
                }
            }
            ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => {
                for remote_entity in forwarded.entities {
                    if let Some(remote_id) = remote_entity.get("id").and_then(|v| v.as_str()).and_then(|id| id.parse::<Identifier>().ok()) {
                        if let Some(local_entity) = entity_map.get_mut(&remote_id) {
                            merge_temporal_entity_attributes(local_entity, &remote_entity);
                        } else {
                            entity_map.insert(remote_id, remote_entity);
                        }
                    }
                }
            }
        }
    }

    entity_map.into_values().collect()
}

/// Merge temporal entity attributes by unioning attribute instance arrays.
///
/// Unlike regular entity merging (which skips existing attributes), temporal merging
/// unions instance arrays for attributes that exist in both entities, since temporal
/// entities carry historical attribute instances.
pub(crate) fn merge_temporal_entity_attributes(local: &mut Value, remote: &Value) {
    if let (Some(local_obj), Some(remote_obj)) = (local.as_object_mut(), remote.as_object()) {
        for (key, remote_value) in remote_obj {
            if key == "id" || key == "type" || key == "@context" {
                continue;
            }
            match local_obj.get_mut(key) {
                Some(local_value) => {
                    // Merge instance arrays for same attribute
                    merge_temporal_instances(local_value, remote_value);
                }
                None => {
                    local_obj.insert(key.clone(), remote_value.clone()); // clone: adding remote temporal attribute not present locally
                }
            }
        }
    }
}

/// Merge temporal attribute instances from remote into local.
///
/// Handles both single-instance (object) and multi-instance (array) forms.
/// Deduplicates by `instanceId` if present. A `temporalValues` attribute object
/// (clause 4.5.9) holds every pair of one `datasetId`, so a remote one joins
/// the local object of the same `datasetId` rather than standing beside it.
fn merge_temporal_instances(local: &mut Value, remote: &Value) {
    let remote_items = match remote {
        Value::Array(arr) => arr.as_slice(),
        Value::Object(_) => from_ref(remote),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => return,
    };
    for item in remote_items {
        if let Some((member, remote_pairs)) = pair_list(item)
            && let Some(local_pairs) = same_dataset_pairs(local, item, member)
        {
            local_pairs.extend(remote_pairs.iter().cloned()); // clone: the merged attribute owns the remote pairs
            continue;
        }
        push_instance(local, item);
    }
}

/// The pair member a `temporalValues` attribute object holds its pairs in,
/// with the pairs.
fn pair_list(attribute: &Value) -> Option<(&'static str, &Vec<Value>)> {
    let member = pair_member(attribute)?;
    Some((member, attribute.get(member)?.as_array()?))
}

/// The pairs of the local `temporalValues` attribute object holding the same
/// `datasetId` as `remote` under `member`, when there is one.
fn same_dataset_pairs<'v>(local: &'v mut Value, remote: &Value, member: &str) -> Option<&'v mut Vec<Value>> {
    let dataset = remote.get("datasetId");
    let candidates: Vec<&'v mut Value> = match local {
        Value::Array(items) => items.iter_mut().collect(),
        object @ Value::Object(_) => vec![object],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    };
    candidates
        .into_iter()
        .find(|candidate| candidate.get("datasetId") == dataset && candidate.get(member).is_some_and(Value::is_array))?
        .get_mut(member)?
        .as_array_mut()
}

/// Add one remote instance beside the local ones, unless a local instance
/// already carries its `instanceId`.
fn push_instance(local: &mut Value, item: &Value) {
    if let Value::Object(_) = local {
        let obj = replace(local, Value::Null);
        *local = Value::Array(vec![obj]);
    }
    let Value::Array(local_arr) = local else {
        return;
    };
    let dominated = item
        .get("instanceId")
        .and_then(|id| id.as_str())
        .and_then(|id| id.parse::<Identifier>().ok())
        .is_some_and(|id| {
            local_arr
                .iter()
                .filter_map(|v| v.get("instanceId").and_then(|id| id.as_str()).and_then(|id| id.parse::<Identifier>().ok()))
                .any(|existing| existing == id)
        });
    if !dominated {
        local_arr.push(item.clone()); // clone: adding remote temporal instance to merged array
    }
}

/// Merge remote temporal entity results for a single entity retrieval.
///
/// Returns the merged temporal entity or None if no entity found from any source.
#[must_use]
pub fn merge_temporal_retrieve_results(local_entity: Option<Value>, remote_results: Vec<ForwardedTemporalEntities>) -> Option<Value> {
    let mut result = local_entity;

    for forwarded in remote_results {
        let is_auxiliary = forwarded.mode == ForwardingMode::Auxiliary;
        for remote_entity in forwarded.entities {
            match &mut result {
                Some(existing) => {
                    merge_temporal_entity_attributes(existing, &remote_entity);
                }
                None => {
                    if !is_auxiliary {
                        result = Some(remote_entity);
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use crate::temporal_result::merge_temporal_instances;
    use serde_json::json;

    /// Clause 4.5.9: a `temporalValues` attribute is one object per
    /// `datasetId`, so the pairs a source answers for the default dataset join
    /// the local object, and those of another dataset stand beside it.
    #[test]
    fn temporal_values_of_one_dataset_fold_into_one_object() {
        let mut local = json!({"type": "Property", "values": [[40, "2020-08-01T12:00:00Z"]]});
        merge_temporal_instances(
            &mut local,
            &json!([
                {"type": "Property", "values": [[60, "2020-08-01T13:00:00Z"]]},
                {"type": "Property", "datasetId": "urn:ds:1", "values": [[70, "2020-08-01T14:00:00Z"]]}
            ]),
        );
        pretty_assertions::assert_eq!(
            local,
            json!([
                {"type": "Property", "values": [[40, "2020-08-01T12:00:00Z"], [60, "2020-08-01T13:00:00Z"]]},
                {"type": "Property", "datasetId": "urn:ds:1", "values": [[70, "2020-08-01T14:00:00Z"]]}
            ])
        );
    }

    /// Normalized instances are unioned, an `instanceId` already held once.
    #[test]
    fn normalized_instances_are_unioned_by_instance_id() {
        let mut local = json!([{"type": "Property", "value": 1, "instanceId": "urn:ngsi-ld:i:1"}]);
        merge_temporal_instances(
            &mut local,
            &json!([
                {"type": "Property", "value": 1, "instanceId": "urn:ngsi-ld:i:1"},
                {"type": "Property", "value": 2, "instanceId": "urn:ngsi-ld:i:2"}
            ]),
        );
        pretty_assertions::assert_eq!(
            local,
            json!([
                {"type": "Property", "value": 1, "instanceId": "urn:ngsi-ld:i:1"},
                {"type": "Property", "value": 2, "instanceId": "urn:ngsi-ld:i:2"}
            ])
        );
    }
}

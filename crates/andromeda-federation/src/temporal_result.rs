//! Deterministic merging of context-source temporal entity responses.

use crate::outcome::ForwardedTemporalEntities;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use indexmap::IndexMap;
use serde_json::Value;
use std::{collections::HashSet, mem::replace, slice::from_ref};

/// Merge local and forwarded temporal query entities by entity identifier.
#[must_use]
pub fn merge_temporal_query_results(local_entities: Vec<Value>, remote_results: Vec<ForwardedTemporalEntities>) -> Vec<Value> {
    if remote_results.is_empty() {
        return local_entities;
    }

    let mut entity_map: IndexMap<Uri, Value> = IndexMap::new();

    for entity in local_entities {
        if let Some(id) = entity.get("id").and_then(|v| v.as_str()).and_then(|id| id.parse().ok()) {
            entity_map.insert(id, entity);
        }
    }

    for forwarded in remote_results {
        match forwarded.mode {
            ForwardingMode::Auxiliary => {
                for remote_entity in forwarded.entities {
                    if let Some(remote_id) = remote_entity.get("id").and_then(|v| v.as_str()).and_then(|id| id.parse::<Uri>().ok())
                        && let Some(local_entity) = entity_map.get_mut(&remote_id)
                    {
                        merge_temporal_entity_attributes(local_entity, &remote_entity);
                    }
                }
            }
            ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => {
                for remote_entity in forwarded.entities {
                    if let Some(remote_id) = remote_entity.get("id").and_then(|v| v.as_str()).and_then(|id| id.parse::<Uri>().ok()) {
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
fn merge_temporal_entity_attributes(local: &mut Value, remote: &Value) {
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
/// Deduplicates by `instanceId` if present.
fn merge_temporal_instances(local: &mut Value, remote: &Value) {
    // Normalize both to arrays for uniform handling
    let local_arr = match local {
        Value::Array(arr) => arr,
        Value::Object(_) => {
            let obj = replace(local, Value::Null);
            *local = Value::Array(vec![obj]);
            // Infallible: we just set `local` to `Value::Array` above.
            let Some(arr) = local.as_array_mut() else { return };
            arr
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => return,
    };

    let remote_items = match remote {
        Value::Array(arr) => arr.as_slice(),
        Value::Object(_) => from_ref(remote),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => return,
    };

    // Collect existing instanceIds for deduplication
    let existing_ids: HashSet<Uri> = local_arr
        .iter()
        .filter_map(|v| v.get("instanceId").and_then(|id| id.as_str()).and_then(|id| id.parse().ok()))
        .collect();

    for item in remote_items {
        let dominated = item
            .get("instanceId")
            .and_then(|id| id.as_str())
            .and_then(|id| id.parse::<Uri>().ok())
            .is_some_and(|id| existing_ids.contains(&id));
        if !dominated {
            local_arr.push(item.clone()); // clone: adding remote temporal instance to merged array
        }
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

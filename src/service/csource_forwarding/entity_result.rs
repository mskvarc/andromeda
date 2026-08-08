//! Merging and classification of forwarded current-entity results.

use crate::service::csource_forwarding::outcome::{ForwardedEntities, ForwardedTemporalEntities, ForwardedWriteResult};
use andromeda_ngsi_ld_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{entity::Entity, registration::ForwardingMode};
use serde_json::Value;
use tracing::warn;

pub(crate) fn parse_forwarded_entities(csr_id: &str, values: Vec<Value>) -> (Vec<Entity>, Vec<NgsiLdWarning>) {
    let mut entities = Vec::with_capacity(values.len());
    let mut warnings = Vec::new();
    for mut value in values {
        if let Value::Object(object) = &mut value {
            object.remove("@context");
        }
        match serde_json::from_value::<Entity>(value) {
            Ok(e) => entities.push(e),
            Err(err) => {
                warn!("Context source {} returned entity that failed to parse: {}", csr_id, err);
                warnings.push(NgsiLdWarning::RevalidationFailed(format!(
                    "Context source {} returned entity that failed to parse: {}",
                    csr_id, err
                )));
            }
        }
    }
    (entities, warnings)
}

/// Merge local and remote entity query results.
///
/// Per NGSI-LD query merge behaviour:
/// - **inclusive** (default): Include both local and remote entities. If same ID exists, merge attributes.
/// - **redirect**: Same as inclusive for query results.
/// - **auxiliary**: Include remote entities and merge when IDs overlap.
/// - **exclusive**: Include remote entities and merge when IDs overlap.
pub fn merge_query_results(local_entities: Vec<Entity>, remote_results: Vec<ForwardedEntities>) -> Vec<Entity> {
    if remote_results.is_empty() {
        return local_entities;
    }

    let mut entity_map: indexmap::IndexMap<String, Entity> = indexmap::IndexMap::with_capacity(local_entities.len());

    for entity in local_entities {
        entity_map.insert(entity.id.to_string(), entity);
    }

    for forwarded in remote_results {
        match forwarded.mode {
            ForwardingMode::Auxiliary | ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => {
                for remote_entity in forwarded.entities {
                    let remote_id = remote_entity.id.to_string();
                    if let Some(local_entity) = entity_map.get_mut(&remote_id) {
                        merge_entity_attributes(local_entity, remote_entity);
                    } else {
                        entity_map.insert(remote_id, remote_entity);
                    }
                }
            }
        }
    }

    entity_map.into_values().collect()
}

/// Merge remote entity's attributes into a local entity.
///
/// Only attribute keys missing locally are filled from the remote copy; local
/// attributes take precedence. Top-level metadata (`id`, `type`, `@context`)
/// is untouched. Non-`attrs` fields (`location`, `observationSpace`,
/// `operationSpace`, `scope`, `expiresAt`) are copied only if unset locally.
fn merge_entity_attributes(local: &mut Entity, remote: Entity) {
    for (key, value) in remote.attrs {
        if !local.attrs.contains_key(&key) {
            local.attrs.insert(key, value);
        }
    }
    if local.location.is_none() {
        local.location = remote.location;
    }
    if local.observation_space.is_none() {
        local.observation_space = remote.observation_space;
    }
    if local.operation_space.is_none() {
        local.operation_space = remote.operation_space;
    }
    if local.scope.is_none() {
        local.scope = remote.scope;
    }
    if local.expires_at.is_none() {
        local.expires_at = remote.expires_at;
    }
}

/// Merge remote entity results for a single entity retrieval.
/// Returns the merged entity or None if no entity found from any source.
pub fn merge_retrieve_results(local_entity: Option<Entity>, remote_results: Vec<ForwardedEntities>) -> Option<Entity> {
    let mut result = local_entity;

    for forwarded in remote_results {
        let is_auxiliary = forwarded.mode == ForwardingMode::Auxiliary;
        for remote_entity in forwarded.entities {
            match &mut result {
                Some(existing) => {
                    merge_entity_attributes(existing, remote_entity);
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

/// Collect all warnings from forwarded entity results.
pub fn collect_warnings(results: &[ForwardedEntities]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// Collect all warnings from forwarded temporal-entity results.
pub fn collect_temporal_warnings(results: &[ForwardedTemporalEntities]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// Collect exclusive attribute names from forwarding results.
///
/// Gathers attribute names marked as exclusive across all forwarded write
/// results. Used by provision handlers to strip exclusive attributes from
/// local operations per spec 4.3.6.3.
pub fn collect_forwarded_exclusive_attrs(results: &[ForwardedWriteResult]) -> Vec<AttributeName> {
    // A forwarded name that is not a valid NGSI-LD attribute name cannot key a
    // locally held attribute, so dropping it here removes nothing that a
    // subsequent strip would have matched.
    results
        .iter()
        .filter(|result| !result.is_failure())
        .flat_map(|r| r.exclusive_attrs.iter())
        .filter_map(|name| name.parse().ok())
        .collect()
}

/// Strip exclusive attributes from a JSON Value object.
///
/// Removes keys matching `exclusive_attrs` from a `Value::Object`. This is a
/// no-op for non-object values. Used for attribute fragment payloads (append,
/// update_attrs) that are `Value` rather than `EntityPatch`.
pub fn strip_exclusive_attrs_from_value(fragment: &mut Value, exclusive_attrs: &[AttributeName]) {
    if let Some(obj) = fragment.as_object_mut() {
        for attr in exclusive_attrs {
            obj.remove(attr.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::service::csource_forwarding::entity_result::parse_forwarded_entities;
    use serde_json::json;

    #[test]
    fn forwarded_json_ld_context_is_metadata_not_an_entity_attribute() {
        let value = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 50},
            "@context": "https://example.com/context.jsonld"
        });

        let (entities, warnings) = parse_forwarded_entities("urn:ngsi-ld:Registration:1", vec![value]);
        pretty_assertions::assert_eq!(entities.len(), 1);
        assert!(warnings.is_empty());
    }
}

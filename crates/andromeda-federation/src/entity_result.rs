//! Merging and classification of federated current-entity results.

use crate::{
    attribute_instance_merge::{MergePolicy, merge_entity_attributes},
    outcome::{ForwardedEntities, ForwardedTemporalEntities, ForwardedWriteResult, SourceContribution},
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{entity::Entity, registration::ForwardingMode};
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_json::Value;
use tracing::{debug, warn};

/// Read the entities a context source answered with.
///
/// Clauses 5.7.1.4 and 5.7.2.4: an entity whose `expiresAt` lies in the past
/// is discarded, and so is every attribute instance whose own `expiresAt` has
/// passed (clause 4.5.5.3), before anything is merged. A payload that is not
/// an entity contributes a clause 6.3.17 warning instead.
pub(crate) fn parse_forwarded_entities(csr_id: &Uri, values: Vec<Value>, now: DateTime<Utc>) -> (Vec<Entity>, Vec<NgsiLdWarning>) {
    let mut entities = Vec::with_capacity(values.len());
    let mut warnings = Vec::new();
    for mut value in values {
        if let Value::Object(object) = &mut value {
            object.remove("@context");
        }
        match deserialize_exact::<Entity>(&value) {
            Ok(mut entity) => {
                if entity.expires_at.is_some_and(|expires_at| expires_at <= now) {
                    debug!("Context source {} returned the expired entity {}, which is discarded", csr_id, entity.id);
                    continue;
                }
                let had_attributes = holds_attributes(&entity);
                entity.remove_expired_attributes(now);
                // Clause 5.7.2.4: an entity whose attribute data has all
                // expired carries nothing to merge and is discarded with it.
                if had_attributes && !holds_attributes(&entity) {
                    debug!(
                        "Context source {} returned the entity {} with only expired attributes, which is discarded",
                        csr_id, entity.id
                    );
                    continue;
                }
                entities.push(entity);
            }
            Err(err) => {
                warn!("Context source {} returned entity that failed to parse: {}", csr_id, err);
                warnings.push(NgsiLdWarning::RevalidationFailed(format!(
                    "Context source {csr_id} returned entity that failed to parse: {err}"
                )));
            }
        }
    }
    (entities, warnings)
}

/// Whether an entity carries any attribute data at all.
fn holds_attributes(entity: &Entity) -> bool {
    !entity.attrs.is_empty() || entity.location.is_some() || entity.observation_space.is_some() || entity.operation_space.is_some()
}

/// The clause 4.5.5 merge policy a registration mode calls for.
const fn policy_of(mode: ForwardingMode) -> MergePolicy {
    match mode {
        ForwardingMode::Inclusive | ForwardingMode::Exclusive | ForwardingMode::Redirect => MergePolicy::Instances,
        ForwardingMode::Auxiliary => MergePolicy::SupplementMissing,
    }
}

/// Merge local and remote entity query results (clause 5.7.2.4).
///
/// The local entities are the base. An entity another source also returned
/// has its attributes merged per clause 4.5.5; one only a source returned is
/// added, except from an auxiliary source, which only supplements entities
/// present already (clause 4.3.6.2).
#[must_use]
pub fn merge_query_results(local_entities: Vec<Entity>, remote_results: Vec<ForwardedEntities>) -> Vec<Entity> {
    if remote_results.is_empty() {
        return local_entities;
    }

    let mut entity_map: IndexMap<Uri, Entity> = IndexMap::with_capacity(local_entities.len());

    for entity in local_entities {
        entity_map.insert(entity.id.clone(), entity); // clone: the index and entity each own the identifier.
    }

    for forwarded in remote_results {
        let policy = policy_of(forwarded.mode);
        for remote_entity in forwarded.entities {
            match entity_map.get_mut(&remote_entity.id) {
                Some(local_entity) => merge_entity_attributes(local_entity, remote_entity, policy),
                None => match policy {
                    MergePolicy::Instances => {
                        entity_map.insert(remote_entity.id.clone(), remote_entity); // clone: the index key and entity each own the identifier.
                    }
                    MergePolicy::SupplementMissing => {}
                },
            }
        }
    }

    entity_map.into_values().collect()
}

/// Merge remote entity results for a single entity retrieval (clause 5.7.1.4).
///
/// Returns the merged entity, or `None` when no source held it.
#[must_use]
pub fn merge_retrieve_results(local_entity: Option<Entity>, remote_results: Vec<ForwardedEntities>) -> Option<Entity> {
    let mut result = local_entity;

    for forwarded in remote_results {
        let policy = policy_of(forwarded.mode);
        for remote_entity in forwarded.entities {
            match &mut result {
                Some(existing) => merge_entity_attributes(existing, remote_entity, policy),
                None => match policy {
                    MergePolicy::Instances => result = Some(remote_entity),
                    MergePolicy::SupplementMissing => {}
                },
            }
        }
    }

    result
}

/// Collect all warnings from forwarded entity results.
#[must_use]
pub fn collect_warnings(results: &[ForwardedEntities]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// Collect all warnings from forwarded temporal-entity results.
#[must_use]
pub fn collect_temporal_warnings(results: &[ForwardedTemporalEntities]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// What each contacted source contributed to the `EntityMap` of a
/// current-entity read (clause 5.2.39), in result order.
pub fn contributions_of(results: &[ForwardedEntities]) -> impl Iterator<Item = SourceContribution> + '_ {
    results.iter().filter_map(ForwardedEntities::contribution)
}

/// What each contacted source contributed to the `EntityMap` of a temporal
/// read (clause 5.2.39), in result order.
pub fn temporal_contributions_of(results: &[ForwardedTemporalEntities]) -> impl Iterator<Item = SourceContribution> + '_ {
    results.iter().filter_map(ForwardedTemporalEntities::contribution)
}

/// The attributes every proxied registration in `results` takes out of local
/// processing, in result order.
#[must_use]
pub fn collect_proxied_attrs(results: &[ForwardedWriteResult]) -> Vec<AttributeName> {
    // A proxied registration owns its attributes whether or not the forward
    // succeeded (clause 4.3.6.3), so a failed or refused forward still keeps
    // them out of local processing.
    results.iter().flat_map(|result| result.proxied_attrs.iter()).cloned().collect()
}

/// Remove the named attributes from a JSON fragment object; a no-op for
/// non-object values. Used for the attribute fragment payloads (append,
/// update) that are a `Value` rather than an `EntityPatch`.
pub fn strip_attributes_from_value(fragment: &mut Value, attributes: &[AttributeName]) {
    if let Some(object) = fragment.as_object_mut() {
        for attribute in attributes {
            object.remove(attribute.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_result::parse_forwarded_entities;
    use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn registration_id() -> Uri {
        "urn:ngsi-ld:Registration:1".parse().expect("registration URI")
    }

    #[test]
    fn forwarded_json_ld_context_is_metadata_not_an_entity_attribute() {
        let value = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 50},
            "@context": "https://example.com/context.jsonld"
        });

        let (entities, warnings) = parse_forwarded_entities(&registration_id(), vec![value], Utc::now());
        pretty_assertions::assert_eq!(entities.len(), 1);
        assert_eq!(warnings, [] as [NgsiLdWarning; 0]);
    }

    #[test]
    fn a_forwarded_entity_keeps_its_exact_decimal_value() {
        let value = serde_json::from_str(r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":0.100000000000000005}}"#)
            .expect("exact remote JSON");
        let (entities, warnings) = parse_forwarded_entities(&registration_id(), vec![value], Utc::now());
        assert_eq!(warnings, [] as [NgsiLdWarning; 0]);
        let encoded = serde_json::to_string(&entities).expect("forwarded response JSON");
        assert!(encoded.contains("0.100000000000000005"), "{encoded}");
    }

    /// Clause 5.7.2.4: an entity whose `expiresAt` lies in the past is
    /// discarded from a source's Entity Array; one still valid is kept.
    #[test]
    fn an_expired_forwarded_entity_is_discarded() {
        let now = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).single().expect("valid instant");
        let expired = json!({"id": "urn:ngsi-ld:Vehicle:old", "type": "Vehicle", "expiresAt": "2000-01-01T00:00:00Z"});
        let valid = json!({"id": "urn:ngsi-ld:Vehicle:new", "type": "Vehicle", "expiresAt": "2999-01-01T00:00:00Z"});
        let (entities, warnings) = parse_forwarded_entities(&registration_id(), vec![expired, valid], now);
        assert_eq!(warnings, [] as [NgsiLdWarning; 0]);
        pretty_assertions::assert_eq!(
            entities.iter().map(|entity| entity.id.as_str()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:new"]
        );
    }

    /// Clause 5.7.2.4: an entity whose every attribute has expired is
    /// discarded whole; one with no attributes at all is not expired.
    #[test]
    fn an_entity_whose_attributes_all_expired_is_discarded() {
        let now = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).single().expect("valid instant");
        let expired =
            json!({"id": "urn:ngsi-ld:Vehicle:old", "type": "Vehicle", "speed": {"type": "Property", "value": 90, "expiresAt": "2000-01-01T00:00:00Z"}});
        let bare = json!({"id": "urn:ngsi-ld:Vehicle:bare", "type": "Vehicle"});
        let (entities, _) = parse_forwarded_entities(&registration_id(), vec![expired, bare], now);
        pretty_assertions::assert_eq!(
            entities.iter().map(|entity| entity.id.as_str()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:bare"]
        );
    }

    /// Clauses 5.7.1.4 and 4.5.5.3: an attribute instance whose `expiresAt`
    /// has passed is discarded before the merge; its siblings survive.
    #[test]
    fn an_expired_forwarded_attribute_instance_is_discarded_before_the_merge() {
        let now = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).single().expect("valid instant");
        let value = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 90, "expiresAt": "2000-01-01T00:00:00Z"},
            "fuelLevel": {"type": "Property", "value": 70, "expiresAt": "2999-01-01T00:00:00Z"}
        });
        let (entities, _) = parse_forwarded_entities(&registration_id(), vec![value], now);
        let entity = entities.first().expect("the entity itself is not expired");
        assert!(!entity.attrs.contains_key("speed"));
        assert!(entity.attrs.contains_key("fuelLevel"));
    }
}

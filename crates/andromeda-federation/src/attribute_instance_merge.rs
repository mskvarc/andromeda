//! The clause 4.5.5 merge of attribute instances received from several sources.
//!
//! Clause 4.5.5.3: instances of one attribute are told apart by `datasetId`
//! (the default instance being the one without), so instances with distinct
//! `datasetId`s coexist in the merged entity, while two instances sharing one
//! are in conflict and the most recent `observedAt` wins, then the most recent
//! `modifiedAt`. Expired instances were already discarded at the registration
//! boundary. Where neither timestamp decides, the locally held instance stays:
//! clause 5.7.1.4 makes local data the base the remote data is merged onto.

use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    entity::Entity,
};
use std::{cmp::Ordering, mem::replace};

/// How a remote entity's attributes fold into the entity being assembled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePolicy {
    /// Clause 4.5.5: instances merge per `datasetId`, the most recent winning.
    Instances,
    /// Clause 4.3.6.2 for an auxiliary registration: an attribute is added
    /// only when no other source supplied it, and never overrides one.
    SupplementMissing,
}

/// Fold `remote`'s attributes into `local` under `policy`.
///
/// Top-level `id` and `type` are untouched. The entity-level `expiresAt`
/// follows clause 4.5.5.3: it is kept only when every version carries one,
/// and then as the furthest in the future.
pub fn merge_entity_attributes(local: &mut Entity, remote: Entity, policy: MergePolicy) {
    for (name, value) in remote.attrs {
        match local.attrs.get_mut(&name) {
            Some(existing) => merge_attribute_value(existing, value, policy),
            None => {
                local.attrs.insert(name, value);
            }
        }
    }
    merge_optional_attribute(&mut local.location, remote.location, policy);
    merge_optional_attribute(&mut local.observation_space, remote.observation_space, policy);
    merge_optional_attribute(&mut local.operation_space, remote.operation_space, policy);
    if local.scope.is_none() {
        local.scope = remote.scope;
    }
    local.expires_at = match (local.expires_at, remote.expires_at) {
        (Some(held), Some(received)) => Some(held.max(received)),
        (None, _) | (_, None) => None,
    };
}

/// Fold one of the entity's named `GeoProperty` members.
fn merge_optional_attribute(local: &mut Option<AttributeValue>, remote: Option<AttributeValue>, policy: MergePolicy) {
    match (local.as_mut(), remote) {
        (Some(existing), Some(received)) => merge_attribute_value(existing, received, policy),
        (None, received @ Some(_)) => *local = received,
        (Some(_) | None, None) => {}
    }
}

/// Fold the instances of one attribute received remotely into the instances
/// held for it.
pub fn merge_attribute_value(local: &mut AttributeValue, remote: AttributeValue, policy: MergePolicy) {
    match policy {
        // The attribute is present already, so an auxiliary source adds nothing.
        MergePolicy::SupplementMissing => {}
        MergePolicy::Instances => {
            let remote_instances = match remote {
                AttributeValue::Single(instance) => vec![*instance],
                AttributeValue::Multiple(instances) => instances,
                // A member carried verbatim has no instances to merge in.
                AttributeValue::Raw(_) => return,
            };
            let held = match replace(local, AttributeValue::Multiple(Vec::new())) {
                AttributeValue::Single(instance) => vec![*instance],
                AttributeValue::Multiple(instances) => instances,
                // A member held verbatim stays as it is.
                AttributeValue::Raw(value) => {
                    *local = AttributeValue::Raw(value);
                    return;
                }
            };
            *local = from_instances(merge_instances(held, remote_instances));
        }
    }
}

/// Merge two instance lists per `datasetId` (clause 4.5.5.3).
fn merge_instances(mut held: Vec<Attribute>, received: Vec<Attribute>) -> Vec<Attribute> {
    for candidate in received {
        match held.iter().position(|existing| existing.dataset_id() == candidate.dataset_id()) {
            Some(index) => {
                if most_recent(&held[index], &candidate) == Ordering::Less {
                    held[index] = candidate;
                }
            }
            None => held.push(candidate),
        }
    }
    held
}

/// Clause 4.5.5.3's order between two conflicting instances: the most recent
/// `observedAt` is the greater, then the most recent `modifiedAt`; instances
/// no timestamp tells apart are equal, and the caller keeps the one it held.
fn most_recent(held: &Attribute, candidate: &Attribute) -> Ordering {
    match (held.common().observed_at, candidate.common().observed_at) {
        (Some(held_at), Some(candidate_at)) if held_at != candidate_at => return held_at.cmp(&candidate_at),
        (Some(_) | None, Some(_) | None) => {}
    }
    match (held.common().modified_at, candidate.common().modified_at) {
        (Some(held_at), Some(candidate_at)) => held_at.cmp(&candidate_at),
        (None, Some(_) | None) | (Some(_), None) => Ordering::Equal,
    }
}

/// The member shape clause 4.5.5 gives a set of instances: one instance is
/// written bare, several as an array.
fn from_instances(mut instances: Vec<Attribute>) -> AttributeValue {
    if instances.len() == 1 {
        match instances.pop() {
            Some(only) => AttributeValue::Single(Box::new(only)),
            None => AttributeValue::Multiple(instances),
        }
    } else {
        AttributeValue::Multiple(instances)
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_instance_merge::{MergePolicy, merge_entity_attributes};
    use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
    use serde_json::{Value, json};

    fn entity(attributes: Value) -> Entity {
        let mut document = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"});
        if let (Some(target), Some(members)) = (document.as_object_mut(), attributes.as_object()) {
            target.extend(members.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
        }
        serde_json::from_value(document).expect("valid entity fixture")
    }

    fn speed_values(entity: &Entity) -> Vec<Value> {
        entity
            .attrs
            .get("speed")
            .map(AttributeValue::instances)
            .into_iter()
            .flatten()
            .map(andromeda_ngsi_ld_model::attribute::instance::Attribute::value)
            .collect()
    }

    /// Clause 4.5.5: instances with distinct `datasetId`s coexist, the default
    /// instance being the one without.
    #[test]
    fn instances_with_distinct_dataset_ids_coexist() {
        let mut local = entity(json!({"speed": {"type": "Property", "value": 10}}));
        let remote = entity(json!({"speed": {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:ds:1"}}));
        merge_entity_attributes(&mut local, remote, MergePolicy::Instances);
        assert!(matches!(local.attrs.get("speed"), Some(AttributeValue::Multiple(instances)) if instances.len() == 2));
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(10), json!(20)]);
    }

    /// Clause 4.5.5.3: for one `datasetId` the most recent `observedAt` wins,
    /// whichever side holds it.
    #[test]
    fn the_same_dataset_id_keeps_the_most_recently_observed_instance() {
        let mut local = entity(json!({"speed": {"type": "Property", "value": 10, "observedAt": "2021-01-01T00:00:00Z"}}));
        let newer = entity(json!({"speed": {"type": "Property", "value": 20, "observedAt": "2022-01-01T00:00:00Z"}}));
        merge_entity_attributes(&mut local, newer, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(20)]);

        let older = entity(json!({"speed": {"type": "Property", "value": 5, "observedAt": "2020-01-01T00:00:00Z"}}));
        merge_entity_attributes(&mut local, older, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(20)]);
        assert!(
            matches!(local.attrs.get("speed"), Some(AttributeValue::Single(_))),
            "one instance stays a bare object"
        );
    }

    /// Clause 4.5.5.3 falls back to `modifiedAt`, and when nothing decides the
    /// locally held instance is the base (clause 5.7.1.4).
    #[test]
    fn modified_at_decides_next_and_an_undecided_conflict_keeps_the_local_instance() {
        let mut local = entity(json!({"speed": {"type": "Property", "value": 10, "modifiedAt": "2021-01-01T00:00:00Z"}}));
        let newer = entity(json!({"speed": {"type": "Property", "value": 20, "modifiedAt": "2022-01-01T00:00:00Z"}}));
        merge_entity_attributes(&mut local, newer, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(20)]);

        let mut local = entity(json!({"speed": {"type": "Property", "value": 10}}));
        let undecided = entity(json!({"speed": {"type": "Property", "value": 20}}));
        merge_entity_attributes(&mut local, undecided, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(10)]);

        let mut local = entity(json!({"speed": {"type": "Property", "value": 10, "observedAt": "2021-01-01T00:00:00Z"}}));
        let same_instant = entity(json!({"speed": {"type": "Property", "value": 20, "observedAt": "2021-01-01T00:00:00Z"}}));
        merge_entity_attributes(&mut local, same_instant, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(10)]);
    }

    /// Clause 4.3.6.2: an auxiliary source adds only what no other source
    /// supplied; a present attribute is never touched.
    #[test]
    fn an_auxiliary_source_only_supplements_missing_attributes() {
        let mut local = entity(json!({"speed": {"type": "Property", "value": 10}}));
        let auxiliary = entity(json!({
            "speed": {"type": "Property", "value": 99, "observedAt": "2030-01-01T00:00:00Z"},
            "fuel": {"type": "Property", "value": 70}
        }));
        merge_entity_attributes(&mut local, auxiliary, MergePolicy::SupplementMissing);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(10)]);
        assert!(local.attrs.contains_key("fuel"));
    }

    /// Clause 4.5.5.3: the entity-level `expiresAt` survives only when every
    /// version carries one, as the furthest in the future; the named
    /// `GeoProperties` merge like any other attribute.
    #[test]
    fn entity_expires_at_and_location_follow_the_conflict_rules() {
        let mut local = entity(json!({"expiresAt": "2030-01-01T00:00:00Z"}));
        let remote = entity(json!({
            "expiresAt": "2031-01-01T00:00:00Z",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        }));
        merge_entity_attributes(&mut local, remote, MergePolicy::Instances);
        pretty_assertions::assert_eq!(local.expires_at.map(|at| at.to_rfc3339()), Some("2031-01-01T00:00:00+00:00".to_string()));
        assert!(local.location.is_some());

        let mut without = entity(json!({}));
        merge_entity_attributes(&mut without, entity(json!({"expiresAt": "2031-01-01T00:00:00Z"})), MergePolicy::Instances);
        assert!(without.expires_at.is_none());
    }
}

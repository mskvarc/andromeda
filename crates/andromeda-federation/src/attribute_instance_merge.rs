//! The clause 4.5.5 merge of attribute instances received from several sources.
//!
//! Clause 4.5.5.3: instances of one attribute are told apart by `datasetId`
//! (the default instance being the one without), so instances with distinct
//! `datasetId`s coexist in the merged entity, while two instances sharing one
//! are in conflict and the most recent `observedAt` wins, then the most recent
//! `modifiedAt`. Expired instances were already discarded at the registration
//! boundary. Where neither timestamp decides, the locally held instance stays:
//! clause 5.7.1.4 makes local data the base the remote data is merged onto.

use andromeda_ngsi_ld_entity::history::instance_recency::InstanceRecency;
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
/// Top-level `id` and `type` are untouched. Under the instance policy the
/// entity-level `expiresAt` follows clause 4.5.5.3: it is kept only when every
/// version carries one, and then as the furthest in the future. Under the
/// supplementing policy it is left alone, because clause 4.3.6.2 puts an
/// auxiliary version outside that conflict. Clause 4.5.5.2 has already moved
/// the transience of each version onto that version's own Attributes, before
/// the first fold.
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
    // Clause 4.18: when representations of the same Entity are merged, "the
    // values of scope are merged" -- keeping only the local set would drop
    // Scopes a Context Source legitimately placed the Entity in. Clause 4.3.6.2
    // bounds that for an auxiliary source: it "never overrides data held
    // directly within a Context Broker", and its data "is only included if it
    // is supplementary to the context data otherwise available", so it supplies
    // the member only where nothing else did -- the same rule every other
    // member of this merge follows.
    local.scope = match (local.scope.take(), remote.scope) {
        (Some(held), Some(received)) => Some(match policy {
            MergePolicy::Instances => held.merged_with(received),
            MergePolicy::SupplementMissing => held,
        }),
        (Some(held), None) => Some(held),
        (None, received) => received,
    };
    local.expires_at = match policy {
        // Clause 4.5.5.3: "If an expiresAt attribute is missing from at least
        // one version of the Entity received from registered Context Sources,
        // the expiresAt attribute is removed from the Entity. Otherwise, the
        // expiresAt attribute of the Entity is set to the DateTime
        // corresponding to the expiresAt DateTime which is furthest in the
        // future." The copy the broker holds itself is one of the versions the
        // clause 5.7.1.4 merge builds on, so a locally held Entity carrying no
        // expiresAt keeps the merged Entity non-transient -- the clause 4.5.5.2
        // pass has already moved the transience a source did report onto that
        // source's own Attributes.
        MergePolicy::Instances => match (local.expires_at, remote.expires_at) {
            (Some(held), Some(received)) => Some(held.max(received)),
            (None, _) | (_, None) => None,
        },
        // Clause 4.3.6.2: an auxiliary Context Source Registration "never
        // overrides data held directly within a Context Broker", so an
        // auxiliary version can neither raise the Entity's expiry nor erase
        // it by not reporting one. Every Entity this policy reaches exists
        // already, so there is no missing member for it to supplement either.
        MergePolicy::SupplementMissing => local.expires_at,
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

/// Clause 4.5.5.3's order between two conflicting instances; instances no
/// timestamp tells apart are equal, and the caller keeps the one it held.
fn most_recent(held: &Attribute, candidate: &Attribute) -> Ordering {
    recency(held).order(&recency(candidate))
}

/// The clause 4.5.5.3 instants one instance carries.
const fn recency(instance: &Attribute) -> InstanceRecency {
    InstanceRecency {
        observed_at: instance.common().observed_at,
        modified_at: instance.common().modified_at,
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
    use andromeda_ngsi_ld_model::{
        attribute::instance::{Attribute, AttributeValue},
        entity::Entity,
    };
    use serde_json::{Value, json};

    fn entity(attributes: &Value) -> Entity {
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
            .map(Attribute::value)
            .collect()
    }

    /// Clause 4.5.5: instances with distinct `datasetId`s coexist, the default
    /// instance being the one without.
    #[test]
    fn instances_with_distinct_dataset_ids_coexist() {
        let mut local = entity(&json!({"speed": {"type": "Property", "value": 10}}));
        let remote = entity(&json!({"speed": {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:ds:1"}}));
        merge_entity_attributes(&mut local, remote, MergePolicy::Instances);
        assert!(matches!(local.attrs.get("speed"), Some(AttributeValue::Multiple(instances)) if instances.len() == 2));
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(10), json!(20)]);
    }

    /// Clause 4.5.5.3: for one `datasetId` the most recent `observedAt` wins,
    /// whichever side holds it.
    #[test]
    fn the_same_dataset_id_keeps_the_most_recently_observed_instance() {
        let mut local = entity(&json!({"speed": {"type": "Property", "value": 10, "observedAt": "2021-01-01T00:00:00Z"}}));
        let newer = entity(&json!({"speed": {"type": "Property", "value": 20, "observedAt": "2022-01-01T00:00:00Z"}}));
        merge_entity_attributes(&mut local, newer, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(20)]);

        let older = entity(&json!({"speed": {"type": "Property", "value": 5, "observedAt": "2020-01-01T00:00:00Z"}}));
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
        let mut local = entity(&json!({"speed": {"type": "Property", "value": 10, "modifiedAt": "2021-01-01T00:00:00Z"}}));
        let newer = entity(&json!({"speed": {"type": "Property", "value": 20, "modifiedAt": "2022-01-01T00:00:00Z"}}));
        merge_entity_attributes(&mut local, newer, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(20)]);

        let mut local = entity(&json!({"speed": {"type": "Property", "value": 10}}));
        let undecided = entity(&json!({"speed": {"type": "Property", "value": 20}}));
        merge_entity_attributes(&mut local, undecided, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(10)]);

        let mut local = entity(&json!({"speed": {"type": "Property", "value": 10, "observedAt": "2021-01-01T00:00:00Z"}}));
        let same_instant = entity(&json!({"speed": {"type": "Property", "value": 20, "observedAt": "2021-01-01T00:00:00Z"}}));
        merge_entity_attributes(&mut local, same_instant, MergePolicy::Instances);
        pretty_assertions::assert_eq!(speed_values(&local), vec![json!(10)]);
    }

    /// Clause 4.3.6.2: an auxiliary source adds only what no other source
    /// supplied; a present attribute is never touched.
    #[test]
    fn an_auxiliary_source_only_supplements_missing_attributes() {
        let mut local = entity(&json!({"speed": {"type": "Property", "value": 10}}));
        let auxiliary = entity(&json!({
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
        let mut local = entity(&json!({"expiresAt": "2030-01-01T00:00:00Z"}));
        let remote = entity(&json!({
            "expiresAt": "2031-01-01T00:00:00Z",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        }));
        merge_entity_attributes(&mut local, remote, MergePolicy::Instances);
        pretty_assertions::assert_eq!(local.expires_at.map(|at| at.to_rfc3339()), Some("2031-01-01T00:00:00+00:00".to_string()));
        assert!(local.location.is_some());

        let mut without = entity(&json!({}));
        merge_entity_attributes(&mut without, entity(&json!({"expiresAt": "2031-01-01T00:00:00Z"})), MergePolicy::Instances);
        assert!(without.expires_at.is_none());
    }

    /// Clause 4.18: "In case multiple representations of the same Entity have
    /// to be merged, e.g. when combining the results of distributed queries,
    /// the values of scope are merged." Keeping only the local set, as the
    /// merge did before, discarded every Scope the remote source reported.
    #[test]
    fn scopes_from_every_representation_are_merged_without_duplicates() {
        let mut local = entity(&json!({"scope": "/madrid/gardens"}));
        let remote = entity(&json!({"scope": ["/madrid/gardens", "/berlin/parks"]}));
        merge_entity_attributes(&mut local, remote, MergePolicy::Instances);
        pretty_assertions::assert_eq!(
            serde_json::to_value(&local.scope).expect("a scope serializes"),
            json!(["/madrid/gardens", "/berlin/parks"])
        );

        // A lone survivor stays bare: "The value of scope is represented as a
        // JSON array in case there is more than one Scope."
        let mut single = entity(&json!({"scope": "/madrid/gardens"}));
        merge_entity_attributes(&mut single, entity(&json!({"scope": "/madrid/gardens"})), MergePolicy::Instances);
        pretty_assertions::assert_eq!(serde_json::to_value(&single.scope).expect("a scope serializes"), json!("/madrid/gardens"));

        // A representation with no scope contributes nothing and removes nothing.
        let mut held = entity(&json!({"scope": "/madrid/gardens"}));
        merge_entity_attributes(&mut held, entity(&json!({})), MergePolicy::Instances);
        assert!(held.scope.is_some());

        let mut absent = entity(&json!({}));
        merge_entity_attributes(&mut absent, entity(&json!({"scope": "/berlin/parks"})), MergePolicy::Instances);
        assert!(absent.scope.is_some());
    }

    /// Clause 4.3.6.2: "An auxiliary Context Source Registration never
    /// overrides data held directly within a Context Broker", and its data "is
    /// only included if it is supplementary to the context data otherwise
    /// available to the Context Broker". An Entity that already carries Scopes
    /// therefore keeps exactly the ones it had.
    #[test]
    fn an_auxiliary_source_adds_no_scope_to_an_entity_that_already_has_one() {
        let mut local = entity(&json!({"scope": "/madrid/gardens"}));
        let auxiliary = entity(&json!({"scope": ["/berlin/parks", "/oslo/forests"]}));
        merge_entity_attributes(&mut local, auxiliary, MergePolicy::SupplementMissing);
        pretty_assertions::assert_eq!(serde_json::to_value(&local.scope).expect("a scope serializes"), json!("/madrid/gardens"));
    }

    /// Clause 4.3.6.2: an auxiliary Context Source Registration "never
    /// overrides data held directly within a Context Broker", so an auxiliary
    /// version that reports no `expiresAt` does not erase the one the primary
    /// sources agreed on, and one that reports a later instant does not raise
    /// it. The clause 4.5.5.3 removal is scoped to the versions that are
    /// parties to the conflict.
    #[test]
    fn an_auxiliary_source_neither_erases_nor_raises_the_entity_expires_at() {
        let mut local = entity(&json!({"expiresAt": "2030-01-01T00:00:00Z"}));
        merge_entity_attributes(&mut local, entity(&json!({})), MergePolicy::SupplementMissing);
        pretty_assertions::assert_eq!(local.expires_at.map(|at| at.to_rfc3339()), Some("2030-01-01T00:00:00+00:00".to_string()));

        merge_entity_attributes(
            &mut local,
            entity(&json!({"expiresAt": "2040-01-01T00:00:00Z"})),
            MergePolicy::SupplementMissing,
        );
        pretty_assertions::assert_eq!(local.expires_at.map(|at| at.to_rfc3339()), Some("2030-01-01T00:00:00+00:00".to_string()));

        let mut without = entity(&json!({}));
        merge_entity_attributes(
            &mut without,
            entity(&json!({"expiresAt": "2040-01-01T00:00:00Z"})),
            MergePolicy::SupplementMissing,
        );
        assert!(without.expires_at.is_none(), "an auxiliary source cannot make locally held data transient");
    }

    /// The other half of clause 4.3.6.2: an auxiliary source does supply the
    /// member when nothing else did, which is what "supplementary" means.
    #[test]
    fn an_auxiliary_source_supplies_a_scope_no_other_source_held() {
        let mut local = entity(&json!({}));
        merge_entity_attributes(&mut local, entity(&json!({"scope": "/berlin/parks"})), MergePolicy::SupplementMissing);
        pretty_assertions::assert_eq!(serde_json::to_value(&local.scope).expect("a scope serializes"), json!("/berlin/parks"));
    }
}

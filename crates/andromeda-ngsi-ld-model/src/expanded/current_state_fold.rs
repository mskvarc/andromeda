//! Folding Attribute instances added to a Temporal Evolution into the
//! Entity's current state.
//!
//! A Temporal Evolution (clause 5.2.20) may carry many instances of one
//! Attribute under one `datasetId`, while current state holds at most one
//! (clause 4.5.5: "There can only be one default Attribute instance for an
//! Attribute with a given Attribute name in any request or response"). The
//! instances a Temporal API write adds therefore reach current state through
//! clause 4.5.5.3's rule for the most current instance: past-due instances are
//! discarded, then the most recent `observedAt` stands, then the most recent
//! `modifiedAt`. The instance being added is the newest write, so it stands
//! whenever no `observedAt` pair says it is older -- and a backfill of
//! observations older than current state changes nothing at all.

use crate::{
    expanded::{
        attribute_expiry::instance_expires_at,
        dataset_id::{InstanceDatasetIdError, get_instance_dataset_id, position_of_dataset},
        instance_key::{AttributeInstanceKey, declared_attribute_type, is_system_attribute},
        node::expanded_object_ref,
        removed_instance::RemovedAttributeInstance,
    },
    instance_recency::InstanceRecency,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use thiserror::Error;

/// A Temporal Evolution fragment that cannot be folded into current state.
#[derive(Debug, Error)]
pub enum CurrentStateFoldError {
    /// The fragment is not an expanded Entity node.
    #[error("the Temporal Evolution fragment is not an expanded entity")]
    NotAnEntity,
    /// An instance's `datasetId` cannot be read, so its identity cannot be
    /// told.
    #[error(transparent)]
    DatasetId(#[from] InstanceDatasetIdError),
}

/// What folding a Temporal Evolution fragment into current state changed.
#[derive(Debug, PartialEq)]
pub struct CurrentStateFold {
    /// The Entity's current state after the fold, as an expanded document.
    pub document: Value,
    /// The instances that now stand in current state because of the fold, as
    /// an expanded Entity node carrying those instances alone.
    pub restated: Value,
    /// The instances the fold removed from current state: a standing deletion
    /// instance of the fragment ends the instance it replaces (clause 4.5.7).
    pub removed: Vec<RemovedAttributeInstance>,
}

/// Fold the Attribute instances of the expanded `evolution` into `current`,
/// the Entity's expanded current state, or into a new Entity when there is
/// none, at the write instant `at`.
///
/// Returns `None` when current state existed and no instance of the fragment
/// stands over what it held. A new Entity is always created, with the
/// fragment's types and Scope, even when no instance of the fragment stands.
/// The instances keep their members except `instanceId`, which identifies an
/// instance of the Temporal Evolution and has no place in current state
/// (clause 4.5.7).
///
/// # Errors
///
/// Returns [`CurrentStateFoldError::NotAnEntity`] when `evolution` is not an
/// expanded Entity node, and [`CurrentStateFoldError::DatasetId`] when an
/// instance's `datasetId` cannot be read.
pub fn fold_into_current_state(current: Option<&Value>, evolution: &Value, at: DateTime<Utc>) -> Result<Option<CurrentStateFold>, CurrentStateFoldError> {
    let fragment = expanded_object_ref(evolution).ok_or(CurrentStateFoldError::NotAnEntity)?;
    let mut entity = match current.and_then(expanded_object_ref) {
        Some(existing) => existing.clone(), // clone: the fold builds the new current state from the stored one
        None => new_entity(fragment),
    };
    let created = current.is_none();
    union_types(&mut entity, fragment);

    let mut touched: Vec<AttributeInstanceKey> = Vec::new();
    let mut removed: Vec<RemovedAttributeInstance> = Vec::new();
    for (name, instances) in fragment.iter().filter_map(|(name, value)| attribute_instances(name, value)) {
        for candidate in instances
            .iter()
            .filter(|instance| instance_expires_at(instance).is_none_or(|expires_at| expires_at > at))
        {
            let key = AttributeInstanceKey {
                attr: name.clone(), // clone: the fold records the identity it touched
                dataset_id: get_instance_dataset_id(candidate)?,
            };
            if stand(&mut entity, &key, candidate, at, &mut removed)? && !touched.contains(&key) {
                touched.push(key);
            }
        }
    }
    if !created && touched.is_empty() {
        return Ok(None);
    }
    let restated = restated_instances(&entity, &touched)?;
    Ok(Some(CurrentStateFold {
        document: Value::Array(vec![Value::Object(entity)]),
        restated,
        removed,
    }))
}

/// The Attribute a member of the fragment names, with its instances, or
/// `None` for a JSON-LD keyword or a system member.
fn attribute_instances<'a>(name: &str, value: &'a Value) -> Option<(AttributeName, &'a Vec<Value>)> {
    if name.starts_with('@') || name == properties::SCOPE.as_str() {
        return None;
    }
    let attribute = name.parse::<AttributeName>().ok()?;
    if is_system_attribute(&attribute) {
        return None;
    }
    value.as_array().map(|instances| (attribute, instances))
}

/// Let `candidate` stand over the instance `entity` holds under `key` when
/// clause 4.5.5.3 does not make it the older one, reporting whether it stood.
///
/// A candidate recording a deletion (it carries `deletedAt`, clause 4.5.7)
/// removes the held instance instead of replacing it.
fn stand(
    entity: &mut Map<String, Value>,
    key: &AttributeInstanceKey,
    candidate: &Value,
    at: DateTime<Utc>,
    removed: &mut Vec<RemovedAttributeInstance>,
) -> Result<bool, InstanceDatasetIdError> {
    let slot = entity.entry(key.attr.as_str().to_owned()).or_insert_with(|| Value::Array(Vec::new()));
    let Some(held_instances) = slot.as_array_mut() else {
        return Ok(false);
    };
    let held_position = position_of_dataset(held_instances, key.dataset_id.as_ref())?;
    let candidate_recency = InstanceRecency {
        observed_at: instant_member(candidate, properties::OBSERVED_AT.as_str()),
        // The candidate is the newest write, so only an `observedAt` pair can
        // make it the older instance.
        modified_at: Some(at),
    };
    if let Some(position) = held_position
        && held_instances
            .get(position)
            .is_some_and(|held| recency_of(held).order(&candidate_recency) == Ordering::Greater)
    {
        return Ok(false);
    }
    let deletion = candidate.get(properties::DELETED_AT.as_str()).is_some();
    match (held_position, deletion) {
        (Some(position), true) => {
            let held = held_instances.remove(position);
            removed.push(RemovedAttributeInstance {
                key: key.clone(), // clone: the removal record owns the identity it names
                attribute_type: declared_attribute_type(&held),
            });
        }
        (None, true) => {}
        (Some(position), false) => held_instances[position] = current_instance(candidate),
        (None, false) => held_instances.push(current_instance(candidate)),
    }
    if held_instances.is_empty() {
        entity.remove(key.attr.as_str());
    }
    Ok(true)
}

/// The instance as current state holds it: every member but the Temporal
/// Evolution's own `instanceId`.
fn current_instance(candidate: &Value) -> Value {
    let mut instance = candidate.clone(); // clone: current state owns its copy of the standing instance
    if let Some(members) = instance.as_object_mut() {
        members.remove(properties::INSTANCE_ID.as_str());
    }
    instance
}

/// The clause 4.5.5.3 instants a held instance carries.
fn recency_of(instance: &Value) -> InstanceRecency {
    InstanceRecency {
        observed_at: instant_member(instance, properties::OBSERVED_AT.as_str()),
        modified_at: instant_member(instance, properties::MODIFIED_AT.as_str()),
    }
}

/// The instant an expanded instance carries under `member`, when it carries
/// one the broker can read.
fn instant_member(instance: &Value, member: &str) -> Option<DateTime<Utc>> {
    instance
        .get(member)
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(|value| value.get("@value"))
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|instant| instant.with_timezone(&Utc))
}

/// A new Entity node with the fragment's identifier, types and Scope.
fn new_entity(fragment: &Map<String, Value>) -> Map<String, Value> {
    let mut entity = Map::new();
    for member in ["@id", "@type", properties::SCOPE.as_str()] {
        if let Some(value) = fragment.get(member) {
            entity.insert(member.to_owned(), value.clone()); // clone: the new Entity owns its identity members
        }
    }
    entity
}

/// Add the fragment's types the Entity does not hold yet (clause 5.6.11.4).
fn union_types(entity: &mut Map<String, Value>, fragment: &Map<String, Value>) {
    let Some(added) = fragment.get("@type").and_then(Value::as_array) else {
        return;
    };
    let held = entity.entry("@type".to_owned()).or_insert_with(|| Value::Array(Vec::new()));
    if let Some(types) = held.as_array_mut() {
        for entity_type in added {
            if !types.contains(entity_type) {
                types.push(entity_type.clone()); // clone: the Entity owns the type it gains
            }
        }
    }
}

/// An Entity node carrying only the instances standing under `touched`.
fn restated_instances(entity: &Map<String, Value>, touched: &[AttributeInstanceKey]) -> Result<Value, InstanceDatasetIdError> {
    let mut restated = Map::new();
    for member in ["@id", "@type"] {
        if let Some(value) = entity.get(member) {
            restated.insert(member.to_owned(), value.clone()); // clone: the restatement owns its identity members
        }
    }
    for key in touched {
        let Some(instances) = entity.get(key.attr.as_str()).and_then(Value::as_array) else {
            continue;
        };
        if let Some(position) = position_of_dataset(instances, key.dataset_id.as_ref())? {
            let standing = restated.entry(key.attr.as_str().to_owned()).or_insert_with(|| Value::Array(Vec::new()));
            if let Some(list) = standing.as_array_mut() {
                list.push(instances[position].clone()); // clone: the restatement owns the standing instance
            }
        }
    }
    Ok(Value::Array(vec![Value::Object(restated)]))
}

#[cfg(test)]
mod tests {
    use crate::expanded::current_state_fold::fold_into_current_state;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    const V: &str = "https://example.org/v";
    const VALUE: &str = "https://uri.etsi.org/ngsi-ld/hasValue";
    const OBSERVED_AT: &str = "https://uri.etsi.org/ngsi-ld/observedAt";
    const MODIFIED_AT: &str = "https://uri.etsi.org/ngsi-ld/modifiedAt";
    const DATASET_ID: &str = "https://uri.etsi.org/ngsi-ld/datasetId";

    fn at(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
    }

    fn instance(value: i64, observed_at: Option<&str>) -> Value {
        let mut instance = json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], VALUE: [{"@value": value}], "https://uri.etsi.org/ngsi-ld/instanceId": [{"@id": "urn:ngsi-ld:instance:x"}]});
        if let Some(observed_at) = observed_at {
            instance[OBSERVED_AT] = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": observed_at}]);
        }
        instance
    }

    fn evolution(instances: &[Value]) -> Value {
        json!([{"@id": "urn:ngsi-ld:TQ:1", "@type": ["https://example.org/TQ"], V: instances}])
    }

    fn values(document: &Value) -> Vec<i64> {
        document[0][V]
            .as_array()
            .expect("the attribute is present")
            .iter()
            .map(|instance| instance[VALUE][0]["@value"].as_i64().expect("an integer value"))
            .collect()
    }

    /// Clause 4.5.5: a new Entity holds one default instance, the one with the
    /// most recent `observedAt`, whatever order the series was written in.
    #[test]
    fn a_new_entity_holds_the_most_recently_observed_instance_alone() {
        let series = evolution(&[
            instance(10, Some("2020-01-01T00:00:00Z")),
            instance(200, Some("2020-01-03T00:00:00Z")),
            instance(50, Some("2020-01-02T00:00:00Z")),
        ]);
        let fold = fold_into_current_state(None, &series, at("2026-01-01T00:00:00Z"))
            .expect("a foldable fragment")
            .expect("a new Entity is always created");
        assert_eq!(values(&fold.document), vec![200]);
        assert!(
            fold.document[0][V][0].get("https://uri.etsi.org/ngsi-ld/instanceId").is_none(),
            "no instanceId in current state"
        );
        assert_eq!(fold.document[0]["@type"], json!(["https://example.org/TQ"]));
        assert_eq!(values(&fold.restated), vec![200]);
    }

    /// Instances without `observedAt` tie, and the later one stands.
    #[test]
    fn instances_without_observed_at_resolve_to_the_later_one() {
        let series = evolution(&[instance(1, None), instance(2, None)]);
        let fold = fold_into_current_state(None, &series, at("2026-01-01T00:00:00Z"))
            .expect("foldable")
            .expect("created");
        assert_eq!(values(&fold.document), vec![2]);
    }

    /// A newer observation replaces the current instance; a backfill of an
    /// older one changes nothing.
    #[test]
    fn a_newer_observation_stands_and_a_backfill_changes_nothing() {
        let mut held = instance(5, Some("2021-01-01T00:00:00Z"));
        held[MODIFIED_AT] = json!([{"@value": "2021-01-01T00:00:00Z"}]);
        let current = json!([{"@id": "urn:ngsi-ld:TQ:1", "@type": ["https://example.org/TQ"], V: [held]}]);

        let newer = fold_into_current_state(
            Some(&current),
            &evolution(&[instance(7, Some("2022-01-01T00:00:00Z"))]),
            at("2026-01-01T00:00:00Z"),
        )
        .expect("foldable")
        .expect("the newer observation stands");
        assert_eq!(values(&newer.document), vec![7]);

        let backfill = fold_into_current_state(
            Some(&current),
            &evolution(&[instance(1, Some("2019-01-01T00:00:00Z"))]),
            at("2026-01-01T00:00:00Z"),
        )
        .expect("foldable");
        assert!(backfill.is_none(), "an older observation leaves current state as it was");
    }

    /// Each `datasetId` is its own instance of the Attribute.
    #[test]
    fn each_dataset_keeps_its_own_standing_instance() {
        let mut named = instance(9, Some("2020-01-01T00:00:00Z"));
        named[DATASET_ID] = json!([{"@id": "urn:ngsi-ld:Dataset:x"}]);
        let series = evolution(&[instance(1, Some("2020-01-02T00:00:00Z")), named]);
        let fold = fold_into_current_state(None, &series, at("2026-01-01T00:00:00Z"))
            .expect("foldable")
            .expect("created");
        let mut held = values(&fold.document);
        held.sort_unstable();
        assert_eq!(held, vec![1, 9]);
    }

    /// Clause 4.22: an instance already past its `expiresAt` never stands.
    #[test]
    fn an_expired_instance_never_stands() {
        let mut expired = instance(99, Some("2025-01-01T00:00:00Z"));
        expired["https://uri.etsi.org/ngsi-ld/expiresAt"] = json!([{"@value": "2025-06-01T00:00:00Z"}]);
        let series = evolution(&[instance(1, Some("2020-01-01T00:00:00Z")), expired]);
        let fold = fold_into_current_state(None, &series, at("2026-01-01T00:00:00Z"))
            .expect("foldable")
            .expect("created");
        assert_eq!(values(&fold.document), vec![1]);
    }

    /// A standing deletion instance ends the current one.
    #[test]
    fn a_standing_deletion_removes_the_current_instance() {
        let current = json!([{"@id": "urn:ngsi-ld:TQ:1", "@type": ["https://example.org/TQ"], V: [instance(5, Some("2021-01-01T00:00:00Z"))]}]);
        let mut deletion = instance(0, Some("2022-01-01T00:00:00Z"));
        deletion["https://uri.etsi.org/ngsi-ld/deletedAt"] = json!([{"@value": "2022-01-01T00:00:00Z"}]);
        let fold = fold_into_current_state(Some(&current), &evolution(&[deletion]), at("2026-01-01T00:00:00Z"))
            .expect("foldable")
            .expect("the deletion stands");
        assert!(fold.document[0].get(V).is_none(), "{}", fold.document);
        assert_eq!(fold.removed.len(), 1);
    }
}

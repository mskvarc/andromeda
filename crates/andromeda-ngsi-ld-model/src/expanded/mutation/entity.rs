//! Entity-wide writes: update (clause 5.6.2), append (5.6.3) and merge patch
//! (5.6.17, behaviour in 5.5.4).

use crate::expanded::{
    dataset_id::get_instance_dataset_id,
    merge::{InstancePatchDepth, deep_merge_attribute, remove_instances_by_dataset},
    mutation::{error::ExpandedEntityError, selection::OverwritePolicy},
    node::{expanded_object_mut, expanded_object_ref},
    null_sentinel::{instance_is_null_sentinel, member_is_null_sentinel},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, rfc3339::whole_seconds};
use andromeda_ngsi_ld_vocabulary::expanded::{classes, properties};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use std::{borrow::Cow, slice::from_ref};

/// NGSI-LD Merge Patch (clause 5.6.17, behaviour in 5.5.4): deep-merge `patch`
/// into `entity`, handling null sentinels for attribute deletion.
///
/// `default_observed_at` is Table 6.5.3.4-1's `observedAt`: when a merged
/// instance names no `observedAt` and the instance it merges into carried one,
/// the instance takes this timestamp.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::EntityNotExpanded`] or
/// [`ExpandedEntityError::PatchNotExpanded`] if either side is not an expanded
/// JSON-LD node.
pub fn merge_entity(entity: &mut Value, patch: &Value, default_observed_at: Option<DateTime<Utc>>) -> Result<(), ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;
    let patch_obj = expanded_object_ref(patch).ok_or(ExpandedEntityError::PatchNotExpanded)?;

    for (key, value) in patch_obj {
        // Merge @type arrays (union, deduplicate) so entity type additions work
        if key == "@type" {
            if let (Some(entity_types), Some(patch_types)) = (entity_obj.get_mut("@type").and_then(Value::as_array_mut), value.as_array()) {
                for t in patch_types {
                    if !entity_types.contains(t) {
                        // clone: the patch is borrowed and the type array takes ownership.
                        entity_types.push(t.clone());
                    }
                }
            }
            continue;
        }
        if key.starts_with('@') {
            continue;
        }

        let Some(patch_instances) = value.as_array() else {
            // clone: both the key and the value are borrowed from the patch and stored owned.
            entity_obj.insert(key.clone(), value.clone());
            continue;
        };

        if patch_instances.len() == 1 && member_is_null_sentinel(&patch_instances[0]) {
            entity_obj.remove(key);
            continue;
        }

        let patch_instances = match default_observed_at {
            Some(observed_at) => Cow::Owned(with_default_observed_at(entity_obj.get(key), patch_instances, observed_at)),
            None => Cow::Borrowed(patch_instances),
        };
        apply_instance_patch(entity_obj, key, &patch_instances, InstancePatchDepth::DeepMerge);
    }

    Ok(())
}

/// Table 6.5.3.4-1 `observedAt`: the patch instances, each one that names no
/// `observedAt` while the existing instance it merges into (same `datasetId`)
/// carries one given `observed_at` in its place.
fn with_default_observed_at(existing: Option<&Value>, patch_instances: &[Value], observed_at: DateTime<Utc>) -> Vec<Value> {
    let existing_instances = existing.and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    patch_instances
        .iter()
        .map(|patch_instance| {
            let dataset_id = get_instance_dataset_id(patch_instance);
            let previously_observed = existing_instances
                .iter()
                .find(|instance| get_instance_dataset_id(instance) == dataset_id)
                .is_some_and(|instance| instance.get(properties::OBSERVED_AT.as_str()).is_some());
            match patch_instance.as_object() {
                Some(instance) if previously_observed && !instance.contains_key(properties::OBSERVED_AT.as_str()) => {
                    let mut stamped = instance.clone(); // clone: the patch is borrowed and the stamped instance is a new value
                    stamped.insert(
                        properties::OBSERVED_AT.as_str().to_string(),
                        json!([{"@type": classes::DATE_TIME.as_str(), "@value": whole_seconds(observed_at)}]),
                    );
                    Value::Object(stamped)
                }
                Some(_) | None => patch_instance.clone(), // clone: the patch is borrowed and the result owns every instance
            }
        })
        .collect()
}

/// Apply one attribute's patch instances, which may mix live updates and null
/// sentinels in a single array (clause 5.6.17 merge, clause 5.6.2 update).
///
/// Each instance is applied in turn: a null sentinel deletes instances by
/// `datasetId` — the default instance when it carries none, per the instance
/// semantics of clause 4.5.5 — and every other instance is deep-merged.
/// Applying them one at a time is what keeps an update that shares the array
/// with a sentinel from being dropped, and stops a datasetId-less sentinel from
/// skipping the instances that follow it.
fn apply_instance_patch(entity_obj: &mut Map<String, Value>, key: &str, patch_instances: &[Value], depth: InstancePatchDepth) {
    if !patch_instances.iter().any(instance_is_null_sentinel) {
        deep_merge_attribute(entity_obj, key, patch_instances, depth);
        return;
    }

    for patch_instance in patch_instances {
        if instance_is_null_sentinel(patch_instance) {
            let dataset_id = get_instance_dataset_id(patch_instance);
            remove_instances_by_dataset(entity_obj, key, dataset_id.as_ref());
        } else {
            deep_merge_attribute(entity_obj, key, from_ref(patch_instance), depth);
        }
    }
}

/// Whether an expanded `scope` member is the NGSI-LD Null of a deleted scope.
///
/// Clause 4.18 admits the sentinel only "in case of deleted scopes", so it is
/// the whole value of the member and never one entry beside a path; a member
/// that mixes the two is refused before a write reaches this point.
fn member_is_scope_deletion(member: &Value) -> bool {
    member.as_array().is_some_and(|values| values.len() == 1 && member_is_null_sentinel(&values[0]))
}

/// Whether an expanded member carries nothing but deletions.
///
/// Clause 4.5.0 spells a deleted member either as the bare NGSI-LD Null the
/// first level takes or as an instance whose value member is the sentinel, and
/// an attribute member is an array of instances. A member every instance of
/// which is a deletion asks for the whole member to go.
fn member_is_deletion(member: &Value) -> bool {
    match member {
        Value::Array(instances) => {
            !instances.is_empty()
                && instances
                    .iter()
                    .all(|instance| member_is_null_sentinel(instance) || instance_is_null_sentinel(instance))
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => {
            member_is_null_sentinel(member) || instance_is_null_sentinel(member)
        }
    }
}

/// What [`drop_deletion_sentinels`] found in the payload it was given.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DroppedSentinels {
    /// At least one member asked for a deletion and was dropped.
    Dropped,
    /// The payload asked for no deletion and is unchanged.
    Unchanged,
}

/// Drop every member of an expanded entity payload that asks for a deletion.
///
/// This is clause 5.5.8 applied to a target that contains nothing: "for each
/// member of the Fragment, whose value is an NGSI-LD Null, contained by the
/// target, the target member is deleted" leaves a deletion the target does not
/// contain with nothing to do. Clause 5.6.8.4 is where that arises -- its
/// update mode runs clause 5.6.2, which supports NGSI-LD Nulls, over a payload
/// that creates the Entity when it does not exist yet -- and the sentinels are
/// dropped rather than stored, because clause 4.5.0 reserves the value for a
/// deletion and a stored one would be served back as data.
///
/// The entity `scope` member of clause 4.18 is dropped on the same terms: the
/// clause admits the sentinel there "only in case of deleted scopes".
pub fn drop_deletion_sentinels(entity: &mut Value) -> DroppedSentinels {
    let Some(entity_obj) = expanded_object_mut(entity) else {
        return DroppedSentinels::Unchanged;
    };
    let deletions: Vec<String> = entity_obj
        .iter()
        .filter(|(key, _)| !key.starts_with('@'))
        .filter(|(key, member)| {
            if key.as_str() == properties::SCOPE.as_str() {
                member_is_scope_deletion(member)
            } else {
                member_is_deletion(member)
            }
        })
        .map(|(key, _)| key.clone()) // clone: the removal outlives the borrow the scan holds
        .collect();
    if deletions.is_empty() {
        return DroppedSentinels::Unchanged;
    }
    for key in deletions {
        entity_obj.remove(&key);
    }
    DroppedSentinels::Dropped
}

/// Append attributes (clause 5.6.3): add attributes from `attrs` to `entity`.
///
/// Under [`OverwritePolicy::Preserve`], existing instances (matched by
/// datasetId) are kept; new instances with a different datasetId are still
/// appended. Under [`OverwritePolicy::Overwrite`], instances are merged by
/// datasetId.
///
/// Returns the attribute IRIs where at least one instance was NOT appended
/// because [`OverwritePolicy::Preserve`] was in force and the instance already
/// existed.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::EntityNotExpanded`] if `entity` is not an
/// expanded JSON-LD node, or [`ExpandedEntityError::AttributesNotObject`] if
/// `attrs` is not a JSON object.
pub fn append_entity_attrs(entity: &mut Value, attrs: &Value, overwrite: OverwritePolicy) -> Result<Vec<AttributeName>, ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;
    let attrs_obj = attrs.as_object().ok_or(ExpandedEntityError::AttributesNotObject)?;
    let no_overwrite = overwrite == OverwritePolicy::Preserve;

    let mut not_appended = Vec::new();

    for (key, value) in attrs_obj {
        if key.starts_with('@') {
            continue;
        }

        // Clause 5.6.3.4: "If scope is included in the Fragment and overwrite is
        // allowed, the scope of the target Entity will become the one included
        // in the Fragment. Otherwise, the Scopes in the Fragment that are not
        // part of the value of scope of the target Entity will be appended".
        if key == properties::SCOPE.as_str() {
            // No NGSI-LD Null reaches here: clause 5.5.4 excepts only the
            // fragments of a partial update or a merge from its first-level
            // prohibition, and neither Append Attributes (clause 5.6.3) nor the
            // batch update of clause 5.6.9 that composes this function is one
            // of them, so both screen the sentinel out before the write runs.
            if let (Some(Value::Array(existing_scopes)), Some(new_scopes)) = (entity_obj.get_mut(key), value.as_array()) {
                if no_overwrite {
                    for s in new_scopes {
                        if !existing_scopes.contains(s) {
                            // clone: the incoming scope is borrowed and the array takes ownership.
                            existing_scopes.push(s.clone());
                        }
                    }
                } else {
                    // clone: both the key and the value are borrowed from the fragment and stored owned.
                    entity_obj.insert(key.clone(), value.clone());
                }
            } else {
                // clone: both the key and the value are borrowed from the fragment and stored owned.
                entity_obj.insert(key.clone(), value.clone());
            }
            continue;
        }

        if !entity_obj.contains_key(key) {
            // clone: both the key and the value are borrowed from the fragment and stored owned.
            entity_obj.insert(key.clone(), value.clone());
            continue;
        }

        // Attribute already exists -- merge by datasetId
        let Some(patch_instances) = value.as_array() else {
            if no_overwrite {
                not_appended.push(
                    key.parse::<AttributeName>()
                        .map_err(|_| ExpandedEntityError::InvalidAttributeName { name: key.clone() })?,
                );
            } else {
                // clone: both the key and the value are borrowed from the fragment and stored owned.
                entity_obj.insert(key.clone(), value.clone());
            }
            continue;
        };

        if no_overwrite {
            // Only add instances whose datasetId doesn't already exist
            let Some(Value::Array(existing_arr)) = entity_obj.get_mut(key) else {
                // clone: both the key and the value are borrowed from the fragment and stored owned.
                entity_obj.insert(key.clone(), value.clone());
                continue;
            };
            let mut any_skipped = false;
            for patch_inst in patch_instances {
                let patch_ds = get_instance_dataset_id(patch_inst);
                let already_exists = existing_arr.iter().any(|inst| get_instance_dataset_id(inst) == patch_ds);
                if already_exists {
                    any_skipped = true;
                } else {
                    // clone: the incoming instance is borrowed and the array takes ownership.
                    existing_arr.push(patch_inst.clone());
                }
            }
            if any_skipped {
                not_appended.push(
                    key.parse::<AttributeName>()
                        .map_err(|_| ExpandedEntityError::InvalidAttributeName { name: key.clone() })?,
                );
            }
        } else {
            // Append with overwrite replaces the matched instance (clause 5.6.3),
            // the same whole-instance semantics as Update Attributes.
            deep_merge_attribute(entity_obj, key, patch_instances, InstancePatchDepth::ReplaceInstance);
        }
    }

    Ok(not_appended)
}

/// Update entity attributes (clause 5.6.2).
///
/// For regular attributes: if the attribute exists, update by datasetId
/// matching; if not, add it. For scope: if scope doesn't exist on the
/// entity, report as not updated; if it does, the fragment's scope replaces it,
/// or deletes it when the fragment's value is the NGSI-LD Null of clause 4.18.
///
/// Returns a list of attribute IRIs that were NOT updated (e.g. scope not
/// present). The caller uses this to decide between 204 and 207.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::EntityNotExpanded`] if `entity` is not an
/// expanded JSON-LD node, or [`ExpandedEntityError::AttributesNotObject`] if
/// `attrs` is not a JSON object.
pub fn update_entity_attrs(entity: &mut Value, attrs: &Value) -> Result<Vec<AttributeName>, ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;
    let attrs_obj = attrs.as_object().ok_or(ExpandedEntityError::AttributesNotObject)?;

    let mut not_updated = Vec::new();

    for (key, value) in attrs_obj {
        // Merge @type arrays (union, deduplicate) so entity type additions work
        if key == "@type" {
            if let (Some(entity_types), Some(patch_types)) = (entity_obj.get_mut("@type").and_then(Value::as_array_mut), value.as_array()) {
                for t in patch_types {
                    if !entity_types.contains(t) {
                        // clone: the patch is borrowed and the type array takes ownership.
                        entity_types.push(t.clone());
                    }
                }
            }
            continue;
        }
        if key.starts_with('@') {
            continue;
        }

        // Clause 5.6.2.4: "If scope is included in the Fragment and the target
        // entity includes scope, replace the scope by the one included in the
        // Fragment, otherwise ignore it." Clause 5.5.8 gives the NGSI-LD Null
        // its own reading of the same member: "For each member of the Fragment,
        // whose value is an NGSI-LD Null, contained by the target, the target
        // member is deleted", which for `scope` is the deleted scope clause
        // 4.18 reserves the sentinel for.
        if key == properties::SCOPE.as_str() {
            if !entity_obj.contains_key(key) {
                not_updated.push(
                    key.parse::<AttributeName>()
                        .map_err(|_| ExpandedEntityError::InvalidAttributeName { name: key.clone() })?,
                );
                continue;
            }
            if member_is_scope_deletion(value) {
                entity_obj.remove(key);
            } else {
                // clone: both the key and the value are borrowed from the fragment and stored owned.
                entity_obj.insert(key.clone(), value.clone());
            }
            continue;
        }

        if !entity_obj.contains_key(key) {
            // Clause 5.5.8 deletes "each member of the Fragment, whose value is
            // an NGSI-LD Null, contained by the target": a deletion the target
            // does not contain has nothing to delete, and appending it would
            // store the reserved value as data.
            if !member_is_deletion(value) {
                // clone: both the key and the value are borrowed from the fragment and stored owned.
                entity_obj.insert(key.clone(), value.clone());
            }
            continue;
        }

        let Some(patch_instances) = value.as_array() else {
            // clone: both the key and the value are borrowed from the fragment and stored owned.
            entity_obj.insert(key.clone(), value.clone());
            continue;
        };

        apply_instance_patch(entity_obj, key, patch_instances, InstancePatchDepth::ReplaceInstance);
    }

    Ok(not_updated)
}

#[cfg(test)]
mod tests {
    use crate::expanded::mutation::{
        entity::{DroppedSentinels, append_entity_attrs, drop_deletion_sentinels, merge_entity, update_entity_attrs},
        error::ExpandedEntityError,
        selection::OverwritePolicy,
        speed_entity,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use serde_json::{Value, json};

    /// The expanded IRI of the entity `scope` member (clause 4.18).
    fn scope_iri() -> &'static str {
        properties::SCOPE.as_str()
    }

    /// One expanded Property instance carrying `value`, optionally tagged with a
    /// `datasetId`. A `value` of `urn:ngsi-ld:null` is a deletion sentinel.
    fn property_instance(value: &Value, dataset: Option<&str>) -> Value {
        let mut node = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        });
        if let Some(dataset) = dataset {
            node["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        node
    }

    /// An entity carrying `speed` with the given instances.
    fn entity_with_speed(instances: &[Value]) -> Value {
        json!([{ "@id": "urn:ngsi-ld:Device:1", "https://example.com/speed": instances }])
    }

    /// The `speed` instances of a merged entity.
    fn speed_instances(entity: &Value) -> Vec<Value> {
        entity[0]["https://example.com/speed"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn a_mixed_merge_array_deletes_one_dataset_and_applies_another() {
        // Clause 5.6.17: a merge array that carries a null sentinel for
        // datasetId A alongside a live update for datasetId B must delete A and
        // apply B. The update must not be dropped for sharing the array with a
        // sentinel.
        let mut entity = entity_with_speed(&[
            property_instance(&json!(10), Some("urn:ngsi-ld:Dataset:A")),
            property_instance(&json!(15), Some("urn:ngsi-ld:Dataset:B")),
        ]);
        let patch = entity_with_speed(&[
            property_instance(&json!("urn:ngsi-ld:null"), Some("urn:ngsi-ld:Dataset:A")),
            property_instance(&json!(20), Some("urn:ngsi-ld:Dataset:B")),
        ]);

        merge_entity(&mut entity, &patch, None).expect("merge succeeds");

        let instances = speed_instances(&entity);
        pretty_assertions::assert_eq!(instances.len(), 1, "A deleted, B kept");
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/datasetId"][0]["@id"], json!("urn:ngsi-ld:Dataset:B"));
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"], json!(20));
    }

    #[test]
    fn a_datasetless_null_sentinel_deletes_only_the_default_and_keeps_a_later_update() {
        // The old code removed the whole attribute on a datasetId-less sentinel
        // and broke out of the loop, dropping the datasetId B update that
        // followed. The default instance must go while B survives and updates.
        let mut entity = entity_with_speed(&[
            property_instance(&json!(10), None),
            property_instance(&json!(15), Some("urn:ngsi-ld:Dataset:B")),
        ]);
        let patch = entity_with_speed(&[
            property_instance(&json!("urn:ngsi-ld:null"), None),
            property_instance(&json!(20), Some("urn:ngsi-ld:Dataset:B")),
        ]);

        merge_entity(&mut entity, &patch, None).expect("merge succeeds");

        let instances = speed_instances(&entity);
        pretty_assertions::assert_eq!(instances.len(), 1, "the default instance is gone, B is kept");
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/datasetId"][0]["@id"], json!("urn:ngsi-ld:Dataset:B"));
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"], json!(20));
    }

    #[test]
    fn a_bare_member_null_sentinel_deletes_every_attribute_instance() {
        let mut entity = entity_with_speed(&[
            property_instance(&json!(10), None),
            property_instance(&json!(15), Some("urn:ngsi-ld:Dataset:B")),
        ]);
        let patch = json!([{
            "https://example.com/speed": [{"@value": "urn:ngsi-ld:null"}]
        }]);

        merge_entity(&mut entity, &patch, None).expect("merge succeeds");

        assert!(entity[0].get("https://example.com/speed").is_none());
    }

    #[test]
    fn update_attributes_deletes_the_default_instance_and_keeps_the_named_update() {
        // ETSI GS CIM 009 v1.9.1 clause 5.6.2 updates each supplied instance;
        // the default-instance deletion sentinel cannot discard a named update
        // later in the same attribute array.
        let mut entity = entity_with_speed(&[
            property_instance(&json!(10), None),
            property_instance(&json!(15), Some("urn:ngsi-ld:Dataset:B")),
        ]);
        let patch = serde_json::json!({
            "https://example.com/speed": [
                property_instance(&json!("urn:ngsi-ld:null"), None),
                property_instance(&json!(20), Some("urn:ngsi-ld:Dataset:B"))
            ]
        });

        let not_updated = update_entity_attrs(&mut entity, &patch).expect("update succeeds");

        pretty_assertions::assert_eq!(not_updated, Vec::<AttributeName>::new());
        let instances = speed_instances(&entity);
        pretty_assertions::assert_eq!(instances.len(), 1);
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/datasetId"][0]["@id"], json!("urn:ngsi-ld:Dataset:B"));
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"], json!(20));
    }

    #[test]
    fn a_datasetless_null_on_a_lone_default_instance_deletes_the_whole_attribute() {
        // With only a default instance present, deleting it empties the array,
        // so the attribute member itself is dropped — the common bare-null case.
        let mut entity = entity_with_speed(&[property_instance(&json!(10), None)]);
        let patch = entity_with_speed(&[property_instance(&json!("urn:ngsi-ld:null"), None)]);

        merge_entity(&mut entity, &patch, None).expect("merge succeeds");

        assert!(entity[0].get("https://example.com/speed").is_none(), "the attribute is gone");
    }

    #[test]
    fn update_attributes_replaces_the_whole_instance_dropping_unmentioned_subattrs() {
        // ETSI GS CIM 009 v1.9.1 clause 5.5.8 EXAMPLE 2: Update Attributes
        // overwrites the whole matched instance, so a `unitCode` the patch omits
        // is removed rather than carried forward.
        let mut existing = property_instance(&json!(25), None);
        existing["https://uri.etsi.org/ngsi-ld/unitCode"] = json!([{"@value": "CEL"}]);
        let mut entity = entity_with_speed(&[existing]);
        let patch = json!({"https://example.com/speed": [property_instance(&json!(100), None)]});

        let not_updated = update_entity_attrs(&mut entity, &patch).expect("update succeeds");

        pretty_assertions::assert_eq!(not_updated, Vec::<AttributeName>::new());
        let instances = speed_instances(&entity);
        pretty_assertions::assert_eq!(instances.len(), 1);
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"], json!(100));
        assert!(
            instances[0].get("https://uri.etsi.org/ngsi-ld/unitCode").is_none(),
            "the omitted sub-attribute is dropped"
        );
    }

    #[test]
    fn merge_recurses_into_a_value_object() {
        // ETSI GS CIM 009 v1.9.1 clause 5.5.12: Merge Entity descends into a
        // Property value object, so touching `street` leaves `city` standing.
        let mut entity = json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "https://example.com/address": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "https://example.com/street": [{"@value": "old"}],
                    "https://example.com/city": [{"@value": "town"}]
                }]
            }]
        }]);
        let patch = json!([{
            "https://example.com/address": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "https://example.com/street": [{"@value": "new"}]
                }]
            }]
        }]);

        merge_entity(&mut entity, &patch, None).expect("merge succeeds");

        let value = &entity[0]["https://example.com/address"][0]["https://uri.etsi.org/ngsi-ld/hasValue"][0];
        pretty_assertions::assert_eq!(value["https://example.com/street"], json!([{"@value": "new"}]));
        pretty_assertions::assert_eq!(value["https://example.com/city"], json!([{"@value": "town"}]));
    }

    #[test]
    fn a_default_observed_at_stamps_only_instances_that_previously_carried_one() {
        // Table 6.5.3.4-1 `observedAt`: the parameter fills in a missing
        // observedAt only where the target instance already had one, and never
        // overrides one the fragment names.
        let mut observed = property_instance(&json!(10), None);
        observed["https://uri.etsi.org/ngsi-ld/observedAt"] = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2020-01-01T00:00:00Z"}]);
        let mut entity = entity_with_speed(&[observed, property_instance(&json!(15), Some("urn:ngsi-ld:Dataset:B"))]);
        entity[0]["https://example.com/brand"] = json!([property_instance(&json!("old"), None)]);
        let mut explicit = property_instance(&json!(20), Some("urn:ngsi-ld:Dataset:B"));
        explicit["https://uri.etsi.org/ngsi-ld/observedAt"] = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2021-06-01T00:00:00Z"}]);
        let patch = json!([{
            "https://example.com/speed": [property_instance(&json!(11), None), explicit],
            "https://example.com/brand": [property_instance(&json!("new"), None)]
        }]);
        let at = "2024-02-03T04:05:06Z".parse::<chrono::DateTime<chrono::Utc>>().expect("valid timestamp");

        merge_entity(&mut entity, &patch, Some(at)).expect("merge succeeds");

        let speed = speed_instances(&entity);
        pretty_assertions::assert_eq!(
            speed[0]["https://uri.etsi.org/ngsi-ld/observedAt"][0]["@value"],
            json!("2024-02-03T04:05:06Z"),
            "the default instance previously observed takes the parameter"
        );
        pretty_assertions::assert_eq!(
            speed[1]["https://uri.etsi.org/ngsi-ld/observedAt"][0]["@value"],
            json!("2021-06-01T00:00:00Z"),
            "an explicit observedAt in the fragment is kept"
        );
        assert!(
            entity[0]["https://example.com/brand"][0]
                .get("https://uri.etsi.org/ngsi-ld/observedAt")
                .is_none(),
            "an attribute that never carried observedAt gets none"
        );
    }

    #[test]
    fn merge_entity_adds_new_attr() {
        let mut entity = json!([{"@type": ["https://example.com/T"], "https://example.com/a": [{"@value": 1}]}]);
        let patch = json!([{"https://example.com/b": [{"@value": 2}]}]);
        merge_entity(&mut entity, &patch, None).unwrap();
        assert!(entity[0].get("https://example.com/b").is_some());
    }

    #[test]
    fn append_no_overwrite_skips_existing() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        let attrs = json!({"https://example.com/a": [{"@value": 99}], "https://example.com/b": [{"@value": 2}]});
        append_entity_attrs(&mut entity, &attrs, OverwritePolicy::Preserve).unwrap();
        pretty_assertions::assert_eq!(entity[0]["https://example.com/a"][0]["@value"], 1);
        pretty_assertions::assert_eq!(entity[0]["https://example.com/b"][0]["@value"], 2);
    }

    #[test]
    fn appending_an_existing_attribute_with_preserve_leaves_it_untouched() {
        // NGSI-LD append semantics with the `noOverwrite` option, ETSI GS CIM
        // 009 v1.9.1 clause 5.6.3.
        let mut entity = json!([{"https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}]}]}]);
        let attrs = json!({"https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}]}]});
        let not_appended = append_entity_attrs(&mut entity, &attrs, OverwritePolicy::Preserve).expect("append succeeds");
        pretty_assertions::assert_eq!(speed_entity::values(&entity), vec![10]);
        pretty_assertions::assert_eq!(
            not_appended,
            vec!["https://example.com/speed".parse::<AttributeName>().expect("valid attribute name")]
        );
    }

    #[test]
    fn appending_an_existing_attribute_with_overwrite_replaces_it() {
        let mut entity = json!([{"https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}]}]}]);
        let attrs = json!({"https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}]}]});
        let not_appended = append_entity_attrs(&mut entity, &attrs, OverwritePolicy::Overwrite).expect("append succeeds");
        pretty_assertions::assert_eq!(speed_entity::values(&entity), vec![20]);
        assert!(not_appended.is_empty(), "{not_appended:?}");
    }

    /// Clause 5.5.8: "For each member of the Fragment, whose value is an
    /// NGSI-LD Null, contained by the target, the target member is deleted."
    /// Clause 4.18 spells a deleted scope with that same sentinel, so an Update
    /// Attributes fragment naming it removes the member rather than storing a
    /// scope named after the reserved string.
    #[test]
    fn update_attributes_deletes_the_scope_the_null_sentinel_names() {
        let mut entity = json!([{"@id": "urn:ngsi-ld:Device:1", scope_iri(): [{"@value": "/Madrid"}]}]);
        let patch = json!({scope_iri(): [{"@value": "urn:ngsi-ld:null"}]});

        let not_updated = update_entity_attrs(&mut entity, &patch).expect("update succeeds");

        pretty_assertions::assert_eq!(not_updated, Vec::<AttributeName>::new());
        assert!(entity[0].get(scope_iri()).is_none(), "the scope member is gone: {entity}");
    }

    /// Clause 5.5.8 deletes "each member of the Fragment, whose value is an
    /// NGSI-LD Null, contained by the target": a deletion the target does not
    /// contain has nothing to delete, and must not be appended as data. Clause
    /// 4.5.0 reserves the value, so a stored one would be served back as if a
    /// Context Producer had written it.
    #[test]
    fn update_attributes_ignores_a_deletion_of_an_attribute_the_target_lacks() {
        let mut entity = json!([{"@id": "urn:ngsi-ld:Device:1"}]);
        let patch = json!({"https://example.com/speed": [property_instance(&json!("urn:ngsi-ld:null"), None)]});

        let not_updated = update_entity_attrs(&mut entity, &patch).expect("update succeeds");

        pretty_assertions::assert_eq!(not_updated, Vec::<AttributeName>::new());
        assert!(entity[0].get("https://example.com/speed").is_none(), "no member was appended: {entity}");
    }

    /// The same holds for the bare first-level sentinel a fragment deletes a
    /// whole member with.
    #[test]
    fn update_attributes_ignores_a_bare_deletion_of_an_attribute_the_target_lacks() {
        let mut entity = json!([{"@id": "urn:ngsi-ld:Device:1"}]);
        let patch = json!({"https://example.com/speed": [{"@value": "urn:ngsi-ld:null"}]});

        update_entity_attrs(&mut entity, &patch).expect("update succeeds");

        assert!(entity[0].get("https://example.com/speed").is_none(), "no member was appended: {entity}");
    }

    /// Clause 5.6.8.4 update mode runs clause 5.6.2 over a payload that also
    /// creates the Entity when it does not exist: on that branch the deletions
    /// have nothing to delete and are dropped, while everything else is stored.
    #[test]
    fn dropping_deletion_sentinels_keeps_the_data_and_removes_every_deletion() {
        let mut payload = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "@type": ["https://example.com/Device"],
            scope_iri(): [{"@value": "urn:ngsi-ld:null"}],
            "https://example.com/speed": [property_instance(&json!(10), None)],
            "https://example.com/gone": [property_instance(&json!("urn:ngsi-ld:null"), None)],
            "https://example.com/bare": [{"@value": "urn:ngsi-ld:null"}]
        }]);

        pretty_assertions::assert_eq!(drop_deletion_sentinels(&mut payload), DroppedSentinels::Dropped);

        pretty_assertions::assert_eq!(payload[0]["@id"], json!("urn:ngsi-ld:Device:1"));
        pretty_assertions::assert_eq!(
            payload[0]["https://example.com/speed"][0]["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"],
            json!(10)
        );
        for dropped in [scope_iri(), "https://example.com/gone", "https://example.com/bare"] {
            assert!(payload[0].get(dropped).is_none(), "{dropped} is dropped: {payload}");
        }
    }

    /// A payload that asks for no deletion is left exactly as it was, so the
    /// create branch keeps using the preparation pass's own projection.
    #[test]
    fn dropping_deletion_sentinels_leaves_a_payload_without_one_unchanged() {
        let original = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            scope_iri(): [{"@value": "/Madrid"}],
            "https://example.com/speed": [property_instance(&json!(10), None)]
        }]);
        let mut payload = original.clone();

        pretty_assertions::assert_eq!(drop_deletion_sentinels(&mut payload), DroppedSentinels::Unchanged);

        pretty_assertions::assert_eq!(payload, original);
    }

    /// An attribute whose instances mix a deletion with live data is not a
    /// deletion of the member: the surviving instances are what the create
    /// branch stores, so the member stays and the patch decides per instance.
    #[test]
    fn an_attribute_mixing_a_deletion_with_data_is_not_dropped_as_a_deletion() {
        let mut payload = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "https://example.com/speed": [
                property_instance(&json!("urn:ngsi-ld:null"), Some("urn:ngsi-ld:Dataset:A")),
                property_instance(&json!(20), Some("urn:ngsi-ld:Dataset:B"))
            ]
        }]);

        pretty_assertions::assert_eq!(drop_deletion_sentinels(&mut payload), DroppedSentinels::Unchanged);

        assert!(payload[0].get("https://example.com/speed").is_some(), "{payload}");
    }

    /// Clause 5.6.2.4: "If scope is included in the Fragment and the target
    /// entity includes scope, replace the scope by the one included in the
    /// Fragment", which a hierarchical path still does.
    #[test]
    fn update_attributes_replaces_a_scope_the_target_already_carries() {
        let mut entity = json!([{"@id": "urn:ngsi-ld:Device:1", scope_iri(): [{"@value": "/Madrid"}]}]);
        let patch = json!({scope_iri(): [{"@value": "/Barcelona"}]});

        update_entity_attrs(&mut entity, &patch).expect("update succeeds");

        pretty_assertions::assert_eq!(entity[0][scope_iri()], json!([{"@value": "/Barcelona"}]));
    }

    /// Clause 5.6.2.4 closes with "otherwise ignore it", so a fragment deleting
    /// a scope the target never had leaves the entity untouched and reports the
    /// member as one this request did not update.
    #[test]
    fn update_attributes_reports_a_scope_deletion_the_target_cannot_take() {
        let mut entity = json!([{"@id": "urn:ngsi-ld:Device:1"}]);
        let patch = json!({scope_iri(): [{"@value": "urn:ngsi-ld:null"}]});

        let not_updated = update_entity_attrs(&mut entity, &patch).expect("update succeeds");

        pretty_assertions::assert_eq!(not_updated.iter().map(AttributeName::as_str).collect::<Vec<&str>>(), vec![scope_iri()]);
        assert!(entity[0].get(scope_iri()).is_none(), "no scope was invented: {entity}");
    }

    /// Clause 5.5.12 removes a target member the fragment sets to an NGSI-LD
    /// Null, and clause 4.18 makes that sentinel the deleted scope.
    #[test]
    fn merge_deletes_the_scope_the_null_sentinel_names() {
        let mut entity = json!([{"@id": "urn:ngsi-ld:Device:1", scope_iri(): [{"@value": "/Madrid"}]}]);
        let patch = json!([{scope_iri(): [{"@value": "urn:ngsi-ld:null"}]}]);

        merge_entity(&mut entity, &patch, None).expect("merge succeeds");

        assert!(entity[0].get(scope_iri()).is_none(), "the scope member is gone: {entity}");
    }

    #[test]
    fn a_non_expanded_entity_is_a_typed_error() {
        let mut entity = json!("not an entity");
        pretty_assertions::assert_eq!(
            merge_entity(&mut entity, &json!([{}]), None).expect_err("must fail"),
            ExpandedEntityError::EntityNotExpanded
        );
    }

    #[test]
    fn a_non_object_attribute_fragment_is_a_typed_error() {
        let mut entity = json!([{}]);
        pretty_assertions::assert_eq!(
            append_entity_attrs(&mut entity, &json!([]), OverwritePolicy::Overwrite).expect_err("must fail"),
            ExpandedEntityError::AttributesNotObject
        );
    }
}

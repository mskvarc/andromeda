//! Entity-wide writes: merge patch, append, and update (clauses 5.6.1 to 5.6.3).

use crate::{
    expanded::{
        dataset_id::get_instance_dataset_id,
        merge::{deep_merge_attribute, remove_instances_by_dataset},
        mutation::{error::ExpandedEntityError, selection::OverwritePolicy},
        node::{expanded_object_mut, expanded_object_ref},
        null_sentinel::instance_is_null_sentinel,
    },
    vocab::expanded::properties,
};
use serde_json::Value;
use std::slice::from_ref;

/// NGSI-LD Merge Patch (clause 5.6.1): deep-merge `patch` into `entity`, handling
/// null sentinels for attribute deletion.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::EntityNotExpanded`] or
/// [`ExpandedEntityError::PatchNotExpanded`] if either side is not an expanded
/// JSON-LD node.
pub fn merge_entity(entity: &mut Value, patch: &Value) -> Result<(), ExpandedEntityError> {
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

        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                // clone: both the key and the value are borrowed from the patch and stored owned.
                entity_obj.insert(key.clone(), value.clone());
                continue;
            }
        };

        if !patch_instances.iter().any(instance_is_null_sentinel) {
            deep_merge_attribute(entity_obj, key, patch_instances);
            continue;
        }

        for patch_instance in patch_instances {
            if instance_is_null_sentinel(patch_instance) {
                let dataset_id = get_instance_dataset_id(patch_instance);
                if dataset_id.is_none() {
                    // A null sentinel without a datasetId deletes every instance.
                    entity_obj.remove(key);
                    break;
                }
                remove_instances_by_dataset(entity_obj, key, dataset_id.as_deref());
            }
        }
    }

    Ok(())
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
pub fn append_entity_attrs(entity: &mut Value, attrs: &Value, overwrite: OverwritePolicy) -> Result<Vec<String>, ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;
    let attrs_obj = attrs.as_object().ok_or(ExpandedEntityError::AttributesNotObject)?;
    let no_overwrite = overwrite == OverwritePolicy::Preserve;

    let mut not_appended = Vec::new();

    for (key, value) in attrs_obj {
        if key.starts_with('@') {
            continue;
        }

        // Scope: merge arrays with deduplication
        if key == properties::SCOPE.as_str() {
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
        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                if no_overwrite {
                    // clone: the reported IRI is borrowed from the fragment key.
                    not_appended.push(key.clone());
                } else {
                    // clone: both the key and the value are borrowed from the fragment and stored owned.
                    entity_obj.insert(key.clone(), value.clone());
                }
                continue;
            }
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
                // clone: the reported IRI is borrowed from the fragment key.
                not_appended.push(key.clone());
            }
        } else {
            deep_merge_attribute(entity_obj, key, patch_instances);
        }
    }

    Ok(not_appended)
}

/// Update entity attributes (clause 5.6.2).
///
/// For regular attributes: if the attribute exists, update by datasetId
/// matching; if not, add it. For scope: if scope doesn't exist on the
/// entity, report as not updated.
///
/// Returns a list of attribute IRIs that were NOT updated (e.g. scope not
/// present). The caller uses this to decide between 204 and 207.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::EntityNotExpanded`] if `entity` is not an
/// expanded JSON-LD node, or [`ExpandedEntityError::AttributesNotObject`] if
/// `attrs` is not a JSON object.
pub fn update_entity_attrs(entity: &mut Value, attrs: &Value) -> Result<Vec<String>, ExpandedEntityError> {
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

        // Scope: only update if already present, otherwise report as not updated
        if key == properties::SCOPE.as_str() {
            if entity_obj.contains_key(key) {
                // clone: both the key and the value are borrowed from the fragment and stored owned.
                entity_obj.insert(key.clone(), value.clone());
            } else {
                // clone: the reported IRI is borrowed from the fragment key.
                not_updated.push(key.clone());
            }
            continue;
        }

        if !entity_obj.contains_key(key) {
            // clone: both the key and the value are borrowed from the fragment and stored owned.
            entity_obj.insert(key.clone(), value.clone());
            continue;
        }

        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                // clone: both the key and the value are borrowed from the fragment and stored owned.
                entity_obj.insert(key.clone(), value.clone());
                continue;
            }
        };

        if patch_instances.iter().any(instance_is_null_sentinel) {
            for patch_instance in patch_instances {
                if instance_is_null_sentinel(patch_instance) {
                    let dataset_id = get_instance_dataset_id(patch_instance);
                    remove_instances_by_dataset(entity_obj, key, dataset_id.as_deref());
                } else {
                    deep_merge_attribute(entity_obj, key, from_ref(patch_instance));
                }
            }
        } else {
            // Update by datasetId: replace matching instances, preserve others
            deep_merge_attribute(entity_obj, key, patch_instances);
        }
    }

    Ok(not_updated)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::expanded::mutation::{
        entity::{append_entity_attrs, merge_entity},
        error::ExpandedEntityError,
        selection::OverwritePolicy,
        speed_entity,
    };
    use serde_json::json;

    #[test]
    fn merge_entity_adds_new_attr() {
        let mut entity = json!([{"@type": ["https://example.com/T"], "https://example.com/a": [{"@value": 1}]}]);
        let patch = json!([{"https://example.com/b": [{"@value": 2}]}]);
        merge_entity(&mut entity, &patch).unwrap();
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
        pretty_assertions::assert_eq!(not_appended, vec!["https://example.com/speed".to_string()]);
    }

    #[test]
    fn appending_an_existing_attribute_with_overwrite_replaces_it() {
        let mut entity = json!([{"https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}]}]}]);
        let attrs = json!({"https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}]}]});
        let not_appended = append_entity_attrs(&mut entity, &attrs, OverwritePolicy::Overwrite).expect("append succeeds");
        pretty_assertions::assert_eq!(speed_entity::values(&entity), vec![20]);
        assert!(not_appended.is_empty(), "{not_appended:?}");
    }

    #[test]
    fn a_non_expanded_entity_is_a_typed_error() {
        let mut entity = json!("not an entity");
        pretty_assertions::assert_eq!(
            merge_entity(&mut entity, &json!([{}])).expect_err("must fail"),
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

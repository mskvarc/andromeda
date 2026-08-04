//! Single-attribute writes: partial update, delete, and replace
//! (clauses 5.6.4, 5.6.5 and 5.6.19).

use crate::{
    expanded::{
        dataset_id::get_instance_dataset_id,
        merge::{merge_json, remove_instances_by_dataset},
        mutation::{error::ExpandedEntityError, selection::DatasetSelection},
        node::expanded_object_mut,
        null_sentinel::instance_is_null_sentinel,
    },
    vocab::expanded::properties,
};
use serde_json::Value;

/// Partial update of a single attribute (clause 5.6.4): merge `fragment` into the
/// attribute identified by `attr_iri`, optionally targeting a specific
/// `dataset_id`.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::AttributeNotFound`],
/// [`ExpandedEntityError::AttributeHasNoInstances`],
/// [`ExpandedEntityError::DatasetNotFound`],
/// [`ExpandedEntityError::DefaultInstanceNotFound`] or
/// [`ExpandedEntityError::AttributeTypeMismatch`] depending on which part of
/// the lookup fails.
pub fn partial_update_attr(entity: &mut Value, attr_iri: &str, dataset_id: Option<&str>, fragment: &Value) -> Result<(), ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;

    if !entity_obj.contains_key(attr_iri) {
        return Err(ExpandedEntityError::AttributeNotFound {
            attribute: attr_iri.to_string(),
        });
    }

    // datasetId selection is controlled by query parameter (datasetId).
    // A datasetId embedded in the payload does not select the target instance.
    let effective_ds: Option<&str> = dataset_id;

    if instance_is_null_sentinel(fragment) {
        remove_instances_by_dataset(entity_obj, attr_iri, effective_ds);
        return Ok(());
    }

    // Extract the fragment's @type (if present) for type-mismatch validation
    let fragment_types: Vec<&str> = fragment
        .get("@type")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    if let Some(ds_id) = effective_ds {
        let Some(Value::Array(instances)) = entity_obj.get_mut(attr_iri) else {
            return Err(ExpandedEntityError::AttributeHasNoInstances {
                attribute: attr_iri.to_string(),
            });
        };

        let found = instances.iter_mut().find(|instance| {
            instance
                .get(properties::DATASET_ID.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@id"))
                .and_then(|id| id.as_str())
                == Some(ds_id)
        });

        match found {
            Some(instance) => {
                validate_type_match(&fragment_types, instance, attr_iri)?;
                merge_json(instance, fragment);
            }
            None => {
                return Err(ExpandedEntityError::DatasetNotFound {
                    attribute: attr_iri.to_string(),
                    dataset_id: ds_id.to_string(),
                });
            }
        }
    } else {
        // No datasetId: target the default instance (the one without datasetId)
        let Some(Value::Array(instances)) = entity_obj.get_mut(attr_iri) else {
            let Some(attr_value) = entity_obj.get_mut(attr_iri) else {
                return Err(ExpandedEntityError::AttributeNotFound {
                    attribute: attr_iri.to_string(),
                });
            };
            if !fragment_types.is_empty() {
                validate_type_match(&fragment_types, attr_value, attr_iri)?;
            }
            merge_json(attr_value, fragment);
            return Ok(());
        };

        let found = instances.iter_mut().find(|instance| get_instance_dataset_id(instance).is_none());

        match found {
            Some(instance) => {
                validate_type_match(&fragment_types, instance, attr_iri)?;
                merge_json(instance, fragment);
            }
            None => {
                return Err(ExpandedEntityError::DefaultInstanceNotFound {
                    attribute: attr_iri.to_string(),
                });
            }
        }
    }

    Ok(())
}

/// Check that the fragment's `@type` (if specified) matches the instance's `@type`.
fn validate_type_match(fragment_types: &[&str], instance: &Value, attr_iri: &str) -> Result<(), ExpandedEntityError> {
    if fragment_types.is_empty() {
        return Ok(());
    }
    let instance_types: Vec<&str> = instance
        .get("@type")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if instance_types.is_empty() {
        return Ok(());
    }
    if fragment_types.iter().any(|ft| instance_types.contains(ft)) {
        Ok(())
    } else {
        Err(ExpandedEntityError::AttributeTypeMismatch {
            attribute: attr_iri.to_string(),
            fragment_types: fragment_types.iter().map(|t| (*t).to_string()).collect(),
            instance_types: instance_types.into_iter().map(str::to_string).collect(),
        })
    }
}

/// Delete a single attribute (clause 5.6.5).
///
/// [`DatasetSelection::All`] removes the key entirely;
/// [`DatasetSelection::Single`] removes only the matching instance;
/// [`DatasetSelection::Default`] removes the instance carrying no `datasetId`.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::AttributeNotFound`] if the entity has no such
/// attribute, or [`ExpandedEntityError::DatasetNotFound`] /
/// [`ExpandedEntityError::DefaultInstanceNotFound`] if the selected instance is
/// absent.
pub fn delete_attr(entity: &mut Value, attr_iri: &str, selection: DatasetSelection<'_>) -> Result<(), ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;

    if !entity_obj.contains_key(attr_iri) {
        return Err(ExpandedEntityError::AttributeNotFound {
            attribute: attr_iri.to_string(),
        });
    }

    match selection {
        DatasetSelection::All => {
            entity_obj.remove(attr_iri);
            Ok(())
        }
        DatasetSelection::Single(dataset_id) => {
            if remove_instances_by_dataset(entity_obj, attr_iri, Some(dataset_id)) {
                Ok(())
            } else {
                Err(ExpandedEntityError::DatasetNotFound {
                    attribute: attr_iri.to_string(),
                    dataset_id: dataset_id.to_string(),
                })
            }
        }
        DatasetSelection::Default => {
            if remove_instances_by_dataset(entity_obj, attr_iri, None) {
                Ok(())
            } else {
                Err(ExpandedEntityError::DefaultInstanceNotFound {
                    attribute: attr_iri.to_string(),
                })
            }
        }
    }
}

/// Replace a single attribute (clause 5.6.19): fully replaces the attribute value.
///
/// When `dataset_id` is provided (from query parameter), it specifies which instance to replace.
/// Otherwise, the datasetId from `value` is used to match. If neither specifies a datasetId,
/// the default instance (without datasetId) is replaced.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::AttributeNotFound`] if the entity has no such
/// attribute, or [`ExpandedEntityError::DatasetNotFound`] /
/// [`ExpandedEntityError::DefaultInstanceNotFound`] if no instance matches.
pub fn replace_attr(entity: &mut Value, attr_iri: &str, dataset_id: Option<&str>, value: &Value) -> Result<(), ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;

    let existing = entity_obj.get(attr_iri).ok_or_else(|| ExpandedEntityError::AttributeNotFound {
        attribute: attr_iri.to_string(),
    })?;

    // Query param datasetId takes precedence over value's datasetId
    let target_ds: Option<String> = match dataset_id {
        Some(ds) => Some(ds.to_string()),
        None => get_instance_dataset_id(value),
    };

    let existing_instances = existing.as_array();

    if let Some(instances) = existing_instances {
        let mut new_instances: Vec<Value> = Vec::with_capacity(instances.len());
        let mut replaced = false;

        for inst in instances {
            let inst_ds = get_instance_dataset_id(inst);
            if inst_ds == target_ds {
                // clone: the replacement is borrowed and the rebuilt array owns its elements.
                new_instances.push(value.clone());
                replaced = true;
            } else {
                // clone: the surviving instance is borrowed from the array being rebuilt.
                new_instances.push(inst.clone());
            }
        }

        if !replaced {
            return match target_ds {
                Some(dataset_id) => Err(ExpandedEntityError::DatasetNotFound {
                    attribute: attr_iri.to_string(),
                    dataset_id,
                }),
                None => Err(ExpandedEntityError::DefaultInstanceNotFound {
                    attribute: attr_iri.to_string(),
                }),
            };
        }

        entity_obj.insert(attr_iri.to_string(), Value::Array(new_instances));
    } else {
        // clone: the replacement is borrowed and the stored array owns its element.
        entity_obj.insert(attr_iri.to_string(), Value::Array(vec![value.clone()]));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::expanded::mutation::{
        attribute::{delete_attr, partial_update_attr, replace_attr},
        error::ExpandedEntityError,
        selection::DatasetSelection,
        speed_entity,
    };
    use serde_json::json;

    #[test]
    fn delete_attr_removes_key() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        delete_attr(&mut entity, "https://example.com/a", DatasetSelection::All).unwrap();
        assert!(entity[0].get("https://example.com/a").is_none());
    }

    #[test]
    fn delete_attr_not_found() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        let result = delete_attr(&mut entity, "https://example.com/missing", DatasetSelection::All);
        assert!(result.is_err());
    }

    #[test]
    fn deleting_all_instances_removes_every_dataset() {
        let mut entity = speed_entity::with_three_instances();
        delete_attr(&mut entity, "https://example.com/speed", DatasetSelection::All).expect("delete succeeds");
        assert!(entity[0].get("https://example.com/speed").is_none());
    }

    #[test]
    fn deleting_a_single_instance_removes_only_that_dataset() {
        let mut entity = speed_entity::with_three_instances();
        delete_attr(&mut entity, "https://example.com/speed", DatasetSelection::Single("urn:ngsi-ld:Dataset:A")).expect("delete succeeds");
        pretty_assertions::assert_eq!(speed_entity::values(&entity), vec![20, 30]);
    }

    #[test]
    fn deleting_the_default_instance_leaves_the_datasets_alone() {
        let mut entity = speed_entity::with_three_instances();
        delete_attr(&mut entity, "https://example.com/speed", DatasetSelection::Default).expect("delete succeeds");
        pretty_assertions::assert_eq!(speed_entity::values(&entity), vec![10, 20]);
    }

    #[test]
    fn deleting_an_absent_attribute_is_a_typed_error() {
        let mut entity = json!([{"https://example.com/other": [{"@value": 1}]}]);
        let error = delete_attr(&mut entity, "https://example.com/speed", DatasetSelection::All).expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::AttributeNotFound {
                attribute: "https://example.com/speed".to_string()
            }
        );
    }

    #[test]
    fn deleting_an_absent_dataset_is_a_typed_error() {
        let mut entity = speed_entity::with_three_instances();
        let error = delete_attr(&mut entity, "https://example.com/speed", DatasetSelection::Single("urn:ngsi-ld:Dataset:Z")).expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::DatasetNotFound {
                attribute: "https://example.com/speed".to_string(),
                dataset_id: "urn:ngsi-ld:Dataset:Z".to_string()
            }
        );
    }

    #[test]
    fn a_type_mismatch_in_a_partial_update_is_a_typed_error() {
        let mut entity = json!([{
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}]
            }]
        }]);
        let fragment = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Vehicle:2"}]
        });
        let error = partial_update_attr(&mut entity, "https://example.com/speed", None, &fragment).expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::AttributeTypeMismatch {
                attribute: "https://example.com/speed".to_string(),
                fragment_types: vec!["https://uri.etsi.org/ngsi-ld/Relationship".to_string()],
                instance_types: vec!["https://uri.etsi.org/ngsi-ld/Property".to_string()],
            }
        );
    }

    #[test]
    fn partially_updating_an_absent_attribute_is_a_typed_error() {
        let mut entity = json!([{"https://example.com/other": [{"@value": 1}]}]);
        let error = partial_update_attr(&mut entity, "https://example.com/speed", None, &json!({})).expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::AttributeNotFound {
                attribute: "https://example.com/speed".to_string()
            }
        );
    }

    #[test]
    fn replacing_an_absent_dataset_is_a_typed_error() {
        let mut entity = speed_entity::with_three_instances();
        let replacement = json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 99}]});
        let error = replace_attr(&mut entity, "https://example.com/speed", Some("urn:ngsi-ld:Dataset:Z"), &replacement).expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::DatasetNotFound {
                attribute: "https://example.com/speed".to_string(),
                dataset_id: "urn:ngsi-ld:Dataset:Z".to_string()
            }
        );
    }
}

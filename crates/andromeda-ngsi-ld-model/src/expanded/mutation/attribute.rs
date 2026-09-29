//! Single-attribute writes: partial update, delete, and replace
//! (clauses 5.6.4, 5.6.5 and 5.6.19).

use crate::expanded::{
    dataset_id::{get_instance_dataset_id, position_of_dataset},
    merge::remove_instances_by_dataset,
    mutation::{
        error::ExpandedEntityError,
        selection::{DatasetSelection, addressed_dataset},
    },
    node::expanded_object_mut,
    null_sentinel::instance_is_null_sentinel,
    partial_update_patch::patch_attribute_instance,
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier};
use iri_rs::IriBuf;
use serde_json::Value;

/// Partial update of a single attribute (clause 5.6.4): patch `fragment` into
/// the attribute identified by `attr_iri`, following clause 5.5.8.
///
/// The instance the patch lands on is the one clause 5.6.4.4 names: "as an
/// instance with the specified datasetId if present", otherwise the default
/// instance, resolved by [`addressed_dataset`].
///
/// A `fragment` that is the NGSI-LD Null of clause 4.5.0 deletes the selected
/// instance, which clause 5.5.8 hands to clause 5.6.5: the attribute itself is
/// gone once its last instance is.
///
/// # Errors
///
/// Returns [`ExpandedEntityError::AttributeNotFound`],
/// [`ExpandedEntityError::AttributeHasNoInstances`],
/// [`ExpandedEntityError::DatasetNotFound`],
/// [`ExpandedEntityError::DefaultInstanceNotFound`] or
/// [`ExpandedEntityError::AttributeTypeMismatch`] depending on which part of
/// the lookup fails, and [`ExpandedEntityError::InvalidDatasetId`] when the
/// fragment or a stored instance carries a `datasetId` that is not an
/// identifier -- never the default instance in its place (clause 4.5.5).
pub fn partial_update_attr(entity: &mut Value, attr_iri: &IriBuf, dataset_id: Option<&Identifier>, fragment: &Value) -> Result<(), ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;

    if !entity_obj.contains_key(attr_iri.as_str()) {
        return Err(ExpandedEntityError::AttributeNotFound { attribute: attr_iri.clone() });
    }

    let addressed = addressed_dataset(dataset_id, fragment)?;
    let effective_ds = addressed.as_ref();

    if instance_is_null_sentinel(fragment) {
        return delete_selected_instance(entity_obj, attr_iri, effective_ds);
    }

    let fragment_types = attribute_types(fragment)?;
    let Some(Value::Array(instances)) = entity_obj.get_mut(attr_iri.as_str()) else {
        return patch_unwrapped_attribute(entity_obj, attr_iri, effective_ds, &fragment_types, fragment);
    };

    let selected = position_of_dataset(instances, effective_ds)?.and_then(|position| instances.get_mut(position));

    let Some(instance) = selected else {
        return Err(missing_instance(attr_iri, effective_ds));
    };
    validate_type_match(&fragment_types, instance, attr_iri)?;
    patch_attribute_instance(instance, fragment);
    Ok(())
}

/// Delete the instance an NGSI-LD Null fragment selected (clauses 5.5.8 and
/// 5.6.5).
fn delete_selected_instance(
    entity_obj: &mut serde_json::Map<String, Value>,
    attr_iri: &IriBuf,
    effective_ds: Option<&Identifier>,
) -> Result<(), ExpandedEntityError> {
    if remove_instances_by_dataset(entity_obj, attr_iri.as_str(), effective_ds)? {
        Ok(())
    } else {
        Err(missing_instance(attr_iri, effective_ds))
    }
}

/// Patch an attribute member the store does not hold as an instance array.
///
/// Clause 4.5.1 wraps multiple instances in an array and leaves a single one
/// bare, so a payload written before expansion normalized it can reach the
/// patch as one node.
fn patch_unwrapped_attribute(
    entity_obj: &mut serde_json::Map<String, Value>,
    attr_iri: &IriBuf,
    effective_ds: Option<&Identifier>,
    fragment_types: &[AttributeType],
    fragment: &Value,
) -> Result<(), ExpandedEntityError> {
    let Some(attr_value) = entity_obj.get_mut(attr_iri.as_str()) else {
        return Err(ExpandedEntityError::AttributeNotFound { attribute: attr_iri.clone() });
    };
    if effective_ds.is_some() && get_instance_dataset_id(attr_value)?.as_ref() != effective_ds {
        return Err(missing_instance(attr_iri, effective_ds));
    }
    if !fragment_types.is_empty() {
        validate_type_match(fragment_types, attr_value, attr_iri)?;
    }
    patch_attribute_instance(attr_value, fragment);
    Ok(())
}

/// The clause 5.6.4.4 `ResourceNotFound` for the instance that was selected but
/// is not there: the one carrying `effective_ds`, or the default instance.
fn missing_instance(attr_iri: &IriBuf, effective_ds: Option<&Identifier>) -> ExpandedEntityError {
    match effective_ds {
        Some(dataset_id) => ExpandedEntityError::DatasetNotFound {
            attribute: attr_iri.clone(),
            dataset_id: Box::new(dataset_id.clone()),
        },
        None => ExpandedEntityError::DefaultInstanceNotFound { attribute: attr_iri.clone() },
    }
}

/// Check that the fragment's `@type` (if specified) matches the instance's `@type`.
fn validate_type_match(fragment_types: &[AttributeType], instance: &Value, attr_iri: &IriBuf) -> Result<(), ExpandedEntityError> {
    if fragment_types.is_empty() {
        return Ok(());
    }
    let instance_types = attribute_types(instance)?;
    if instance_types.is_empty() {
        return Ok(());
    }
    if fragment_types.iter().any(|ft| instance_types.contains(ft)) {
        Ok(())
    } else {
        Err(ExpandedEntityError::AttributeTypeMismatch {
            attribute: attr_iri.clone(),
            fragment_types: fragment_types.to_vec(),
            instance_types,
        })
    }
}

/// Parses every JSON-LD type assigned to an expanded attribute.
fn attribute_types(value: &Value) -> Result<Vec<AttributeType>, ExpandedEntityError> {
    value
        .get("@type")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| {
            value
                .as_str()
                .and_then(AttributeType::from_expanded_iri)
                .ok_or(ExpandedEntityError::InvalidAttributeType)
        })
        .collect()
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
/// absent, and [`ExpandedEntityError::InvalidDatasetId`] when an instance of
/// the attribute carries a `datasetId` that is not an identifier.
pub fn delete_attr(entity: &mut Value, attr_iri: &IriBuf, selection: DatasetSelection<'_>) -> Result<(), ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;

    if !entity_obj.contains_key(attr_iri.as_str()) {
        return Err(ExpandedEntityError::AttributeNotFound { attribute: attr_iri.clone() });
    }

    match selection {
        DatasetSelection::All => {
            entity_obj.remove(attr_iri.as_str());
            Ok(())
        }
        DatasetSelection::Single(dataset_id) => {
            // Instances record the datasetId as its raw `@id`; the target URI is
            // rendered once to that form to select by string equality.
            if remove_instances_by_dataset(entity_obj, attr_iri.as_str(), Some(dataset_id))? {
                Ok(())
            } else {
                Err(ExpandedEntityError::DatasetNotFound {
                    attribute: attr_iri.clone(),
                    dataset_id: Box::new(dataset_id.clone()),
                })
            }
        }
        DatasetSelection::Default => {
            if remove_instances_by_dataset(entity_obj, attr_iri.as_str(), None)? {
                Ok(())
            } else {
                Err(ExpandedEntityError::DefaultInstanceNotFound { attribute: attr_iri.clone() })
            }
        }
    }
}

/// Replace a single attribute (clause 5.6.19): fully replaces the attribute value.
///
/// The instance replaced is the one [`addressed_dataset`] resolves from the
/// `dataset_id` URL parameter and the `datasetId` of `value`; when neither
/// names one, it is the default instance (clause 4.5.5).
///
/// # Errors
///
/// Returns [`ExpandedEntityError::AttributeNotFound`] if the entity has no such
/// attribute, or [`ExpandedEntityError::DatasetNotFound`] /
/// [`ExpandedEntityError::DefaultInstanceNotFound`] if no instance matches,
/// and [`ExpandedEntityError::InvalidDatasetId`] when the value or a stored
/// instance carries a `datasetId` that is not an identifier.
pub fn replace_attr(entity: &mut Value, attr_iri: &IriBuf, dataset_id: Option<&Identifier>, value: &Value) -> Result<(), ExpandedEntityError> {
    let entity_obj = expanded_object_mut(entity).ok_or(ExpandedEntityError::EntityNotExpanded)?;

    let existing = entity_obj
        .get(attr_iri.as_str())
        .ok_or_else(|| ExpandedEntityError::AttributeNotFound { attribute: attr_iri.clone() })?;

    let target_ds = addressed_dataset(dataset_id, value)?;

    let existing_instances = existing.as_array();

    if let Some(instances) = existing_instances {
        let mut new_instances: Vec<Value> = Vec::with_capacity(instances.len());
        let mut replaced = false;

        for inst in instances {
            let inst_ds = get_instance_dataset_id(inst)?;
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
            // The datasetId that matched nothing, for the ProblemDetail: the
            // query-parameter URI if one was given, else the value's own.
            return match target_ds {
                Some(dataset_id) => Err(ExpandedEntityError::DatasetNotFound {
                    attribute: attr_iri.clone(),
                    dataset_id: Box::new(dataset_id),
                }),
                None => Err(ExpandedEntityError::DefaultInstanceNotFound { attribute: attr_iri.clone() }),
            };
        }

        entity_obj.insert(attr_iri.as_str().to_string(), Value::Array(new_instances));
    } else {
        // clone: the replacement is borrowed and the stored array owns its element.
        entity_obj.insert(attr_iri.as_str().to_string(), Value::Array(vec![value.clone()]));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::expanded::mutation::{
        attribute::{delete_attr, partial_update_attr, replace_attr},
        error::ExpandedEntityError,
        selection::DatasetSelection,
        speed_entity,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier, identifier_length::IDENTIFIER_BYTE_LIMIT};
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::str::FromStr;

    /// An expanded attribute IRI, as the mutation functions take it.
    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("a valid IRI")
    }

    /// A `datasetId`, as the mutation functions take it.
    fn dataset_uri(value: &str) -> Identifier {
        Identifier::from_str(value).expect("a valid dataset URI")
    }

    #[test]
    fn delete_attr_removes_key() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        delete_attr(&mut entity, &iri("https://example.com/a"), DatasetSelection::All).unwrap();
        assert!(entity[0].get("https://example.com/a").is_none());
    }

    #[test]
    fn delete_attr_not_found() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        let result = delete_attr(&mut entity, &iri("https://example.com/missing"), DatasetSelection::All);
        assert!(result.is_err());
    }

    #[test]
    fn deleting_all_instances_removes_every_dataset() {
        let mut entity = speed_entity::with_three_instances();
        delete_attr(&mut entity, &iri("https://example.com/speed"), DatasetSelection::All).expect("delete succeeds");
        assert!(entity[0].get("https://example.com/speed").is_none());
    }

    #[test]
    fn deleting_a_single_instance_removes_only_that_dataset() {
        let mut entity = speed_entity::with_three_instances();
        delete_attr(
            &mut entity,
            &iri("https://example.com/speed"),
            DatasetSelection::Single(&dataset_uri("urn:ngsi-ld:Dataset:A")),
        )
        .expect("delete succeeds");
        pretty_assertions::assert_eq!(speed_entity::values(&entity), vec![20, 30]);
    }

    #[test]
    fn deleting_the_default_instance_leaves_the_datasets_alone() {
        let mut entity = speed_entity::with_three_instances();
        delete_attr(&mut entity, &iri("https://example.com/speed"), DatasetSelection::Default).expect("delete succeeds");
        pretty_assertions::assert_eq!(speed_entity::values(&entity), vec![10, 20]);
    }

    #[test]
    fn deleting_an_absent_attribute_is_a_typed_error() {
        let mut entity = json!([{"https://example.com/other": [{"@value": 1}]}]);
        let error = delete_attr(&mut entity, &iri("https://example.com/speed"), DatasetSelection::All).expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::AttributeNotFound {
                attribute: iri("https://example.com/speed")
            }
        );
    }

    #[test]
    fn deleting_an_absent_dataset_is_a_typed_error() {
        let mut entity = speed_entity::with_three_instances();
        let error = delete_attr(
            &mut entity,
            &iri("https://example.com/speed"),
            DatasetSelection::Single(&dataset_uri("urn:ngsi-ld:Dataset:Z")),
        )
        .expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::DatasetNotFound {
                attribute: iri("https://example.com/speed"),
                dataset_id: Box::new(dataset_uri("urn:ngsi-ld:Dataset:Z"))
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
        let error = partial_update_attr(&mut entity, &iri("https://example.com/speed"), None, &fragment).expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::AttributeTypeMismatch {
                attribute: iri("https://example.com/speed"),
                fragment_types: vec![AttributeType::Relationship],
                instance_types: vec![AttributeType::Property],
            }
        );
    }

    #[test]
    fn partially_updating_an_absent_attribute_is_a_typed_error() {
        let mut entity = json!([{"https://example.com/other": [{"@value": 1}]}]);
        let error = partial_update_attr(&mut entity, &iri("https://example.com/speed"), None, &json!({})).expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::AttributeNotFound {
                attribute: iri("https://example.com/speed")
            }
        );
    }

    #[test]
    fn replacing_an_absent_dataset_is_a_typed_error() {
        let mut entity = speed_entity::with_three_instances();
        let replacement = json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 99}]});
        let error = replace_attr(
            &mut entity,
            &iri("https://example.com/speed"),
            Some(&dataset_uri("urn:ngsi-ld:Dataset:Z")),
            &replacement,
        )
        .expect_err("must fail");
        pretty_assertions::assert_eq!(
            error,
            ExpandedEntityError::DatasetNotFound {
                attribute: iri("https://example.com/speed"),
                dataset_id: Box::new(dataset_uri("urn:ngsi-ld:Dataset:Z"))
            }
        );
    }

    /// Clause 4.5.5: a fragment whose `datasetId` cannot be read names no
    /// instance the broker can identify, so the write is refused rather than
    /// landing on the default instance.
    #[test]
    fn a_partial_update_with_an_unreadable_dataset_id_never_touches_the_default_instance() {
        let mut entity = speed_entity::with_three_instances();
        let before = entity.clone(); // clone: the unchanged original is compared after the refused write
        let long_dataset = format!("urn:ngsi-ld:Dataset:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let fragment = json!({
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 99}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": long_dataset}]
        });

        let result = partial_update_attr(&mut entity, &iri("https://example.com/speed"), None, &fragment);

        assert!(matches!(result, Err(ExpandedEntityError::InvalidDatasetId(_))), "{result:?}");
        pretty_assertions::assert_eq!(entity, before);
    }
}

//! Narrowing an entity's attribute instances by `datasetId` (clause 4.5.5).

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;

/// Retain only the attribute instances whose `datasetId` the caller named.
///
/// An attribute left with no instance is removed entirely: clause 4.5.5 makes
/// `datasetId` an instance selector, and an attribute whose every instance was
/// filtered out is not part of the answer. Clause 5.7.1.4 applies this to every
/// Attribute of the entity, so `location`, `observationSpace` and
/// `operationSpace` are narrowed like any other.
pub(super) fn filter_entity_by_dataset_id(entity: &mut Entity, dataset_ids: &[DatasetSelector]) {
    let selected = |dataset_id: Option<&Identifier>| dataset_ids.iter().any(|selector| selector.selects(dataset_id));
    entity.retain_attribute_members(|_name, member| match member {
        AttributeValue::Single(attr) => selected(attr.as_ref().dataset_id()),
        AttributeValue::Multiple(list) => {
            list.retain(|attr| selected(attr.dataset_id()));
            match list.len() {
                0 => false,
                1 => {
                    *member = AttributeValue::Single(Box::new(list.remove(0)));
                    true
                }
                _ => true,
            }
        }
        // The NGSI-LD Null in place of a whole member names no dataset.
        AttributeValue::NgsiLdNull => true,
    });
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::query::dataset_filter::filter_entity_by_dataset_id;
    use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use serde_json::{Value, json};

    fn selectors(raw: &[&str]) -> Vec<DatasetSelector> {
        raw.iter().map(|entry| DatasetSelector::parse(entry).expect("valid selector")).collect()
    }

    fn point(x: f64) -> Value {
        json!({"type": "Point", "coordinates": [x, 2.0]})
    }

    fn entity(members: Value) -> Entity {
        let mut document = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"});
        if let (Some(document), Value::Object(members)) = (document.as_object_mut(), members) {
            document.extend(members);
        }
        serde_json::from_value(document).expect("a valid entity")
    }

    fn dataset_of(member: Option<&AttributeValue>) -> Vec<Option<String>> {
        match member {
            Some(AttributeValue::Single(attr)) => vec![attr.as_ref().dataset_id().map(ToString::to_string)],
            Some(AttributeValue::Multiple(list)) => list.iter().map(|attr| attr.dataset_id().map(ToString::to_string)).collect(),
            Some(AttributeValue::NgsiLdNull) | None => Vec::new(),
        }
    }

    /// Clause 4.5.5: "In case that no Attribute instances match the provided
    /// datasetIds, then the Attribute shall not be returned with the Entity".
    #[test]
    fn a_location_no_instance_of_which_carries_a_requested_dataset_id_is_left_out() {
        let mut vehicle = entity(json!({
            "speed": {"type": "Property", "value": 10},
            "position": {"type": "GeoProperty", "value": point(3.0)},
            "location": {"type": "GeoProperty", "value": point(1.0)}
        }));

        filter_entity_by_dataset_id(&mut vehicle, &selectors(&["urn:ngsi-ld:Dataset:zz"]));

        assert!(vehicle.location.is_none());
        assert!(vehicle.attrs.is_empty());
    }

    #[test]
    fn an_observation_space_and_an_operation_space_carrying_no_requested_dataset_id_are_left_out() {
        let mut vehicle = entity(json!({
            "observationSpace": {"type": "GeoProperty", "value": point(1.0)},
            "operationSpace": {"type": "GeoProperty", "value": point(2.0)}
        }));

        filter_entity_by_dataset_id(&mut vehicle, &selectors(&["urn:ngsi-ld:Dataset:zz"]));

        assert!(vehicle.observation_space.is_none());
        assert!(vehicle.operation_space.is_none());
    }

    #[test]
    fn the_location_instance_carrying_the_requested_dataset_id_is_kept_alone() {
        let mut vehicle = entity(json!({
            "location": [
                {"type": "GeoProperty", "value": point(1.0)},
                {"type": "GeoProperty", "value": point(2.0), "datasetId": "urn:ngsi-ld:Dataset:gps"}
            ]
        }));

        filter_entity_by_dataset_id(&mut vehicle, &selectors(&["urn:ngsi-ld:Dataset:gps"]));

        assert!(matches!(vehicle.location, Some(AttributeValue::Single(_))));
        pretty_assertions::assert_eq!(dataset_of(vehicle.location.as_ref()), vec![Some("urn:ngsi-ld:Dataset:gps".to_owned())]);
    }

    #[test]
    fn the_none_token_keeps_the_default_location_instance() {
        let mut vehicle = entity(json!({
            "location": [
                {"type": "GeoProperty", "value": point(1.0)},
                {"type": "GeoProperty", "value": point(2.0), "datasetId": "urn:ngsi-ld:Dataset:gps"}
            ]
        }));

        filter_entity_by_dataset_id(&mut vehicle, &selectors(&["@none"]));

        pretty_assertions::assert_eq!(dataset_of(vehicle.location.as_ref()), vec![None]);
    }

    #[test]
    fn instances_of_an_attribute_matching_several_requested_dataset_ids_stay_an_array() {
        let mut vehicle = entity(json!({
            "speed": [
                {"type": "Property", "value": 1},
                {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:a"},
                {"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:b"}
            ]
        }));

        filter_entity_by_dataset_id(&mut vehicle, &selectors(&["@none", "urn:ngsi-ld:Dataset:b"]));

        pretty_assertions::assert_eq!(dataset_of(vehicle.attrs.get("speed")), vec![None, Some("urn:ngsi-ld:Dataset:b".to_owned())]);
    }
}

//! A partial attribute update changes the members it names of an attribute, the instance of a dataset it names, or the
//! targets of a Relationship, and keeps the rest (clause 5.6.4).

use crate::{
    entity_operations::{delete_entities, ld_json_body_client, partial_update, partial_update_document, retrieve_entity_json},
    vehicle_entity_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS, provided_by_john_doe, speedometer_speed_56_patch},
    vehicle_fixtures::create_vehicle_brand_parked_speed_sources,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{Attribute, AttributeName, Change, Lossless, NgsiLdMediaType, Relationship};
use serde_json::{Value, json};

/// The `speed` the setup of the cases loads: 55 from a Speedometer in its dataset, and 54.5 from a GPS in its dataset.
fn speed_as_loaded(namespace: &Namespace) -> Value {
    json!([
        {
            "type": "Property",
            "value": 55,
            "source": {"type": "Property", "value": "Speedometer"},
            "datasetId": namespace.urn_text("Property", "speedometer"),
        },
        {
            "type": "Property",
            "value": 54.5,
            "source": {"type": "Property", "value": "GPS"},
            "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
        },
    ])
}

/// Pins: Check that one can perform a partial update on an entity attribute
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_01_01_check_that_one_can_partially_update_an_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let is_parked: AttributeName = "isParked".parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update_document(&client, &id, &is_parked, &provided_by_john_doe(&namespace)?).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "brandName": {"type": "Property", "value": "Mercedes"},
            "isParked": {
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                "observedAt": "2017-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "JohnDoe")},
            },
            "speed": speed_as_loaded(&namespace),
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can perform a partial update on an entity attribute
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_01_02_check_that_one_can_partially_update_an_attribute_by_specifying_the_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(&client, &id, &speed, Change::from(speedometer_speed_56_patch(&namespace)?), Lossless::Concise).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "brandName": {"type": "Property", "value": "Mercedes"},
            "isParked": {
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                "observedAt": "2017-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
            },
            "speed": [
                {
                    "type": "Property",
                    "value": 56,
                    "source": {"type": "Property", "value": "Speedometer"},
                    "datasetId": namespace.urn_text("Property", "speedometer"),
                },
                {
                    "type": "Property",
                    "value": 54.5,
                    "source": {"type": "Property", "value": "GPS"},
                    "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
                },
            ],
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can perform a partial update on an entity attribute
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_01_03_check_that_one_can_partially_update_a_relationship_to_multiple_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let is_parked: AttributeName = "isParked".parse()?;
    let destinations = Relationship::builder(vec![
        namespace.urn("OffStreetParking", "Downtown1")?,
        namespace.urn("OffStreetParking", "Downtown2")?,
    ])
    .build()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(
        &client,
        &id,
        &is_parked,
        Change::from(Attribute::from(destinations).into_patch()),
        Lossless::Concise,
    )
    .await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "brandName": {"type": "Property", "value": "Mercedes"},
            "isParked": {
                "type": "Relationship",
                "object": [
                    namespace.urn_text("OffStreetParking", "Downtown1"),
                    namespace.urn_text("OffStreetParking", "Downtown2"),
                ],
                "observedAt": "2017-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
            },
            "speed": speed_as_loaded(&namespace),
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

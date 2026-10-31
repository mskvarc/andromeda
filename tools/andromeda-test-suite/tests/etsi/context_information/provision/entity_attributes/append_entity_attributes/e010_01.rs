//! Attributes are appended to an entity, a new attribute beside those it holds, and a `speed` instance with a
//! `datasetId` the entity does not hold yet (clause 5.6.3).

use crate::{
    entity_operations::{append_attributes, delete_entities, ld_json_body_client, retrieve_entity_json},
    vehicle_entity_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS, speed_and_added_attribute},
    vehicle_fixtures::create_vehicle_brand_parked_speed_sources,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{EntityFragment, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: Check that one can append entity attributes
/// Clause: 5.6.3
#[gtest]
#[tokio::test]
async fn e010_01_01_append_entity_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder().property("attribute_to_be_added", 23).build()?;
    append_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment, Overwrite::Replace).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &id).await?;
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
            ],
            "attribute_to_be_added": {"type": "Property", "value": 23},
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can append entity attributes
/// Clause: 5.6.3
#[gtest]
#[tokio::test]
async fn e010_01_02_append_entity_attributes_with_different_datasetid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = speed_and_added_attribute(&namespace, "speedometerA4567-speed2")?;
    append_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment, Overwrite::Replace).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &id).await?;
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
                {
                    "type": "Property",
                    "value": 56,
                    "source": {"type": "Property", "value": "Speedometer"},
                    "datasetId": namespace.urn_text("Property", "speedometerA4567-speed2"),
                },
            ],
            "attribute_to_be_added": {"type": "Property", "value": 23},
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

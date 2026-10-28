//! ETSI `055_02`: a multi-instance entity attribute is replaced, and the `createdAt` of its first instance keeps the value
//! it had (clauses 5.6.19 and 6.7.3.3).

use crate::{
    suite_keywords::{create_vehicle_parked_speed_sources, delete_entities, retrieve_entity_sys_attrs_json},
    suite_payloads::{VEHICLE_PARKED_SPEED_SOURCE_TERMS, gps_speed_dataset},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Input, Property},
};
use serde_json::json;

/// The Speedometer `speed` of 60 that replaces the default instance of the replace cases.
fn speedometer_60() -> Result<Attribute<Input>> {
    Ok(Attribute::from(
        Property::builder(60).property("source", Property::builder("Speedometer")).build()?,
    ))
}

/// The GPS `speed` of 70 observed at 2017-07-29T11:00:04Z, in the GPS dataset of the replace cases.
fn gps_speed_70(namespace: &Namespace) -> Result<Attribute<Input>> {
    Ok(Attribute::from(
        Property::builder(70)
            .property("source", Property::builder("GPS"))
            .observed_at("2017-07-29T11:00:04Z".parse()?)
            .dataset_id(gps_speed_dataset(namespace)?)
            .build()?,
    ))
}

/// The `isParked` the replace cases keep: the Downtown1 relationship observed at 2017-07-29T12:00:04Z, provided by Bob.
fn retained_is_parked(namespace: &Namespace) -> serde_json::Value {
    json!({
        "type": "Relationship",
        "object": namespace.urn_text("OffStreetParking", "Downtown1"),
        "observedAt": "2017-07-29T12:00:04Z",
        "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
    })
}

/// Pins: Check that one can replace an existing multi-instance entity attribute and that its createdAt Temporal
/// Property remains unchanged
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_02_01_replace_entity_attribute_giving_no_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let before = retrieve_entity_sys_attrs_json(&client.with_accept(Accept::any()), &id).await?;
    let created_at = before.pointer("/speed/0/createdAt");
    verify_that!(created_at, some(anything()))?;

    let replaced = client
        .with_responses()
        .entities()
        .replace_attribute(&id, &speed, &speedometer_60()?)
        .send()
        .await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))?;
    expect_that!(replaced?.body.len(), eq(0));

    let after = retrieve_entity_sys_attrs_json(&client.with_accept(Accept::any()), &id).await?;
    expect_that!(
        &after,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "scope": "/Madrid/Centro",
            "isParked": retained_is_parked(&namespace),
            "speed": [
                {"type": "Property", "value": 60, "source": {"type": "Property", "value": "Speedometer"}},
                {
                    "type": "Property",
                    "value": 54.5,
                    "source": {"type": "Property", "value": "GPS"},
                    "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
                },
            ],
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("createdAt")
        .hiding_everywhere("modifiedAt")
    );
    expect_that!(after.pointer("/speed/0/createdAt"), eq(created_at));

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can replace an existing multi-instance entity attribute and that its createdAt Temporal
/// Property remains unchanged
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_02_02_replace_entity_attribute_giving_a_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let before = retrieve_entity_sys_attrs_json(&client.with_accept(Accept::any()), &id).await?;
    let created_at = before.pointer("/speed/0/createdAt");
    verify_that!(created_at, some(anything()))?;

    let replaced = client
        .with_responses()
        .entities()
        .replace_attribute(&id, &speed, &gps_speed_70(&namespace)?)
        .send()
        .await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))?;
    expect_that!(replaced?.body.len(), eq(0));

    let after = retrieve_entity_sys_attrs_json(&client.with_accept(Accept::any()), &id).await?;
    expect_that!(
        &after,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "scope": "/Madrid/Centro",
            "isParked": retained_is_parked(&namespace),
            "speed": [
                {"type": "Property", "value": 55, "source": {"type": "Property", "value": "Speedometer"}},
                {
                    "type": "Property",
                    "value": 70,
                    "source": {"type": "Property", "value": "GPS"},
                    "observedAt": "2017-07-29T11:00:04Z",
                    "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
                },
            ],
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("createdAt")
        .hiding_everywhere("modifiedAt")
    );
    expect_that!(after.pointer("/speed/0/createdAt"), eq(created_at));

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

//! An entity attribute is replaced, and its `createdAt` keeps the value it had (clauses 5.6.19 and 6.7.3.3).

use crate::{
    entity_operations::{delete_entities, retrieve_entity_sys_attrs_json},
    vehicle_entity_payloads::{VEHICLE_PARKED_SPEED_SOURCE_TERMS, is_parked_downtown2},
    vehicle_fixtures::create_vehicle_parked_speed_sources,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Property},
};
use serde_json::json;

/// The `speed` the replace cases leave as it was: the Speedometer instance of 55 and the GPS instance of 54.5 in its
/// dataset.
fn retained_speed(namespace: &Namespace) -> serde_json::Value {
    json!([
        {"type": "Property", "value": 55, "source": {"type": "Property", "value": "Speedometer"}},
        {
            "type": "Property",
            "value": 54.5,
            "source": {"type": "Property", "value": "GPS"},
            "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
        },
    ])
}

/// Pins: Check that one can replace an existing entity attribute and that its createdAt Temporal Property remains
/// unchanged
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_01_01_replace_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let is_parked: AttributeName = "isParked".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let before = retrieve_entity_sys_attrs_json(&client.with_accept(Accept::any()), &id).await?;
    let created_at = before.pointer("/isParked/createdAt");
    verify_that!(created_at, some(anything()))?;

    let replacement = Attribute::from(is_parked_downtown2(&namespace)?);
    let replaced = client.with_responses().entities().replace_attribute(&id, &is_parked, &replacement).send().await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))?;
    expect_that!(replaced?.body.len(), eq(0));

    let after = retrieve_entity_sys_attrs_json(&client.with_accept(Accept::any()), &id).await?;
    expect_that!(
        &after,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "scope": "/Madrid/Centro",
            "isParked": {
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", "Downtown2"),
                "observedAt": "2017-07-29T15:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
            },
            "speed": retained_speed(&namespace),
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("createdAt")
        .hiding_everywhere("modifiedAt")
    );
    expect_that!(after.pointer("/isParked/createdAt"), eq(created_at));

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can replace an existing entity attribute and that its createdAt Temporal Property remains
/// unchanged
/// Clause: 5.6.19, 6.7.3.3
#[gtest]
#[tokio::test]
async fn e055_01_02_replace_entity_attribute_giving_a_new_attribute_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_parked_speed_sources(&broker, &namespace, &VEHICLE_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let is_parked: AttributeName = "isParked".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let before = retrieve_entity_sys_attrs_json(&client.with_accept(Accept::any()), &id).await?;
    let created_at = before.pointer("/isParked/createdAt");
    verify_that!(created_at, some(anything()))?;

    let replacement = Attribute::from(Property::builder(true).build()?);
    let replaced = client.with_responses().entities().replace_attribute(&id, &is_parked, &replacement).send().await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))?;
    expect_that!(replaced?.body.len(), eq(0));

    let after = retrieve_entity_sys_attrs_json(&client.with_accept(Accept::any()), &id).await?;
    expect_that!(
        &after,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "scope": "/Madrid/Centro",
            "isParked": {"type": "Property", "value": true},
            "speed": retained_speed(&namespace),
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("createdAt")
        .hiding_everywhere("modifiedAt")
    );
    expect_that!(after.pointer("/isParked/createdAt"), eq(created_at));

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

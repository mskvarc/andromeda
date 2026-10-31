//! The temporal evolutions of the Vehicles and the Bus of a context are queried with `pick` and `omit`, over their
//! attributes and over their core members (clauses 5.7.4 and 4.2.1). Each answer is compared as one document: the
//! `instanceId` a broker adds is allowed below the root, and the root's members are counted exactly.

use crate::vehicle_fixtures::{create_vehicle_pair_and_bus, delete_vehicle_pair_and_bus};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    query::{
        projection::{self, EntityMember},
        temporal_query,
        type_selection,
    },
};
use serde_json::json;

/// Pins: Check that one can query temporal entities with pick on attributes
/// Clause: 5.7.4, 4.2.1
#[gtest]
#[tokio::test]
async fn e021_21_01_query_with_pick_on_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_projected(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Bus").or("Vehicle"))
        .pick(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "speed": [
                    {"type": "Property", "value": 45, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 25, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:07:00Z"},
                ],
            },
            {
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
                ],
            },
            {
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;
    Ok(())
}

/// Pins: Check that one can query temporal entities with omit on attributes
/// Clause: 5.7.4, 4.2.1
#[gtest]
#[tokio::test]
async fn e021_21_02_query_with_omit_on_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_projected(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Bus").or("Vehicle"))
        .omit(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Bus", "021-21-C"),
                "type": "Bus",
                "fuelLevel": [
                    {"type": "Property", "value": 210, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 145, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 124, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-21-A"),
                "type": "Vehicle",
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-21-B"),
                "type": "Vehicle",
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;
    Ok(())
}

/// Pins: Check that one can query temporal entities with pick on core members
/// Clause: 5.7.4, 4.2.1
#[gtest]
#[tokio::test]
async fn e021_21_03_query_with_pick_on_non_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_projected(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Bus").or("Vehicle"))
        .pick(projection::member(EntityMember::Id).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Bus", "021-21-C")},
            {"id": namespace.urn_text("Vehicle", "021-21-A")},
            {"id": namespace.urn_text("Vehicle", "021-21-B")},
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;
    Ok(())
}

/// Pins: Check that one can query temporal entities with omit on core members
/// Clause: 5.7.4, 4.2.1
#[gtest]
#[tokio::test]
async fn e021_21_04_query_with_omit_on_non_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_projected(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Bus").or("Vehicle"))
        .omit(projection::member(EntityMember::Type).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Bus", "021-21-C"),
                "speed": [
                    {"type": "Property", "value": 45, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 25, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 210, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 145, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 124, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-21-A"),
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-21-B"),
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;
    Ok(())
}

/// Pins: Check that one can query temporal entities with pick on attributes and core members
/// Clause: 5.7.4, 4.2.1
#[gtest]
#[tokio::test]
async fn e021_21_05_query_with_pick_on_attributes_and_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_projected(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Bus").or("Vehicle"))
        .pick(projection::member(EntityMember::Id).attribute("speed").build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Bus", "021-21-C"),
                "speed": [
                    {"type": "Property", "value": 45, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 25, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-21-A"),
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-21-B"),
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;
    Ok(())
}

/// Pins: Check that one can query temporal entities with omit on attributes and core members
/// Clause: 5.7.4, 4.2.1
#[gtest]
#[tokio::test]
async fn e021_21_06_query_with_omit_on_attributes_and_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_projected(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::entity_type("Bus").or("Vehicle"))
        .omit(projection::member(EntityMember::Type).attribute("speed").build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Bus", "021-21-C"),
                "fuelLevel": [
                    {"type": "Property", "value": 210, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 145, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 124, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-21-A"),
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-21-B"),
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-21", "021-21-C").await?;
    Ok(())
}

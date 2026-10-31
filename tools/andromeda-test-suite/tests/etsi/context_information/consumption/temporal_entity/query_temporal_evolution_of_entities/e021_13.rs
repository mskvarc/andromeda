//! The temporal evolutions of the entities are queried through the entityOperations binding, a POST whose `Query` body
//! names the entity types and the temporal query (clause 5.7.4). The Bus of the setup is answered for no case, since
//! the Bus is never named and the Vehicle of the instant alone is selected.
//!
//! The answers are compared exactly with the test's expectations.

use crate::vehicle_fixtures::{create_vehicle_pair_and_bus, delete_vehicle_pair_and_bus};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    query::{temporal_query, type_selection},
};
use serde_json::{Value, json};

/// The expected answer for Vehicle `<ns>-021-13-B`, with every instance it holds in September 2020.
fn september_vehicle(namespace: &Namespace) -> Value {
    json!({
        "id": namespace.urn_text("Vehicle", "021-13-B"),
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
            {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
            {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
        ],
        "fuelLevel": [
            {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
            {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
            {"type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z"},
        ],
    })
}

/// Pins: Check that one can query the temporal evolution of entities using the entityOperations method
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_13_01_after_one_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-13", "021-13-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-08-02T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .method(QueryMethod::Post)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(json!([september_vehicle(&namespace)])).hiding("instanceId"));

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-13", "021-13-A").await?;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities using the entityOperations method
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_13_02_before_one_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-13", "021-13-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::before("2020-08-02T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .method(QueryMethod::Post)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-13-A"),
                "type": "Vehicle",
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
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-13", "021-13-A").await?;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities using the entityOperations method
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_13_03_between_one_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-13", "021-13-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::between("2020-08-02T12:05:00Z".parse()?, "2020-09-01T13:06:00Z".parse()?))
        .entity_type("Vehicle")
        .method(QueryMethod::Post)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-13-B"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                    {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-13", "021-13-A").await?;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities using the entityOperations method
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_13_04_after_two_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-13", "021-13-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-08-02T12:05:00Z".parse()?))
        .entity_type(type_selection::entity_type("Vehicle").or("Bus"))
        .method(QueryMethod::Post)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(json!([september_vehicle(&namespace)])).hiding("instanceId"));

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-13", "021-13-A").await?;
    Ok(())
}

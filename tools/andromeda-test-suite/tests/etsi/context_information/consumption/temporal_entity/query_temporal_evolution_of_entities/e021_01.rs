//! ETSI `021_01`: the temporal evolutions of the Vehicles of a context are queried after, before and between two
//! instants (clause 5.7.4). The Bus of the setup is never selected, since each query names the Vehicle type.
//!
//! The answers are compared with the case's expectations as the suite's `Compare Dictionaries Ignoring Keys` makes
//! them: the `instanceId` a broker adds is allowed below the root, and the root's members are counted exactly.

use crate::suite_keywords::{create_vehicle_pair_and_bus, delete_vehicle_pair_and_bus};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::temporal_query};
use serde_json::json;

/// Pins: Check that one can query the temporal evolution of entities
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_01_01_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-01", "021-01-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::after("2020-08-01T12:04:00Z".parse()?))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-01-A"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-01-B"),
                "type": "Vehicle",
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

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-01", "021-01-A").await?;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_01_02_before() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-01", "021-01-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::before("2020-09-01T13:06:00Z".parse()?))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-01-A"),
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
            {
                "id": namespace.urn_text("Vehicle", "021-01-B"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-01", "021-01-A").await?;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_01_03_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-01", "021-01-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query(temporal_query::between("2020-08-01T12:04:00Z".parse()?, "2020-09-01T13:06:00Z".parse()?))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-01-A"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
                    {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
                ],
            },
            {
                "id": namespace.urn_text("Vehicle", "021-01-B"),
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
                    {"type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z"},
                ],
                "fuelLevel": [
                    {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                    {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
                ],
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-01", "021-01-A").await?;
    Ok(())
}

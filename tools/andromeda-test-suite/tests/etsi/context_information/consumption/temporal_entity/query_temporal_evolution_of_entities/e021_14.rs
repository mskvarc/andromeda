//! ETSI `021_14`: the temporal evolutions of the Vehicles are queried in the simplified temporal representation, each
//! attribute written as its type and its `values`, pairs of a value and an instant (clause 5.7.4).
//!
//! The answers are compared with the case's expectations as the suite's `Compare Dictionaries Ignoring Keys` makes
//! them.

use crate::suite_keywords::{create_vehicle_pair_and_bus, delete_vehicle_pair_and_bus};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    query::temporal_query,
};
use serde_json::json;

/// Pins: Check that one can query the temporal evolution of entities with the simplified representation
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_14_01_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-14", "021-14-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::after("2020-08-01T12:04:00Z".parse()?))
        .entity_type("Vehicle")
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-14-A"),
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "values": [[80, "2020-08-01T12:05:00Z"], [100, "2020-08-01T12:07:00Z"]],
                },
                "fuelLevel": {
                    "type": "Property",
                    "values": [[53, "2020-08-01T13:05:00Z"], [40, "2020-08-01T14:07:00Z"]],
                },
            },
            {
                "id": namespace.urn_text("Vehicle", "021-14-B"),
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "values": [[120, "2020-09-01T12:03:00Z"], [80, "2020-09-01T12:05:00Z"], [100, "2020-09-01T12:07:00Z"]],
                },
                "fuelLevel": {
                    "type": "Property",
                    "values": [[67, "2020-09-01T12:03:00Z"], [53, "2020-09-01T13:05:00Z"], [40, "2020-09-01T14:07:00Z"]],
                },
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-14", "021-14-A").await?;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities with the simplified representation
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_14_02_before() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-14", "021-14-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::before("2020-09-01T13:06:00Z".parse()?))
        .entity_type("Vehicle")
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-14-A"),
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "values": [[120, "2020-08-01T12:03:00Z"], [80, "2020-08-01T12:05:00Z"], [100, "2020-08-01T12:07:00Z"]],
                },
                "fuelLevel": {
                    "type": "Property",
                    "values": [[67, "2020-08-01T12:03:00Z"], [53, "2020-08-01T13:05:00Z"], [40, "2020-08-01T14:07:00Z"]],
                },
            },
            {
                "id": namespace.urn_text("Vehicle", "021-14-B"),
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "values": [[120, "2020-09-01T12:03:00Z"], [80, "2020-09-01T12:05:00Z"], [100, "2020-09-01T12:07:00Z"]],
                },
                "fuelLevel": {
                    "type": "Property",
                    "values": [[67, "2020-09-01T12:03:00Z"], [53, "2020-09-01T13:05:00Z"]],
                },
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-14", "021-14-A").await?;
    Ok(())
}

/// Pins: Check that one can query the temporal evolution of entities with the simplified representation
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_14_02_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_pair_and_bus(&broker, &namespace, "021-14", "021-14-A").await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_values(temporal_query::between("2020-08-01T12:04:00Z".parse()?, "2020-09-01T13:06:00Z".parse()?))
        .entity_type("Vehicle")
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Vehicle", "021-14-A"),
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "values": [[80, "2020-08-01T12:05:00Z"], [100, "2020-08-01T12:07:00Z"]],
                },
                "fuelLevel": {
                    "type": "Property",
                    "values": [[53, "2020-08-01T13:05:00Z"], [40, "2020-08-01T14:07:00Z"]],
                },
            },
            {
                "id": namespace.urn_text("Vehicle", "021-14-B"),
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "values": [[120, "2020-09-01T12:03:00Z"], [80, "2020-09-01T12:05:00Z"], [100, "2020-09-01T12:07:00Z"]],
                },
                "fuelLevel": {
                    "type": "Property",
                    "values": [[67, "2020-09-01T12:03:00Z"], [53, "2020-09-01T13:05:00Z"]],
                },
            },
        ]))
        .hiding("instanceId")
    );

    delete_vehicle_pair_and_bus(&broker, &namespace, "021-14", "021-14-A").await?;
    Ok(())
}

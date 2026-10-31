//! The temporal evolutions of the Vehicles are queried in aggregated temporal representation, one aggregate method or
//! several, per hour or per day, for every attribute or for one of them (clauses 5.7.3 and 4.5.19).
//!
//! The aggregated arrays are compared exactly with the test's expectations.

use crate::vehicle_fixtures::create_vehicle_speed_fuel_level;
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::evolution::AggregationMethod, query::temporal_query};
use serde_json::json;

/// Pins: Check that one can retrieve the temporal evolution of entities with the aggregated temporal representation
/// Clause: 5.7.3, 4.5.19
#[gtest]
#[tokio::test]
async fn e021_17_01_one_aggregate_method_aggregated_by_one_hour_duration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_aggregated(
            temporal_query::after("2020-01-01T12:03:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Avg])
                .aggregation_period("PT1H".parse()?),
        )
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": {"type": "Property", "avg": [[100.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"]]},
            "fuelLevel": {"type": "Property", "avg": [
                [67.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
                [53.0, "2020-08-01T13:03:00Z", "2020-08-01T14:03:00Z"],
                [40.0, "2020-08-01T14:03:00Z", "2020-08-01T15:03:00Z"],
            ]},
        }]))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of entities with the aggregated temporal representation
/// Clause: 5.7.3, 4.5.19
#[gtest]
#[tokio::test]
async fn e021_17_02_one_aggregate_method_aggregated_by_one_hour_duration_asking_for_one_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_aggregated(
            temporal_query::after("2020-01-01T12:03:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Avg])
                .aggregation_period("PT1H".parse()?),
        )
        .entity_type("Vehicle")
        .attrs(vec!["fuelLevel".parse()?])
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": id.to_string(),
            "type": "Vehicle",
            "fuelLevel": {"type": "Property", "avg": [
                [67.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
                [53.0, "2020-08-01T13:03:00Z", "2020-08-01T14:03:00Z"],
                [40.0, "2020-08-01T14:03:00Z", "2020-08-01T15:03:00Z"],
            ]},
        }]))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of entities with the aggregated temporal representation
/// Clause: 5.7.3, 4.5.19
#[gtest]
#[tokio::test]
async fn e021_17_03_multiple_aggregate_methods_aggregated_by_one_hour_duration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_aggregated(
            temporal_query::after("2020-01-01T12:03:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Avg, AggregationMethod::Max])
                .aggregation_period("PT1H".parse()?),
        )
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "avg": [[100.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"]],
                "max": [[120.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"]],
            },
            "fuelLevel": {
                "type": "Property",
                "avg": [
                    [67.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
                    [53.0, "2020-08-01T13:03:00Z", "2020-08-01T14:03:00Z"],
                    [40.0, "2020-08-01T14:03:00Z", "2020-08-01T15:03:00Z"],
                ],
                "max": [
                    [67.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
                    [53.0, "2020-08-01T13:03:00Z", "2020-08-01T14:03:00Z"],
                    [40.0, "2020-08-01T14:03:00Z", "2020-08-01T15:03:00Z"],
                ],
            },
        }]))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of entities with the aggregated temporal representation
/// Clause: 5.7.3, 4.5.19
#[gtest]
#[tokio::test]
async fn e021_17_04_multiple_aggregate_methods_aggregated_by_one_day_duration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .query_aggregated(
            temporal_query::after("2020-01-01T12:03:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Min, AggregationMethod::Max])
                .aggregation_period("P1D".parse()?),
        )
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "min": [[80.0, "2020-08-01T12:03:00Z", "2020-08-02T12:03:00Z"]],
                "max": [[120.0, "2020-08-01T12:03:00Z", "2020-08-02T12:03:00Z"]],
            },
            "fuelLevel": {
                "type": "Property",
                "min": [[40.0, "2020-08-01T12:03:00Z", "2020-08-02T12:03:00Z"]],
                "max": [[67.0, "2020-08-01T12:03:00Z", "2020-08-02T12:03:00Z"]],
            },
        }]))
        .hiding("instanceId")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

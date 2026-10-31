//! The temporal evolution of an entity is retrieved in the aggregated temporal representation, each attribute as its
//! aggregate values for one-hour or one-day periods, named by `options=aggregatedValues` (clauses 5.7.3 and 4.5.19).
//!
//! The answer is compared exactly with the test's expectation.

use crate::vehicle_fixtures::create_vehicle_speed_fuel_level;
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{EntityId, evolution::TemporalAttributeName},
};
use serde_json::{Value, json};

/// Retrieves the aggregates of `id` under `vocabulary`'s context, for the aggregation methods `methods`
/// (`aggrMethods`), the period `period` (`aggrPeriodDuration`) and the attributes `attrs`, requires 200 and answers the
/// body.
async fn aggregated(
    broker: &BrokerClient,
    vocabulary: &Vocabulary,
    id: &EntityId,
    methods: &str,
    period: &str,
    attrs: Vec<TemporalAttributeName>,
) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_aggregated(id)
        .spelling(RepresentationSpelling::default().in_options())
        .aggregation_methods(methods.parse()?)
        .aggregation_period(period.parse()?)
        .attrs(attrs)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can retrieve the temporal evolution of an entity with the aggregated temporal representation
/// Clause: 5.7.3, 4.5.19
#[gtest]
#[tokio::test]
async fn e020_11_01_one_aggregate_method_aggregated_by_one_hour_duration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = aggregated(&broker, &vocabulary, &id, "avg", "PT1H", Vec::new()).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "avg": [
                [100.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
            ],
        },
        "fuelLevel": {
            "type": "Property",
            "avg": [
                [67.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
                [53.0, "2020-08-01T13:03:00Z", "2020-08-01T14:03:00Z"],
                [40.0, "2020-08-01T14:03:00Z", "2020-08-01T15:03:00Z"],
            ],
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity with the aggregated temporal representation
/// Clause: 5.7.3, 4.5.19
#[gtest]
#[tokio::test]
async fn e020_11_02_one_aggregate_method_aggregated_by_one_hour_duration_asking_for_one_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = aggregated(&broker, &vocabulary, &id, "avg", "PT1H", vec!["fuelLevel".parse()?]).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "fuelLevel": {
            "type": "Property",
            "avg": [
                [67.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
                [53.0, "2020-08-01T13:03:00Z", "2020-08-01T14:03:00Z"],
                [40.0, "2020-08-01T14:03:00Z", "2020-08-01T15:03:00Z"],
            ],
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity with the aggregated temporal representation
/// Clause: 5.7.3, 4.5.19
#[gtest]
#[tokio::test]
async fn e020_11_03_multiple_aggregate_methods_aggregated_by_one_hour_duration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = aggregated(&broker, &vocabulary, &id, "avg,max", "PT1H", Vec::new()).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "avg": [
                [100.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
            ],
            "max": [
                [120.0, "2020-08-01T12:03:00Z", "2020-08-01T13:03:00Z"],
            ],
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
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity with the aggregated temporal representation
/// Clause: 5.7.3, 4.5.19
#[gtest]
#[tokio::test]
async fn e020_11_04_multiple_aggregate_methods_aggregated_by_one_day_duration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = aggregated(&broker, &vocabulary, &id, "min,max", "P1D", Vec::new()).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "min": [
                [80.0, "2020-08-01T12:03:00Z", "2020-08-02T12:03:00Z"],
            ],
            "max": [
                [120.0, "2020-08-01T12:03:00Z", "2020-08-02T12:03:00Z"],
            ],
        },
        "fuelLevel": {
            "type": "Property",
            "min": [
                [40.0, "2020-08-01T12:03:00Z", "2020-08-02T12:03:00Z"],
            ],
            "max": [
                [67.0, "2020-08-01T12:03:00Z", "2020-08-02T12:03:00Z"],
            ],
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

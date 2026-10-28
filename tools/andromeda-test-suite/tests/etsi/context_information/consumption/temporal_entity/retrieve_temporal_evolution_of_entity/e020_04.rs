//! ETSI `020_04`: the temporal evolution of an entity is retrieved for the instances that a temporal query
//! selects, after, before or between two instants (clause 5.7.3).
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::suite_keywords::create_vehicle_speed_fuel_level;
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
    http::headers::Accept,
    model::EntityId,
    query::temporal_query::{self, builders::TemporalQueryBuilder},
};
use serde_json::{Value, json};

/// Retrieves the temporal evolution of `id` under `vocabulary`'s context, selected by `query`, requires 200 and answers
/// the body.
async fn retrieved(broker: &BrokerClient, vocabulary: &Vocabulary, id: &EntityId, query: TemporalQueryBuilder) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(id)
        .temporal_q(query)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can retrieve the temporal evolution of an entity matching the given NGSI-LD temporal query
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_04_01_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = retrieved(&broker, &vocabulary, &id, temporal_query::after("2020-08-01T13:03:00Z".parse()?)).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "fuelLevel": [
            {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
            {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity matching the given NGSI-LD temporal query
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_04_02_before() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = retrieved(&broker, &vocabulary, &id, temporal_query::before("2020-08-01T12:05:00Z".parse()?)).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "fuelLevel": [
            {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
        ],
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity matching the given NGSI-LD temporal query
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_04_03_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = retrieved(
        &broker,
        &vocabulary,
        &id,
        temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?),
    )
    .await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "fuelLevel": [
            {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
        ],
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
            {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

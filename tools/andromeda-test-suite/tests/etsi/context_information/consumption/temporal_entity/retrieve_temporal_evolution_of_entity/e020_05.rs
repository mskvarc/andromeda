//! ETSI `020_05`: the last N instances of each attribute of a temporal evolution are retrieved, N below the number of
//! instances and N at least the number of instances (clause 5.7.3).
//!
//! The answer is compared with the case's expectation as the suite's `Compare Dictionaries Ignoring Keys` makes it.

use crate::{
    suite_keywords::create_temporal_evolution,
    suite_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_five_instances},
};
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
use ngsi_ld::{http::headers::Accept, model::EntityId};
use serde_json::{Value, json};

/// Creates the case's Vehicle as its suite setup does, and answers the vocabulary it is stored under.
async fn created_vehicle(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_five_instances(namespace, 1)?).await
}

/// Retrieves the last `count` instances of each attribute of `id` under `vocabulary`'s context, requires 200 and
/// answers the body.
async fn last_instances(broker: &BrokerClient, vocabulary: &Vocabulary, id: &EntityId, count: u64) -> Result<Value> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(id)
        .last_n(count)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can retrieve the temporal evolution of the last N instances of entity attributes
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_05_01_retrieve_some_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = last_instances(&broker, &vocabulary, &id, 4).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 105, "observedAt": "2020-08-01T12:27:00Z"},
            {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:29:00Z"},
            {"type": "Property", "value": 125, "observedAt": "2020-08-01T12:31:00Z"},
            {"type": "Property", "value": 130, "observedAt": "2020-08-01T12:35:00Z"},
        ],
        "fuelLevel": [
            {"type": "Property", "value": 31, "observedAt": "2020-08-01T20:07:00Z"},
            {"type": "Property", "value": 28, "observedAt": "2020-08-01T21:03:00Z"},
            {"type": "Property", "value": 24, "observedAt": "2020-08-01T21:05:00Z"},
            {"type": "Property", "value": 19, "observedAt": "2020-08-01T22:07:00Z"},
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of the last N instances of entity attributes
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_05_02_retrieve_all_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = last_instances(&broker, &vocabulary, &id, 7).await?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 85, "observedAt": "2020-08-01T12:25:00Z"},
            {"type": "Property", "value": 105, "observedAt": "2020-08-01T12:27:00Z"},
            {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:29:00Z"},
            {"type": "Property", "value": 125, "observedAt": "2020-08-01T12:31:00Z"},
            {"type": "Property", "value": 130, "observedAt": "2020-08-01T12:35:00Z"},
        ],
        "fuelLevel": [
            {"type": "Property", "value": 35, "observedAt": "2020-08-01T20:05:00Z"},
            {"type": "Property", "value": 31, "observedAt": "2020-08-01T20:07:00Z"},
            {"type": "Property", "value": 28, "observedAt": "2020-08-01T21:03:00Z"},
            {"type": "Property", "value": 24, "observedAt": "2020-08-01T21:05:00Z"},
            {"type": "Property", "value": 19, "observedAt": "2020-08-01T22:07:00Z"},
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

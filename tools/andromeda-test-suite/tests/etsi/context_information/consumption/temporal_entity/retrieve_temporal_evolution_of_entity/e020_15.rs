//! The temporal evolution of an entity with a `JsonProperty` is retrieved in the normalized and in the simplified
//! temporal representation (clauses 5.7.3, 4.5.7 and 4.5.9).
//!
//! The answer is compared exactly with the test's expectation.

use crate::temporal_operations::create_temporal_evolution;
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
    model::{Input, JsonProperty, evolution::TemporalEntity},
};
use serde_json::{Value, json};

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the cases: JSON speeds of 120, 80 and 100 observed at 12:03, 12:05 and
/// 12:07 on 2020-08-01 in UTC.
fn vehicle_with_json_speeds(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    let speeds = [(120, "2020-08-01T12:03:00Z"), (80, "2020-08-01T12:05:00Z"), (100, "2020-08-01T12:07:00Z")];
    let mut builder = TemporalEntity::builder().id(namespace.urn("Vehicle", 1)?).entity_type("Vehicle");
    for (value, observed_at) in speeds {
        builder = builder.json_property(
            "speed",
            JsonProperty::builder(format!(r#"{{"value":{value}}}"#).parse()?).observed_at(observed_at.parse()?),
        );
    }
    Ok(builder.build()?)
}

/// Creates the test's Vehicle, and answers the vocabulary it is stored under.
async fn created_vehicle(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &["Vehicle", "speed"], &vehicle_with_json_speeds(namespace)?).await
}

/// Pins: Check that you can retrieve the temporal evolution of an entity with a JSON property
/// Clause: 5.7.3, 4.5.7
#[gtest]
#[tokio::test]
async fn e020_15_01_retrieve_the_normalized_temporal_representation_of_an_entity_with_a_json_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": [
            {"type": "JsonProperty", "json": {"value": 120}, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "JsonProperty", "json": {"value": 80}, "observedAt": "2020-08-01T12:05:00Z"},
            {"type": "JsonProperty", "json": {"value": 100}, "observedAt": "2020-08-01T12:07:00Z"},
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(
        body.get("speed")
            .and_then(Value::as_array)
            .is_some_and(|instances| instances.iter().all(|instance| !instance.pointer("/json/value").is_some_and(Value::is_f64))),
        is_true()
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that you can retrieve the temporal evolution of an entity with a JSON property
/// Clause: 5.7.3, 4.5.9
#[gtest]
#[tokio::test]
async fn e020_15_02_retrieve_the_simplified_temporal_representation_of_an_entity_with_a_json_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_values(&id)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "speed": {
            "type": "JsonProperty",
            "jsons": [
                [{"json": {"value": 120}}, "2020-08-01T12:03:00Z"],
                [{"json": {"value": 80}}, "2020-08-01T12:05:00Z"],
                [{"json": {"value": 100}}, "2020-08-01T12:07:00Z"],
            ],
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

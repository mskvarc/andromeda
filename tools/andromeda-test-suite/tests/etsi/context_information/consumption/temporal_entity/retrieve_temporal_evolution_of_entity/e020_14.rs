//! ETSI `020_14`: the temporal evolution of an entity whose two attributes hold instances at different times is
//! retrieved for a time range that empties one of them (clauses 5.7.3 and 6.3.10).

use crate::{suite_keywords::create_temporal_evolution, suite_payloads::VEHICLE_SPEED_FUEL_TERMS};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::request_to,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::model::{Input, Property, evolution::TemporalEntity};
use serde_json::Value;

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the cases: 59 speeds observed at 01:01 to 01:59 on 2020-01-01 and 59
/// fuel levels observed at 01:01 to 01:59 on 2020-01-02, in UTC.
fn vehicle_with_unsynchronized_instances(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    let mut builder = TemporalEntity::builder().id(namespace.urn("Vehicle", 1)?).entity_type("Vehicle");
    for minute in 1..=59 {
        builder = builder.property(
            "speed",
            Property::builder(minute).observed_at(format!("2020-01-01T01:{minute:02}:00Z").parse()?),
        );
        builder = builder.property(
            "fuelLevel",
            Property::builder(minute).observed_at(format!("2020-01-02T01:{minute:02}:00Z").parse()?),
        );
    }
    Ok(builder.build()?)
}

/// Creates the case's Vehicle as its suite setup does, and answers the vocabulary it is stored under.
async fn created_vehicle(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_with_unsynchronized_instances(namespace)?).await
}

/// Sends a `GET` of `target`, relative to the API root and under `vocabulary`'s context, and answers the body as the
/// broker wrote it. The typed reader refuses the empty attribute list this answer carries, so the body is read as
/// JSON.
async fn read_as_written(broker: &BrokerClient, vocabulary: &Vocabulary, target: &str) -> Result<Value> {
    let client = broker.with_context(vocabulary.context())?;
    let answer = client.send_unchecked(request_to(&client, Method::GET, target)?).await?;
    Ok(serde_json::from_slice(answer.body())?)
}

/// Whether `value` is an empty array, object or string, the values whose `length` is 0 in `JSONPath`.
fn is_empty_container(value: &Value) -> bool {
    value.as_array().is_some_and(Vec::is_empty) || value.as_object().is_some_and(serde_json::Map::is_empty) || value.as_str().is_some_and(str::is_empty)
}

/// Pins: Check that the time range cut before the second attribute
/// Clause: 5.7.3, 6.3.10
#[gtest]
#[tokio::test]
async fn e020_14_01_retrieve_an_entity_with_60_instances_of_unsynchronized_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = read_as_written(
        &broker,
        &vocabulary,
        &format!("temporal/entities/{id}?timerel=after&timeAt=2019-01-01T01%3A01%3A00Z"),
    )
    .await?;
    expect_that!(body.get("fuelLevel").is_some_and(is_empty_container), is_true());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that the time range cut before the second attribute
/// Clause: 5.7.3, 6.3.10
#[gtest]
#[tokio::test]
async fn e020_14_02_retrieve_the_entity_with_last_n() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let body = read_as_written(
        &broker,
        &vocabulary,
        &format!("temporal/entities/{id}?timerel=before&timeAt=2021-01-01T01%3A01%3A00Z&lastN=100"),
    )
    .await?;
    expect_that!(body.get("speed").is_some_and(is_empty_container), is_true());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

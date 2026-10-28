//! ETSI `020_21`: the temporal evolution of an entity is retrieved with its attributes, or its attributes and core
//! members, projected by `pick` or `omit` (clauses 5.7.3 and 4.21).

use crate::suite_keywords::create_vehicle_speed_fuel_level;
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    query::projection::{self, EntityMember},
};
use serde_json::{Value, json};

/// The integer `value` of every instance of `attribute` in `body`, in the order the answer holds the instances. A value
/// that is not an integer, a float such as `120.0` included, is `None`.
fn instance_values(body: &Value, attribute: &str) -> Vec<Option<i64>> {
    body.get(attribute)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|instance| instance.get("value").and_then(Value::as_i64))
        .collect()
}

/// Pins: Check that one can retrieve an entity with pick on attributes
/// Clause: 5.7.3, 4.21
/// Changed: the value of every instance is checked to be an integer, where the expectation checks it only for an
/// instance whose `observedAt` is not the exact text.
#[gtest]
#[tokio::test]
async fn e020_21_01_retrieve_with_pick_on_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_projected(&id)
        .pick(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!({
            "speed": [
                {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
                {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
            ],
        }))
        .hiding("instanceId")
    );
    expect_that!(
        instance_values(&body, "speed"),
        unordered_elements_are![some(eq(&120)), some(eq(&80)), some(eq(&100))]
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve an entity with omit on attributes
/// Clause: 5.7.3, 4.21
/// Changed: the value of every instance is checked to be an integer, where the expectation checks it only for an
/// instance whose `observedAt` is not the exact text.
#[gtest]
#[tokio::test]
async fn e020_21_02_retrieve_with_omit_on_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .omit(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "fuelLevel": [
                {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
                {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
                {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
            ],
        }))
        .hiding("instanceId")
    );
    expect_that!(
        instance_values(&body, "fuelLevel"),
        unordered_elements_are![some(eq(&67)), some(eq(&53)), some(eq(&40))]
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve an entity with pick on attributes and core members
/// Clause: 5.7.3, 4.21
/// Changed: the value of every instance is checked to be an integer, where the expectation checks it only for an
/// instance whose `observedAt` is not the exact text.
#[gtest]
#[tokio::test]
async fn e020_21_03_retrieve_with_pick_on_attributes_and_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_projected(&id)
        .pick(projection::member(EntityMember::Id).attribute("speed").build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "speed": [
                {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
                {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"},
                {"type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z"},
            ],
        }))
        .hiding("instanceId")
    );
    expect_that!(
        instance_values(&body, "speed"),
        unordered_elements_are![some(eq(&120)), some(eq(&80)), some(eq(&100))]
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve an entity with omit on attributes and core members
/// Clause: 5.7.3, 4.21
/// Changed: the value of every instance is checked to be an integer, where the expectation checks it only for an
/// instance whose `observedAt` is not the exact text.
#[gtest]
#[tokio::test]
async fn e020_21_04_retrieve_with_omit_on_attributes_and_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_projected(&id)
        .omit(projection::member(EntityMember::Type).attribute("speed").build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "fuelLevel": [
                {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"},
                {"type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z"},
                {"type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z"},
            ],
        }))
        .hiding("instanceId")
    );
    expect_that!(
        instance_values(&body, "fuelLevel"),
        unordered_elements_are![some(eq(&67)), some(eq(&53)), some(eq(&40))]
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

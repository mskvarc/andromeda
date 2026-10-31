//! An attribute of an entity is deleted, its default instance, one dataset or every instance (clause 5.6.5).

use crate::{
    entity_operations::{create_entity, delete_entities, retrieve_entity_json},
    vehicle_entity_payloads::dataset,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{AttributeName, Entity, Input, NgsiLdMediaType, Property, Value},
};
use serde_json::json;

/// The terms of the `@context` the deletion cases register.
const TERMS: [&str; 3] = ["Vehicle", "speed", "source"];

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the deletion cases: a `speed` of 55 from a Feeling as the default
/// instance, a `speed` of 57 from a Speedometer in the dataset `urn:ngsi-ld:Dataset:<ns>-speedometer`, and a `speed`
/// of 54.5 from a GPS in the dataset `urn:ngsi-ld:Dataset:<ns>-gps`.
fn vehicle_speed_sources(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(55).property("source", Property::builder("Feeling")))
        .property(
            "speed",
            Property::builder(57)
                .property("source", Property::builder("Speedometer"))
                .dataset_id(dataset(namespace, "speedometer")?),
        )
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", Property::builder("GPS"))
                .dataset_id(dataset(namespace, "gps")?),
        )
        .build()?)
}

/// The setup of the deletion tests: registers `Vehicle`, `speed` and `source`, and creates the Vehicle of
/// [`vehicle_speed_sources`] in their context, answering the vocabulary.
async fn create_vehicle_speed_sources(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &TERMS).await?;
    create_entity(broker, &vocabulary, &vehicle_speed_sources(namespace)?).await?;
    Ok(vocabulary)
}

/// The `speed` the cases keep after the default instance is deleted: the Speedometer instance of 57 and the GPS
/// instance of 54.5, each in its dataset.
fn speedometer_and_gps(namespace: &Namespace) -> serde_json::Value {
    json!([
        {
            "type": "Property",
            "value": 57,
            "source": {"type": "Property", "value": "Speedometer"},
            "datasetId": namespace.urn_text("Dataset", "speedometer"),
        },
        {
            "type": "Property",
            "value": 54.5,
            "source": {"type": "Property", "value": "GPS"},
            "datasetId": namespace.urn_text("Dataset", "gps"),
        },
    ])
}

/// The `speed` the cases keep after the GPS instance is deleted: the Feeling instance of 55 without a dataset, and the
/// Speedometer instance of 57 in its dataset.
fn feeling_and_speedometer(namespace: &Namespace) -> serde_json::Value {
    json!([
        {"type": "Property", "value": 55, "source": {"type": "Property", "value": "Feeling"}},
        {
            "type": "Property",
            "value": 57,
            "source": {"type": "Property", "value": "Speedometer"},
            "datasetId": namespace.urn_text("Dataset", "speedometer"),
        },
    ])
}

/// Pins: Check that one can delete an attribute from an entity
/// Clause: 5.6.5
#[gtest]
#[tokio::test]
async fn e013_01_01_delete_an_attribute_with_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_sources(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let deleted = client
        .with_responses()
        .entities()
        .delete_attribute(&id, &speed)
        .instances(InstanceSelection::Selected(None))
        .send()
        .await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body.len(), eq(0));

    let body = retrieve_entity_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": speedometer_and_gps(&namespace),
        }))
        .hiding_everywhere("@context")
    );

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete an attribute from an entity
/// Clause: 5.6.5
#[gtest]
#[tokio::test]
async fn e013_01_02_delete_an_attribute_with_a_specific_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_sources(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let gps = namespace.dataset_id("Dataset", "gps")?;
    let deleted = client
        .with_responses()
        .entities()
        .delete_attribute(&id, &speed)
        .instances(InstanceSelection::Selected(Some(&gps)))
        .send()
        .await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body.len(), eq(0));

    let body = retrieve_entity_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": feeling_and_speedometer(&namespace),
        }))
        .hiding_everywhere("@context")
    );

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete an attribute from an entity
/// Clause: 5.6.5
#[gtest]
#[tokio::test]
async fn e013_01_03_delete_all_attribute_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_sources(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let deleted = client
        .with_responses()
        .entities()
        .delete_attribute(&id, &speed)
        .instances(InstanceSelection::All)
        .send()
        .await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body.len(), eq(0));

    let body = retrieve_entity_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
        }))
        .hiding_everywhere("@context")
    );

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

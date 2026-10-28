//! ETSI `015_01`: an attribute of a temporal representation of an entity is deleted, the default instance, one
//! dataset or every instance (clause 5.6.13).

use crate::suite_keywords::{create_vehicle_speed_fuel_level_with_dataset, naming_delete_all_false, retrieve_temporal_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{DeleteTemporalAttribute, InstanceSelection},
    model::{AttributeName, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: Check that one can delete an attribute of a temporal representation of an entity with simple temporal
/// properties
/// Clause: 5.6.13
#[gtest]
#[tokio::test]
async fn e015_01_01_delete_an_attribute_from_a_temporal_representation_of_an_entity_without_delete_all_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fuel_level: AttributeName = "fuelLevel".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let operation = DeleteTemporalAttribute::builder().id(&id).name(&fuel_level).build();
    let request = naming_delete_all_false(&client, &operation)?;
    let deleted = client.send_unchecked(request).await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body().len(), eq(0));

    let body = retrieve_temporal_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
            ],
            "fuelLevel": [
                {
                    "type": "Property",
                    "value": 40,
                    "observedAt": "2020-09-01T14:07:00Z",
                    "datasetId": namespace.urn_text("Vehicle", "12345-fuel"),
                },
            ],
        }))
        .hiding("instanceId")
        .hiding_everywhere("@context")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can delete an attribute of a temporal representation of an entity with simple temporal
/// properties
/// Clause: 5.6.13
#[gtest]
#[tokio::test]
async fn e015_01_02_delete_an_attribute_from_a_temporal_representation_of_an_entity_with_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fuel_level: AttributeName = "fuelLevel".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let dataset = namespace.urn("Vehicle", "12345-fuel")?;
    let operation = DeleteTemporalAttribute::builder()
        .id(&id)
        .name(&fuel_level)
        .instances(InstanceSelection::Dataset(dataset.as_uri()))
        .build();
    let request = naming_delete_all_false(&client, &operation)?;
    let deleted = client.send_unchecked(request).await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body().len(), eq(0));

    let body = retrieve_temporal_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
            ],
            "fuelLevel": [
                {"type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z"},
            ],
        }))
        .hiding("instanceId")
        .hiding_everywhere("@context")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can delete an attribute of a temporal representation of an entity with simple temporal
/// properties
/// Clause: 5.6.13
#[gtest]
#[tokio::test]
async fn e015_01_03_delete_an_attribute_from_a_temporal_representation_of_an_entity_with_delete_all() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fuel_level: AttributeName = "fuelLevel".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let deleted = client
        .with_responses()
        .temporal()
        .delete_attribute(&id, &fuel_level)
        .instances(InstanceSelection::All)
        .send()
        .await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body.len(), eq(0));

    let body = retrieve_temporal_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"},
                {"type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z"},
            ],
        }))
        .hiding("instanceId")
        .hiding_everywhere("@context")
    );

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

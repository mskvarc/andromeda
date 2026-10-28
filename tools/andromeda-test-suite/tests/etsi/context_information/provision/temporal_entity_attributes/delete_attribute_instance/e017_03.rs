//! ETSI `017_03`: an attribute instance is not deleted when its entity, attribute or instance is not found (clause
//! 5.6.15).

use crate::{
    suite_keywords::{create_temporal_evolution, create_vehicle_speed_fuel_level_with_dataset, first_speed_instance_id, retrieve_temporal_sys_attrs_json},
    suite_payloads::vehicle_speed_fuel_level_with_dataset,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, InstanceId, NgsiLdMediaType};

/// Pins: Check that one cannot delete an attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not found
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_03_01_delete_an_attribute_instance_in_temporal_representation_of_an_entity_if_the_entity_with_given_id_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let missing = namespace.urn("Vehicle", 2)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    let answer = client.with_responses().temporal().delete_instance(&missing, &speed, &instance_id).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot delete an attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not found
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_03_02_delete_an_attribute_instance_in_temporal_representation_of_an_entity_if_the_target_attribute_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(
        &broker,
        &namespace,
        &["Vehicle", "speed", "fuelLevel", "speed2"],
        &vehicle_speed_fuel_level_with_dataset(&namespace)?,
    )
    .await?;
    let id = namespace.urn("Vehicle", 1)?;
    let absent: AttributeName = "speed2".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    let answer = client.with_responses().temporal().delete_instance(&id, &absent, &instance_id).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot delete an attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not found
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_03_03_delete_an_attribute_instance_in_temporal_representation_of_an_entity_if_the_target_attribute_instance_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    first_speed_instance_id(&body)?;
    let absent: InstanceId = format!("urn:ngsi-ld:{namespace}-01234567890123456789").parse()?;
    let answer = client.with_responses().temporal().delete_instance(&id, &speed, &absent).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

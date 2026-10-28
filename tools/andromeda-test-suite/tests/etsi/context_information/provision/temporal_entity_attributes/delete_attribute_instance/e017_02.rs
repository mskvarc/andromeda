//! ETSI `017_02`: an attribute instance is not deleted by a request whose entity id, instance id or attribute name
//! is invalid or missing (clause 5.6.15).

use crate::suite_keywords::{answered_with, create_vehicle_speed_fuel_level_with_dataset, first_speed_instance_id, retrieve_temporal_sys_attrs_json};
use andromeda_test_suite::{broker, broker::BrokerClient, namespace::Namespace, teardown::AnyAnswer, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::NgsiLdMediaType;

/// Sends the `DELETE` of `target` in the context `client` carries and requires the 400 status.
async fn refused_instance_deletion(client: &BrokerClient, target: &str) -> Result<()> {
    answered_with(client, request_to(client, Method::DELETE, target)?, StatusCode::BAD_REQUEST).await
}

/// Pins: Check that one cannot delete an attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_02_01_delete_an_attribute_instance_in_temporal_representation_of_an_entity_if_the_entity_id_is_not_valid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_instance_deletion(&client, &format!("temporal/entities/invalidId/attrs/speed/{instance_id}")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot delete an attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_02_02_delete_an_attribute_instance_in_temporal_representation_of_an_entity_if_the_entity_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_instance_deletion(&client, &format!("temporal/entities//attrs/speed/{instance_id}")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot delete an attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_02_03_delete_an_attribute_instance_in_temporal_representation_of_an_entity_if_the_instance_id_is_not_valid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    first_speed_instance_id(&body)?;
    refused_instance_deletion(&client, &format!("temporal/entities/{id}/attrs/speed/invalidId")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot delete an attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_02_04_delete_an_attribute_instance_in_temporal_representation_of_an_entity_if_the_attribute_name_is_not_a_valid_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_instance_deletion(&client, &format!("temporal/entities/{id}/attrs/invalid(Name/{instance_id}")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot delete an attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.15
#[gtest]
#[tokio::test]
async fn e017_02_05_delete_an_attribute_instance_in_temporal_representation_of_an_entity_if_the_attribute_name_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_instance_deletion(&client, &format!("temporal/entities/{id}/attrs//{instance_id}")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

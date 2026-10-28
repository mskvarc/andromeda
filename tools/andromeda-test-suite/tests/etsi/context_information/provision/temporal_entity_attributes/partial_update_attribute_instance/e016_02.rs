//! ETSI `016_02`: an attribute instance is not modified by a request whose entity id, instance id or attribute name is
//! invalid or missing (clause 5.6.14).

use crate::{
    suite_keywords::{answered_with, create_vehicle_speed_fuel_level_with_dataset, first_speed_instance_id, retrieve_temporal_sys_attrs_json},
    suite_payloads::replacement_speed_instance,
};
use andromeda_test_suite::{broker, broker::BrokerClient, namespace::Namespace, teardown::AnyAnswer, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::ModifyAttributeInstance,
    model::{AttributeName, EntityId, InstanceId, NgsiLdMediaType},
};

/// Sends the `PATCH` of the instance `instance_id` of `speed` written to `target` (relative to the API root) in the
/// context `client` carries, and requires `status`.
async fn refused_modification(
    client: &BrokerClient,
    id: &EntityId,
    name: &AttributeName,
    instance_id: &InstanceId,
    target: &str,
    status: StatusCode,
) -> Result<()> {
    let instance = replacement_speed_instance()?;
    let operation = ModifyAttributeInstance::builder()
        .id(id)
        .name(name)
        .instance_id(instance_id)
        .instance(&instance)
        .build();
    answered_with(client, client.unchecked(&operation)?.uri(api_uri(client, target)?), status).await
}

/// Pins: Check that one cannot partially modify attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.14
/// Corrected: fix(016): send the `EntityTemporal` Fragment 5.6.14 defines, not a bare instance (5.6.14.3)
#[gtest]
#[tokio::test]
async fn e016_02_01_modify_attribute_instance_in_temporal_representation_of_an_entity_if_the_entity_id_is_not_valid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_modification(
        &client,
        &id,
        &speed,
        &instance_id,
        &format!("temporal/entities/invalidId/attrs/speed/{instance_id}"),
        StatusCode::BAD_REQUEST,
    )
    .await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot partially modify attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.14
/// Corrected: fix(016): send the `EntityTemporal` Fragment 5.6.14 defines, not a bare instance (5.6.14.3)
#[gtest]
#[tokio::test]
async fn e016_02_02_modify_attribute_instance_in_temporal_representation_of_an_entity_if_the_entity_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_modification(
        &client,
        &id,
        &speed,
        &instance_id,
        &format!("temporal/entities//attrs/speed/{instance_id}"),
        StatusCode::BAD_REQUEST,
    )
    .await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot partially modify attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.14
/// Corrected: fix(016): send the `EntityTemporal` Fragment 5.6.14 defines, not a bare instance (5.6.14.3)
#[gtest]
#[tokio::test]
async fn e016_02_03_modify_attribute_instance_in_temporal_representation_of_an_entity_if_the_instance_id_is_not_valid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_modification(
        &client,
        &id,
        &speed,
        &instance_id,
        &format!("temporal/entities/{id}/attrs/speed/invalidId"),
        StatusCode::BAD_REQUEST,
    )
    .await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot partially modify attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.14
/// Corrected: fix(016): send the `EntityTemporal` Fragment 5.6.14 defines, not a bare instance (5.6.14.3)
#[gtest]
#[tokio::test]
async fn e016_02_04_modify_attribute_instance_in_temporal_representation_of_an_entity_if_the_instance_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_modification(
        &client,
        &id,
        &speed,
        &instance_id,
        &format!("temporal/entities/{id}/attrs/speed/"),
        StatusCode::METHOD_NOT_ALLOWED,
    )
    .await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot partially modify attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.14
/// Corrected: fix(016): send the `EntityTemporal` Fragment 5.6.14 defines, not a bare instance (5.6.14.3)
#[gtest]
#[tokio::test]
async fn e016_02_05_modify_attribute_instance_in_temporal_representation_of_an_entity_if_the_attribute_name_is_not_a_valid_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_modification(
        &client,
        &id,
        &speed,
        &instance_id,
        &format!("temporal/entities/{id}/attrs/invalid(Id/{instance_id}"),
        StatusCode::BAD_REQUEST,
    )
    .await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot partially modify attribute instance in temporal representation of an entity if the
/// EntityId/AttributeId/InstanceId is not right
/// Clause: 5.6.14
/// Corrected: fix(016): send the `EntityTemporal` Fragment 5.6.14 defines, not a bare instance (5.6.14.3)
/// Corrected: `fix(016_02_06)`: empty attribute name is badrequestdata not 405 (5.6.14.4)
#[gtest]
#[tokio::test]
async fn e016_02_06_modify_attribute_instance_in_temporal_representation_of_an_entity_if_the_attribute_name_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let body = retrieve_temporal_sys_attrs_json(&client.with_accept(NgsiLdMediaType::JsonLd), &id).await?;
    let instance_id = first_speed_instance_id(&body)?;
    refused_modification(
        &client,
        &id,
        &speed,
        &instance_id,
        &format!("temporal/entities/{id}/attrs//{instance_id}"),
        StatusCode::BAD_REQUEST,
    )
    .await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

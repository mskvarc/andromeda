//! ETSI `015_02`: an attribute of a temporal representation of an entity is not deleted by a request whose entity id
//! is missing or invalid, or whose attribute name is invalid (clause 5.6.13).

use crate::suite_keywords::{create_vehicle_speed_fuel_level_with_dataset, refused};
use andromeda_test_suite::{
    broker,
    broker::BrokerClient,
    namespace::Namespace,
    teardown::AnyAnswer,
    wire::{content_type, request_to},
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::model::{APPLICATION_JSON, ErrorType};

/// Sends the `DELETE` of `target` as `application/json` and requires the 400 `BadRequestData` refusal with a title.
async fn refused_deletion(broker: &BrokerClient, target: &str) -> Result<()> {
    refused(
        broker,
        content_type(request_to(broker, Method::DELETE, target)?, &APPLICATION_JSON)?,
        ErrorType::BadRequestData,
    )
    .await
}

/// Pins: Check that an error is raised if one deletes an attribute to temporal entity with an unknown/invalid
/// Entity/Attribute Id
/// Clause: 5.6.13
#[gtest]
#[tokio::test]
async fn e015_02_01_delete_an_attribute_to_a_temporal_representation_of_an_entity_with_a_missing_entity_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;

    refused_deletion(&broker, "temporal/entities//attrs/speed?deleteAll=false").await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that an error is raised if one deletes an attribute to temporal entity with an unknown/invalid
/// Entity/Attribute Id
/// Clause: 5.6.13
#[gtest]
#[tokio::test]
async fn e015_02_02_delete_an_attribute_to_a_temporal_representation_of_an_entity_with_an_invalid_entity_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;

    refused_deletion(&broker, "temporal/entities/invalidId/attrs/speed?deleteAll=false").await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that an error is raised if one deletes an attribute to temporal entity with an unknown/invalid
/// Entity/Attribute Id
/// Clause: 5.6.13
#[gtest]
#[tokio::test]
async fn e015_02_03_delete_an_attribute_to_a_temporal_representation_of_an_entity_with_an_invalid_attribute_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;

    refused_deletion(&broker, &format!("temporal/entities/{id}/attrs/invalid(Name?deleteAll=false")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

//! An attribute of a temporal representation of an entity is not deleted when the entity does not exist or does not
//! carry the attribute (clause 5.6.13).

use crate::{
    temporal_operations::create_temporal_evolution,
    vehicle_evolution_payloads::vehicle_speed_fuel_level_with_dataset,
    vehicle_fixtures::create_vehicle_speed_fuel_level_with_dataset,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{AttributeName, ErrorType},
};

/// Pins: Check that an error is raised if one deletes a temporal entity with an unknown EntityId/Attribute Id
/// Clause: 5.6.13
#[gtest]
#[tokio::test]
async fn e015_03_01_delete_an_attribute_to_a_temporal_entity_if_the_entity_id_does_not_exist() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level_with_dataset(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let missing = namespace.urn("Vehicle", 2)?;
    let fuel_level: AttributeName = "fuelLevel".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let answer = client
        .with_responses()
        .temporal()
        .delete_attribute(&missing, &fuel_level)
        .instances(InstanceSelection::Selected(None))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that an error is raised if one deletes a temporal entity with an unknown EntityId/Attribute Id
/// Clause: 5.6.13
#[gtest]
#[tokio::test]
async fn e015_03_02_delete_an_attribute_to_a_temporal_entity_if_the_entity_does_not_contain_the_target_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(
        &broker,
        &namespace,
        &["Vehicle", "speed", "fuelLevel", "notExistingAttribute"],
        &vehicle_speed_fuel_level_with_dataset(&namespace)?,
    )
    .await?;
    let id = namespace.urn("Vehicle", 1)?;
    let absent: AttributeName = "notExistingAttribute".parse()?;
    let client = broker.with_context(vocabulary.context())?;

    let answer = client
        .with_responses()
        .temporal()
        .delete_attribute(&id, &absent)
        .instances(InstanceSelection::Selected(None))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

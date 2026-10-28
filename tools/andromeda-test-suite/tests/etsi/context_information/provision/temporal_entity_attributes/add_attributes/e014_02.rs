//! ETSI `014_02`: instances are not added to a temporal representation of an entity through an empty or an invalid entity
//! id (clause 5.6.12).

use crate::{
    suite_keywords::{create_temporal_evolution, ld_json_body_client},
    suite_payloads::{VEHICLE_SPEED_FUEL_ADDED_TERMS, added_attribute_instances, vehicle_speed_fuel_level_with_dataset},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire::send_to};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::operation::AddTemporalAttributes;

/// Pins: Check that an error is raised if one adds a temporal entity attribute with a non-existing/invalid `EntityId`
/// Clause: 5.6.12
#[gtest]
#[tokio::test]
async fn e014_02_01_add_an_attribute_to_a_temporal_representation_of_an_entity_with_an_empty_entity_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(
        &broker,
        &namespace,
        &VEHICLE_SPEED_FUEL_ADDED_TERMS,
        &vehicle_speed_fuel_level_with_dataset(&namespace)?,
    )
    .await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let attributes = added_attribute_instances()?;
    let operation = AddTemporalAttributes::builder().id(&id).attributes(&attributes).build();
    let answer = send_to(&client, &operation, "temporal/entities//attrs").await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that an error is raised if one adds a temporal entity attribute with a non-existing/invalid `EntityId`
/// Clause: 5.6.12
#[gtest]
#[tokio::test]
async fn e014_02_02_add_an_attribute_to_a_temporal_representation_of_an_entity_with_an_invalid_entity_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(
        &broker,
        &namespace,
        &VEHICLE_SPEED_FUEL_ADDED_TERMS,
        &vehicle_speed_fuel_level_with_dataset(&namespace)?,
    )
    .await?;

    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let attributes = added_attribute_instances()?;
    let operation = AddTemporalAttributes::builder().id(&id).attributes(&attributes).build();
    let answer = send_to(&client, &operation, "temporal/entities/thisIsAninvalidId/attrs").await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

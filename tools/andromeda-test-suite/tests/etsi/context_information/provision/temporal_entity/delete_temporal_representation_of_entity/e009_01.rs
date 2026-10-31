//! A temporal representation of an entity with simple temporal properties is deleted, and it is no longer retrieved
//! (clause 5.6.16).

use crate::{
    temporal_operations::create_temporal_evolution,
    vehicle_evolution_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_speed_fuel_level_with_dataset},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Check that one can delete a temporal representation of an entity with simple temporal properties
/// Clause: 5.6.16
#[gtest]
#[tokio::test]
async fn e009_01_01_delete_a_temporal_representation_of_an_entity_with_simple_temporal_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(
        &broker,
        &namespace,
        &VEHICLE_SPEED_FUEL_TERMS,
        &vehicle_speed_fuel_level_with_dataset(&namespace)?,
    )
    .await?;

    let id = namespace.urn("Vehicle", 1)?;
    let deleted = broker.with_responses().temporal().delete(&id).send().await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))?;
    expect_that!(deleted?.body.len(), eq(0));

    let client = broker.with_context(vocabulary.context())?;
    let retrieved = client.with_responses().temporal().retrieve(&id).send().await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    Ok(())
}

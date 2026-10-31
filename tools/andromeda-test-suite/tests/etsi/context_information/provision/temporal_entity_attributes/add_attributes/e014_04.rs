//! Instances are not added to a temporal representation of an entity with a body that is invalid or empty (clause
//! 5.6.12).

use crate::{
    entity_operations::ld_json_body_client,
    temporal_operations::create_temporal_evolution,
    unchecked_requests::{post_empty, with_doubled_separator},
    vehicle_evolution_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_speed_fuel_level_on, vehicle_speed_fuel_level_with_dataset},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::CreateOrUpdateTemporalEvolution,
    model::{APPLICATION_LD_JSON, ErrorType},
};

/// Pins: Check that an error is raised if one adds a temporal entity attribute with empty/invalid content
/// Clause: 5.6.12
#[gtest]
#[tokio::test]
async fn e014_04_01_add_an_attribute_to_a_temporal_representation_of_an_entity_with_invalid_content() -> Result<()> {
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
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let evolution = vehicle_speed_fuel_level_on(&namespace, "8313926113148778", "2020-09-01")?;
    let operation = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    let request = client.unchecked(&operation)?.uri(api_uri(&client, &format!("temporal/entities/{id}/attrs"))?);
    let answer = client.send_unchecked(with_doubled_separator(request)).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that an error is raised if one adds a temporal entity attribute with empty/invalid content
/// Clause: 5.6.12
#[gtest]
#[tokio::test]
async fn e014_04_02_add_an_attribute_to_a_temporal_representation_of_an_entity_with_empty_content() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_temporal_evolution(
        &broker,
        &namespace,
        &VEHICLE_SPEED_FUEL_TERMS,
        &vehicle_speed_fuel_level_with_dataset(&namespace)?,
    )
    .await?;

    let id = namespace.urn("Vehicle", 1)?;
    let path = format!("temporal/entities/{id}/attrs");
    let answer = broker.send_unchecked(post_empty(&broker, &path, &APPLICATION_LD_JSON)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

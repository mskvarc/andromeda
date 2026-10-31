//! The temporal evolution of an entity is not retrieved when `pick` names no member the entity holds, or when `omit`
//! names every member it holds (clauses 5.7.3 and 4.21).

use crate::{
    temporal_operations::create_temporal_evolution,
    vehicle_evolution_payloads::vehicle_speed_fuel_level,
    vehicle_fixtures::create_vehicle_speed_fuel_level,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{ErrorType, NgsiLdMediaType},
    query::projection::{self, EntityMember},
};

/// Pins: Check that a `ResourceNotFound` error is returned if pick does not match any entity member
/// Clause: 5.7.3, 4.21
#[gtest]
#[tokio::test]
async fn e020_23_01_retrieve_with_unmatched_pick() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(
        &broker,
        &namespace,
        &["Vehicle", "speed", "fuelLevel", "unknown"],
        &vehicle_speed_fuel_level(&namespace)?,
    )
    .await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .temporal()
        .retrieve_projected(&id)
        .pick(projection::attributes(["unknown"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that a `ResourceNotFound` error is returned if omit contains all entity members
/// Clause: 5.7.3, 4.21
#[gtest]
#[tokio::test]
async fn e020_23_02_retrieve_with_full_excluding_omit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let omit = projection::member(EntityMember::Id)
        .member(EntityMember::Type)
        .attribute("fuelLevel")
        .attribute("speed")
        .build()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .temporal()
        .retrieve_projected(&id)
        .omit(omit)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

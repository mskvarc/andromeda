//! ETSI `020_22`: the temporal evolution of an entity is not retrieved when `pick` and `omit` name the same member,
//! or when `attrs` is sent beside `pick` or `omit` (clauses 5.7.3 and 4.21).

use crate::suite_keywords::create_vehicle_speed_fuel_level;
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{ErrorType, NgsiLdMediaType},
    query::projection,
};

/// Pins: Check that a `BadRequestDataException` is returned if an entity member is present in pick and omit
/// Clause: 5.7.3, 4.21
/// Defect: D-80: Andromeda admits a member named in both pick and omit instead of refusing it with `BadRequestData`;
/// GET temporal/entities/{id}?pick=speed&omit=speed answers 404 `ResourceNotFound`
#[gtest]
#[tokio::test]
async fn e020_22_01_retrieve_with_same_entity_member_in_pick_and_omit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .temporal()
        .retrieve_projected(&id)
        .pick(projection::attributes(["speed"]).build()?)
        .omit(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if pick and attrs query params are present
/// Clause: 5.7.3, 4.21
#[gtest]
#[tokio::test]
async fn e020_22_02_retrieve_with_pick_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .temporal()
        .retrieve_projected(&id)
        .attrs(vec!["fuelLevel".parse()?])
        .pick(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that a `BadRequestDataException` is returned if omit and attrs query params are present
/// Clause: 5.7.3, 4.21
/// Changed: the suite names this case `02_22_03`, a typo for `020_22_03`; the file name uses `020-22-03`
#[gtest]
#[tokio::test]
async fn e020_22_03_retrieve_with_omit_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .temporal()
        .retrieve_projected(&id)
        .attrs(vec!["fuelLevel".parse()?])
        .omit(projection::attributes(["speed"]).build()?)
        .send()
        .await;
    expect_that!(&received, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

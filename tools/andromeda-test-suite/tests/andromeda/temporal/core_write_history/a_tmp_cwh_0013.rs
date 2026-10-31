//! Batch create gives its entities no temporal evolution.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Batch Entity Creation of two Entities answers 201 and current state holds them, while Retrieve Temporal
/// Evolution answers 404 for each
/// Clause: 4.3.1, 5.6.7, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0013_batch_create_gives_its_entities_no_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let entities = [
        Entity::builder()
            .id(sensor_id.clone())
            .entity_type(sensor_type.clone())
            .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(sensor_2_id.clone())
            .entity_type(sensor_type.clone())
            .property("v", Property::builder(2).observed_at("2026-07-01T00:00:00Z".parse()?))
            .build()?,
    ];
    broker_not_recorded.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_2_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("v", any_attribute().value(2)));
    let answer = broker_not_recorded
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker_not_recorded
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_2_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}

//! An entity created through the temporal api keeps its current state.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Temporal Evolution of an Entity created through Create Temporal Evolution with temperature 1
/// observed at 2026-01-01T00:00 and 2 at 01:00 answers 204; the Temporal Evolution is then 404 `ResourceNotFound`
/// and the Entity still holds temperature 2
/// Clause: 5.6.16, 5.6.11, 4.3.1
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_tmp_del_9003_an_entity_created_through_the_temporal_api_keeps_its_current_state() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_temporal_deletion_id = namespace.urn("Device", "temporal-deletion")?;
    let device_type = namespace.entity_type("Device")?;
    let device_temporal_deletion_evolution = TemporalEntity::builder()
        .id(device_temporal_deletion_id.clone())
        .entity_type(device_type)
        .property("temperature", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .property("temperature", Property::builder(2).observed_at("2026-01-01T01:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&device_temporal_deletion_evolution)
        .send()
        .await?
        .created()?;
    broker.temporal().delete(&device_temporal_deletion_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_temporal_deletion_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_temporal_deletion_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(2)));
    Ok(())
}

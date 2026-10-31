//! An entity created through the temporal api leaves its temporal evolution.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::{Done, Upserted},
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Entity of an Entity created through Create Temporal Evolution with temperature 1 observed at
/// 2026-01-01T00:00 and 2 at 01:00 answers 204; the Entity is then 404 `ResourceNotFound` and its Temporal
/// Evolution still holds both instances
/// Clause: 5.6.6, 5.6.11, 4.3.1
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_ent_del_9006_an_entity_created_through_the_temporal_api_leaves_its_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_core_deletion_id = namespace.urn("Device", "core-deletion")?;
    let device_type = namespace.entity_type("Device")?;
    let device_core_deletion_evolution = TemporalEntity::builder()
        .id(device_core_deletion_id.clone())
        .entity_type(device_type)
        .property("temperature", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .property("temperature", Property::builder(2).observed_at("2026-01-01T01:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&device_core_deletion_evolution).send().await?.created()?;
    broker.entities().delete(&device_core_deletion_id).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_core_deletion_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_core_deletion_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "temperature",
            all!(
                contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).value(1)),
                contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                contains(any_attribute().observed_at("2026-01-01T01:00:00Z".parse()?).value(2)),
                contains(any_attribute().observed_at("2026-01-01T01:00:00Z".parse()?)).times(eq(1))
            )
        )
    );
    Ok(())
}

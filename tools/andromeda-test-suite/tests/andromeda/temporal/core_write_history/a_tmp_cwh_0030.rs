//! Deferred temporal api writes land and core writes do not.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    eventually::retried,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: on the deferred broker, the seeded v instances land in the Temporal Evolution, and a Batch Entity Upsert and
/// an Update Attributes of v answer 204 and change current state but land no instance
/// Clause: 4.3.1, 5.6.2, 5.6.8, 5.6.11, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded and `append_mode` deferred, which lands the Temporal API's history after the write answers
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0030_deferred_temporal_api_writes_land_and_core_writes_do_not() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_deferred = broker::client(BrokerRole::Deferred)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_deferred.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    retried(async || {
        let retrieved = broker_deferred
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve(&sensor_id)
            .send()
            .await?;
        let evolution = retrieved.body();
        verify_that!(evolution, has_evolution("v", len(eq(2))))?;
        Ok(())
    })
    .await?;
    let entities = [Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?];
    broker_deferred
        .entities()
        .batch_upsert(&entities)
        .mode(UpsertMode::Update)
        .send()
        .await?
        .done()?;
    let fragment = EntityFragment::builder()
        .property("v", Property::builder(5).observed_at("2026-07-01T00:02:00Z".parse()?))
        .build()?;
    broker_deferred.entities().update_attributes(&sensor_id, &fragment).send().await?.done()?;
    let marker = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("marker", Property::builder(1).observed_at("2026-07-01T00:59:00Z".parse()?))
        .build()?
        .attributes;
    broker_deferred.temporal().add_attributes(&sensor_id, &marker).send().await?;
    // The drain lands queued history in order, so once the marker written after the Core writes has landed, anything
    // those writes had queued would have landed too.
    retried(async || {
        let retrieved = broker_deferred
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve(&sensor_id)
            .send()
            .await?;
        let evolution = retrieved.body();
        verify_that!(
            evolution,
            has_evolution("marker", contains(any_attribute().value(1).with_member(Member::InstanceId)))
        )?;
        Ok(())
    })
    .await?;
    let retrieved = broker_deferred
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "v",
            all!(
                len(eq(2)),
                contains(
                    any_attribute()
                        .value(1)
                        .observed_at("2026-07-01T00:00:00Z".parse()?)
                        .with_member(Member::InstanceId)
                ),
                contains(
                    any_attribute()
                        .value(2)
                        .observed_at("2026-07-01T00:01:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    let retrieved_2 = broker_deferred
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let entity = retrieved_2.body();
    expect_that!(entity, has_attribute("v", any_attribute().value(5)));
    Ok(())
}

//! Replacing an entity records neither its instances nor the attributes it drops.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    entity_matcher::{has_attribute, lacks_attribute},
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Replace Entity of a seeded Entity with one holding only u answers 204 and current state holds u and no v,
/// while the Temporal Evolution holds only the seeded v instances and no u
/// Clause: 4.3.1, 5.6.18, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0003_replacing_an_entity_records_neither_its_instances_nor_the_attributes_it_drops() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    let sensor_replacement = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("u", Property::builder(9).observed_at("2026-07-01T00:05:00Z".parse()?))
        .build()?;
    broker_not_recorded.entities().replace(&sensor_replacement).send().await?.done()?;
    let retrieved = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("v"));
    expect_that!(entity, has_attribute("u", any_attribute().value(9)));
    let retrieved_2 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
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
    expect_that!(evolution, lacks_evolution("u"));
    Ok(())
}

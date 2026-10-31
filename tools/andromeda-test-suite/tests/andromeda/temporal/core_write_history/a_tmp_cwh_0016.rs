//! Batch merge records nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityFragment, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Batch Entity Merge of a seeded Entity answers 204 and current state takes v, while the Temporal Evolution
/// holds only the seeded v instances
/// Clause: 4.3.1, 5.6.20, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0016_batch_merge_records_nothing() -> Result<()> {
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
    let fragments = [EntityFragment::for_entity(sensor_id.clone(), sensor_type.clone())
        .property("v", Property::builder(5).observed_at("2026-07-01T00:02:00Z".parse()?))
        .build()?];
    broker_not_recorded.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("v", any_attribute().value(5)));
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
    Ok(())
}

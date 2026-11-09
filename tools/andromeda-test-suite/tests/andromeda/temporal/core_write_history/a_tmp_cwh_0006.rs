//! An expired entity leaves its temporal evolution as it was.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    config::BrokerRole,
    eventually::retried,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::{Done, Upserted},
    problem::answered,
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, EntityFragment, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: a seeded Entity merged with an `expiresAt` three seconds ahead is removed from current state by the expiry
/// sweep, while its Temporal Evolution holds only the seeded v instances and no deletion
/// Clause: 4.3.1, 4.22, 5.6.17, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded; 4.22 leaves the lag of the clean-up open, and the file waits up to 30 seconds for it
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0006_an_expired_entity_leaves_its_temporal_evolution_as_it_was() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type)
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    // Read after the create, which a loaded broker can take seconds over, so the expiry is still ahead of the merge.
    let in_3_seconds: DateTime = (Utc::now() + TimeDelta::seconds(3)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let fragment = EntityFragment::builder().expires_at(in_3_seconds).build()?;
    broker_not_recorded.entities().merge(&sensor_id, &fragment).send().await?.done()?;
    retried(async || {
        let answer = broker_not_recorded
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&sensor_id)
            .send()
            .await;
        verify_that!(&answer, answered(StatusCode::NOT_FOUND))?;
        Ok(())
    })
    .await?;
    let retrieved = broker_not_recorded
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
    Ok(())
}

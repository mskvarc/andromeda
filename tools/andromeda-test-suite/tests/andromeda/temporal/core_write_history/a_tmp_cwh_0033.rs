//! A respond async batch update is left out of history.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    outcome::Upserted,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchUpdate},
    model::{EntityFragment, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: on the not-recorded broker, a Batch Entity Update of v to 5 answered 204 with Preference-Applied:
/// respond-async changes current state, and the Temporal Evolution read straight after it, with no retry, holds
/// only the two seeded v instances
/// Clause: 4.3.1, 5.6.9, 5.6.11, 5.7.3
/// Recorded: Andromeda configuration and extension, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core
/// API write updates the Temporal Evolution ("should consider updating"); this broker runs with
/// `core_write_history` not-recorded, and Prefer: respond-async on batch operations (IETF RFC 7240 section 4.1)
/// queues the history a recorded write adds, which every temporal read answers at once
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0033_a_respond_async_batch_update_is_left_out_of_history() -> Result<()> {
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
    let request = respond_async(broker_not_recorded.unchecked(&BatchUpdate::builder().fragments(&fragments).build())?);
    let answer = broker_not_recorded.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::NO_CONTENT));
    // Asked at once, with no retry.
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
                contains(property().value(1).observed_at("2026-07-01T00:00:00Z".parse()?)),
                contains(property().value(2).observed_at("2026-07-01T00:01:00Z".parse()?))
            )
        )
    );
    instance_id_where(evolution, &"v".parse()?, property().value(1).observed_at("2026-07-01T00:00:00Z".parse()?))?;
    instance_id_where(evolution, &"v".parse()?, property().value(2).observed_at("2026-07-01T00:01:00Z".parse()?))?;
    let current = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&sensor_id)
        .send()
        .await?;
    expect_that!(current.body(), has_attribute("v", any_attribute().value(5)));
    Ok(())
}

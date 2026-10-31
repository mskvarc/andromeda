//! Unrecorded core writes and temporal writes still notify.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    body_pattern::BodyPattern,
    broker,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityFragment, NgsiLdMediaType, Property, Representation, evolution::TemporalEntity},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription watching v is notified of an unrecorded Update Attributes with v 5 and of a newer Temporal
/// API instance with v 8, and the Temporal Evolution holds the seeded instances and the Temporal API one
/// Clause: 4.3.1, 5.6.2, 5.6.12, 5.8.1, 5.7.3
/// Recorded: Andromeda configuration, not v1.9.1: clause 4.3.1 leaves it to the broker whether a Core API write
/// updates the Temporal Evolution ("should consider updating"); this broker runs with `core_write_history`
/// not-recorded
#[gtest]
#[tokio::test]
async fn a_tmp_cwh_0032_unrecorded_core_writes_and_temporal_writes_still_notify() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let subscription_id = namespace.subscription_id(1)?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(1).observed_at("2026-07-01T00:00:00Z".parse()?))
        .property("v", Property::builder(2).observed_at("2026-07-01T00:01:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([sensor_type.as_str()])
        .watched_attributes(["v"])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker_not_recorded.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder()
        .property("v", Property::builder(5).observed_at("2026-07-01T00:02:00Z".parse()?))
        .build()?;
    broker_not_recorded.entities().update_attributes(&sensor_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Sensor:{namespace}-1' && @.v.value == 5)]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected, 1).await?;
    let instances = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("v", Property::builder(8).observed_at("2026-07-01T00:10:00Z".parse()?))
        .build()?
        .attributes;
    broker_not_recorded.temporal().add_attributes(&sensor_id, &instances).send().await?;
    let selected_2 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Sensor:{namespace}-1' && @.v.value == 8)]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected_2, 1).await?;
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
                len(eq(3)),
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
                ),
                contains(
                    any_attribute()
                        .value(8)
                        .observed_at("2026-07-01T00:10:00Z".parse()?)
                        .with_member(Member::InstanceId)
                )
            )
        )
    );
    Ok(())
}

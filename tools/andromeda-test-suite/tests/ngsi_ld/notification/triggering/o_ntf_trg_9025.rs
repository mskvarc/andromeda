//! A backfill older than the current state notifies nothing.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, evolution::TemporalEntity},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with the default triggers is notified of nothing for observations of v in 2019, older than
/// its current 2021 value, appended by Add Attributes and by Create Temporal Evolution
/// Clause: 5.8.6, 4.5.5.3, 5.6.11.4, 5.6.12.4
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9025_a_backfill_older_than_the_current_state_notifies_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let twn_backfill_id = namespace.urn("TwnBackfill", 1)?;
    let twn_backfill_type = namespace.entity_type("TwnBackfill")?;
    let subscription_twn_backfill_id = namespace.subscription_id("TwnBackfill")?;
    let twn_backfill = Entity::builder()
        .id(twn_backfill_id.clone())
        .entity_type(twn_backfill_type.clone())
        .property("v", Property::builder(5).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&twn_backfill).send().await?.done()?;
    let subscription_twn_backfill = Subscription::builder()
        .id(subscription_twn_backfill_id.clone())
        .entities([twn_backfill_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("TwnBackfill")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_twn_backfill).send().await?;
    let instances = TemporalEntity::builder()
        .id(twn_backfill_id.clone())
        .entity_type(twn_backfill_type.clone())
        .property("v", Property::builder(1).observed_at("2019-01-01T00:00:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&twn_backfill_id, &instances).send().await?;
    let twn_backfill_evolution = TemporalEntity::builder()
        .id(twn_backfill_id.clone())
        .entity_type(twn_backfill_type.clone())
        .property("v", Property::builder(2).observed_at("2019-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&twn_backfill_evolution).send().await?.updated()?;
    let fragment = EntityFragment::builder().property("fence", format!("fence-{namespace}")).build()?;
    broker.entities().append_attributes(&twn_backfill_id, &fragment).send().await?.done()?;
    // A notification owed before the fence write is delivered before the fence's, so once the fence's has arrived the journal holds every notification the earlier writes owed.
    let selected = [BodyPattern::Contains(format!("fence-{namespace}"))];
    sink.wait_for_at_least_where("TwnBackfill", &selected, 1).await?;
    let selected_2 = [BodyPattern::DoesNotContain(format!("fence-{namespace}"))];
    let notifications = sink.received_where("TwnBackfill", &selected_2).await?;
    expect_that!(notifications, len(eq(0)));
    broker.subscriptions().delete(&subscription_twn_backfill_id).send().await?;
    Ok(())
}

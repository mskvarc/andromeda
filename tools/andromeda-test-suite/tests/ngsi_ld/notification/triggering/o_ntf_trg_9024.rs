//! A temporal upsert adding an attribute notifies its creation.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A Subscription triggered by `attributeCreated` is notified once of Create Temporal Evolution of an
/// existing Entity with w observed 7 in 2022, which answers 204, carrying w 7
/// Clause: 5.8.1.4, 5.6.11.4
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9024_a_temporal_upsert_adding_an_attribute_notifies_its_creation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let twn_upsert_id = namespace.urn("TwnUpsert", 1)?;
    let twn_upsert_type = namespace.entity_type("TwnUpsert")?;
    let subscription_twn_upsert_id = namespace.subscription_id("TwnUpsert")?;
    let twn_upsert = Entity::builder()
        .id(twn_upsert_id.clone())
        .entity_type(twn_upsert_type.clone())
        .property("v", Property::builder(5).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&twn_upsert).send().await?.done()?;
    let subscription_twn_upsert = Subscription::builder()
        .id(subscription_twn_upsert_id.clone())
        .entities([twn_upsert_type.as_str()])
        .notification_triggers([NotificationTrigger::AttributeCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("TwnUpsert")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_twn_upsert).send().await?;
    let twn_upsert_evolution = TemporalEntity::builder()
        .id(twn_upsert_id.clone())
        .entity_type(twn_upsert_type.clone())
        .property("w", Property::builder(7).observed_at("2022-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&twn_upsert_evolution).send().await?.updated()?;
    let fragment = EntityFragment::builder().property("fence", format!("fence-{namespace}")).build()?;
    broker.entities().append_attributes(&twn_upsert_id, &fragment).send().await?.done()?;
    // A notification owed before the fence write is delivered before the fence's, so once the fence's has arrived the journal holds every notification the earlier writes owed.
    let selected = [BodyPattern::Contains(format!("fence-{namespace}"))];
    sink.wait_for_at_least_where("TwnUpsert", &selected, 1).await?;
    let selected_2 = [BodyPattern::DoesNotContain(format!("fence-{namespace}"))];
    let notifications = sink.received_where("TwnUpsert", &selected_2).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("w", any_attribute().value(7))));
    broker.subscriptions().delete(&subscription_twn_upsert_id).send().await?;
    Ok(())
}

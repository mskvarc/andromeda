//! A temporal create notifies the created entity once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityFragment, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with the default triggers is notified once of Create Temporal Evolution of an unknown
/// Entity whose v is observed 10, 50 and 20, carrying the Entity with v holding the most recent observation 50
/// Clause: 5.8.1.4, 5.8.6, 5.6.11.4, 4.5.5.3
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9021_a_temporal_create_notifies_the_created_entity_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_twn_create_id = namespace.subscription_id("TwnCreate")?;
    let twn_create_type = namespace.entity_type("TwnCreate")?;
    let twn_create_id = namespace.urn("TwnCreate", 1)?;
    let subscription_twn_create = Subscription::builder()
        .id(subscription_twn_create_id.clone())
        .entities([twn_create_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("TwnCreate")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_twn_create).send().await?;
    let twn_create_evolution = TemporalEntity::builder()
        .id(twn_create_id.clone())
        .entity_type(twn_create_type.clone())
        .property("v", Property::builder(10).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("v", Property::builder(50).observed_at("2020-01-03T00:00:00Z".parse()?))
        .property("v", Property::builder(20).observed_at("2020-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&twn_create_evolution).send().await?.created()?;
    let fragment = EntityFragment::builder().property("fence", format!("fence-{namespace}")).build()?;
    broker.entities().append_attributes(&twn_create_id, &fragment).send().await?.done()?;
    // A notification owed before the fence write is delivered before the fence's, so once the fence's has arrived the journal holds every notification the earlier writes owed.
    let selected = [BodyPattern::Contains(format!("fence-{namespace}"))];
    sink.wait_for_at_least_where("TwnCreate", &selected, 1).await?;
    let selected_2 = [BodyPattern::DoesNotContain(format!("fence-{namespace}"))];
    let notifications = sink.received_where("TwnCreate", &selected_2).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, all!(has_id(&twn_create_id), has_attribute("v", any_attribute().value(50))))
    );
    broker.subscriptions().delete(&subscription_twn_create_id).send().await?;
    Ok(())
}

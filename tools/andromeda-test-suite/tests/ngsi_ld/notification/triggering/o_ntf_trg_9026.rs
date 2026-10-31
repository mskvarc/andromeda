//! An unwatched temporal append notifies nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Attribute, AttributeName, Entity, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription watching w is notified of nothing for a newer observation of v appended to the Temporal
/// Evolution, and once for a newer observation of w, carrying w 3
/// Clause: 5.8.1.4, 5.6.12.4
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9026_an_unwatched_temporal_append_notifies_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("TwnWatched")?;
    let watched_type = namespace.entity_type("TwnWatched")?;
    let watched_id = namespace.urn("TwnWatched", 1)?;
    let watched = Entity::builder()
        .id(watched_id.clone())
        .entity_type(watched_type.clone())
        .property("v", Property::builder(5).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&watched).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([watched_type.as_str()])
        .watched_attributes(["w"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("TwnWatched")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let unwatched = TemporalEntity::builder()
        .id(watched_id.clone())
        .entity_type(watched_type.clone())
        .property("v", Property::builder(9).observed_at("2030-01-01T00:00:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&watched_id, &unwatched).send().await?;
    let watched_instance = TemporalEntity::builder()
        .id(watched_id.clone())
        .entity_type(watched_type)
        .property("w", Property::builder(3).observed_at("2030-01-01T00:00:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&watched_id, &watched_instance).send().await?;
    let fence = format!("fence-{namespace}");
    let patch = Attribute::from(Property::builder(fence.as_str()).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&watched_id, &"w".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    // A notification owed before the fence write is delivered before the fence's, so once the fence's has arrived the
    // journal holds every notification the earlier writes owed.
    sink.wait_for_at_least_where("TwnWatched", &[BodyPattern::Contains(fence.clone())], 1).await?;
    let before_fence = sink.received_where("TwnWatched", &[BodyPattern::DoesNotContain(fence)]).await?;
    expect_that!(before_fence, len(eq(1)));
    let notification = before_fence.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("w", any_attribute().value(3))));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

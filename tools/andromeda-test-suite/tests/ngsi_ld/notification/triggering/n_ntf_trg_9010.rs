//! Watched attributes gate a single notification.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription watching only b is notified once of the creation of an Entity with a, b and c, and not of
/// an update of a
/// Clause: 5.8.1.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9010_watched_attributes_gate_a_single_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_np_watched_id = namespace.subscription_id("np-watched")?;
    let np_watched_type = namespace.entity_type("NpWatched")?;
    let np_watched_id = namespace.urn("NpWatched", 1)?;
    let subscription_np_watched = Subscription::builder()
        .id(subscription_np_watched_id.clone())
        .entities([np_watched_type.as_str()])
        .watched_attributes(["b"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-watched")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_watched).send().await?;
    let np_watched = Entity::builder()
        .id(np_watched_id.clone())
        .entity_type(np_watched_type.clone())
        .property("a", 1)
        .property("b", 2)
        .property("c", 3)
        .build()?;
    broker.entities().create(&np_watched).send().await?.done()?;
    sink.wait_for_at_least("np-watched", 1).await?;
    let fragment = EntityFragment::builder().property("a", 10).build()?;
    broker.entities().update_attributes(&np_watched_id, &fragment).send().await?.done()?;
    // The update of the watched b is owed the next notification; notifications of one subscription are delivered in order, so a second notification of the write under test would be in the journal before it.
    let fragment_2 = EntityFragment::builder().property("b", "barrier").build()?;
    broker.entities().update_attributes(&np_watched_id, &fragment_2).send().await?.done()?;
    let selected = [BodyPattern::Contains("\"barrier\"".to_owned())];
    sink.wait_for_at_least_where("np-watched", &selected, 1).await?;
    let notifications = sink.received("np-watched").await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("b", any_attribute().value("barrier"))));
    let notifications_2 = sink.received("np-watched").await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&np_watched_id)));
    broker.subscriptions().delete(&subscription_np_watched_id).send().await?;
    Ok(())
}

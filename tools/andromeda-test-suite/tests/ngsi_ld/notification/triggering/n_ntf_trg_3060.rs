//! A subscription q with an unquoted uri containing dots notifies the entity it selects.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, Lossless, NgsiLdMediaType, Representation, Uri},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose q equals an unquoted URI containing dots notifies the entity whose Property value or
/// Relationship object is that URI, and not an entity holding another
/// Clause: 4.9, 5.8.6, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_trg_3060_a_subscription_q_with_an_unquoted_uri_containing_dots_notifies_the_entity_it_selects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let thing_type = namespace.entity_type("Thing")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let thing_other_id = namespace.urn("Thing", "other")?;
    let thing_id = namespace.urn("Thing", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([thing_type.as_str()])
        .q(q::attr("link").eq(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?).build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([thing_type.as_str()])
        .q(q::attr("ref")
            .eq(q::UnquotedUri::try_from("https://sub.example.org/dir.v2/target.json".parse::<Uri>()?)?)
            .build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    // an entity matching neither is created first: its creation must notify neither subscription, so the oldest notification of each is the matching entity's
    let thing_other = Entity::builder()
        .id(thing_other_id)
        .entity_type(thing_type.clone())
        .property("link", "http://example.org/other")
        .relationship("ref", "http://example.org/other".parse::<EntityId>()?)
        .build()?;
    broker.entities().create(&thing_other).send().await?.done()?;
    let thing = Entity::builder()
        .id(thing_id.clone())
        .entity_type(thing_type.clone())
        .property("link", "http://example.org/a")
        .relationship("ref", "https://sub.example.org/dir.v2/target.json".parse::<EntityId>()?)
        .build()?;
    broker.entities().create(&thing).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    sink.wait_for_at_least(2, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&thing_id)));
    let notifications_2 = sink.received(2).await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&thing_id)));
    Ok(())
}

//! A create of three attributes notifies once.

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

/// Pins: A Subscription with the default triggers and no `watchedAttributes` is notified once of the creation of an
/// Entity with three attributes, carrying all three
/// Clause: 5.8.6, Table 5.2.12-1
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9004_a_create_of_three_attributes_notifies_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_np_create_id = namespace.subscription_id("np-create")?;
    let np_create_type = namespace.entity_type("NpCreate")?;
    let np_create_id = namespace.urn("NpCreate", 1)?;
    let subscription_np_create = Subscription::builder()
        .id(subscription_np_create_id.clone())
        .entities([np_create_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-create")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_create).send().await?;
    let np_create = Entity::builder()
        .id(np_create_id.clone())
        .entity_type(np_create_type.clone())
        .property("a", 1)
        .property("b", 2)
        .property("c", 3)
        .build()?;
    broker.entities().create(&np_create).send().await?.done()?;
    // The barrier update is owed the next notification; notifications of one subscription are delivered in order, so a second notification of the write under test would be in the journal before it.
    let fragment = EntityFragment::builder().property("a", "barrier").build()?;
    broker.entities().update_attributes(&np_create_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains("\"barrier\"".to_owned())];
    sink.wait_for_at_least_where("np-create", &selected, 1).await?;
    let notifications = sink.received("np-create").await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("a", any_attribute().value("barrier"))));
    let notifications_2 = sink.received("np-create").await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(
        notification_2.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&np_create_id),
                has_attribute("a", any_attribute().value(1)),
                has_attribute("b", any_attribute().value(2)),
                has_attribute("c", any_attribute().value(3))
            )
        )
    );
    broker.subscriptions().delete(&subscription_np_create_id).send().await?;
    Ok(())
}

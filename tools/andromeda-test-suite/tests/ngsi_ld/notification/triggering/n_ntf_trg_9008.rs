//! A merge of two attributes notifies once.

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

/// Pins: A Merge Entity updating one attribute and appending another notifies a Subscription to the Entity type
/// once
/// Clause: 5.6.17.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9008_a_merge_of_two_attributes_notifies_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let np_merge_id = namespace.urn("NpMerge", 1)?;
    let np_merge_type = namespace.entity_type("NpMerge")?;
    let subscription_np_merge_id = namespace.subscription_id("np-merge")?;
    let np_merge = Entity::builder()
        .id(np_merge_id.clone())
        .entity_type(np_merge_type.clone())
        .property("a", 1)
        .property("b", 2)
        .property("c", 3)
        .build()?;
    broker.entities().create(&np_merge).send().await?.done()?;
    let subscription_np_merge = Subscription::builder()
        .id(subscription_np_merge_id.clone())
        .entities([np_merge_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-merge")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_merge).send().await?;
    let fragment = EntityFragment::builder().property("a", 10).property("d", 4).build()?;
    broker.entities().merge(&np_merge_id, &fragment).send().await?.done()?;
    // The barrier update is owed the next notification; notifications of one subscription are delivered in order, so a second notification of the write under test would be in the journal before it.
    let fragment_2 = EntityFragment::builder().property("a", "barrier").build()?;
    broker.entities().update_attributes(&np_merge_id, &fragment_2).send().await?.done()?;
    let selected = [BodyPattern::Contains("\"barrier\"".to_owned())];
    sink.wait_for_at_least_where("np-merge", &selected, 1).await?;
    let notifications = sink.received("np-merge").await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("a", any_attribute().value("barrier"))));
    let notifications_2 = sink.received("np-merge").await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(
        notification_2.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&np_merge_id),
                has_attribute("a", any_attribute().value(10)),
                has_attribute("d", any_attribute().value(4))
            )
        )
    );
    broker.subscriptions().delete(&subscription_np_merge_id).send().await?;
    Ok(())
}

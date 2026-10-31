//! A batch upsert create and update each notify once.

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
    http::operation::UpsertMode,
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Batch Entity Upsert that creates an Entity, and one in update mode that changes two of its attributes,
/// each notify a Subscription to its type once, the second carrying both new values
/// Clause: 5.6.8.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9007_a_batch_upsert_create_and_update_each_notify_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_np_upsert_id = namespace.subscription_id("np-upsert")?;
    let np_upsert_type = namespace.entity_type("NpUpsert")?;
    let np_upsert_id = namespace.urn("NpUpsert", 1)?;
    let subscription_np_upsert = Subscription::builder()
        .id(subscription_np_upsert_id.clone())
        .entities([np_upsert_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-upsert")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_upsert).send().await?;
    let entities = [Entity::builder()
        .id(np_upsert_id.clone())
        .entity_type(np_upsert_type.clone())
        .property("a", 1)
        .property("b", 2)
        .build()?];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    sink.wait_for_at_least("np-upsert", 1).await?;
    // The update is owed the next notification; notifications of one subscription are delivered in order, so a second notification of the write under test would be in the journal before it.
    let entities_2 = [Entity::builder()
        .id(np_upsert_id.clone())
        .entity_type(np_upsert_type.clone())
        .property("a", 10)
        .property("b", 20)
        .build()?];
    broker.entities().batch_upsert(&entities_2).mode(UpsertMode::Update).send().await?.done()?;
    sink.wait_for_at_least("np-upsert", 2).await?;
    let notifications = sink.received("np-upsert").await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("a", any_attribute().value(1))));
    let notification_2 = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification_2.data().as_slice(),
        nth(
            0,
            all!(has_attribute("a", any_attribute().value(10)), has_attribute("b", any_attribute().value(20)))
        )
    );
    // The barrier update is owed the next notification; notifications of one subscription are delivered in order, so a second notification of the write under test would be in the journal before it.
    let fragment = EntityFragment::builder().property("a", "barrier").build()?;
    broker.entities().update_attributes(&np_upsert_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains("\"barrier\"".to_owned())];
    sink.wait_for_at_least_where("np-upsert", &selected, 1).await?;
    let notifications_2 = sink.received("np-upsert").await?;
    expect_that!(notifications_2, len(eq(3)));
    let notification_3 = notifications_2.get(2).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_3.data().as_slice(), nth(0, has_attribute("a", any_attribute().value("barrier"))));
    broker.subscriptions().delete(&subscription_np_upsert_id).send().await?;
    Ok(())
}

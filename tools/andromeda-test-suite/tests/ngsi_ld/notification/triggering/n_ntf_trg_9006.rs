//! A batch create notifies once per entity.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Batch Entity Creation of two Entities with two attributes each notifies a Subscription to their type
/// twice, once per Entity
/// Clause: 5.5.11.0, 5.6.7.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9006_a_batch_create_notifies_once_per_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_np_batch_id = namespace.subscription_id("np-batch")?;
    let np_batch_type = namespace.entity_type("NpBatch")?;
    let np_batch_id = namespace.urn("NpBatch", 1)?;
    let np_batch_2_id = namespace.urn("NpBatch", 2)?;
    let subscription_np_batch = Subscription::builder()
        .id(subscription_np_batch_id.clone())
        .entities([np_batch_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-batch")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_batch).send().await?;
    let entities = [
        Entity::builder()
            .id(np_batch_id.clone())
            .entity_type(np_batch_type.clone())
            .property("a", 1)
            .property("b", 2)
            .build()?,
        Entity::builder()
            .id(np_batch_2_id.clone())
            .entity_type(np_batch_type.clone())
            .property("a", 1)
            .property("b", 2)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let fragment = EntityFragment::builder().property("a", "barrier").build()?;
    broker.entities().update_attributes(&np_batch_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder().property("a", "barrier").build()?;
    broker.entities().update_attributes(&np_batch_2_id, &fragment_2).send().await?.done()?;
    // Each barrier update is owed the next notification of its Entity; the notifications of one Entity are delivered in order, so a second notification of the write under test would be in the journal before its barrier.
    let selected = [BodyPattern::Contains("\"barrier\"".to_owned())];
    sink.wait_for_at_least_where("np-batch", &selected, 2).await?;
    let notifications = sink.received("np-batch").await?;
    expect_that!(notifications, len(eq(4)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), len(eq(1)));
    let notification_2 = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    let notification_3 = notifications.get(2).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_3.data().as_slice(), len(eq(1)));
    let notification_4 = notifications.get(3).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_4.data().as_slice(), len(eq(1)));
    let selected_2 = [
        BodyPattern::Contains(format!("\"urn:ngsi-ld:NpBatch:{namespace}-1\"")),
        BodyPattern::DoesNotContain("\"barrier\"".to_owned()),
    ];
    let notifications_2 = sink.received_where("np-batch", &selected_2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let selected_3 = [
        BodyPattern::Contains(format!("\"urn:ngsi-ld:NpBatch:{namespace}-2\"")),
        BodyPattern::DoesNotContain("\"barrier\"".to_owned()),
    ];
    let notifications_3 = sink.received_where("np-batch", &selected_3).await?;
    expect_that!(notifications_3, len(eq(1)));
    broker.subscriptions().delete(&subscription_np_batch_id).send().await?;
    Ok(())
}

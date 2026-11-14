//! A Subscription's q is read over the Entity state its trigger names.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{AttributeName, Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// The write a case sends to a Meter holding `n` 10 and `s` "abc".
#[derive(Clone, Copy)]
enum Write {
    /// Update `s`, leaving `n` as it is.
    UpdateS,
    /// Delete `n`.
    DeleteN,
    /// Delete the Meter.
    DeleteEntity,
}

/// Creates a Meter holding `n` 10 and `s` "abc", subscribes to Meters with q n==10, no watchedAttributes and
/// `trigger`, sends `write`, and expects one notification naming the Meter.
async fn expect_notified(trigger: NotificationTrigger, write: Write) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_id: EntityId = namespace.urn("Meter", 1)?;
    let meter = Entity::builder()
        .id(meter_id.clone())
        .entity_type(meter_type.clone())
        .property("n", 10)
        .property("s", "abc")
        .build()?;
    broker.entities().create(&meter).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("state")?)
        .entities([meter_type.as_str()])
        .q("n==10".parse()?)
        .notification_triggers([trigger])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(0)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    match write {
        Write::UpdateS => {
            let fragment = EntityFragment::builder().property("s", "abd").build()?;
            broker.entities().update_attributes(&meter_id, &fragment).send().await?.done()?;
        }
        Write::DeleteN => {
            broker
                .entities()
                .delete_attribute(&meter_id, &"n".parse::<AttributeName>()?)
                .send()
                .await?
                .done()?;
        }
        Write::DeleteEntity => broker.entities().delete(&meter_id).send().await?.done()?,
    }
    let notifications = sink.wait_for_at_least(0, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&meter_id)));
    Ok(())
}

/// Pins: a Subscription to Meters with q n==10, no watchedAttributes and the trigger attributeUpdated is notified of
/// an update of `s` on a Meter holding `n` 10; the q is met by the Entity the write leaves, whichever Attribute the
/// write changed
/// Clause: 5.8.6, 4.9
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9039_an_update_of_another_attribute_notifies_when_the_state_meets_q() -> Result<()> {
    expect_notified(NotificationTrigger::AttributeUpdated, Write::UpdateS).await
}

/// Pins: a Subscription to Meters with q n==10 and the trigger attributeDeleted is notified when `n` is deleted from
/// a Meter holding `n` 10; the q of a deletion is met by the state the write removed the Attribute from
/// Clause: 5.8.6, 4.9
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9040_deleting_the_attribute_q_reads_notifies_through_the_state_before_the_write() -> Result<()> {
    expect_notified(NotificationTrigger::AttributeDeleted, Write::DeleteN).await
}

/// Pins: a Subscription to Meters with q n==10 and the trigger entityDeleted is notified when a Meter holding `n` 10
/// is deleted; the q of an Entity deletion is met by the state the deletion removed
/// Clause: 5.8.6, 4.9
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9041_deleting_the_entity_notifies_through_the_state_the_deletion_removed() -> Result<()> {
    expect_notified(NotificationTrigger::EntityDeleted, Write::DeleteEntity).await
}

//! An entity deletion notifies once.

use andromeda_test_suite::{
    broker,
    entity_matcher::lacks_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Deleting an Entity notifies a Subscription listening to `entityDeleted` and `attributeDeleted` once, with
/// the Entity carrying `deletedAt` and none of its attributes
/// Clause: 5.6.6.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9012_an_entity_deletion_notifies_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let np_delete_id = namespace.urn("NpDelete", 1)?;
    let np_delete_type = namespace.entity_type("NpDelete")?;
    let subscription_np_delete_id = namespace.subscription_id("np-delete")?;
    let np_delete = Entity::builder()
        .id(np_delete_id.clone())
        .entity_type(np_delete_type.clone())
        .property("a", 1)
        .property("b", 2)
        .property("c", 3)
        .build()?;
    broker.entities().create(&np_delete).send().await?.done()?;
    let subscription_np_delete = Subscription::builder()
        .id(subscription_np_delete_id.clone())
        .entities([np_delete_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityDeleted, NotificationTrigger::AttributeDeleted])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-delete")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_delete).send().await?;
    broker.entities().delete(&np_delete_id).send().await?.done()?;
    let np_delete_2 = Entity::builder()
        .id(np_delete_id.clone())
        .entity_type(np_delete_type.clone())
        .property("a", 1)
        .property("b", 2)
        .property("c", 3)
        .build()?;
    broker.entities().create(&np_delete_2).send().await?.done()?;
    // The deletion of the recreated Entity is owed the next notification of that Entity; the notifications of one Entity are delivered in order, so a second notification of the write under test would be in the journal before it.
    broker.entities().delete(&np_delete_id).send().await?.done()?;
    sink.wait_for_at_least("np-delete", 2).await?;
    let notifications = sink.received("np-delete").await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), len(eq(1)));
    let item = notification.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::DeletedAt).ok(), some(anything()));
    expect_that!(notification.data().as_slice(), nth(0, all!(has_id(&np_delete_id), lacks_attribute("a"))));
    let notification_2 = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&np_delete_id)));
    broker.subscriptions().delete(&subscription_np_delete_id).send().await?;
    Ok(())
}

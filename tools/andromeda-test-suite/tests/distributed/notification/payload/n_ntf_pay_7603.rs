//! A deletion is notified with deleted at.

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
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Delete Entity on broker A notifies a subscription created on broker A with `notificationTrigger`
/// `entityDeleted` with the Entity carrying `deletedAt` and no attribute
/// Clause: 5.8.6, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_pay_7603_a_deletion_is_notified_with_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker.entities().delete(&vehicle_id).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    let item = notification.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::DeletedAt).ok(), some(anything()));
    expect_that!(notification.data().as_slice(), nth(0, all!(has_id(&vehicle_id), lacks_attribute("speed"))));
    Ok(())
}

//! Sys attrs adds created at and modified at.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SystemAttributeReporting},
};

/// Pins: A notification of a creation on broker A to a subscription created on broker A with `sysAttrs` true
/// carries `createdAt` and `modifiedAt` on the Entity
/// Clause: 5.8.6, 5.2.14, 4.8
#[gtest]
#[tokio::test]
async fn n_ntf_pay_7604_sys_attrs_adds_created_at_and_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .system_attributes(SystemAttributeReporting::Include)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    let item = notification.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(system_time::of_entity(item, SystemMember::ModifiedAt).ok(), some(anything()));
    Ok(())
}

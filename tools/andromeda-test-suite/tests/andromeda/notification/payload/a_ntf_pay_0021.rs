//! A durable entity deletion notification carries the removed entity.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    entity_matcher::lacks_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    system_time::{self, SystemMember},
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription, SystemAttributeReporting},
};

/// Pins: under durable delivery an Entity deletion notified with `sysAttrs` carries the removed Entity's id and
/// type with `deletedAt`, `createdAt` and `modifiedAt`, and none of its attributes
/// Clause: 5.8.6, 4.8
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where the queued event stores the removed state once
#[gtest]
#[tokio::test]
async fn a_ntf_pay_0021_a_durable_entity_deletion_notification_carries_the_removed_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let ngsi_ld_building_id = format!("urn:ngsi-ld:Building:{namespace}").parse::<EntityId>()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .system_attributes(SystemAttributeReporting::Include)
                .build()?,
        )
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let entity = Entity::builder()
        .id(format!("urn:ngsi-ld:Building:{namespace}").parse::<EntityId>()?)
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    broker_durable.entities().create(&entity).send().await?.done()?;
    broker_durable.entities().delete(&ngsi_ld_building_id).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    let item = notification.data().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::DeletedAt).ok(), some(anything()));
    expect_that!(system_time::of_entity(item, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(system_time::of_entity(item, SystemMember::ModifiedAt).ok(), some(anything()));
    expect_that!(
        notification.data().as_slice(),
        nth(0, all!(has_id(&ngsi_ld_building_id), lacks_attribute("name"), has_type(&building_type)))
    );
    Ok(())
}

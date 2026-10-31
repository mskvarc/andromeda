//! An entity deletion with a flat join carries only the deleted entity.

use andromeda_test_suite::{
    broker,
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
    query::Join,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A notification of an Entity deletion on a subscription with join flat carries the deleted Entity alone,
/// with `deletedAt`, and no Linked Entity
/// Clause: 5.8.6, 4.5.23.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0018_an_entity_deletion_with_a_flat_join_carries_only_the_deleted_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let city = Entity::builder().id(city_id.clone()).entity_type(city_type).build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .scope("/Paris/Tourism")
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .relationship("locatedAt", city_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityDeleted])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .join_level(1)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    broker.entities().delete(&building_id).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    let item = notification.data().as_slice().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::DeletedAt).ok(), some(anything()));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_id)));
    Ok(())
}

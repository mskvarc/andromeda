//! Listed attributes keep the entity expiry.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, spelling_matcher::alone};
use googletest::prelude::*;
use ngsi_ld::{
    model::{DateTime, Entity, EntityFragment, Lossless, NgsiLdMediaType, Scope},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Of one append, a subscription listing the attribute name and one listing none are notified of the Entity
/// with the same `expiresAt`, and only the one listing none carries scope
/// Clause: 5.8.6, 5.2.14, 4.22
/// Recorded: clause 4.22 makes the `expiresAt` of an Entity a system attribute and Table 5.2.14-1 has `sysAttrs`
/// govern only `createdAt` and `modifiedAt`; Andromeda notifies the `expiresAt` of the Entity whatever `sysAttrs`
/// says, and notification.attributes, a pick of Attributes, leaves it as it is
#[gtest]
#[tokio::test]
async fn o_ntf_pay_0017_listed_attributes_keep_the_entity_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
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
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .attributes(["name"])
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let fragment = EntityFragment::builder().property("name", "Tour Eiffel").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    let item = notification.data().as_slice().first().or_fail()?;
    expect_that!(item.expires_at, some(eq("2099-01-01T00:00:00Z".parse::<DateTime>()?)));
    expect_that!(item.scope.as_ref(), none());
    sink.wait_for_at_least(2, 1).await?;
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.subscription_id(), eq(&subscription_2_id));
    let item_2 = notification_2.data().as_slice().first().or_fail()?;
    expect_that!(item_2.expires_at, some(eq("2099-01-01T00:00:00Z".parse::<DateTime>()?)));
    expect_that!(item_2.scope.as_ref(), some(alone(&"/Paris/Tourism".parse::<Scope>()?)));
    Ok(())
}

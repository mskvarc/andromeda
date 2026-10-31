//! A periodic notification with omit type carries no type.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::projection::Projection,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A periodic notification of a subscription whose notification omit names type carries the Entity with its
/// id and Attributes and without type
/// Clause: 5.8.6, 4.21, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0009_a_periodic_notification_with_omit_type_carries_no_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city = Entity::builder().id(city_id.clone()).entity_type(city_type).build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .relationship("locatedAt", city_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .time_interval(2)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .omit("type".parse::<Projection>()?)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.projected_entities(Lossless::Normalized)?;
    let entity = notification.data().first().or_fail()?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(entity.id, some(eq(&building_id)));
    expect_that!(entity.entity_type, none());
    expect_that!(entity, has_attribute("name", any_attribute()));
    Ok(())
}

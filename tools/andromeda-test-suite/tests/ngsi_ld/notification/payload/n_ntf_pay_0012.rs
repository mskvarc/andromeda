//! A flat join reaches exactly join level hops and one by default.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    query::Join,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Over a Building linked to a City, the City to a Country and the Country to a Continent, a notification
/// with join flat carries the Building and the City for `joinLevel` 1, the Country too for `joinLevel` 2, the
/// Continent too for `joinLevel` 3, and the Building and the City when `joinLevel` is absent
/// Clause: 5.8.6, 4.5.23.3, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0012_a_flat_join_reaches_exactly_join_level_hops_and_one_by_default() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let continent_id = namespace.urn("Continent", 1)?;
    let continent_type = namespace.entity_type("Continent")?;
    let country_id = namespace.urn("Country", 1)?;
    let country_type = namespace.entity_type("Country")?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription_4_id = namespace.subscription_id(4)?;
    let continent = Entity::builder()
        .id(continent_id.clone())
        .entity_type(continent_type)
        .property("name", "Europe")
        .build()?;
    broker.entities().create(&continent).send().await?.done()?;
    let country = Entity::builder()
        .id(country_id.clone())
        .entity_type(country_type)
        .property("name", "Italy")
        .relationship("partOf", continent_id.clone())
        .build()?;
    broker.entities().create(&country).send().await?.done()?;
    let city = Entity::builder()
        .id(city_id.clone())
        .entity_type(city_type)
        .relationship("locatedIn", country_id.clone())
        .build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .relationship("locatedAt", city_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .join_level(1)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .join_level(2)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .join_level(3)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_3).send().await?;
    let subscription_4 = Subscription::builder()
        .id(subscription_4_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(4)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_4).send().await?;
    let fragment = EntityFragment::builder().property("name", "Updated").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(2)));
    expect_that!(notification.data().as_slice(), contains(has_id(&building_id)));
    expect_that!(notification.data().as_slice(), contains(has_id(&city_id)));
    sink.wait_for_at_least(2, 1).await?;
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.subscription_id(), eq(&subscription_2_id));
    expect_that!(notification_2.data().as_slice(), len(eq(3)));
    expect_that!(notification_2.data().as_slice(), contains(has_id(&building_id)));
    expect_that!(notification_2.data().as_slice(), contains(has_id(&city_id)));
    expect_that!(notification_2.data().as_slice(), contains(has_id(&country_id)));
    sink.wait_for_at_least(3, 1).await?;
    let notifications_3 = sink.received(3).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notification_3 = notifications_3.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_3.subscription_id(), eq(&subscription_3_id));
    expect_that!(notification_3.data().as_slice(), len(eq(4)));
    expect_that!(notification_3.data().as_slice(), contains(has_id(&building_id)));
    expect_that!(notification_3.data().as_slice(), contains(has_id(&city_id)));
    expect_that!(notification_3.data().as_slice(), contains(has_id(&country_id)));
    expect_that!(notification_3.data().as_slice(), contains(has_id(&continent_id)));
    sink.wait_for_at_least(4, 1).await?;
    let notifications_4 = sink.received(4).await?;
    expect_that!(notifications_4, len(eq(1)));
    let notification_4 = notifications_4.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_4.subscription_id(), eq(&subscription_4_id));
    expect_that!(notification_4.data().as_slice(), len(eq(2)));
    expect_that!(notification_4.data().as_slice(), contains(has_id(&building_id)));
    expect_that!(notification_4.data().as_slice(), contains(has_id(&city_id)));
    Ok(())
}

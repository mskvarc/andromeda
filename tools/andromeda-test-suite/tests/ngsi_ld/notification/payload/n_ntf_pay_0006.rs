//! A periodic flat join carries the linked entity beside the notified one.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::Join,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A periodic notification of a subscription with join flat carries the notified Building and, beside it in
/// data, the City its Relationship names
/// Clause: 5.8.6, 4.5.23.3, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0006_a_periodic_flat_join_carries_the_linked_entity_beside_the_notified_one() -> Result<()> {
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
                .join(Join::Flat)
                .join_level(1)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(2)));
    expect_that!(notification.data().as_slice(), contains(has_id(&building_id)));
    expect_that!(notification.data().as_slice(), contains(has_id(&city_id)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

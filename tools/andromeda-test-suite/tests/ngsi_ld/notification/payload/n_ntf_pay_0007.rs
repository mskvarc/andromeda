//! A periodic inline join nests the linked entity.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::Join,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A periodic notification of a subscription with join inline carries the notified Building alone in data,
/// with the City nested under the Relationship that names it
/// Clause: 5.8.6, 4.5.23.2, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0007_a_periodic_inline_join_nests_the_linked_entity() -> Result<()> {
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
                .join(Join::Inline)
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
    expect_that!(notification.data().as_slice(), len(eq(1)));
    let item = notification.data().as_slice().first().or_fail()?;
    expect_that!(linked_entities(item, &["locatedAt"]), nth(0, has_id(&city_id)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_id)));
    Ok(())
}

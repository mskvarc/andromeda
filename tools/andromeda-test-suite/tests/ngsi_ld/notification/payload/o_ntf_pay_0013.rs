//! A flat join delivers the linked entities in breadth first order.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    query::Join,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Two notifications of a subscription with join flat and `joinLevel` 2 each carry the Building first, with
/// the value its append wrote, then the City it links to, then the Country the City links to
/// Clause: 5.8.6, 4.5.23.3
/// Recorded: clause 4.5.23.3 does not fix the order of the Entities a flat join returns; Andromeda delivers the
/// notified Entity first and the Linked Entities in the breadth-first order in which the traversal first reaches
/// them
#[gtest]
#[tokio::test]
async fn o_ntf_pay_0013_a_flat_join_delivers_the_linked_entities_in_breadth_first_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let country_id = namespace.urn("Country", 1)?;
    let country_type = namespace.entity_type("Country")?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let country = Entity::builder()
        .id(country_id.clone())
        .entity_type(country_type)
        .property("name", "Portugal")
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
                .join_level(2)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("name", "Updated once").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let fragment_2 = EntityFragment::builder().property("name", "Updated twice").build()?;
    broker.entities().append_attributes(&building_id, &fragment_2).send().await?.done()?;
    sink.wait_for_at_least(1, 2).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(3)));
    expect_that!(
        notification.data().as_slice(),
        nth(0, all!(has_id(&building_id), has_attribute("name", any_attribute().value("Updated once"))))
    );
    expect_that!(notification.data().as_slice(), nth(1, has_id(&city_id)));
    expect_that!(notification.data().as_slice(), nth(2, has_id(&country_id)));
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(2)));
    let notification_2 = notifications_2.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.subscription_id(), eq(&subscription_id));
    expect_that!(notification_2.data().as_slice(), len(eq(3)));
    expect_that!(
        notification_2.data().as_slice(),
        nth(0, all!(has_id(&building_id), has_attribute("name", any_attribute().value("Updated twice"))))
    );
    expect_that!(notification_2.data().as_slice(), nth(1, has_id(&city_id)));
    expect_that!(notification_2.data().as_slice(), nth(2, has_id(&country_id)));
    Ok(())
}

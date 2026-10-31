//! A near geo query with a three dimensional reference notifies horizontally.

use andromeda_test_suite::{
    attribute_matcher::geo_property,
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
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        Representation,
        geometry::{Position, Shape},
    },
    query::geoquery,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose `geoQ` is near within 100 m of a three-ordinate reference notifies an entity created
/// 500 m above that position, with its altitude, and not one created far away
/// Clause: 5.8.6, 5.2.13, 4.10
/// Recorded: clause 5.8.6 evaluates the `geoQ` of a subscription against an entity as clause 4.10 does a query,
/// which defers to OGC 06-103r4; clause 4.10 defers the geometric relationships to OGC 06-103r4, whose clause
/// 6.1.2.5 says spatial operations work in the "map geometry" of the data and "will therefore not reflect z or m
/// values in calculations" and whose clause 6.1.15.1 defines the relationships by the projection of the objects
/// onto the horizontal surface, so a georel that ignores the altitude is the specified behaviour; the user ruled
/// this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_ntf_trg_0004_a_near_geo_query_with_a_three_dimensional_reference_notifies_horizontally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let far_id = namespace.urn("Building", "far")?;
    let near_id = namespace.urn("Building", "near")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)).build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let far = Entity::builder()
        .id(far_id)
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_with_altitude(30.0, 10.0, 500.0)?))
        .build()?;
    broker.entities().create(&far).send().await?.done()?;
    let near = Entity::builder()
        .id(near_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 500.0)?))
        .build()?;
    broker.entities().create(&near).send().await?.done()?;
    // The oldest notification is the matching entity: the entity created first lies outside the geoQ and sent none.
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&near_id),
                has_attribute(
                    "location",
                    geo_property().geometry(Shape::Point(Position::try_with_altitude(13.4, 52.52, 500.0)?))
                )
            )
        )
    );
    Ok(())
}

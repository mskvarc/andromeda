//! Overlaps between a Point and a Polygon selects nothing in a query and a subscription alike.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The square from (0, 0) to (2, 2) every request of the test refers to.
fn reference() -> Result<Shape> {
    Ok(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
        Position::try_new(0.0, 0.0)?,
        Position::try_new(2.0, 0.0)?,
        Position::try_new(2.0, 2.0)?,
        Position::try_new(0.0, 2.0)?,
        Position::try_new(0.0, 0.0)?,
    ])?])?))
}

/// Pins: with georel overlaps and the reference square from (0, 0) to (2, 2), a Patch located at the Point (1, 1)
/// inside the square is neither returned by Query Entities nor notified by a Subscription, while a Patch located at
/// the square from (1, 1) to (3, 3) is returned and notified
/// Clause: 4.10, 5.7.2.4, 5.8.6
/// Recorded: clause 4.10 defines overlaps as OGC 06-103r4 does, for two geometries of the same dimension, so a Point
/// never overlaps a Polygon
#[gtest]
#[tokio::test]
async fn n_qry_geo_9094_overlaps_between_a_point_and_a_polygon_selects_nothing_in_a_query_and_a_subscription_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let patch_type = namespace.entity_type("Patch")?;
    let point_id = namespace.urn("Patch", "point")?;
    let square_id = namespace.urn("Patch", "square")?;
    let subscription_id = namespace.subscription_id("overlaps")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([patch_type.as_str()])
        .geo_q(geoquery::overlaps(reference()?).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let point = Entity::builder()
        .id(point_id)
        .entity_type(patch_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(1.0, 1.0)?))
        .build()?;
    broker.entities().create(&point).send().await?.done()?;
    let square = Entity::builder()
        .id(square_id.clone())
        .entity_type(patch_type.clone())
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(1.0, 1.0)?,
                Position::try_new(3.0, 1.0)?,
                Position::try_new(3.0, 3.0)?,
                Position::try_new(1.0, 3.0)?,
                Position::try_new(1.0, 1.0)?,
            ])?])?),
        )
        .build()?;
    broker.entities().create(&square).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::overlaps(reference()?))
        .entity_type(&patch_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&square_id)));
    // The oldest notification is the square's: the Point, created first, sent none.
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&square_id)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

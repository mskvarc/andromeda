//! Overlaps selects a line that runs along the reference and crosses back over it, in a query and a subscription
//! alike.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        geometry::{LineString, Position, Shape},
    },
    query::geoquery,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// A line through `positions`, each a longitude and a latitude.
fn line(positions: &[(f64, f64)]) -> Result<Shape> {
    Ok(Shape::LineString(LineString::try_new(
        positions
            .iter()
            .map(|&(longitude, latitude)| -> Result<Position> { Ok(Position::try_new(longitude, latitude)?) })
            .collect::<Result<Vec<_>>>()?,
    )?))
}

/// The route of three segments every request of the test refers to.
fn reference() -> Result<Shape> {
    line(&[
        (17.041_112_899_780_3, 48.208_068_847_656_3),
        (17.044_538_497_924_8, 48.199_306_488_037_1),
        (17.049_634_933_471_7, 48.196_449_279_785_2),
        (17.052_634_9, 48.198_449_3),
    ])
}

/// Pins: with georel overlaps and a reference `LineString` of three segments, a Shape whose `LineString` runs along the
/// reference's second segment, turns back and crosses that segment, and a Shape whose `LineString` runs along the
/// same segment and leaves it at its far end, are both returned by Query Entities of their type and both notified by
/// a Subscription
/// Clause: 4.10, 5.7.2.4, 5.8.6
/// Recorded: clause 4.10 defines overlaps by the Simple Features relation, under which two lines whose interiors share
/// a segment and of which neither contains the other overlap, wherever else either runs; the Entity type is a
/// default-tenant type located by `location`, the shape of query the in-memory spatial index narrows once the broker
/// has loaded it
#[gtest]
#[tokio::test]
async fn n_qry_geo_9103_overlaps_selects_a_line_that_runs_along_the_reference_and_crosses_back_over_it_in_a_query_and_a_subscription_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let shape_type = namespace.entity_type("Shape")?;
    let crossing_back_id = namespace.urn("Shape", "crossing-back")?;
    let control_id = namespace.urn("Shape", "control")?;
    let subscription_id = namespace.subscription_id("overlaps")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([shape_type.as_str()])
        .geo_q(geoquery::overlaps(reference()?).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let crossing_back = Entity::builder()
        .id(crossing_back_id.clone())
        .entity_type(shape_type.clone())
        .geo_property(
            "location",
            line(&[
                (17.044_538_497_924_8, 48.199_306_488_037_1),
                (17.049_634_933_471_7, 48.196_449_279_785_2),
                (17.048_576_354_980_5, 48.197_162_628_173_8),
                (17.046_571_731_567_4, 48.193_210_601_806_6),
            ])?,
        )
        .build()?;
    broker.entities().create(&crossing_back).send().await?.done()?;
    let control = Entity::builder()
        .id(control_id.clone())
        .entity_type(shape_type.clone())
        .geo_property(
            "location",
            line(&[
                (17.044_538_497_924_8, 48.199_306_488_037_1),
                (17.049_634_933_471_7, 48.196_449_279_785_2),
                (17.05, 48.19),
            ])?,
        )
        .build()?;
    broker.entities().create(&control).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::overlaps(reference()?))
        .entity_type(&shape_type)
        .send()
        .await?;
    expect_that!(page.items(), unordered_elements_are![has_id(&crossing_back_id), has_id(&control_id)]);
    // Notifications of two Entities arrive in either order.
    sink.wait_for_at_least(1, 2).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(2)));
    let mut notified = Vec::new();
    for received in &notifications {
        let notification = received.entities(Lossless::Normalized)?;
        expect_that!(notification.subscription_id(), eq(&subscription_id));
        notified.extend(notification.into_data());
    }
    let notified = notified;
    expect_that!(notified.as_slice(), unordered_elements_are![has_id(&crossing_back_id), has_id(&control_id)]);
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

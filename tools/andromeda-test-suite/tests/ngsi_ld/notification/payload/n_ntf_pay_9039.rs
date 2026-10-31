//! A self intersecting polygon is notified as written.

use andromeda_test_suite::{
    attribute_matcher::geo_property,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Creating a Building whose location is the self-intersecting Polygon
/// `[[[0,0]`,`[2,2]`,`[2,0]`,`[0,2]`,`[0,0]`]] notifies that location as that Polygon
/// Clause: 4.7.1, 4.7.2, 5.3.1
/// Recorded: clause 4.7.1 makes the `GeoProperty` value the value read back, and clause 4.7.2 accepts every
/// geometry meeting IETF RFC 7946, whose section 3.1.6 sets no simplicity rule on a ring; Andromeda keeps a
/// self-intersecting Polygon as written and judges a geoquery against its `ST_MakeValid` repair, whose lobes are
/// the triangles (0,0)-(1,1)-(0,2) and (1,1)-(2,2)-(2,0)
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9039_a_self_intersecting_polygon_is_notified_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "bow-tie-notified")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let bow_tie = Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
        Position::try_new(0.0, 0.0)?,
        Position::try_new(2.0, 2.0)?,
        Position::try_new(2.0, 0.0)?,
        Position::try_new(0.0, 2.0)?,
        Position::try_new(0.0, 0.0)?,
    ])?])?);
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", bow_tie.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        elements_are![all!(has_id(&building_id), has_attribute("location", geo_property().geometry(bow_tie)))]
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

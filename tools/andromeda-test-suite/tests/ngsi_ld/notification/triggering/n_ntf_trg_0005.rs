//! A within geo query with a three dimensional polygon notifies a two dimensional entity.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        Representation,
        geometry::{Position, Shape},
    },
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose `geoQ` is within a Polygon drawn at 250 m notifies a two-ordinate entity inside its
/// footprint, and not one outside it
/// Clause: 5.8.6, 5.2.13, 4.10
/// Recorded: clause 5.8.6 evaluates the `geoQ` of a subscription against an entity as clause 4.10 does a query,
/// which defers to OGC 06-103r4; clause 4.10 defers the geometric relationships to OGC 06-103r4, whose clause
/// 6.1.2.5 says spatial operations work in the "map geometry" of the data and "will therefore not reflect z or m
/// values in calculations" and whose clause 6.1.15.1 defines the relationships by the projection of the objects
/// onto the horizontal surface, so a georel that ignores the altitude is the specified behaviour; the user ruled
/// this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_ntf_trg_0005_a_within_geo_query_with_a_three_dimensional_polygon_notifies_a_two_dimensional_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_out_id = namespace.urn("Building", "out")?;
    let building_in_id = namespace.urn("Building", "in")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"georel": "within", "geometry": "Polygon", "coordinates": [[[13.0, 52.0, 250.0], [14.0, 52.0, 250.0], [14.0, 53.0, 250.0], [13.0, 53.0, 250.0], [13.0, 52.0, 250.0]]]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("geoQ", &geo_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let building_out = Entity::builder()
        .id(building_out_id)
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(30.0, 10.0)?))
        .build()?;
    broker.entities().create(&building_out).send().await?.done()?;
    let building_in = Entity::builder()
        .id(building_in_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
        .build()?;
    broker.entities().create(&building_in).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    // The oldest notification is the matching entity: the entity created first lies outside the geoQ and sent none.
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_in_id)));
    Ok(())
}

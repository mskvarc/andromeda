//! A geo query matches a dataset instance of a three dimensional geo property.

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
        GeoProperty,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        Representation,
        geometry::{Position, Shape},
    },
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose `geoQ` names `observationPoint` notifies an entity whose default instance is far away
/// and whose `datasetId` instance, 800 m up, is within 100 m of the reference, and not an entity with no instance
/// near
/// Clause: 5.8.6, 5.2.13, 4.10, 4.5.5
/// Recorded: clause 5.7.2.4, which a subscription `geoQ` follows, lets one `datasetId` instance meeting the
/// restriction suffice; the match here rests on the 800 m altitude of that instance being ignored by near, clause
/// 4.10 defers the geometric relationships to OGC 06-103r4, whose clause 6.1.2.5 says spatial operations work in
/// the "map geometry" of the data and "will therefore not reflect z or m values in calculations" and whose clause
/// 6.1.15.1 defines the relationships by the projection of the objects onto the horizontal surface, so a georel
/// that ignores the altitude is the specified behaviour; the user ruled this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_ntf_trg_0006_a_geo_query_matches_a_dataset_instance_of_a_three_dimensional_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_off_id = namespace.urn("Building", "off")?;
    let building_multi_id = namespace.urn("Building", "multi")?;
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
    let geo_q: RawJson =
        r#"{"georel": "near;maxDistance==100", "geometry": "Point", "coordinates": [13.4, 52.52, 0.0], "geoproperty": "observationPoint"}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("geoQ", &geo_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let building_off = Entity::builder()
        .id(building_off_id)
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
        .geo_property("observationPoint", Shape::Point(Position::try_with_altitude(30.0, 10.0, 800.0)?))
        .build()?;
    broker.entities().create(&building_off).send().await?.done()?;
    let building_multi = Entity::builder()
        .id(building_multi_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(30.0, 10.0)?))
        .geo_property("observationPoint", Shape::Point(Position::try_new(30.0, 10.0)?))
        .geo_property(
            "observationPoint",
            GeoProperty::builder(Shape::Point(Position::try_with_altitude(13.4, 52.52, 800.0)?)).dataset_id(namespace.dataset_id("Dataset", "sensor-a")?),
        )
        .build()?;
    broker.entities().create(&building_multi).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    // The oldest notification is the matching entity: the entity created first lies outside the geoQ and sent none.
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_multi_id)));
    Ok(())
}

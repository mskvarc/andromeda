//! Near subscriptions sharing a geo property each notify only the entity near them.

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

/// Pins: Five `geoQ` near;maxDistance==200 Subscriptions decided against the same Entities each notify exactly the
/// one Entity within 200 m of their reference on the `GeoProperty` they name: three on location 559 m apart, one on
/// `observationPoint` at the first reference, and one with no entities selector at the first reference
/// Clause: 4.10, 5.8.6, 5.2.12
/// Tenant: private (one Subscription watches `location` with no entities selector, so it is notified of every
/// Entity of the tenant near its reference)
#[gtest]
#[tokio::test]
async fn n_ntf_trg_0019_near_subscriptions_sharing_a_geo_property_each_notify_only_the_entity_near_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription_4_id = namespace.subscription_id(4)?;
    let subscription_5_id = namespace.subscription_id(5)?;
    let building_none_id = namespace.urn("Building", "none")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_c_id = namespace.urn("Building", "c")?;
    let building_op_id = namespace.urn("Building", "op")?;
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
    let geo_q: RawJson = r#"{"georel": "near;maxDistance==200", "geometry": "Point", "coordinates": [-29.61, -59.83]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("geoQ", &geo_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q_2: RawJson = r#"{"georel": "near;maxDistance==200", "geometry": "Point", "coordinates": [-29.6, -59.83]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_2.render())?.set_member("geoQ", &geo_q_2);
    let request_2 = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q_3: RawJson = r#"{"georel": "near;maxDistance==200", "geometry": "Point", "coordinates": [-29.59, -59.83]}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(subscription_3.render())?.set_member("geoQ", &geo_q_3);
    let request_3 = broker.unchecked(&CreateSubscription { subscription: &subscription_3 })?.document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::CREATED));
    let subscription_4 = Subscription::builder()
        .id(subscription_4_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(4)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q_4: RawJson =
        r#"{"georel": "near;maxDistance==200", "geometry": "Point", "coordinates": [-29.61, -59.83], "geoproperty": "observationPoint"}"#.parse()?;
    let document_4 = UncheckedDocument::try_from(subscription_4.render())?.set_member("geoQ", &geo_q_4);
    let request_4 = broker.unchecked(&CreateSubscription { subscription: &subscription_4 })?.document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::CREATED));
    let subscription_5 = Subscription::builder()
        .id(subscription_5_id.clone())
        .watched_attributes(["location"])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(5)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q_5: RawJson = r#"{"georel": "near;maxDistance==200", "geometry": "Point", "coordinates": [-29.61, -59.83]}"#.parse()?;
    let document_5 = UncheckedDocument::try_from(subscription_5.render())?.set_member("geoQ", &geo_q_5);
    let request_5 = broker.unchecked(&CreateSubscription { subscription: &subscription_5 })?.document(&document_5);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, answered(StatusCode::CREATED));
    let building_none = Entity::builder()
        .id(building_none_id)
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(-29.5, -59.7)?))
        .geo_property("observationPoint", Shape::Point(Position::try_new(-29.5, -59.7)?))
        .build()?;
    broker.entities().create(&building_none).send().await?.done()?;
    let building_a = Entity::builder()
        .id(building_a_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(-29.6101, -59.83)?))
        .geo_property("observationPoint", Shape::Point(Position::try_new(-29.5, -59.7)?))
        .build()?;
    broker.entities().create(&building_a).send().await?.done()?;
    let building_b = Entity::builder()
        .id(building_b_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(-29.6, -59.8301)?))
        .build()?;
    broker.entities().create(&building_b).send().await?.done()?;
    let building_c = Entity::builder()
        .id(building_c_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(-29.59, -59.83)?))
        .build()?;
    broker.entities().create(&building_c).send().await?.done()?;
    let building_op = Entity::builder()
        .id(building_op_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(-29.5, -59.7)?))
        .geo_property("observationPoint", Shape::Point(Position::try_new(-29.61, -59.8301)?))
        .build()?;
    broker.entities().create(&building_op).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    sink.wait_for_at_least(2, 1).await?;
    sink.wait_for_at_least(3, 1).await?;
    sink.wait_for_at_least(4, 1).await?;
    sink.wait_for_at_least(5, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_a_id)));
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.subscription_id(), eq(&subscription_2_id));
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&building_b_id)));
    let notifications_3 = sink.received(3).await?;
    expect_that!(notifications_3, len(eq(1)));
    let notification_3 = notifications_3.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_3.subscription_id(), eq(&subscription_3_id));
    expect_that!(notification_3.data().as_slice(), len(eq(1)));
    expect_that!(notification_3.data().as_slice(), nth(0, has_id(&building_c_id)));
    let notifications_4 = sink.received(4).await?;
    expect_that!(notifications_4, len(eq(1)));
    let notification_4 = notifications_4.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_4.subscription_id(), eq(&subscription_4_id));
    expect_that!(notification_4.data().as_slice(), len(eq(1)));
    expect_that!(notification_4.data().as_slice(), nth(0, has_id(&building_op_id)));
    let notifications_5 = sink.received(5).await?;
    expect_that!(notifications_5, len(eq(1)));
    let notification_5 = notifications_5.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_5.subscription_id(), eq(&subscription_5_id));
    expect_that!(notification_5.data().as_slice(), len(eq(1)));
    expect_that!(notification_5.data().as_slice(), nth(0, has_id(&building_a_id)));
    Ok(())
}

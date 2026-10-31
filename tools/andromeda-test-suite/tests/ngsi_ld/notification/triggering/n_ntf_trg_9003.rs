//! A near subscription matches a polygon by its nearest wall.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
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
        Attribute,
        AttributeName,
        Entity,
        GeoProperty,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Creating a Polygon whose nearest wall is 100.06 m from the reference point notifies the Subscription with
/// `geoQ` near;maxDistance==150 and not the one with near;minDistance==150, which is first notified when the entity
/// moves 46 km away
/// Clause: 4.10, 5.8.1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9003_a_near_subscription_matches_a_polygon_by_its_nearest_wall() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_within_id = namespace.subscription_id("within")?;
    let near_building_type = namespace.entity_type("NearBuilding")?;
    let near_building_notified_id = namespace.urn("NearBuilding", "notified")?;
    let subscription_beyond_id = namespace.subscription_id("beyond")?;
    let subscription_within = Subscription::builder()
        .id(subscription_within_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&near_building_type).build()?).with_ids(near_building_notified_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("within")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"georel": "near;maxDistance==150", "geometry": "Point", "coordinates": [8.8072, 53.0758]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_within.render())?.set_member("geoQ", &geo_q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_within,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription_beyond = Subscription::builder()
        .id(subscription_beyond_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&near_building_type).build()?).with_ids(near_building_notified_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("beyond")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q_2: RawJson = r#"{"georel": "near;minDistance==150", "geometry": "Point", "coordinates": [8.8072, 53.0758]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_beyond.render())?.set_member("geoQ", &geo_q_2);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_beyond,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let near_building_notified = Entity::builder()
        .id(near_building_notified_id.clone())
        .entity_type(near_building_type.clone())
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(8.808_693, 53.075_171)?,
                Position::try_new(8.810_783, 53.075_171)?,
                Position::try_new(8.810_783, 53.076_429)?,
                Position::try_new(8.808_693, 53.076_429)?,
                Position::try_new(8.808_693, 53.075_171)?,
            ])?])?),
        )
        .build()?;
    broker.entities().create(&near_building_notified).send().await?.done()?;
    sink.wait_for_at_least("within", 1).await?;
    let notifications = sink.received("within").await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&near_building_notified_id)));
    // The move is owed the near;minDistance==150 Subscription a notification; notifications of one subscription are delivered in order, so one sent for the creation would be in the journal before it.
    let patch = Attribute::from(GeoProperty::builder(Shape::Point(Position::try_new(9.5, 53.0758)?)).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&near_building_notified_id, &"location".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    sink.wait_for_at_least("beyond", 1).await?;
    let notifications_2 = sink.received("beyond").await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification_2.data().as_slice(),
        nth(
            0,
            has_attribute(
                "location",
                any_attribute()
                    .value_member(["type"], "Point")
                    .value_member(["coordinates", "0"], Value::Number("9.5".parse()?))
            )
        )
    );
    broker.subscriptions().delete(&subscription_within_id).send().await?;
    broker.subscriptions().delete(&subscription_beyond_id).send().await?;
    Ok(())
}

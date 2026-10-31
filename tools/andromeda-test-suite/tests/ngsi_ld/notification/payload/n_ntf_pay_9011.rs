//! A geojson notification carries its context in the body by default.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals, json_exists},
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification to an endpoint accepting application/geo+json with no `receiverInfo` carries @context in
/// its body and no Link header
/// Clause: 6.3.8, 5.8.1
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9011_a_geojson_notification_carries_its_context_in_the_body_by_default() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("geo-body")?;
    let vehicle_type = namespace.entity_type("GeoVehicleGeoBody")?;
    let vehicle_id = namespace.urn("Vehicle", "geo-body")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("geo-body")?).accept(NgsiLdMediaType::GeoJson).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type)
        .geo_property("location", Shape::Point(Position::try_new(1.0, 2.0)?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least("geo-body", 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?;
    // Where the `@context` member sits is the subject, which the model reads into one place, so the notification is
    // read as received.
    expect_that!(&notification.body, json_equals("$.data.type", "FeatureCollection"));
    expect_that!(&notification.body, json_exists("$.data['@context']"));
    expect_that!(&notification.body, json_absent("$['@context']"));
    expect_that!(&notification.body, json_exists("$.id"));
    expect_that!(&notification.body, json_equals("$.type", "Notification"));
    expect_that!(&notification.body, json_exists("$.notifiedAt"));
    expect_that!(&notification.body, json_equals("$.subscriptionId", subscription_id.to_string()));
    expect_that!(notification.headers.get(LINK), none());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

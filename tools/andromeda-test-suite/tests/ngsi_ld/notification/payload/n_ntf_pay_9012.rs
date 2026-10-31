//! A geojson notification preferring json carries its context in a link.

use andromeda_test_suite::{body_json::json_absent, broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, wire::header_values};
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

/// Pins: A notification to an endpoint accepting application/geo+json whose `receiverInfo` holds Prefer body=json
/// carries a JSON-LD context Link header and no @context in its body
/// Clause: 6.3.8, 5.8.1
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9012_a_geojson_notification_preferring_json_carries_its_context_in_a_link() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("geo-link")?;
    let vehicle_type = namespace.entity_type("GeoVehicleGeoLink")?;
    let vehicle_id = namespace.urn("Vehicle", "geo-link")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("geo-link")?)
                    .accept(NgsiLdMediaType::GeoJson)
                    .receiver_info("Prefer", "body=json")
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type)
        .geo_property("location", Shape::Point(Position::try_new(1.0, 2.0)?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least("geo-link", 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?;
    expect_that!(header_values(&notification.headers, &LINK), contains(contains_substring("json-ld#context")));
    // The absent `@context` member is a fact of the bytes, so it is read as received.
    expect_that!(&notification.body, json_absent("$['@context']"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}

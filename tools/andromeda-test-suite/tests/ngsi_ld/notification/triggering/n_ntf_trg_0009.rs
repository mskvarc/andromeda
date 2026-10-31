//! A geo query matching only an expired location does not notify.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
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
        EntityFragment,
        GeoProperty,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A subscription whose `geoQ` lies around the only location instance of an Entity, an instance whose
/// `expiresAt` has passed, is not notified of a change to that Entity, while a subscription without `geoQ` is
/// notified of it with no location; the `geoQ` subscription is notified of the next change, to an Entity whose
/// location is valid
/// Clause: 4.22, 5.8.6, 4.10
#[gtest]
#[tokio::test]
async fn n_ntf_trg_0009_a_geo_query_matching_only_an_expired_location_does_not_notify() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_sentinel_id = namespace.urn("Building", "sentinel")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Hall")
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.52)?)).expires_at("2000-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let building_sentinel = Entity::builder()
        .id(building_sentinel_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Gate")
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
        .build()?;
    broker.entities().create(&building_sentinel).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["name"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([building_type.as_str()])
        .watched_attributes(["name"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q: RawJson =
        r#"{"georel": "within", "geometry": "Polygon", "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_2.render())?.set_member("geoQ", &geo_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let fragment = EntityFragment::builder().property("name", "Renamed hall").build()?;
    broker.entities().update_attributes(&building_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder().property("name", "Renamed gate").build()?;
    broker.entities().update_attributes(&building_sentinel_id, &fragment_2).send().await?.done()?;
    // The subscription without geoQ is notified of both changes; notifications of two Entities arrive in either order.
    sink.wait_for_at_least(1, 2).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(2)));
    let mut changed = Vec::new();
    for received in &notifications {
        changed.extend(received.entities(Lossless::Normalized)?.into_data());
    }
    let changed = changed;
    expect_that!(
        changed.as_slice(),
        contains(all!(
            has_id(&building_id),
            has_attribute("name", any_attribute().value("Renamed hall")),
            lacks_attribute("location")
        ))
    );
    // One notification, of the change made second: the change to the expired Entity sent none.
    sink.wait_for_at_least(2, 1).await?;
    let notifications_2 = sink.received(2).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification_2.data().as_slice(),
        nth(0, all!(has_id(&building_sentinel_id), has_attribute("location", any_attribute())))
    );
    Ok(())
}

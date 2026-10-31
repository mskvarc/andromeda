//! A key values notification keeps the altitude of the location.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    simplified_matcher::{has_length_at, has_value_at},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        Entity,
        NgsiLdMediaType,
        Representation,
        SimplifiedSpelling,
        Value,
        geometry::{Position, Shape},
    },
    query::geoquery,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a notification in the `keyValues` format carries the location as the bare geometry with all three
/// ordinates
/// Clause: 5.8.6, 4.5.4, 4.7.1
#[gtest]
#[tokio::test]
async fn n_ntf_pay_0001_a_key_values_notification_keeps_the_altitude_of_the_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_new(13.4, 52.52)?)).build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .simplified_spelling(SimplifiedSpelling::KeyValues)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.simplified()?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_value_at("location", ["type"], "Point"),
                has_length_at("location", ["coordinates"], 3),
                has_value_at("location", ["coordinates", "0"], Value::Number("13.4".parse()?)),
                has_value_at("location", ["coordinates", "1"], Value::Number("52.52".parse()?)),
                has_value_at("location", ["coordinates", "2"], 300)
            )
        )
    );
    Ok(())
}

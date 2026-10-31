//! A geo q is notified of an area reaching into it.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, notification_sink::NotificationSink, rectangle_polygon::rectangle_polygon};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output, Representation},
    query::geoquery,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: A Context Source Registration Subscription whose `geoQ` is within a box around Madrid is not notified of a
/// registration located over an area around Berlin and is notified of one located over an area covering Spain: its
/// only notification carries that registration alone
/// Clause: 5.11.2.4, 5.11.7, 4.10
/// Recorded: clause 5.11.2.4 matches the `geoQ` against the registration location "if the location directly matches
/// or if the location possibly contains locations that would match the geoquery"; the area covering Spain is not
/// within the box but holds locations that are
#[gtest]
#[tokio::test]
async fn n_css_ntf_9010_a_geo_q_is_notified_of_an_area_reaching_into_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let subscription_id = format!("urn:ngsi-ld:Subscription:{namespace}").parse::<SubscriptionId>()?;
    let registration_berlin_id = namespace.registration_id("berlin")?;
    let registration_spain_id = namespace.registration_id("spain")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .geo_q(geoquery::within(rectangle_polygon(-4.2, 39.9, -3.2, 40.9)?).build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration_berlin = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_berlin_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(building_type.clone())]).build()?])
        .location(rectangle_polygon(12.0, 51.0, 15.0, 54.0)?)
        .build()?;
    broker.registrations().register(&registration_berlin).send().await?;
    let registration_spain = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_spain_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(building_type.clone())]).build()?])
        .location(rectangle_polygon(-9.0, 36.0, 3.0, 44.0)?)
        .build()?;
    broker.registrations().register(&registration_spain).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(
        notification.data().first().map(CSourceRegistration::<Output>::id),
        some(eq(&registration_spain_id))
    );
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    broker.registrations().delete(&registration_berlin_id).send().await?;
    broker.registrations().delete(&registration_spain_id).send().await?;
    Ok(())
}

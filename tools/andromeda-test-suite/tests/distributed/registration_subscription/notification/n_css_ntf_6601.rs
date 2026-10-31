//! Geo context source filter and scope gate the notified registrations.

use andromeda_test_suite::{broker, config::SuiteConfig, context_source_property::property_value, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        NgsiLdMediaType,
        OneOrMany,
        Output,
        Scope,
        geometry::{Position, Shape},
    },
    query::{geoquery, q, scope_query::ScopeQuery},
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription with a `geoQ`, with and without geoproperty location, is
/// notified of the registration located near the point and not of the one located elsewhere, and one with a csf and
/// a `scopeQ` only of the registration satisfying both
/// Clause: 5.11.7, 5.12
#[gtest]
#[tokio::test]
async fn n_css_ntf_6601_geo_context_source_filter_and_scope_gate_the_notified_registrations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let near_berlin = || -> Result<Shape> { Ok(Shape::Point(Position::try_new(13.404, 52.52)?)) };
    let paris = || -> Result<Shape> { Ok(Shape::Point(Position::try_new(2.349, 48.853)?)) };
    let subscription = Subscription::registration_builder()
        .id(namespace.subscription_id(1)?)
        .entities([namespace.entity_type("VehicleG1")?.to_string()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .geo_q(geoquery::near_max(3000, near_berlin()?).geoproperty("location").build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(namespace.subscription_id(2)?)
        .entities([namespace.entity_type("VehicleG2")?.to_string()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .geo_q(geoquery::near_max(3000, near_berlin()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_2).send().await?;
    let subscription_3 = Subscription::registration_builder()
        .id(namespace.subscription_id(3)?)
        .entities([namespace.entity_type("VehicleC")?.to_string()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .scope_q("/Madrid/#".parse::<ScopeQuery>()?)
        .csf(q::attr("tier").eq("gold").build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_3).send().await?;
    for (suffix, type_base, shape) in [
        (1, "VehicleG1", paris()?),
        (2, "VehicleG1", near_berlin()?),
        (3, "VehicleG2", paris()?),
        (4, "VehicleG2", near_berlin()?),
    ] {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(namespace.registration_id(suffix)?)
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(namespace.entity_type(type_base)?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .build()?])
            .location(shape)
            .build()?;
        broker.registrations().register(&registration).send().await?;
    }
    for (suffix, tier, scope) in [(5, "silver", "/Madrid/Gardens"), (6, "gold", "/Berlin/Parks"), (7, "gold", "/Madrid/Gardens")] {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(namespace.registration_id(suffix)?)
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(namespace.entity_type("VehicleC")?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .build()?])
            .property("tier", property_value(tier))
            .scope(OneOrMany::try_from(vec![scope.parse::<Scope>()?])?)
            .build()?;
        broker.registrations().register(&registration).send().await?;
    }
    for (endpoint, notified_suffix) in [(1, 2), (2, 4), (3, 7)] {
        sink.wait_for_at_least(endpoint, 1).await?;
        let notifications = sink.received(endpoint).await?;
        expect_that!(notifications, len(eq(1)));
        let notification = notifications.first().or_fail()?.registrations()?;
        expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
        expect_that!(notification.data(), len(eq(1)));
        let notified_id: RegistrationId = namespace.registration_id(notified_suffix)?;
        expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&notified_id)));
    }
    Ok(())
}

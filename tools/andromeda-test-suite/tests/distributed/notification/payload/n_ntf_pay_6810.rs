//! A relayed entity is amended to the conformance the subscription names.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    context_source::ContextSource,
    core_context::core_context,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_body, placed_copy_id, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, ConformanceVersion, Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, Property},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A relayed entity holding a Property with `observedAt` and a `LanguageProperty` is notified under a
/// subscription with `ngsildConformance` 1.0 without `observedAt` and with the `LanguageProperty` as a Property
/// Clause: 5.8.6, 4.3.6.8
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6810_a_relayed_entity_is_amended_to_the_conformance_the_subscription_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .ngsild_conformance(ConformanceVersion::new(1, 0))
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let relayed = Entity::output_builder()
        .id(namespace.urn("Vehicle", "conformance")?)
        .entity_type(vehicle_type)
        .property("speed", Property::builder(20).observed_at("2025-01-01T00:00:00Z".parse()?))
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Tower"), ("de", "Turm")])?),
        )
        .build()?;
    let body = notification_body(&namespace, &copy_id, vec![relayed], core_context()?)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let received = notifications.first().or_fail()?;
    // The amended members are read as received: an absent `observedAt` and the reduced Property kind.
    expect_that!(&received.body, json_equals("$.data[0].speed.value", 20));
    expect_that!(&received.body, json_absent("$.data[0].speed.observedAt"));
    expect_that!(&received.body, json_equals("$.data[0].name.type", "Property"));
    Ok(())
}

//! Overlapping id patterns reach the source as the registered pattern, and the subscription's pattern decides what is
//! relayed.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    core_context::core_context,
    forwarded_body,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{copy_selector, notification_body, placed_copy, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, Lossless, NgsiLdMediaType},
    query::{EntitySelector, IdPattern, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription selecting a type and an `idPattern` that overlaps the registered `idPattern` without
/// either containing the other places a copy whose only selector names the type and the registered `idPattern`; a
/// notification of the copy carrying one entity both patterns match and one only the registered pattern matches is
/// relayed to the subscriber with the first entity alone
/// Clause: 5.8.1.4, 4.3.6.1, 5.12, 5.8.6
#[gtest]
#[tokio::test]
async fn n_sub_cre_6812_overlapping_id_patterns_reach_the_source_as_the_registered_pattern_and_the_subscription_pattern_decides_the_relay() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let both_id = namespace.urn("Vehicle", "a1")?;
    let registered_only_id = namespace.urn("Vehicle", "b1")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    // Any kind of identifier of the test ending in `-a…`: wider than the registered `Vehicle` pattern on the kind,
    // narrower on the suffix.
    let subscribed_pattern = format!("{namespace}-a").parse::<IdPattern>()?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_id_pattern(subscribed_pattern)])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    let expected = copy_selector(&vehicle_type)?.with_id_pattern(namespace.id_pattern("Vehicle")?);
    expect_that!(copy.entities(), elements_are![eq(&expected)]);
    let copy_id = copy.id().cloned().or_fail()?;
    let notified = vec![
        Entity::output_builder()
            .id(both_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 40)
            .build()?,
        Entity::output_builder()
            .id(registered_only_id)
            .entity_type(vehicle_type.clone())
            .property("speed", 41)
            .build()?,
    ];
    let body = notification_body(&namespace, &copy_id, notified, core_context()?)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), elements_are![has_id(&both_id)]);
    Ok(())
}

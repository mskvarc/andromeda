//! A type-only selector reaches a source registered by id pattern with the registered pattern.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{copy_selector, placed_copy, register_copy_source},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription selecting a type alone over a `federationOps` registration of that type and an
/// `idPattern` places a copy whose only selector names the type and the registered `idPattern`
/// Clause: 5.8.1.4, 4.3.6.1, 5.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_6809_a_type_only_selector_reaches_a_source_registered_by_id_pattern_with_the_registered_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    let expected = copy_selector(&vehicle_type)?.with_id_pattern(namespace.id_pattern("Vehicle")?);
    expect_that!(copy.entities(), elements_are![eq(&expected)]);
    Ok(())
}

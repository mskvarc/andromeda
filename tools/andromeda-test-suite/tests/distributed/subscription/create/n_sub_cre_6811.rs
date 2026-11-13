//! A selector of several types reaches a source with only the registered types.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{copy_selector, placed_copy, register_copy_source_of},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder, type_selection},
    registration::EntityInfo,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription selecting `Vehicle|Bus` over a `federationOps` registration of `Vehicle` alone places a
/// copy whose only selector names `Vehicle`
/// Clause: 5.8.1.4, 4.3.6.1, 4.17
#[gtest]
#[tokio::test]
async fn n_sub_cre_6811_a_selector_of_several_types_reaches_a_source_with_only_the_registered_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let bus_type = namespace.entity_type("Bus")?;
    register_copy_source_of(&broker, &source, &namespace, "s1", vec![EntityInfo::new(vehicle_type.clone())]).await?;
    let either = SelectorTypeBuilder::from(type_selection::entity_type(vehicle_type.as_str()).or(bus_type.as_str())).build()?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([EntitySelector::new(either)])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    expect_that!(copy.entities(), elements_are![eq(&copy_selector(&vehicle_type)?)]);
    Ok(())
}

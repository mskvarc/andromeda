//! A type-only selector reaches a source registered for one entity naming that entity.

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
    registration::EntityInfo,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription selecting a type alone over a `federationOps` registration of one entity of that type
/// places a copy whose only selector names the type and the registered `id`
/// Clause: 5.8.1.4, 4.3.6.1, 5.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_6808_a_type_only_selector_reaches_a_source_registered_for_one_entity_naming_that_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 7)?;
    let registered = EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone());
    register_copy_source_of(&broker, &source, &namespace, "s1", vec![registered]).await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    let expected = copy_selector(&vehicle_type)?.with_ids(vehicle_id);
    expect_that!(copy.entities(), elements_are![eq(&expected)]);
    Ok(())
}

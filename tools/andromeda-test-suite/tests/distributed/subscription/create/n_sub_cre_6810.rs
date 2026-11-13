//! A selector pattern reaches a source registered for one entity as that entity, and one the entity misses places no
//! copy.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{copy_selector, placed_copy, register_copy_source_of},
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    registration::EntityInfo,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription selecting a type and an `idPattern` the registered `id` does not match places no copy,
/// and one selecting the type and an `idPattern` the registered `id` matches places a copy whose only selector names
/// the type and the registered `id`
/// Clause: 5.8.1.4, 4.3.6.1, 5.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_6810_a_selector_pattern_reaches_a_source_registered_for_one_entity_as_that_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 7)?;
    let registered = EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone());
    register_copy_source_of(&broker, &source, &namespace, "s1", vec![registered]).await?;
    let missing = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_id_pattern(namespace.id_pattern_ending("Vehicle", "8")?)])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&missing).send().await?;
    let matching = Subscription::builder()
        .id(namespace.subscription_id(2)?)
        .description("matching")
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&matching).send().await?;
    let copy = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    expect_that!(copy.description(), some(eq("matching")));
    let expected = copy_selector(&vehicle_type)?.with_ids(vehicle_id);
    expect_that!(copy.entities(), elements_are![eq(&expected)]);
    // Copies are placed before Create Subscription answers, so a copy of the first subscription would have arrived
    // before the copy of the second.
    let placed = source.received_on(Method::POST, "/s1/ngsi-ld/v1/subscriptions").await?;
    expect_that!(placed, len(eq(1)));
    Ok(())
}

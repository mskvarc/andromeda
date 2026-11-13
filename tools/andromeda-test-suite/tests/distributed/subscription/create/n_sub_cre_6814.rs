//! A subscription naming no entities reaches a source with the registered entities.

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

/// Pins: Create Subscription with `watchedAttributes` and no entities over a `federationOps` registration of a type
/// and an `idPattern` places a copy whose only selector names the registered type and `idPattern`
/// Clause: 5.8.1.4, 4.3.6.1, 5.12
/// Tenant: private (a subscription naming no entities matches every registration of its tenant (5.12), so in the
/// shared tenant it would be forwarded to every other file's context sources)
#[gtest]
#[tokio::test]
async fn n_sub_cre_6814_a_subscription_naming_no_entities_reaches_a_source_with_the_registered_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_copy_source(&tenant, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .watched_attributes([namespace.attribute("speed")?.to_string()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription).send().await?;
    let copy = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    let expected = copy_selector(&vehicle_type)?.with_id_pattern(namespace.id_pattern("Vehicle")?);
    expect_that!(copy.entities(), elements_are![eq(&expected)]);
    Ok(())
}

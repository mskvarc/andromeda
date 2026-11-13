//! A subscription whose selectors miss the registration places no copy.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{placed_copy, register_copy_source},
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription selecting a type a `federationOps` registration does not offer places no copy on that
/// source, while one selecting the registered type does
/// Clause: 5.8.1.4, 5.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_6813_a_subscription_whose_selectors_miss_the_registration_places_no_copy() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let bus_type = namespace.entity_type("Bus")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let missing = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([bus_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&missing).send().await?;
    let matching = Subscription::builder()
        .id(namespace.subscription_id(2)?)
        .description("matching")
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&matching).send().await?;
    let copy = forwarded_body::subscription(&placed_copy(&source, "s1", &vehicle_type).await?)?;
    expect_that!(copy.description(), some(eq("matching")));
    // Copies are placed before Create Subscription answers, so a copy of the first subscription would have arrived
    // before the copy of the second.
    let placed = source.received_on(Method::POST, "/s1/ngsi-ld/v1/subscriptions").await?;
    expect_that!(placed, len(eq(1)));
    Ok(())
}

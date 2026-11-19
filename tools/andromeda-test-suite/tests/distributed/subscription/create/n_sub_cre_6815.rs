//! A context source filter places a copy only at the sources whose registration meets it.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{register_numbered_copy_source, register_tiered_copy_source},
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::{EntitySplitting, q},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The `csf` a copy carries: `tier=="gold"`, its term compacted under the `@context` the copy is sent with, since the
/// grammar of clause 4.9 admits only short-hand names in an attribute path.
const GOLD_TIER: &str = r#"tier=="gold""#;

/// Pins: Create Subscription by type with csf tier=="gold", once with `splitEntities` and once without, over three
/// `federationOps` registrations of the type, one with tier gold, one with tier silver and one with no tier, places
/// each copy at the gold source alone, and each copy carries the csf
/// Clause: 4.9, 5.2.12, 5.8.1.4, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_6815_a_context_source_filter_places_a_copy_only_at_the_sources_whose_registration_meets_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    register_tiered_copy_source(&broker, &source, &namespace, "s1", 1, "gold").await?;
    register_tiered_copy_source(&broker, &source, &namespace, "s2", 2, "silver").await?;
    register_numbered_copy_source(&broker, &source, &namespace, "s3", 3).await?;
    let complete = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .csf(q::attr("tier").eq("gold").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let split = Subscription::builder()
        .id(namespace.subscription_id(2)?)
        .entities([vehicle_type.as_str()])
        .split_entities(EntitySplitting::Split)
        .csf(q::attr("tier").eq("gold").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let _cleanup = Cleanup::of(&broker)
        .registration(&namespace.registration_id(1)?)
        .registration(&namespace.registration_id(2)?)
        .registration(&namespace.registration_id(3)?)
        .subscription(&namespace.subscription_id(1)?)
        .subscription(&namespace.subscription_id(2)?);
    broker.subscriptions().create(&complete).send().await?;
    broker.subscriptions().create(&split).send().await?;
    let placed = source.wait_for_on(Method::POST, "/s1/ngsi-ld/v1/subscriptions", 2).await?;
    expect_that!(placed, len(eq(2)));
    // The csf is read as the text the copy carries, the spelling the Context Source expands.
    for copy in &placed {
        expect_that!(copy.body.as_slice(), json_equals("$.csf", GOLD_TIER));
    }
    // Copies are placed before Create Subscription answers, so a copy for the silver or the untiered source would
    // have arrived by now.
    expect_that!(source.received_on(Method::POST, "/s2/ngsi-ld/v1/subscriptions").await?, is_empty());
    expect_that!(source.received_on(Method::POST, "/s3/ngsi-ld/v1/subscriptions").await?, is_empty());
    Ok(())
}

//! A registration updated to meet a context source filter receives the copy and one updated away loses it.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    context_source_property::property_value,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::register_tiered_copy_source,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::q,
    registration::RegistrationFragment,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The `csf` a copy carries: `tier=="gold"` with its term expanded under the core context, as the copy's `q` is.
const GOLD_TIER: &str = r#""https://uri.etsi.org/ngsi-ld/default-context/tier"=="gold""#;

/// Pins: A Subscription by type with csf tier=="gold" places no copy at a `federationOps` registration of the type
/// with tier silver; a registration with tier gold created after the Subscription receives a copy carrying the csf,
/// the silver registration updated to tier gold then receives one, and the first registration updated to tier silver
/// has its copy deleted
/// Clause: 5.2.12, 5.8.1.4, 5.11.2.4, 5.11.7
#[gtest]
#[tokio::test]
async fn n_sub_cre_6816_a_registration_updated_to_meet_a_context_source_filter_receives_the_copy_and_one_updated_away_loses_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    register_tiered_copy_source(&broker, &source, &namespace, "s2", 2, "silver").await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .csf(q::attr("tier").eq("gold").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let _cleanup = Cleanup::of(&broker)
        .registration(&namespace.registration_id(1)?)
        .registration(&namespace.registration_id(2)?)
        .subscription(&subscription_id);
    broker.subscriptions().create(&subscription).send().await?;
    // Copies are placed before Create Subscription answers, so a copy for the silver source would have arrived by now.
    expect_that!(source.received_on(Method::POST, "/s2/ngsi-ld/v1/subscriptions").await?, is_empty());
    register_tiered_copy_source(&broker, &source, &namespace, "s1", 1, "gold").await?;
    let placed = source.wait_for_on(Method::POST, "/s1/ngsi-ld/v1/subscriptions", 1).await?;
    expect_that!(placed, len(eq(1)));
    // The typed reader refuses the expanded csf of a copy, so the body is read for it.
    expect_that!(placed.first().or_fail()?.body.as_slice(), json_equals("$.csf", GOLD_TIER));
    let into_gold = RegistrationFragment::builder().property("tier", property_value("gold")).build()?;
    broker.registrations().update(&namespace.registration_id(2)?, &into_gold).send().await?;
    let placed_2 = source.wait_for_on(Method::POST, "/s2/ngsi-ld/v1/subscriptions", 1).await?;
    expect_that!(placed_2, len(eq(1)));
    let out_of_gold = RegistrationFragment::builder().property("tier", property_value("silver")).build()?;
    broker.registrations().update(&namespace.registration_id(1)?, &out_of_gold).send().await?;
    let deleted = source
        .wait_for_matching(&SourceRequest::matching(Method::DELETE, "/s1/ngsi-ld/v1/subscriptions/.+"), 1)
        .await?;
    expect_that!(deleted, len(eq(1)));
    Ok(())
}

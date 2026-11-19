//! A changed context source filter moves the copies to the sources meeting it.

use andromeda_test_suite::{
    body_json::json_equals,
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
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
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionMember},
};

/// The `csf` the second copy carries: `tier=="silver"`, its term compacted under the `@context` the copy is sent
/// with, since the grammar of clause 4.9 admits only short-hand names in an attribute path.
const SILVER_TIER: &str = r#"tier=="silver""#;

/// Pins: Update Subscription replacing csf tier=="gold" with tier=="silver" deletes the copy at the gold source and
/// places one carrying the new csf at the silver source, and a later Update Subscription deleting csf with the
/// NGSI-LD Null patches the silver copy with csf written as the NGSI-LD Null and places a copy at the gold source again
/// Clause: 4.9, 5.2.12, 5.5.8, 5.8.1.4, 5.8.2.4, 5.11.2.4, 5.11.3.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_6808_a_changed_context_source_filter_moves_the_copies_to_the_sources_meeting_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    register_tiered_copy_source(&broker, &source, &namespace, "s1", 1, "gold").await?;
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
    let placed = source.wait_for_on(Method::POST, "/s1/ngsi-ld/v1/subscriptions", 1).await?;
    expect_that!(placed, len(eq(1)));
    expect_that!(source.received_on(Method::POST, "/s2/ngsi-ld/v1/subscriptions").await?, is_empty());
    let to_silver = SubscriptionFragment::builder().csf(q::attr("tier").eq("silver").build()?).build()?;
    broker.subscriptions().update(&subscription_id, &to_silver).send().await?;
    let deleted = source
        .wait_for_matching(&SourceRequest::matching(Method::DELETE, "/s1/ngsi-ld/v1/subscriptions/.+"), 1)
        .await?;
    expect_that!(deleted, len(eq(1)));
    let placed_2 = source.wait_for_on(Method::POST, "/s2/ngsi-ld/v1/subscriptions", 1).await?;
    expect_that!(placed_2, len(eq(1)));
    // The csf is read as the text the copy carries, the spelling the Context Source expands.
    expect_that!(placed_2.first().or_fail()?.body.as_slice(), json_equals("$.csf", SILVER_TIER));
    let unfiltered = SubscriptionFragment::builder().delete(SubscriptionMember::Csf).build()?;
    broker.subscriptions().update(&subscription_id, &unfiltered).send().await?;
    let patched = source
        .wait_for_matching(
            &SourceRequest::matching(Method::PATCH, "/s2/ngsi-ld/v1/subscriptions/.+").body(BodyPattern::Selects("$[?(@.csf == 'urn:ngsi-ld:null')]".parse()?)),
            1,
        )
        .await?;
    expect_that!(patched, len(ge(1)));
    let placed_again = source.wait_for_on(Method::POST, "/s1/ngsi-ld/v1/subscriptions", 2).await?;
    expect_that!(placed_again, len(eq(2)));
    Ok(())
}

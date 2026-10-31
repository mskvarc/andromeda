//! A member deleted by update reaches a 1 4 source as json null.

use andromeda_test_suite::{
    body_json::json_equals,
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{placed_copy_id, register_copy_source_amended},
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionMember},
};

/// Pins: Update Subscription deleting `expiresAt` patches the copy at a source whose registration names
/// `ngsildConformance` 1.4 with `expiresAt` written as JSON null
/// Clause: 4.3.6.6, 5.8.1.4, 5.8.2.4, 5.5.8
/// Recorded: clause 4.3.6.6 amends the forwarded payload to the source's version; V1.4.2 clause 5.5.8 removes each
/// member of the fragment whose value is null, so the deletion is written as JSON null
#[gtest]
#[tokio::test]
async fn n_sub_upd_6806_a_member_deleted_by_update_reaches_a_1_4_source_as_json_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    register_copy_source_amended(&broker, &source, &namespace, "s1", "1.4").await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let fragment = SubscriptionFragment::builder().delete(SubscriptionMember::ExpiresAt).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let patched =
        SourceRequest::new(Method::PATCH, &format!("/s1/ngsi-ld/v1/subscriptions/{copy_id}")).body(BodyPattern::Selects("$[?(@.expiresAt == null)]".parse()?));
    let forwarded = source.wait_for_matching(&patched, 1).await?;
    expect_that!(forwarded, len(eq(1)));
    // The deletion is the JSON null the model never writes, so the forwarded fragment is read as received.
    let body = forwarded.first().or_fail()?.body.as_slice();
    expect_that!(body, json_equals("$.expiresAt", serde_json::Value::Null));
    expect_that!(String::from_utf8_lossy(body).into_owned(), not(contains_substring("urn:ngsi-ld:null")));
    Ok(())
}

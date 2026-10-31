//! A member deleted by update is sent to the copy as the ngsi ld null.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{placed_copy_id, register_copy_source},
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionMember},
};

/// Pins: Update Subscription setting `expiresAt` and then deleting it with the NGSI-LD Null patches the copy with
/// the expiry and then with `expiresAt` written as the NGSI-LD Null
/// Clause: 5.8.2.4, 5.5.8
/// Recorded: 5.8.2.4 says the update is followed by the copy but not how a member the update deleted is conveyed to
/// the source; Andromeda conveys it as the NGSI-LD Null, the one deletion clause 5.5.8 gives a fragment
#[gtest]
#[tokio::test]
async fn o_sub_upd_6803_a_member_deleted_by_update_is_sent_to_the_copy_as_the_ngsi_ld_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let fragment = SubscriptionFragment::builder().expires_at("2099-01-01T00:00:00Z".parse()?).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let set = SourceRequest::new(Method::PATCH, &format!("/s1/ngsi-ld/v1/subscriptions/{copy_id}"))
        .body(BodyPattern::Selects("$[?(@.expiresAt == '2099-01-01T00:00:00Z')]".parse()?));
    let forwarded = source.wait_for_matching(&set, 1).await?;
    expect_that!(forwarded, len(ge(1)));
    let deletion = SubscriptionFragment::builder().delete(SubscriptionMember::ExpiresAt).build()?;
    broker.subscriptions().update(&subscription_id, &deletion).send().await?;
    let deleted = SourceRequest::new(Method::PATCH, &format!("/s1/ngsi-ld/v1/subscriptions/{copy_id}"))
        .body(BodyPattern::Selects("$[?(@.expiresAt == 'urn:ngsi-ld:null')]".parse()?));
    let forwarded_2 = source.wait_for_matching(&deleted, 1).await?;
    expect_that!(forwarded_2, len(ge(1)));
    Ok(())
}

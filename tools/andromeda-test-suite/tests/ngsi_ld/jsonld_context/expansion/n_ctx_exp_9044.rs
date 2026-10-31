//! A subscription update under a scoped body context is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::{core_context_list, core_entry},
    namespace::Namespace,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription whose body @context defines gear with a scoped @context answers 400 `BadRequestData`
/// and changes nothing, and with the core context alone 204
/// Clause: 5.5.7, 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9044_a_subscription_update_under_a_scoped_body_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_updated_id = namespace.subscription_id("updated")?;
    let device_type = namespace.entity_type("Device")?;
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let subscription_updated = Subscription::builder()
        .id(subscription_updated_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    core_context_list_body.subscriptions().create(&subscription_updated).send().await?;
    let fragment = SubscriptionFragment::builder().description("rescoped").build()?;
    let answer = body_context
        .with_responses()
        .subscriptions()
        .update(&subscription_updated_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_updated_id)
        .send()
        .await?;
    expect_that!(subscription.description(), none());
    let fragment_2 = SubscriptionFragment::builder().description("still fine").build()?;
    core_context_list_body
        .subscriptions()
        .update(&subscription_updated_id, &fragment_2)
        .send()
        .await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_updated_id)
        .send()
        .await?;
    expect_that!(subscription_2.description(), some(eq("still fine")));
    broker.subscriptions().delete(&subscription_updated_id).send().await?;
    Ok(())
}
